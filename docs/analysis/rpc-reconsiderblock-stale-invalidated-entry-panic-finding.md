# RPC reconsiderblock Stale Invalidation Panic Finding

Date: 2026-05-04

## Summary

`reconsiderblock` can leave the successfully reconsidered invalidation record in
`NonFinalizedState::invalidated_blocks`, because it removes the record from a
cloned `IndexMap` instead of the live map. A second `reconsiderblock` for the
same block hash can then replay the same invalidated blocks into an already
restored chain and panic in `Chain::cmp`'s duplicate-tip invariant.

Zebra's dev and release binary profiles use `panic = "abort"`, so this is
process-fatal in `zebrad`.

## Preconditions

- JSON-RPC is enabled and the caller can use the trusted/control RPC methods
  `invalidateblock` and `reconsiderblock`.
- The node is past checkpoint-only processing and has a non-finalized state.
- A non-finalized block with descendants has been invalidated, then
  reconsidered successfully.
- The caller sends `reconsiderblock` for that same invalidated root hash again.

This is not a consensus failure and not unauthenticated P2P. It is an
authenticated/trusted-RPC availability issue, and becomes remote if RPC is
exposed or credentials are compromised.

## Evidence

- `zebra-rpc/src/methods.rs:2930-2943` parses the caller-supplied hash and
  sends `Request::ReconsiderBlock` to state.
- `zebra-state/src/service.rs:1217-1235` forwards `ReconsiderBlock` through the
  non-finalized write task and maps the result to `Response::Reconsidered`.
- `zebra-state/src/service/write.rs:362-366` handles
  `NonFinalizedWriteMessage::Reconsider` by calling
  `non_finalized_state.reconsider_block(hash, &finalized_state.db)`.
- `zebra-state/src/service/non_finalized_state.rs:426-437` locates an
  invalidated entry by scanning `self.invalidated_blocks`.
- `zebra-state/src/service/non_finalized_state.rs:439-444` then calls
  `self.invalidated_blocks.clone().shift_remove(height)`, which removes the
  record from a temporary clone rather than from the live
  `self.invalidated_blocks`.
- `zebra-state/src/service/non_finalized_state.rs:481-486` replays the saved
  invalidated blocks with `Chain::push(...).expect(...)`.
- `zebra-state/src/service/non_finalized_state.rs:497-499` reinserts the
  restored chain and filters by `root_parent_hash`, not by the restored tip
  hash.
- `zebra-state/src/service/non_finalized_state/chain.rs:2320-2345` implements
  `Ord for Chain`; equal tip hashes reach
  `unreachable!("Chain tip block hashes are always unique")`.

Because the stale invalidation record remains after the first successful
reconsider, the second reconsider can reconstruct the same chain suffix and try
to insert a chain whose tip hash is already present.

## Local Reproduction

Added a durable current-behavior unit test:
`zebra-state/src/service/non_finalized_state/tests/vectors.rs:473`.

The test shape:

1. Create `block1 -> block2 -> block3`.
2. Commit all three blocks into `NonFinalizedState`.
3. Call `state.invalidate_block(block2.hash())`.
4. Call `state.reconsider_block(block2.hash(), &finalized_state.db)` once.
5. Assert that `state.invalidated_blocks()` still contains an entry whose first
   hash is `block2.hash()`.
6. Call `state.reconsider_block(block2.hash(), &finalized_state.db)` again.

Focused command:

```sh
cargo test -p zebra-state reconsider_block_twice_replays_stale_invalidated_entry_today --lib
```

Result:

```text
test service::non_finalized_state::tests::vectors::reconsider_block_twice_replays_stale_invalidated_entry_today - should panic ... ok
```

The broader vector module also passes with the current-behavior proof included:

```sh
cargo test -p zebra-state service::non_finalized_state::tests::vectors --lib
```

That run completed with `18 passed`.

## Impact

An authenticated RPC caller can abort Zebra by repeating `reconsiderblock` after
a successful invalidate/reconsider cycle. The attacker does not need a special
same-height fork shape for this variant; a single invalidated non-finalized
block with descendants is enough.

The practical severity is limited by the trusted-RPC precondition, but the bug
is stronger than ordinary hardening because it is a confirmed process-fatal
panic in an exposed state-control RPC path.

## Suggested Fix Direction

- Remove the invalidation record from the live `self.invalidated_blocks`, not a
  clone.
- Keep the record live until replay has succeeded, then remove it atomically
  with restored-chain insertion.
- Replace `Chain::push(...).expect(...)` with a typed `ReconsiderError` so
  unexpected replay failure cannot panic the process.
- Add a regression test that `reconsider_block()` removes the live invalidated
  entry and a second reconsider returns `MissingInvalidatedBlock` instead of
  panicking.

## Disclosure Triage

Recommended private maintainer heads-up.

This is not a consensus divergence and not an unauthenticated P2P issue. But it
is a confirmed process-fatal availability bug reachable through a trusted RPC
method, and the trigger is simpler than the same-height fork invalidation panic.

Confidence: high for the stale live-entry bug and second-reconsider panic;
medium for practical exploitability because it requires RPC access and an
operator-style invalidation/reconsider sequence.
