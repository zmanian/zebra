# Anchor and Nullifier Reorg A3 Revisit Note

Date: 2026-05-03

Scope: pass-5 Workstream A3 follow-up on Sapling/Orchard anchors and
Sprout/Sapling/Orchard nullifier membership under non-finalized forks, root
finalization, and proposal/mempool read paths.

## Result

No fresh private-disclosure consensus issue was confirmed in this pass.

The core A3 invariant appears sound in the reviewed code: non-finalized chain
updates add anchors, note commitment trees, and nullifiers when a block is
pushed, and inverse operations remove the same data when a tip is popped for a
fork or a root is popped for finalization. Mempool and proposal validation read
from the published read-state snapshot plus finalized DB, not from the
writer-private post-finalize/pre-commit intermediate state.

The main residual gap is test coverage: Sapling anchor behavior has a direct
test, nullifier behavior covers all three shielded pools, but the current anchor
test module still has a TODO for direct Orchard anchor coverage.

One API-contract nuance remains: proposal validation reuses the real
non-finalized validation code on a cloned read-time snapshot. That is good for
validation parity, but it is not a reservation of the checked parent/tip. A
proposal can be valid when checked and fail later if the live write state has
advanced or reorged before submission, or it can become a side-chain block
rather than the best-chain block the caller expected. That is not an
anchor/nullifier corruption issue, but it should be documented for mining/RPC
callers.

## Evidence

- `zebra-state/src/service/check/anchors.rs:49-69` checks Sapling anchors
  against the non-finalized chain snapshot and finalized DB.
- `zebra-state/src/service/check/anchors.rs:91-114` applies the same
  non-finalized-or-finalized lookup rule to the Orchard shared anchor.
- `zebra-state/src/service/check/nullifier.rs:69-88` rejects a nullifier if it
  appears in either the non-finalized chain or finalized chain.
- `zebra-state/src/service/check/nullifier.rs:103-129` applies that duplicate
  nullifier check to Sprout, Sapling, and Orchard nullifiers.
- `zebra-state/src/service/check/nullifier.rs:164-181` inserts non-finalized
  nullifiers while rejecting duplicates in the active chain.
- `zebra-state/src/service/check/nullifier.rs:212-225` removes the nullifiers
  that were added by a reverted block and asserts the add/remove invariant.
- `zebra-state/src/service/non_finalized_state/chain.rs:280-282` initializes a
  new non-finalized chain with the finalized-tip Sprout, Sapling, and Orchard
  trees/anchors.
- `zebra-state/src/service/non_finalized_state/chain.rs:400-416` implements
  fork creation by cloning the chain and repeatedly popping tips above the fork
  point.
- `zebra-state/src/service/non_finalized_state/chain.rs:791-838` and
  `zebra-state/src/service/non_finalized_state/chain.rs:852-908` add and remove
  Sapling trees/anchors.
- `zebra-state/src/service/non_finalized_state/chain.rs:991-1042` and
  `zebra-state/src/service/non_finalized_state/chain.rs:1057-1113` add and
  remove Orchard trees/anchors using the same structure.
- `zebra-state/src/service/non_finalized_state/chain.rs:1460-1486` updates the
  note commitment trees and inserts Sapling/Orchard subtrees after the parallel
  tree update succeeds.
- `zebra-state/src/service/non_finalized_state/chain.rs:1628-1634` adds
  Sprout, Sapling, and Orchard shielded data during block insertion.
- `zebra-state/src/service/non_finalized_state/chain.rs:1805-1825` removes the
  same shielded data and trees during block revert.
- `zebra-state/src/service/non_finalized_state/chain.rs:2074-2105`,
  `zebra-state/src/service/non_finalized_state/chain.rs:2129-2159`, and
  `zebra-state/src/service/non_finalized_state/chain.rs:2177-2207` wire the
  add/remove operations for Sprout, Sapling, and Orchard nullifiers.
- `zebra-state/src/service/non_finalized_state/tests/prop.rs:623-647`
  explicitly includes note commitment trees, anchors, and nullifier sets in the
  forked-vs-pushed internal-state property check.
- `zebra-state/src/service/check/tests/anchors.rs:193-341` directly tests
  Sapling anchor rejection before priming and acceptance after priming.
- `zebra-state/src/service/check/tests/anchors.rs:343` still records the direct
  Orchard anchor test as TODO.
- `zebra-state/src/service.rs:1655-1692` validates block proposals against a
  cloned latest non-finalized state and drops that clone after returning the
  result.

The companion finalization-window note covers the read-order concern:
`docs/analysis/shielded-reorg-finalization-read-order-note.md`.

## Local Verification

Targeted tests passed:

```sh
cargo test -p zebra-state service::check::tests::anchors --lib
cargo test -p zebra-state service::check::tests::nullifier --lib
cargo test -p zebra-state service::non_finalized_state::tests::prop::forked_equals_pushed --lib
```

Observed results:

- anchor tests: 2 passed;
- nullifier tests: 13 passed;
- non-finalized fork property tests: 2 passed.

## Classification

Eliminated as a fresh private vulnerability in this pass.

Recommended public hardening:

- add direct Orchard anchor tests mirroring the Sapling anchor test;
- add an explicit two-branch shielded reorg regression where an anchor exists
  only on the discarded branch and a nullifier spent on the discarded branch is
  spendable on the replacement branch;
- document `CheckBlockProposalValidity` as snapshot-based/advisory, not a
  guarantee that the same block will later commit to the same live state;
- keep the finalization-window regression separate, because that tests
  read-state publication rather than fork add/remove symmetry.
