# Sync Stall Fast Re-Obtain (#5709) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the initial-sync checkpoint-contiguity stall (#5709) self-heal in ~90s instead of wedging silently for ~8 minutes, by detecting a persistent gap and re-obtaining tips from the state tip immediately.

**Architecture:** The `CheckpointVerifier` publishes a `watch` "gap signal" (`Some(contiguous_height)` while waiting on a gap, `None` otherwise). The receiver is threaded through `router::init` and `start.rs` into `ChainSync`. The syncer's pause loop races `downloads.next()` against a stall deadline; on a persistent gap stall (tip frozen + saturated + gap signal `Some` and unchanged) it returns `SyncError::Stalled`, and `sync()` does `cancel_all()` + immediate re-obtain. No change to block validation/commit.

**Tech Stack:** Rust, Tokio (`watch`, `time::timeout_at`), Tower services. Crates: `zebra-consensus`, `zebrad`.

**Spec:** `docs/superpowers/specs/2026-05-29-sync-stall-fast-reobtain-design.md`

**Testing note:** Per project memory, never run `cargo test --workspace`. Use per-crate `--lib` (e.g. `cargo test -p zebra-consensus --lib`, `cargo test -p zebrad --lib`). Use `cargo build -p <crate>` to check compilation.

---

## File Structure

- `zebra-consensus/src/checkpoint.rs` — add `gap_sender` field + `gap_receiver()` accessor; publish gap state in `target_checkpoint_height`. (verifier signal)
- `zebra-consensus/src/router.rs` — `init` returns the gap receiver as a new tuple element. (plumbing)
- `zebrad/src/commands/start.rs` — destructure the new receiver, pass to `ChainSync::new`. (plumbing)
- `zebrad/src/components/sync.rs` — new constants, `SyncError` enum, `Config.stall_restart_timeout`, `ChainSync` fields, stall detection in the pause loop, fast-restart in `sync()`. (syncer)
- Tests colocated: `zebra-consensus/src/checkpoint/tests/*` and `zebrad/src/components/sync/tests/*`.

---

## Task 1: CheckpointVerifier publishes a gap watch signal

**Files:**
- Modify: `zebra-consensus/src/checkpoint.rs` (struct fields ~118-178; `from_checkpoint_list` ~257-299; `target_checkpoint_height` ~409-479)
- Test: `zebra-consensus/src/checkpoint/tests/` (add to the existing checkpoint test module)

- [ ] **Step 1: Write the failing test**

Add a test that drives `CheckpointVerifier` directly (no router/Buffer). It subscribes to the gap receiver, feeds a contiguous prefix then a block past a gap, and asserts the signal becomes `Some(contiguous_height)`; then feeds the missing block and asserts it returns to `None`.

```rust
#[tokio::test]
async fn checkpoint_gap_signal_reports_contiguous_height() {
    let _init_guard = zebra_test::init();
    let network = Network::Mainnet;
    // Build a verifier with a short checkpoint list from generated blocks.
    // (Mirror the existing `continuous_blockchain`/`from_list` test setup in this module.)
    let (checkpoint_verifier, gap_rx /* watch::Receiver<Option<Height>> */) =
        verifier_with_gap_receiver(&network);

    // Initially no gap.
    assert_eq!(*gap_rx.borrow(), None);

    // Feed heights [1, 2] then [4] (gap at 3): verifier should report Some(Height(2)).
    submit_block(&mut checkpoint_verifier, height_1_block).await;
    submit_block(&mut checkpoint_verifier, height_2_block).await;
    submit_block(&mut checkpoint_verifier, height_4_block).await; // creates the gap
    assert_eq!(*gap_rx.borrow(), Some(Height(2)));

    // Feed the missing height 3: gap closes.
    submit_block(&mut checkpoint_verifier, height_3_block).await;
    assert_ne!(*gap_rx.borrow(), Some(Height(2)));
}
```

