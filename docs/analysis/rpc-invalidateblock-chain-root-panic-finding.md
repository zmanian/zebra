# RPC invalidateblock Chain-Root Panic Finding

Date: 2026-05-04

## Summary

`invalidateblock` can panic when the target hash is the non-finalized root of a
tracked chain. In that branch, `NonFinalizedState::invalidate_block()` calls
`self.chain_set.remove(&chain)`. Because `chain_set` is a `BTreeSet<Arc<Chain>>`,
removal compares the lookup key against the stored chain using `Chain::cmp`.
When the key is the same chain, both chains have the same tip hash, and
`Chain::cmp` reaches `unreachable!("Chain tip block hashes are always unique")`.

Zebra's dev and release binary profiles use `panic = "abort"`, so this is
process-fatal in `zebrad`.

## Preconditions

- JSON-RPC is enabled and the caller can use the trusted/control RPC method
  `invalidateblock`.
- The node is past checkpoint-only processing and has a non-finalized state.
- The caller invalidates the root block of any tracked non-finalized chain.

This is not a consensus failure and not unauthenticated P2P. It is an
authenticated/trusted-RPC availability issue, and becomes remote if RPC is
exposed or credentials are compromised.

## Evidence

- `zebra-rpc/src/methods.rs:2917-2927` parses the caller-supplied hash and
  sends `Request::InvalidateBlock` to state.
- `zebra-state/src/service.rs:1195-1212` handles `Request::InvalidateBlock`
  through the state service and maps the result to `Response::Invalidated`.
- `zebra-state/src/service/write.rs:357-360` handles
  `NonFinalizedWriteMessage::Invalidate` by calling
  `non_finalized_state.invalidate_block(hash)`.
- `zebra-state/src/service/non_finalized_state.rs:376-381` finds a chain
  containing the target hash, then calls `self.chain_set.remove(&chain)` when
  `chain.non_finalized_root_hash() == block_hash`.
- `zebra-state/src/service/non_finalized_state/chain.rs:2320-2345` implements
  `Ord for Chain`; when cumulative work ties, it compares the two tip hashes and
  treats equal tip hashes as unreachable.

For a `BTreeSet::remove(&chain)` lookup against the exact stored chain, equal
tip hashes are expected, not unreachable. The ordering invariant is therefore
unsafe for set lookup/removal paths that compare an existing chain against
itself.

## Local Reproduction

Added a durable current-behavior unit test:
`zebra-state/src/service/non_finalized_state/tests/vectors.rs:402`.

The test shape is:

1. Create `block1 -> block2`.
2. Commit both blocks into `NonFinalizedState`.
3. Call `state.invalidate_block(block1.hash())`, where `block1` is the
   non-finalized root.

Focused command:

```sh
cargo test -p zebra-state invalidating_chain_root_panics_when_removing_existing_chain_today --lib
```

Result:

```text
test service::non_finalized_state::tests::vectors::invalidating_chain_root_panics_when_removing_existing_chain_today - should panic ... ok
```

The broader vector module also passes with the current-behavior proof included:

```sh
cargo test -p zebra-state service::non_finalized_state::tests::vectors --lib
```

That run completed with `18 passed`.

## Impact

An authenticated RPC caller can abort Zebra by calling `invalidateblock` on a
non-finalized chain root. This is a simpler trigger than the same-height sibling
fork panic because it does not require two competing sibling tips.

The exact operational reachability depends on when the node exposes block hashes
for non-finalized roots and whether an RPC caller can target them. In normal
sync, Zebra's non-finalized state intentionally contains recent chain segments,
so the root of a non-finalized chain is a normal state shape.

## Suggested Fix Direction

- Do not call `BTreeSet::remove(&chain)` with a `Chain` ordering that panics on
  equality.
- Remove by rebuilding/filtering the set using a predicate that avoids comparing
  equal-tip `Chain` values, for example retaining chains whose root/tip hash is
  not the target chain's root/tip.
- Alternatively, make `Ord for Chain` total and non-panicking for equal tip
  hashes, then enforce uniqueness explicitly at insertion boundaries.
- Add a regression test that invalidating a non-finalized root returns success
  or a typed error without panicking.

## Disclosure Triage

Recommended private maintainer heads-up.

This is not a consensus divergence and not an unauthenticated P2P issue. But it
is a confirmed process-fatal trusted-RPC availability bug, and the trigger is
simpler than the previously documented same-height fork invalidation panic.

Confidence: high for the panic and API-level reproducer; medium for practical
exploitability because it requires RPC access and a target non-finalized root
hash.
