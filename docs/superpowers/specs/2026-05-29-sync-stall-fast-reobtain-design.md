# Design: Fast gap-detected re-obtain for initial-sync stalls (#5709)

- **Status:** Draft for review
- **Issue:** ZcashFoundation/zebra#5709 ("Fix repeated block timeouts during initial sync")
- **Branch:** `fix/5709-sync-stall` (off `main` / v4.5.1)
- **Date:** 2026-05-29

## Problem

During initial sync (checkpoint phase) Zebra deterministically wedges when the download
lookahead queue saturates (`in_flight` ~999/1000). The `CheckpointVerifier` requires a
strictly contiguous block range from the last verified checkpoint; a single missing/late
block (introduced by last-hash stripping in `obtain_tips`/`extend_tips`, fork divergence
across the 3-peer fanout, or a dropped download) leaves every already-downloaded higher
block parked in the verifier, holding `in_flight` slots. The syncer, once saturated, parks
on `downloads.next().await` and stops extending tips — so it never re-requests the missing
block. Recovery only happens via the 8-minute `BLOCK_VERIFY_TIMEOUT` (which wraps both the
verifier and `try_to_sync_once`) followed by a 67s `SYNC_RESTART_DELAY`. Net effect:
~30-90s of progress, then a multi-minute silent stall, repeating.

Confirmed identical on v4.4.1 and v4.5.1. Hardware/peer-count independent. Evidence that it
is *verify-parked, not download-stuck*: during the stall CPU ~0%, disk idle, and all peer
TCP connections show empty send/recv queues (no outstanding block requests).

### Why a literal per-height re-request is infeasible

The syncer is **hash-keyed**, not height-keyed: it downloads `BlocksByHash`, tracks
`cancel_handles: HashMap<block::Hash, _>`, and only learns a block's height after download.
A contiguity gap means the **hash** of the missing height is unknown (it was last-hash
stripped or never returned by the fanout). The only way to obtain that hash is to re-walk
tips via `FindBlocks` from the committed state tip. Therefore any real re-request reduces to
"re-walk tips from the state tip and re-queue" — which is exactly what a manual restart does,
and why a manual bounce reliably produces a burst.

## Goals

- Eliminate the multi-minute silent wedge: detect a gap stall in ~90s and self-heal in
  seconds, automatically.
- Distinguish a *gap stall* (restart helps) from a *complete-but-slowly-verifying range*
  (restart would waste work) so we never throw away legitimate progress.
- Surface the stall at WARN with the gap location, instead of total silence.
- Touch **no** block-validation or commit logic; correctness path unchanged.

## Non-goals

- Preventing gaps at the source (last-hash stripping is a zcashd-compat workaround; out of
  scope and higher risk).
- Per-block surgical re-queue without `cancel_all()` (rejected: marginal benefit over
  re-walk, higher regression risk).
- Removing the 8-minute `BLOCK_VERIFY_TIMEOUT` backstop (kept as a safety net).

## Design overview (approach "C2")

Two cooperating pieces:

1. **`CheckpointVerifier` publishes a gap signal** via a `watch` channel.
2. **The syncer detects a persistent gap stall and fast-restarts** (re-obtains tips from the
   state tip immediately, skipping the 8-min timeout and the 67s delay).

### Component 1 — verifier gap signal (`zebra-consensus`)

`CheckpointVerifier` gains:

```rust
gap_sender: watch::Sender<Option<block::Height>>,
```

Semantics of the published value:
- `Some(contiguous_height)` — the verifier is in `WaitingForBlocks`: it has buffered blocks
  above a gap and is stalled; `contiguous_height + 1` is the next needed height.
- `None` — no gap (either idle, or a complete range is verifying/committing).