> Implementer: reuse this module's existing block-generation helpers (`continuous_blockchain`, `from_list`) rather than the pseudocode helpers above. The `submit_block` calls are `checkpoint_verifier.ready().await?.call(block).await` futures — they will stay pending while a gap exists, so spawn them or poll without awaiting completion (the existing tests already do this pattern).

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p zebra-consensus --lib checkpoint_gap_signal -- --nocapture`
Expected: FAIL — no `gap_receiver()` / field does not exist (compile error).

- [ ] **Step 3: Add the `gap_sender` field**

In the `CheckpointVerifier` struct (~118-178) add:

```rust
/// Publishes the highest contiguous queued height while the verifier is
/// waiting on a gap below the next checkpoint (`Some`), or `None` when there
/// is no gap. Used by the syncer to detect contiguity stalls. Observability
/// only — does not affect verification.
gap_sender: watch::Sender<Option<block::Height>>,
```

Add the import at the top of the file: `use tokio::sync::watch;`

- [ ] **Step 4: Initialize it in `from_checkpoint_list` and add the accessor**

In `from_checkpoint_list` (~262), before constructing the struct:

```rust
let (gap_sender, _gap_receiver) = watch::channel(None);
```

Add `gap_sender,` to the `CheckpointVerifier { ... }` initializer (~277). Then add an accessor method in the same `impl` block:

```rust
/// Returns a receiver for the verifier's contiguity-gap signal.
///
/// `Some(height)` means the verifier is waiting for the block at
/// `height + 1` to extend a contiguous range toward the next checkpoint;
/// `None` means no gap.
pub(crate) fn gap_receiver(&self) -> watch::Receiver<Option<block::Height>> {
    self.gap_sender.subscribe()
}
```

- [ ] **Step 5: Publish the gap state from `target_checkpoint_height`**

`target_checkpoint_height(&self)` (~409) — `watch::Sender::send` takes `&self`, so this is fine. Replace the early genesis-wait return and the final return so the signal is updated on every pass:

At the genesis-wait early return (~414-416):
```rust
BeforeGenesis if !self.queued.contains_key(&block::Height(0)) => {
    tracing::trace!("Waiting for genesis block");
    metrics::counter!("checkpoint.waiting.count").increment(1);
    let _ = self.gap_sender.send(Some(block::Height(0)));
    return WaitingForBlocks;
}
```

At the final return (~476-478), replace with:
```rust
match target_checkpoint {
    Some(height) => {
        // A checkpoint range is ready to verify: no contiguity gap.
        let _ = self.gap_sender.send(None);
        Checkpoint(height)
    }
    None => {
        // Waiting for more blocks above `pending_height`: report the gap.
        let _ = self.gap_sender.send(Some(pending_height));
        WaitingForBlocks
    }
}
```

Also handle `FinalCheckpoint => return FinishedVerifying` (~420): publish `None` before returning (no gap once finished):
```rust
FinalCheckpoint => {
    let _ = self.gap_sender.send(None);
    return FinishedVerifying;
}
```

- [ ] **Step 6: Run test to verify it passes**

Run: `cargo test -p zebra-consensus --lib checkpoint_gap_signal -- --nocapture`
Expected: PASS.

- [ ] **Step 7: Confirm no regression in the verifier suite**

Run: `cargo test -p zebra-consensus --lib checkpoint`
Expected: PASS (all existing checkpoint tests green).

- [ ] **Step 8: Commit**

```bash
git add zebra-consensus/src/checkpoint.rs
git commit -m "feat(consensus): CheckpointVerifier publishes contiguity-gap watch signal (#5709)"
```

---

## Task 2: Thread the gap receiver through router::init and start.rs

**Files:**
- Modify: `zebra-consensus/src/router.rs` (`init` signature/return ~246-392; verifier built ~378)
- Modify: `zebrad/src/commands/start.rs` (~228-246)
- Modify: `zebrad/src/components/sync.rs` (`ChainSync::new` signature + struct field)

This task is compile-checked, not unit tested (pure plumbing). A new field is wired but unused until Task 5.

- [ ] **Step 1: `router::init` returns the receiver**

In `router.rs`, extend the return tuple type (~251-259) with `watch::Receiver<Option<block::Height>>` and add `use tokio::sync::watch;` if absent. After constructing `checkpoint` (~378), before moving it into the router:

```rust
let checkpoint = CheckpointVerifier::from_checkpoint_list(list, network, tip, state_service);
let checkpoint_gap_receiver = checkpoint.gap_receiver();
let router = BlockVerifierRouter { checkpoint, max_checkpoint_height, block };
```

Update the final return (~391):
```rust
(router, transaction, task_handles, max_checkpoint_height, checkpoint_gap_receiver)
```

Update the doc comment / any other callers of `init` (e.g. `init_test`, tests) to destructure the extra element (use `_` where unused).

- [ ] **Step 2: `start.rs` destructures and forwards it**

At start.rs ~228:
```rust
let (
    block_verifier_router,
    tx_verifier,
    consensus_task_handles,
    max_checkpoint_height,
    checkpoint_gap_receiver,
) = zebra_consensus::router::init(/* unchanged args */).await;
```

Pass it to `ChainSync::new` (~238) as a new argument: `checkpoint_gap_receiver,`.

- [ ] **Step 3: `ChainSync::new` accepts and stores it**

In sync.rs add a struct field near `past_lookahead_limit_receiver` (~389):
```rust
/// Signal from the checkpoint verifier: `Some(height)` while it waits on a
/// contiguity gap. Used to distinguish a gap stall from slow verification.
checkpoint_gap_receiver: watch::Receiver<Option<block::Height>>,
```
Add `use tokio::sync::watch;` if absent. Add the parameter to `new(...)` and set the field in the constructed `Self`.

- [ ] **Step 4: Compile both crates**

Run: `cargo build -p zebra-consensus -p zebrad`
Expected: builds clean (warnings about the unused field are acceptable until Task 5; add `#[allow(dead_code)]` on the field temporarily if `-D warnings` blocks the build, removed in Task 5).

