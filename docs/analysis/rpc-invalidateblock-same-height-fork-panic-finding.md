# RPC invalidateblock Same-Height Fork Panic Finding

Date: 2026-05-03

## Summary

`invalidateblock` can panic the state write task when it is called on two
competing non-finalized fork tips at the same height that share the same parent.

The first invalidation removes one fork tip and inserts the shared parent-only
chain. The second invalidation removes the sibling fork tip and tries to insert
that same parent-only chain again. Insertion happens before the chain-set
filter runs, so the `BTreeSet<Arc<Chain>>` compares two chains with the same
tip hash and reaches `unreachable!("Chain tip block hashes are always unique")`.

Zebra's dev and release profiles use `panic = "abort"`, so in the binary this
is process-fatal.

## Preconditions

- JSON-RPC is enabled and the caller can use the trusted/control RPC method
  `invalidateblock`.
- The node is past checkpoint-only processing and has a non-finalized state.
- The non-finalized state contains two competing valid fork tips at the same
  height with the same parent.

This is not peer-only and not consensus-invalid acceptance. It is an
authenticated/trusted-RPC availability issue. It becomes remote if RPC is
exposed or credentials are compromised.

## Evidence

- `zebra-rpc/src/methods.rs:2917-2927` parses the caller-supplied block hash
  and sends `Request::InvalidateBlock` to state.
- `zebra-state/src/service.rs:829-845` forwards invalidation requests to the
  non-finalized block write task.
- `zebra-state/src/service/write.rs:354-360` handles
  `NonFinalizedWriteMessage::Invalidate` by calling
  `non_finalized_state.invalidate_block(hash)`.
- `zebra-state/src/service/non_finalized_state/chain.rs:379-388` returns a new
  chain without the invalidated block and its descendants. For a fork tip, that
  new chain is the shared parent chain.
- `zebra-state/src/service/non_finalized_state.rs:388-392` inserts that new
  chain with `insert_with(...)`, then filters out chains containing the
  invalidated hash.
- `zebra-state/src/service/non_finalized_state.rs:257-265` implements
  `insert_with(...)` by calling `self.chain_set.insert(chain)` before applying
  the provided filter.
- `zebra-state/src/service/non_finalized_state/chain.rs:2320-2345` implements
  `Ord for Chain`; when cumulative work ties, it compares tip hashes and treats
  equal tip hashes as unreachable.
- `zebra-state/src/service/non_finalized_state.rs:397-404` also documents the
  adjacent same-height invalidation limitation: invalidated records are keyed
  only by height, not by hash.

## Local Reproduction

Added a durable current-behavior unit test:
`zebra-state/src/service/non_finalized_state/tests/vectors.rs:434`.

The test shape:

1. Create `block1`.
2. Create two different fake children of `block1` at the same height:
   `block2a = block1.make_fake_child().set_work(10)` and
   `block2b = block1.make_fake_child().set_work(11)`.
3. Commit `block1`, `block2a`, and `block2b` into `NonFinalizedState`.
4. Call `state.invalidate_block(block2a.hash())`.
5. Call `state.invalidate_block(block2b.hash())`.

Focused command:

```sh
cargo test -p zebra-state invalidating_same_height_fork_tips_panics_today --lib
```

Result:

```text
test service::non_finalized_state::tests::vectors::invalidating_same_height_fork_tips_panics_today - should panic ... ok
```

The broader vector module also passes with the current-behavior proof included:

```sh
cargo test -p zebra-state service::non_finalized_state::tests::vectors --lib
```

That run completed with `18 passed`.

## Impact

An authenticated RPC caller can abort Zebra if the node's non-finalized state
contains two sibling fork tips and the caller invalidates both siblings in
sequence.

This is plausibly reachable in normal operation because non-finalized state is
designed to hold competing forks. The attacker does not need to construct an
invalid block; they only need to know or create two valid same-parent fork tips
that Zebra has accepted into non-finalized state, then call the trusted RPC.

Severity is lower than a peer-only unauthenticated DoS because `invalidateblock`
is a control-plane RPC. But it is stronger than ordinary hardening because the
failure is a confirmed process-fatal panic in a supported state operation.

## Suggested Fix Direction

- Make invalidation idempotent when the shortened parent chain already exists
  in `chain_set`.
- Avoid inserting into the `BTreeSet` before duplicate/sibling cleanup when the
  candidate chain can have the same tip as an existing chain.
- Do not rely on `Ord::cmp` panicking to enforce uniqueness; reject or merge the
  duplicate chain explicitly before `BTreeSet` comparison can observe it.
- Fix the adjacent invalidated-block cache keying so same-height invalidated
  blocks are tracked by hash or by `(height, hash)`, rather than replacing each
  other by height.
- Add a regression test with two valid sibling fork tips and two sequential
  `invalidate_block()` calls, asserting the second call returns success or a
  typed error without panicking.

## Disclosure Triage

Recommended private maintainer heads-up.

This is not a consensus divergence and not an unauthenticated P2P issue. But it
is a confirmed process-fatal availability bug reachable through an RPC method
that may be operationally exposed in copied deployments or by compromised RPC
credentials.

Confidence: high for the panic condition and API-level reproducer; medium for
practical exploitability because it requires the right non-finalized fork shape
and RPC access.
