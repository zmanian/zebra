# Shielded Reorg Finalization Read-Order Note

Date: 2026-05-03

Last updated: 2026-05-09

Disposition: local-only eliminated lead. Do not post publicly without explicit
re-authorization.

Scope: pass-5 Workstream A3 follow-up on Sapling/Orchard anchor and nullifier
consistency under non-finalized reorgs and finalization-adjacent reads.

## Result

The suspected finalization publication race is eliminated for the current read
paths reviewed here.

The scary shape was:

1. `NonFinalizedState::finalize()` removes the finalized root block from the
   writer-owned non-finalized state.
2. `FinalizedState::commit_finalized_direct()` has not yet written that root
   block's anchors, nullifiers, and note trees to RocksDB.
3. A concurrent mempool or proposal validation request observes that
   post-finalize in-memory state plus pre-commit finalized DB, making the root
   block temporarily disappear from both sources.

That window is not exposed to these read requests. The read-only state service
uses a `watch::Receiver<NonFinalizedState>` snapshot. The writer sends the
snapshot before finalization mutates the writer-owned state, and does not publish
the post-finalize state until a later channel update. Concurrent readers
therefore see either the pre-finalization snapshot, or a later fully published
state, not the private post-finalize/pre-DB intermediate state.

## Evidence

- `zebra-state/src/service.rs:1239-1265` redirects
  `CheckBestChainTipNullifiersAndAnchors` and `CheckBlockProposalValidity`
  requests to `ReadStateService`.
- `zebra-state/src/service.rs:923-932` shows `ReadStateService` reads the latest
  non-finalized state from a `watch::Receiver`, returning cloned watch data or a
  borrowed best-chain snapshot.
- `zebra-state/src/service/write.rs:97-123` sends
  `non_finalized_state.clone()` through the watch channel in
  `update_latest_chain_channels()`.
- `zebra-state/src/service/write.rs:428-450` publishes the non-finalized
  snapshot before entering the finalization loop, then calls
  `non_finalized_state.finalize()` and `commit_finalized_direct()` on the
  writer-owned state.
- `zebra-state/src/service.rs:1573-1588` performs mempool best-tip nullifier and
  anchor checks against the watch snapshot plus the finalized DB.
- `zebra-state/src/service.rs:1655-1691` validates block proposals by cloning
  the watch snapshot, mutating only that clone, and dropping it after the
  request.
- `zebra-state/src/service/check/nullifier.rs:103-129` rejects a nullifier if it
  exists in either the non-finalized chain snapshot or finalized DB.
- `zebra-state/src/service/check/anchors.rs:24-124` accepts Sapling/Orchard
  anchors if they exist in either the non-finalized chain snapshot or finalized
  DB.

## Residual Behavior

There can be overlap between a published pre-finalization non-finalized snapshot
and a finalized DB that has already committed the same root block. For the
reviewed anchor and nullifier checks, that overlap is conservative:

- duplicate nullifier checks reject if either source contains the nullifier;
- anchor checks accept if either source contains the anchor.

Proposal validation can also be based on a stale snapshot while the real writer
is finalizing, but proposal acceptance is not reused as commit authority. A later
real block submission goes through full validation and commit again.

## Local Verification

Targeted existing tests were rerun on 2026-05-09 for the underlying A3
invariants:

```sh
cargo test -p zebra-state service::check::tests::anchors --lib
cargo test -p zebra-state service::check::tests::nullifier --lib
cargo test -p zebra-state service::non_finalized_state::tests::prop::forked_equals_pushed --lib
```

Observed result: all selected tests passed: 2 anchor tests, 13 nullifier tests,
and 2 non-finalized fork/finalization property tests.

## Classification

Eliminated as a private vulnerability. A useful local regression-test
improvement would be a direct Orchard anchor test and a finalization-window test
that asserts read requests use the published pre-finalization snapshot rather
than writer-private post-finalize state.

## Duplicate Check

Read-only duplicate search refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra shielded reorg finalization read order anchors nullifiers'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "CheckBestChainTipNullifiersAndAnchors"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "CheckBlockProposalValidity" "anchors"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "non-finalized" "watch" "finalization" "nullifiers"'
```

No direct issue hits were returned for the read-order/finalization race shape.
The closest historical hit is closed #5716, which added best-chain mempool
contextual validation for anchors and nullifiers. That is adjacent coverage, not
an unresolved duplicate of this eliminated read-snapshot lead.
