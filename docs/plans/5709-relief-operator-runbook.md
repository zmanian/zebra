# #5709 relief test — operator runbook

**Branch:** `exp/5709-retry-missing-blocks` (on `zmanian/zebra` fork)
**What it is:** the full #5709 fix stack **plus** the inventory de-route relief.

This is the branch that should make the cloud node clear "stuck blocks" in
seconds instead of ~90s, and keep the frontier advancing without manual
restarts.

---

## What changed vs. the last branch you ran

The previous branch (`exp/5709-bound-download-permit-wait`) advanced, but slowly:
each "poisoned" block sat un-routable for up to ~1.5 min before clearing. We
found why.

**Root cause:** when a peer returns `NotFound` for a block (or a connection
drops mid-request), Zebra marks that peer "missing" the block in its inventory
registry, and the router (`route_inv`) then avoids that peer. The mark lasts up
to ~2× the rotation interval (~106s). Under load, *every* connected peer ends up
marked missing the same block, so the router fails the request instantly with a
synthetic `NotFoundRegistry` error — even though the block provably exists on the
canonical chain. Checkpoint verification needs a contiguous run, so one
un-routable block freezes the whole frontier until the marks rotate out.

**The relief (this branch):** for **block** requests only, when all ready peers
are marked missing, `route_inv` now retries one peer anyway instead of failing.
Transaction requests are unchanged (they still fail with `NotFoundRegistry`,
preserving DoS protection).

This sits on top of the four earlier fixes already in your last branch:
1. contiguity-gap stall detector + fast-restart (config-gated, default 90s)
2. don't restart the whole syncer on a single block download failure
3. don't poison inventory routing when a connection drops mid-request
4. bound total block-download time including the concurrency-permit wait

---

## Build & run

```bash
cd <zebra-worktree>
git fetch origin           # 'origin' = your zmanian/zebra fork
git checkout exp/5709-retry-missing-blocks
git pull

# Release build (same as before)
cargo build --release --bin zebrad

# Run against the SAME data dir / config you've been using
./target/release/zebrad -c <your-zebrad.toml> start
```

No config changes are required. The defaults are:
- `sync.stall_restart_timeout = "90s"` (set `"0s"` to disable the fast-restart
  safety net if you want to isolate the relief's effect)

---

## What to watch

The relief branch has **no `dbg5709` instrumentation** — it's a clean branch.
Observe via the normal signals:

**1. Frontier height — should advance steadily, no long flat spots.**
- Metric: `checkpoint.verified.height` (Prometheus), or just watch the
  `verified` height in the INFO sync-progress logs.
- Expectation: no multi-minute flat spots. Previously a poisoned block held the
  frontier flat for ~60–106s; now it should clear in **seconds**.

**2. The retry path firing (optional, debug-level).**
If you want to *see* the relief working, raise the log level for the peer-set
target:
```bash
RUST_LOG="info,zebra_network::peer_set::set=debug" ./target/release/zebrad -c <cfg> start
```
Look for:
```
all ready peers marked missing this block; retrying one anyway (#5709)
```
Each line is one block that previously would have stalled the frontier and is
now being retried immediately. A steady trickle is normal and healthy under
load; what matters is the frontier keeps moving.

**3. Stall fast-restarts — should become rare or stop.**
- Look for WARN logs mentioning a contiguity-gap stall / fast-restart.
- With the relief, the underlying gap should resolve on its own, so these should
  fire much less often (ideally not at all). If they're still firing every ~90s,
  capture the logs — that means a *different* gap source is in play.

---

## Success criteria

- [ ] Frontier (`checkpoint.verified.height`) advances past the heights where it
      previously stalled (~1.43M, ~1.73M) without manual intervention.
- [ ] No multi-minute flat spots in verified height.
- [ ] `retrying one anyway (#5709)` appears (if you enabled debug) and the
      frontier keeps moving right after each one.
- [ ] Sync reaches the network tip, or runs for a multi-hour window with steady
      progress and no stall-restart loop.

## If it still stalls

Capture and send back:
1. `verified` height over time (the flat spot's start/end heights + wall-clock
   duration).
2. Peer count at the stall (`zcash.net.peers` or the INFO logs).
3. INFO logs for ~2 min around the stall, plus
   `zebra_network::peer_set::set=debug` if you can.
4. Whether `stall_restart_timeout` was at `90s` or `0s`.

That tells us whether any block is still going un-routable (relief not enough,
needs the rotation-interval change too) or whether a *new* gap source has
appeared.
