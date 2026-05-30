//! Tests for the pure checkpoint-contiguity stall decision function.

use std::time::Duration;

use tokio::time::Instant;
use zebra_chain::block::Height;

/// All conditions true: frozen tip, saturated queue, not thrashing, a
/// persistent (unchanged) gap, and an inert verifier. This must be detected as
/// a stall.
#[test]
fn gap_stall_detected_when_frozen_saturated_gap_unchanged() {
    let now = Instant::now();
    let timeout = Duration::from_secs(90);

    let detected = super::super::detect_gap_stall(
        timeout,
        now,
        now - Duration::from_secs(90), // last_tip_advance (frozen >= timeout)
        now - Duration::from_secs(600), // last_stall_restart (not thrashing)
        999,                           // in_flight
        500,                           // saturation_threshold
        Some(Height(1000)),            // gap_now
        Some(Height(1000)),            // gap_snapshot (unchanged)
        7,                             // verifier_liveness_now
        7,                             // verifier_liveness_snapshot (inert: unchanged)
    );

    assert!(
        detected,
        "all-true conditions should be detected as a stall"
    );
}

/// Fast restart disabled (`timeout == ZERO`) always returns false, even when
/// every other condition would indicate a stall.
#[test]
fn no_stall_when_disabled() {
    let now = Instant::now();

    let detected = super::super::detect_gap_stall(
        Duration::ZERO,
        now,
        now - Duration::from_secs(90),
        now - Duration::from_secs(600),
        999,
        500,
        Some(Height(1000)),
        Some(Height(1000)),
        7,
        7,
    );

    assert!(!detected, "ZERO timeout disables fast restart");
}

/// No gap reported (`gap_now == None`) means the verifier is not blocked on
/// contiguity, so this is not a gap stall.
#[test]
fn no_stall_when_no_gap() {
    let now = Instant::now();
    let timeout = Duration::from_secs(90);

    let detected = super::super::detect_gap_stall(
        timeout,
        now,
        now - Duration::from_secs(90),
        now - Duration::from_secs(600),
        999,
        500,
        None, // gap_now
        None, // gap_snapshot
        7,
        7,
    );

    assert!(!detected, "no gap means no gap stall");
}

/// The gap changed since the deadline was armed (the verifier made progress),
/// so this is not a persistent stall.
#[test]
fn no_stall_when_gap_changed() {
    let now = Instant::now();
    let timeout = Duration::from_secs(90);

    let detected = super::super::detect_gap_stall(
        timeout,
        now,
        now - Duration::from_secs(90),
        now - Duration::from_secs(600),
        999,
        500,
        Some(Height(1001)), // gap_now (advanced)
        Some(Height(1000)), // gap_snapshot
        7,
        7,
    );

    assert!(!detected, "a changed gap means the verifier made progress");
}

/// The download queue is not saturated, so we are still making download
/// progress and should not restart.
#[test]
fn no_stall_when_not_saturated() {
    let now = Instant::now();
    let timeout = Duration::from_secs(90);

    let detected = super::super::detect_gap_stall(
        timeout,
        now,
        now - Duration::from_secs(90),
        now - Duration::from_secs(600),
        100, // in_flight < saturation_threshold
        500, // saturation_threshold
        Some(Height(1000)),
        Some(Height(1000)),
        7,
        7,
    );

    assert!(!detected, "an unsaturated queue is not a stall");
}

/// A stall restart happened very recently (within `MIN_STALL_RESTART_INTERVAL`),
/// so we throttle and do not restart again yet.
#[test]
fn no_stall_when_thrashing() {
    let now = Instant::now();
    let timeout = Duration::from_secs(90);

    let detected = super::super::detect_gap_stall(
        timeout,
        now,
        now - Duration::from_secs(90),
        now, // last_stall_restart just now (< MIN_STALL_RESTART_INTERVAL)
        999,
        500,
        Some(Height(1000)),
        Some(Height(1000)),
        7,
        7,
    );

    assert!(!detected, "a recent restart should throttle the next one");
}

/// The tip advanced recently (within `timeout`), so the syncer is making
/// progress and is not stalled.
#[test]
fn no_stall_when_tip_recently_advanced() {
    let now = Instant::now();
    let timeout = Duration::from_secs(90);

    let detected = super::super::detect_gap_stall(
        timeout,
        now,
        now, // last_tip_advance recent (< timeout)
        now - Duration::from_secs(600),
        999,
        500,
        Some(Height(1000)),
        Some(Height(1000)),
        7,
        7,
    );

    assert!(!detected, "a recently advanced tip is not a stall");
}

/// EXPERIMENTAL (#5709): a busy-but-slow verifier must NOT be restarted.
///
/// This is the sandblasting doom-loop regression. Every time-based condition
/// reads like a stall — the tip is frozen past the timeout, the queue is
/// saturated, we are not thrashing, and the contiguity frontier (the gap) is
/// unchanged — because a single sandblasting-era checkpoint range can take
/// minutes to download and verify. But the verifier-liveness counter advanced
/// (it queued/verified blocks in the meantime), so the verifier is busy, not
/// wedged. Restarting here would cancel the in-progress work and doom-loop
/// (observed as ~4,199 wholesale download cancellations on a live node), so this
/// must NOT be detected as a stall.
#[test]
fn no_stall_when_verifier_made_progress() {
    let now = Instant::now();
    let timeout = Duration::from_secs(90);

    let detected = super::super::detect_gap_stall(
        timeout,
        now,
        now - Duration::from_secs(90),  // tip frozen past timeout
        now - Duration::from_secs(600), // not thrashing
        999,                            // saturated
        500,
        Some(Height(1000)), // gap_now
        Some(Height(1000)), // gap_snapshot (frontier static)
        42,                 // verifier_liveness_now
        7,                  // verifier_liveness_snapshot (advanced => busy)
    );

    assert!(
        !detected,
        "a verifier that advanced its liveness counter is busy, not wedged",
    );
}