It is updated from `process_checkpoint_range()` (`&mut self`, checkpoint.rs ~774) on every
pass, including the early `WaitingForBlocks` return (checkpoint.rs ~800-802) — that early
return is exactly the gap case and **must** publish `Some(contiguous_height)`. It is reset to
`None` when a range commits and progress advances. (Publishing only needs `&watch::Sender`,
so a `&self` site such as `target_checkpoint_height()` at checkpoint.rs ~435-478, which
computes `pending_height` and sets the `checkpoint.queued.continuous.height` gauge at line
450, is also a valid place to publish if preferred.)

Exposure: `CheckpointVerifier::subscribe_gap() -> watch::Receiver<Option<block::Height>>`.
The consensus init path (`zebra_consensus::router::init` and the block-verifier
construction) returns this receiver alongside the verifier service so it can be threaded to
the syncer. **This is the only cross-crate plumbing.** No change to verification behavior.

### Component 2 — syncer stall detection + fast re-obtain (`zebrad`)

`ChainSync` gains:
- `stall_restart_timeout: Option<Duration>` (config; default `Some(90s)`).
- `gap_signal: zs::WatchReceiver<Option<block::Height>>` (from Component 1).
- internal tracking: `last_tip_advance: Instant`, `last_stall_restart: Instant`.

**Detection** lives in the `try_to_sync_once` pause loop (sync.rs ~632-649). Today that loop
blocks on a bare `self.downloads.next().await`, which never wakes when all blocks are
verify-parked. Replace with a race:

```rust
tokio::select! {
    response = self.downloads.next() => {
        let response = response.expect("downloads is nonempty");
        let before = self.latest_chain_tip.best_tip_height();
        self.handle_block_response(response)?;
        if self.latest_chain_tip.best_tip_height() > before {
            self.last_tip_advance = Instant::now();   // progress → reset
        }
        self.update_metrics();
    }
    _ = sleep_until(self.last_tip_advance + stall_timeout), if stall_timeout_enabled => {
        if self.is_gap_stalled() {
            return Err(SyncError::Stalled);
        }
        // not a gap stall (complete range verifying slowly): reset and keep waiting,
        // the 8-min BLOCK_VERIFY_TIMEOUT remains the backstop.
        self.last_tip_advance = Instant::now();
    }
}
```

`is_gap_stalled()` is true iff **all** hold:
- state tip has not advanced for `stall_restart_timeout`,
- `in_flight` is still at/over the pause threshold,
- the verifier `gap_signal` is `Some(_)` and has not advanced over the window (a real,
  persistent contiguity gap — not a slowly-committing complete range). "Has not advanced" is
  tracked by snapshotting the `gap_signal` value when the stall deadline is (re)armed and
  comparing it at fire time; any change resets the deadline,
- we did not already stall-restart within `MIN_STALL_RESTART_INTERVAL` (~30s; thrash guard).

**Recovery.** `try_to_sync` propagates `SyncError::Stalled`; the `sync()` loop handles it
distinctly:

```rust
match self.try_to_sync().await {
    Ok(())                  => { sleep(SYNC_RESTART_DELAY).await; }          // 67s, unchanged
    Err(SyncError::Stalled) => { self.downloads.cancel_all();
                                 self.last_stall_restart = Instant::now();
                                 sleep(STALL_RESTART_DELAY).await; }          // ~5s
    Err(SyncError::Other(e))=> { warn!(?e, ...); self.downloads.cancel_all();
                                 sleep(SYNC_RESTART_DELAY).await; }           // 67s, unchanged
}
```

`cancel_all()` empties the pipeline; the next `obtain_tips()` rebuilds the locator from the
committed state tip and re-walks bottom-up, downloading the low gap blocks before
re-saturation — the same mechanism as a manual bounce, ~8 minutes faster.

### Component 3 — WARN on stall (byproduct)

At the moment `is_gap_stalled()` returns true, emit a single rate-limited `WARN`:

```
WARN syncer stalled on checkpoint gap; restarting
     gap_height=<contiguous+1> state_tip=<h> in_flight=<n> lookahead_limit=<l>
     stalled_for=<secs>
```

