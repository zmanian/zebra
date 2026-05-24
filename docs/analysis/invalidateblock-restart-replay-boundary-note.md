# InvalidateBlock Restart Replay Boundary Note

Date: 2026-05-09

Status: local-only audit note. Do not post publicly without explicit user
direction.

## Summary

`invalidateblock` invalidations are held in `NonFinalizedState` memory, not in a
durable denylist. After a restart, Zebra can restore or re-receive a
previously-invalidated non-finalized block because the new
`NonFinalizedState` starts with an empty `invalidated_blocks` map.

This looks like a weak operational quarantine boundary rather than a clean
remote vulnerability: `invalidateblock` is trusted RPC control-plane behavior,
and it is unclear whether Zebra intends invalidations to survive restart.

## Source Evidence

- `zebra-state/src/service/non_finalized_state.rs:59` stores invalidated blocks
  in the `NonFinalizedState` struct.
- `zebra-state/src/service/non_finalized_state.rs:123` initializes
  `invalidated_blocks` with `Default::default()` for a new non-finalized state.
- `zebra-state/src/service/non_finalized_state.rs:398` through `:408` inserts
  invalidated records into that in-memory map and bounds the map by
  `MAX_INVALIDATED_BLOCKS`.
- `zebra-state/src/service/non_finalized_state.rs:558` through `:562` rejects
  a candidate block only if its hash is present in the current in-memory
  invalidated-block map.
- `zebra-state/src/service/non_finalized_state.rs:195` restores backup blocks
  into a freshly constructed non-finalized state.
- `zebra-state/src/service/non_finalized_state/backup.rs:34` through `:59`
  reads backup blocks and commits them; it does not load any durable
  invalidation state.
- `zebra-state/src/service/non_finalized_state/backup.rs:83` through `:99`
  eventually deletes backup files no longer present in non-finalized state, but
  `zebra-state/src/service/non_finalized_state/backup.rs:115` rate-limits backup
  updates by `MIN_DURATION_BETWEEN_BACKUP_UPDATES`.

So there are two replay routes after an operator invalidates a non-finalized
block:

- crash or restart before the backup task has removed the now-invalidated block
  file, then backup restore can commit it into the fresh state;
- restart after invalidation, then receive the same block again from peers after
  the fresh invalidated-block map starts empty.

## Current-Behavior Proof

Added focused state-level proofs for both replay routes:

- `zebra-state/src/service/non_finalized_state/tests/vectors.rs` now has
  `fresh_non_finalized_state_forgets_invalidated_block_today`.
- The test commits a two-block non-finalized chain, invalidates the second
  block, and confirms the live state rejects the same block with
  `BlockPreviouslyInvalidated`.
- It then constructs a fresh `NonFinalizedState`, commits the same parent, and
  confirms the previously invalidated child is accepted because the invalidation
  map is empty in the fresh state.
- `zebra-state/src/service/non_finalized_state/tests/vectors.rs` now also has
  `backup_restore_replays_invalidated_block_today`.
- That test writes the non-finalized chain to the backup cache, invalidates the
  child in memory without updating the backup, then restores from the stale
  backup into a fresh `NonFinalizedState` and confirms the invalidated child is
  present again.

Verification:

```sh
cargo test -p zebra-state fresh_non_finalized_state_forgets_invalidated_block_today --lib
cargo test -p zebra-state backup_restore_replays_invalidated_block_today --lib
```

Result on 2026-05-09: both passed.

## Impact

An operator who uses `invalidateblock` to quarantine a non-finalized block can
lose that quarantine across restart. A previously-invalidated block can become
eligible for normal contextual validation again, and if it is otherwise valid
against the current state it can be reintroduced.

This does not give an unauthenticated peer a direct new capability by itself:
the initial invalidation action is trusted RPC, and the reintroduced block must
still pass normal validation. The concern is operational correctness and
compatibility expectations for `invalidateblock`, especially during incident
response when operators may expect an invalidated hash to remain denied until
explicit `reconsiderblock`.

## Duplicate Check

Public issue searches returned zero results for:

- `repo:ZcashFoundation/zebra invalidated_blocks restart`
- `repo:ZcashFoundation/zebra invalidateblock backup replay`
- `repo:ZcashFoundation/zebra BlockPreviouslyInvalidated restart`
- `repo:ZcashFoundation/zebra invalidateblock persistent`

Local docs already cover several nearby `invalidateblock` and `reconsiderblock`
issues:

- process-fatal trusted-RPC panics;
- stale invalidated entries after `reconsiderblock`;
- same-height invalidated-record keying;
- finalized-height invalidated-record retention.

Those notes do not appear to record the restart durability boundary directly.

## Recommended Fix

- Decide whether Zebra's `invalidateblock` is intended to be durable across
  restart. If not, document the volatility explicitly in RPC/user docs and
  incident-response notes.
- If durability is intended, persist a small bounded invalidated-block denylist
  in finalized-state disk format or a separate state file, and remove entries
  only on explicit `reconsiderblock`, finalization beyond the relevant horizon,
  or bounded retention expiry.
- Make backup restore check the persisted denylist before committing restored
  non-finalized blocks.

## Confidence

High confidence in the re-receive behavior because the focused test confirms
the invalidation map is memory-only across fresh state construction. Medium
confidence in the backup replay route from source ordering. Low to medium
confidence in security severity because the issue depends on trusted RPC
semantics and may be an undocumented product decision rather than a vulnerability.