- [ ] **Step 5: Commit**

```bash
git add zebra-consensus/src/router.rs zebrad/src/commands/start.rs zebrad/src/components/sync.rs
git commit -m "feat(sync): thread checkpoint gap signal from router into ChainSync (#5709)"
```

---

## Task 3: Add `stall_restart_timeout` config + constants

**Files:**
- Modify: `zebrad/src/components/sync.rs` (constants ~50-211; `Config` ~229-274; `Default` ~276+)
- Test: `zebrad/src/components/sync/tests/` (config default + deserialization)

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn stall_restart_timeout_default_is_90s() {
    let config = super::Config::default();
    assert_eq!(config.stall_restart_timeout, Some(Duration::from_secs(90)));
}

#[test]
fn stall_restart_timeout_can_be_disabled_via_toml() {
    let toml = "stall_restart_timeout = false"; // serde humantime_serde Option → None when absent;
    // Implementer: pick the representation that matches Zebra's Option<Duration> serde convention
    // used elsewhere (see other Option<Duration> config fields). Assert None round-trips.
    let config: super::Config = toml::from_str(toml).unwrap_or_default();
    let _ = config; // adjust assertion to the chosen disable representation
}
```

> Implementer: search the codebase for an existing `Option<Duration>` config field with `humantime_serde` to copy the exact serde attribute and disable representation, so this matches Zebra conventions. If none exists, use `#[serde(default, with = "humantime_serde")]` on `Option<Duration>` (humantime_serde supports `Option`).

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p zebrad --lib stall_restart_timeout`
Expected: FAIL — field does not exist.

- [ ] **Step 3: Add constants**

Near the other sync constants (after `SYNC_RESTART_DELAY` ~206):
```rust
/// Delay before re-obtaining tips after a detected contiguity stall.
///
/// Much shorter than [`SYNC_RESTART_DELAY`] because a stall restart is a
/// deliberate, immediate recovery, not a backoff after an error.
const STALL_RESTART_DELAY: Duration = Duration::from_secs(5);

/// Minimum interval between successive stall restarts, to avoid thrashing
/// when a region genuinely has no peer serving the missing block.
const MIN_STALL_RESTART_INTERVAL: Duration = Duration::from_secs(30);

