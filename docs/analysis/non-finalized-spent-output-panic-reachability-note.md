# Non-Finalized Spent Output Panic Reachability Note

Date: 2026-05-03

## Summary

The non-finalized state has internal panic guards for a `ContextuallyVerifiedBlock`
whose transparent inputs are not represented in its `spent_outputs` map. Those
panics do not appear attacker-reachable through ordinary peer block sync,
`submitblock`, `getblocktemplate` proposal validation, or the current
`TrustedChainSync` path.

This is an eliminated private remote DoS lead. The adjacent trusted-indexer
sync boundary remains public hardening, but it does not bypass the spent-output
builder that protects this specific invariant.

## Panic Preconditions

The forward panic is in transparent input indexing:

- `zebra-state/src/service/non_finalized_state/chain.rs:1946-1983`

For a non-test build, it requires all of:

- a transparent input with a non-null outpoint,
- `Chain::update_chain_tip_with()` has already inserted that outpoint into
  `self.spent_utxos`,
- `ContextuallyVerifiedBlock.spent_outputs` lacks the same outpoint.

The revert panic has the same shape while undoing a previously pushed block:

- `zebra-state/src/service/non_finalized_state/chain.rs:2003-2035`

That makes the panic an internal invariant failure: a block is being applied or
reverted with a malformed contextual spent-output map.

## Normal Block and RPC Paths

The normal state-service path prevents this by constructing the spent-output map
before any `Chain` mutation:

- `zebra-state/src/service/write.rs:55-69` runs
  `validate_and_commit_non_finalized()`.
- `zebra-state/src/service/non_finalized_state.rs:551-607` calls
  `check::utxo::transparent_spend()` before building the contextual block.
- `zebra-state/src/service/check/utxo.rs:38-97` walks every transparent input
  and either returns all referenced UTXOs or errors before commit.
- `zebra-state/src/service/check/utxo.rs:126-173` turns missing, duplicate, or
  early transparent spends into typed validation errors.
- `zebra-state/src/request.rs:473-513` builds `ContextuallyVerifiedBlock` and
  extends `spent_outputs` with same-block `new_outputs`, so earlier same-block
  spends are present for `Chain` indexing.

`submitblock` and ordinary peer block verification enter this same path through
the consensus router. `getblocktemplate` proposal validation also uses the same
contextual validation helper on a cloned non-finalized state:

- `zebra-state/src/service.rs:1655-1692`

## TrustedChainSync

`TrustedChainSync` still bypasses the normal `initial_contextual_validity()` gate
when importing streamed indexer blocks:

- `zebra-rpc/src/sync.rs:174-186`
- `zebra-rpc/src/sync.rs:215-225`

But `try_commit()` still calls `NonFinalizedState::commit_new_chain()` or
`commit_block()`, and both converge on `validate_and_commit()`, which still calls
`check::utxo::transparent_spend()` before `ContextuallyVerifiedBlock` creation.
So this path can preserve the already documented trusted-indexer hash/context
concerns without making this missing-spent-output panic remotely reachable.

The hash/body mismatch noted in `BlockAndHash::decode()` is likewise adjacent,
not a route to this panic:

- `zebra-rpc/src/indexer.rs:60-78`
- `zebra-state/src/request.rs:539-556`

The streamed hash may poison mirror indexing if the endpoint is malicious, but
the block body's transaction hashes and ordered outputs are still recomputed
from the deserialized block body before spent-output lookup.

## Verification

Existing targeted UTXO tests cover the failure modes that prevent this panic:

```sh
cargo test -p zebra-state service::check::tests::utxo --lib
```

Result: 11 passed, including:

- `reject_missing_transparent_spend`
- `reject_earlier_transparent_spend_from_this_block`
- `reject_duplicate_transparent_spend_in_same_chain_from_previous_block`
- `reject_duplicate_transparent_spend_in_same_block_from_previous_block`

RepoPrompt builder pass `panic-reachability-audit-E5C550` reached the same
classification: ordinary P2P/RPC paths are protected, `TrustedChainSync` still
uses the spent-output builder, and the lead is eliminated for private remote
DoS.

## Triage

Classification: eliminated as a private remote DoS lead.

Residual public hardening:

- Add an explicit `TrustedChainSync` invalid-transparent-spend regression proving
  a malicious stream returns a contextual validation error rather than panicking.
- Add an internal-only test proving the panic requires a deliberately malformed
  `ContextuallyVerifiedBlock`.
- Keep the trusted-indexer hash/body validation issue tracked separately in
  `docs/analysis/trusted-chain-sync-indexer-validation-boundary-note.md`.