This replaces the silent multi-minute wedge with one actionable line.

## Config

```rust
/// How long the syncer waits for the state tip to advance while the download
/// queue is saturated before assuming a checkpoint-contiguity stall and
/// restarting from the current tip. `None` disables fast restart (legacy
/// behavior: recover only via the 8-minute verify timeout).
#[serde(default = "default_stall_restart_timeout", with = "humantime_serde")]
pub stall_restart_timeout: Option<Duration>,   // default Some(90s)
```

Constraint (validated at startup, clamp + warn): `stall_restart_timeout < BLOCK_VERIFY_TIMEOUT`.
Settable via `ZEBRA_SYNC__STALL_RESTART_TIMEOUT` (scalar/humantime — env-var safe; the plan
must confirm the `SYNC` env segment matches how zebrad derives env names for the `config.sync`
section). The field lives in the sync `Config` struct (sync.rs ~229-274), which uses
`#[serde(deny_unknown_fields, default)]`; adding a field there is forward-compatible.

New constants in sync.rs: `STALL_RESTART_DELAY` (~5s), `MIN_STALL_RESTART_INTERVAL` (~30s).

## Error handling / edge cases

- **Disabled (`None`):** the `select!` stall arm is gated off → behavior byte-identical to
  today.
- **Slow-but-progressing verification:** tip advances → deadline resets → no trigger. If tip
  is frozen but `gap_signal == None` (complete range committing slowly), we do **not**
  fast-restart; the 8-min backstop still applies.
- **Thrash:** `MIN_STALL_RESTART_INTERVAL` prevents tight restart loops when a region
  genuinely has no peer serving the block; falls back to the normal 67s path.
- **Backstop intact:** outer `timeout(BLOCK_VERIFY_TIMEOUT, try_to_sync_once)` unchanged.

## Testing (TDD)

Write failing tests first:

1. **Syncer stall → fast restart** (`zebrad/src/components/sync/tests/`): mock services that
   saturate `in_flight`, hold the verifier in `WaitingForBlocks` (gap_signal `Some`), and
   freeze the state tip. Assert `try_to_sync` returns `SyncError::Stalled` and `sync()`
   re-enters `obtain_tips` within ~`stall_restart_timeout` (use paused tokio time), not 8 min.
2. **No false trigger on slow-but-progressing verify:** tip advancing (or gap_signal `None`)
   → no `Stalled`.
3. **Verifier gap signal** (`zebra-consensus`): drive `CheckpointVerifier` directly via
   `Service::call` (constructed with `new`/`from_checkpoint_list`, no router/Buffer stack
   needed); feed a contiguous prefix then a gap; assert the watch publishes
   `Some(contiguous_height)`; feed the missing block; assert it returns to `None` and the
   range commits.
4. **Disabled path:** `stall_restart_timeout = None` reproduces current behavior.
5. Full existing sync + consensus suites stay green (no `--workspace`; per-crate `--lib`).

## Risks & scope

- Control-flow change in the syncer pause loop and `sync()` restart scheduling, plus a new
  `SyncError` discriminant. Moderate, well-bounded.
- Cross-crate plumbing of the watch receiver out of `zebra-consensus`. The only API surface
  change; no behavioral change to verification.
- Config addition (backward compatible via `#[serde(default)]`).
- **No** change to how blocks are validated or committed — the consensus-critical path is
  untouched.

## Decisions

- **On by default**, `stall_restart_timeout = Some(90s)` — the change must actually fix #5709
  for users, not sit behind an undiscovered knob. (Requires maintainer buy-in on changed
  default sync behavior.)
- **Full watch plumbing** — needed to distinguish gap stalls from slow verification, avoiding
  the "fast restart wastes legitimate progress" failure mode.

## Contribution status

Stays local pending a Zebra maintainer acknowledging this approach on #5709 (contributor is
not a maintainer). AI-assisted (Claude Code); to be disclosed in any PR.