/// Default value for [`Config::stall_restart_timeout`].
const DEFAULT_STALL_RESTART_TIMEOUT: Duration = Duration::from_secs(90);
```

- [ ] **Step 4: Add the config field + default**

In `Config` (~273, after `parallel_cpu_threads` or grouped with sync timing):
```rust
/// How long the syncer waits for the state tip to advance while the download
/// queue is saturated and the checkpoint verifier reports a contiguity gap,
/// before restarting sync from the current tip.
///
/// This recovers from initial-sync stalls (see issue #5709) in seconds
/// instead of waiting for the multi-minute block verify timeout. Must be
/// shorter than the internal block verify timeout. Set to `None` to disable
/// fast restart and fall back to the legacy timeout-only recovery.
#[serde(default, with = "humantime_serde")]
pub stall_restart_timeout: Option<Duration>,
```
In `Default` (~278): `stall_restart_timeout: Some(DEFAULT_STALL_RESTART_TIMEOUT),`

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p zebrad --lib stall_restart_timeout`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add zebrad/src/components/sync.rs
git commit -m "feat(sync): add sync.stall_restart_timeout config (default 90s) (#5709)"
```

---

## Task 4: `SyncError` enum + fast-restart in `sync()`

**Files:**
- Modify: `zebrad/src/components/sync.rs` (`sync()` ~536-559; `try_to_sync` ~575-605; `try_to_sync_once` ~615-682 signature; add `ChainSync` fields `last_tip_advance`, `last_stall_restart`)

This task introduces the error discriminant and the restart scheduling, but the detector that *produces* `Stalled` lands in Task 5. After this task, behavior is unchanged (nothing returns `Stalled` yet); verified by the existing suite staying green.

- [ ] **Step 1: Define `SyncError`**

Add near the top of the impl module:
```rust
/// Outcome of a failed sync run, distinguishing a recoverable contiguity
/// stall (fast restart) from any other error (normal backoff restart).
enum SyncError {
    /// A persistent checkpoint-contiguity gap was detected while saturated.
    /// Restart immediately from the current tip.
    Stalled,
    /// Any other error. Restart after the normal delay.
    Other(color_eyre::Report),
}

impl From<color_eyre::Report> for SyncError {
    fn from(error: color_eyre::Report) -> Self {
        SyncError::Other(error)
    }
}
```

- [ ] **Step 2: Add timing fields to `ChainSync`**

Near the internal sync state fields (~389):
```rust
/// When the state tip last advanced, used for stall detection.
last_tip_advance: std::time::Instant,
/// When the syncer last performed a stall restart, used to throttle restarts.
last_stall_restart: std::time::Instant,
```
Initialize both in `new(...)` to `Instant::now()`.

> Note: `Instant::now()` in `new` is fine. In tests that use Tokio paused time, the detector compares against `tokio::time::Instant`; the implementer should use `tokio::time::Instant` for the stall deadline math (Task 5) so paused-time tests work, and keep these fields as `tokio::time::Instant`.

- [ ] **Step 3: Change `try_to_sync` / `try_to_sync_once` return types**

`try_to_sync` → `Result<(), SyncError>`; `try_to_sync_once` → `Result<IndexSet<block::Hash>, SyncError>`. The inner `?` operators on `Report`-returning calls (`obtain_tips`, `extend_tips`, `request_blocks`, `handle_block_response`) work via `From<Report>`. At the `timeout(BLOCK_VERIFY_TIMEOUT, self.try_to_sync_once(...))` site (~595), the `Elapsed` → error conversion must map into `SyncError::Other` (wrap the elapsed as a `Report` as today, then `.into()`).

- [ ] **Step 4: Rewrite the `sync()` loop body**

Replace ~542-558:
```rust
match self.try_to_sync().await {
    Ok(()) => {}
    Err(SyncError::Stalled) => {
        self.downloads.cancel_all();
        self.last_stall_restart = tokio::time::Instant::now();
        self.update_metrics();
        info!(
            state_tip = ?self.latest_chain_tip.best_tip_height(),
            "restarting sync after contiguity stall"
        );
        sleep(STALL_RESTART_DELAY).await;
        continue;
    }
    Err(SyncError::Other(error)) => {
        warn!(?error, "sync error, restarting");
        self.downloads.cancel_all();
    }
}

self.update_metrics();
let restart_delay = if self.is_regtest { REGTEST_SYNC_RESTART_DELAY } else { SYNC_RESTART_DELAY };
info!(timeout = ?restart_delay, state_tip = ?self.latest_chain_tip.best_tip_height(), "waiting to restart sync");
sleep(restart_delay).await;
```

> Preserve existing log semantics; the key change is the `Stalled` arm with the short delay and `continue`.

- [ ] **Step 5: Compile + run existing sync suite (no new behavior yet)**

Run: `cargo build -p zebrad && cargo test -p zebrad --lib sync`
Expected: builds; existing sync tests PASS (nothing emits `Stalled` yet).

- [ ] **Step 6: Commit**

```bash
git add zebrad/src/components/sync.rs
git commit -m "refactor(sync): SyncError discriminant + fast-restart scheduling in sync() (#5709)"
```

---

## Task 5: Stall detection in the pause loop

**Files:**
- Modify: `zebrad/src/components/sync.rs` (`try_to_sync_once` pause loop ~632-649; add helper `is_gap_stalled`)
- Test: `zebrad/src/components/sync/tests/vectors.rs` (or the appropriate sync test module)

- [ ] **Step 1: Write the failing test (stall → fast restart)**

Using Tokio paused time and the existing sync test harness (mock peer set / verifier — see `sync/tests/vectors.rs` for the established mock pattern), drive the syncer so `in_flight` saturates, the `checkpoint_gap_receiver` holds `Some(h)`, and the state tip is frozen. Assert `try_to_sync` resolves to `Err(SyncError::Stalled)` within `stall_restart_timeout` (advance paused time), well before `BLOCK_VERIFY_TIMEOUT`.

```rust
#[tokio::test(start_paused = true)]
async fn syncer_fast_restarts_on_persistent_gap_stall() {
    // Build ChainSync with mocks: peer set that returns blocks to saturate in_flight,
    // a verifier that never commits (holds gap_receiver = Some), frozen latest_chain_tip,
    // and stall_restart_timeout = Some(90s).
    // Advance time past 90s; assert the run ends in SyncError::Stalled (or that sync()
    // performs cancel_all + re-obtain) rather than blocking to 8 minutes.
}
```

Also add:
- `syncer_does_not_restart_when_tip_advancing` — tip advances each interval → no `Stalled` before 8 min.
- `syncer_does_not_restart_when_no_gap` — gap_receiver = `None` (slow-but-complete verify) → no `Stalled`.
- `disabled_stall_timeout_preserves_legacy_behavior` — `stall_restart_timeout = None` → original bare-await path, no `Stalled`.

> Implementer: match the construction style in `sync/tests/vectors.rs`. If full ChainSync construction is heavy, factor `is_gap_stalled` to be unit-testable in isolation (pure function of: now, last_tip_advance, in_flight, lookahead_limit, gap snapshot, last_stall_restart) and unit-test that directly, plus one integration test for the loop wiring.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p zebrad --lib syncer_fast_restarts_on_persistent_gap_stall`
Expected: FAIL (no detection; would hang/timeout or never return `Stalled`).

- [ ] **Step 3: Add `is_gap_stalled` helper**

```rust
/// Returns true if the syncer appears wedged on a checkpoint-contiguity gap:
/// the state tip is frozen, the download queue is saturated, the verifier is
/// reporting an unchanged gap, and we have not just restarted.
fn is_gap_stalled(&mut self, gap_snapshot: Option<block::Height>, now: tokio::time::Instant) -> bool {
    let Some(timeout) = self.stall_restart_timeout else { return false; };
    let saturated = self.downloads.in_flight() >= self.lookahead_limit(0) / 2;
    let gap_now = *self.checkpoint_gap_receiver.borrow();
    let tip_frozen = now.duration_since(self.last_tip_advance) >= timeout;
    let not_thrashing = now.duration_since(self.last_stall_restart) >= MIN_STALL_RESTART_INTERVAL;
    saturated && tip_frozen && not_thrashing && gap_now.is_some() && gap_now == gap_snapshot
}
```

Add the `stall_restart_timeout: Option<Duration>` field to `ChainSync` (copied from config in `new`).

- [ ] **Step 4: Rewrite the pause loop with a stall deadline**

Replace the pause-loop body (~636-648). Capture the gap snapshot and tip when arming; race `downloads.next()` against the deadline:

```rust
while self.downloads.in_flight() >= self.lookahead_limit(extra_hashes.len())
    || (self.downloads.in_flight() >= self.lookahead_limit(extra_hashes.len()) / 2
        && self.past_lookahead_limit_receiver.cloned_watch_data())
{
    let response = if let Some(timeout) = self.stall_restart_timeout {
        let gap_snapshot = *self.checkpoint_gap_receiver.borrow();
        let deadline = self.last_tip_advance + timeout;
        match tokio::time::timeout_at(deadline, self.downloads.next()).await {
            Ok(response) => response.expect("downloads is nonempty"),
            Err(_elapsed) => {
                let now = tokio::time::Instant::now();
                if self.is_gap_stalled(gap_snapshot, now) {
                    warn!(
                        gap_height = ?self.checkpoint_gap_receiver.borrow().map(|h| h + 1),
                        state_tip = ?self.latest_chain_tip.best_tip_height(),
                        in_flight = self.downloads.in_flight(),
                        lookahead_limit = self.lookahead_limit(extra_hashes.len()),
                        "syncer stalled on checkpoint gap; restarting from tip",
                    );
                    return Err(SyncError::Stalled);
                }
                // Not a gap stall (progressing, or complete range verifying slowly):
                // re-arm by treating this as a liveness tick.
                self.last_tip_advance = now;
                continue;
            }
        }
    } else {
        self.downloads.next().await.expect("downloads is nonempty")
    };

    let before = self.latest_chain_tip.best_tip_height();
    self.handle_block_response(response)?;
    if self.latest_chain_tip.best_tip_height() > before {
        self.last_tip_advance = tokio::time::Instant::now();
    }
    self.update_metrics();
}
```

Also reset `last_tip_advance` whenever the state tip advances on the normal (non-paused) path, and set it when entering `try_to_sync` so a fresh run starts the clock. Remove any temporary `#[allow(dead_code)]` added in Task 2.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p zebrad --lib syncer_fast_restarts_on_persistent_gap_stall syncer_does_not_restart_when_tip_advancing syncer_does_not_restart_when_no_gap disabled_stall_timeout_preserves_legacy_behavior`
Expected: PASS.

- [ ] **Step 6: Full local gate**

Run:
```bash
cargo fmt --all -- --check
cargo clippy -p zebra-consensus -p zebrad --all-targets -- -D warnings
cargo test -p zebra-consensus --lib
cargo test -p zebrad --lib sync
```
Expected: all clean/green.

- [ ] **Step 7: Commit**

```bash
git add zebrad/src/components/sync.rs
git commit -m "feat(sync): detect checkpoint-contiguity stall and fast-restart from tip (#5709)"
```

---

## Task 6: Docs + changelog

**Files:**
- Modify: `CHANGELOG.md` (`[Unreleased]`)
- Modify: book config docs if sync config is documented there (search `checkpoint_verify_concurrency_limit` in `book/`)

- [ ] **Step 1: Changelog entry**

Under `[Unreleased]`:
```
### Changed
- The syncer now detects initial-sync checkpoint-contiguity stalls and restarts
  from the current tip within `sync.stall_restart_timeout` (default 90s) instead
  of waiting for the multi-minute block verify timeout (#5709). Set
  `sync.stall_restart_timeout` to disable.
```
Apply label `C-bug` on the PR.

- [ ] **Step 2: Commit**

```bash
git add CHANGELOG.md book/
git commit -m "docs: changelog for #5709 sync stall fast-restart"
```

---

## Done criteria

- All new tests pass; `zebra-consensus` and `zebrad` `--lib` suites green.
- `cargo fmt --check` and `clippy -D warnings` clean for both crates.
- Default behavior: fast restart on persistent gap within ~90s; no false restart on slow-but-progressing verification; `None` reproduces legacy behavior.
- No change to block validation/commit logic.

## Out of scope (do not implement)

- Per-block surgical re-queue without `cancel_all`.
- Removing last-hash stripping in `obtain_tips`/`extend_tips`.
- Changing `BLOCK_VERIFY_TIMEOUT` (kept as backstop).
