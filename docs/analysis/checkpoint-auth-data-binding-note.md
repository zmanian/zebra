# Checkpoint Auth-Data Binding Note

Date: 2026-05-04

Last updated: 2026-05-09

Disposition: local-only eliminated lead. Do not post publicly without explicit
re-authorization.

Scope: follow-up on the checkpoint-verifier boundary for NU5/V5 transaction
authorizing data. The question was whether checkpoint-routed blocks can persist
with mutated V5 authorizing data when their transaction IDs and legacy merkle
root still match a checkpoint block hash.

## Summary

Result: eliminated for bad-state persistence in the current code.

The checkpoint verifier itself does not bind NU5 authorizing data before it
queues a block, but checkpoint blocks still pass through finalized-state
commitment validation before `ZebraDb::write_block()`. That validation recomputes
`hashBlockCommitments` from the previous history-tree root and the block's
`auth_data_root()`, then rejects a mismatch.

The residual issue is only a deferred-validation trust boundary: a malformed
same-header/same-txid-merkle-root block can survive checkpoint hash-chain
processing until state commit, where it should fail. This is not a consensus
acceptance or persisted-state corruption vulnerability.

Follow-up proof on 2026-05-09 confirmed the narrower queue behavior: while a
checkpoint range is still incomplete, a newer block body with the same block
hash and transaction mined IDs but different V5 authorizing data replaces the
older queued body. If the range later completes, the replacement body is the one
sent to state, and a state-commit failure resets checkpoint verifier progress.
That is still a deferred-validation/liveness hardening concern, not bad-state
persistence. The normal sync and inbound block downloaders both deduplicate
pending downloads by block hash, so this proof is not currently a high-confidence
remote P2P report by itself.

## Checkpoint Path

`zebra-consensus/src/router.rs:197-215` routes block proposals at or below the
maximum checkpoint height to an immediate error, and normal blocks at or below
that height to the checkpoint verifier.

`zebra-consensus/src/checkpoint.rs:591-632` performs checkpoint `check_block()`
validation: coinbase height, checkpoint height, PoW/difficulty or no-PoW
difficulty threshold, deferred-pool accounting, and transaction merkle-root
validity. It then returns a `CheckpointVerifiedBlock`.

The checkpoint verifier explicitly documents the deferred auth-data boundary in
`zebra-consensus/src/checkpoint.rs:676-683`: duplicate queued blocks with the same
block hash replace the older queued block because signatures, proofs, or scripts
could differ even when the block hash is the same, and the authorizing-data hash
is not checked until checkpoint blocks reach state.

After the checkpoint hash-chain chooses the expected hash, `zebra-consensus/src/checkpoint.rs:1131-1142`
sends the block to state as `Request::CommitCheckpointVerifiedBlock` and awaits a
`Response::Committed`.

## State Commit Check

`zebra-state/src/request.rs:270-274` documents the intended split:
`CheckpointVerifier` does not bind transaction authorizing data to
`ChainHistoryBlockTxAuthCommitmentHash`, but `NonFinalizedState` and
`FinalizedState` do.

The checkpoint commit request is handled in `zebra-state/src/service.rs:1048-1070`,
where it queues the block through `queue_and_commit_to_finalized_state()`.
`zebra-state/src/service.rs:471-511` inserts the queued checkpoint block by parent
hash and drains ready blocks to the finalized write task. `zebra-state/src/service.rs:556-603`
sends ready checkpoint blocks to the finalized write channel.

The write task calls `FinalizedState::commit_finalized()` in
`zebra-state/src/service/write.rs:270-309`. That function immediately delegates
to `commit_finalized_direct()` in `zebra-state/src/service/finalized_state.rs:274-284`.

The critical check is in the checkpoint branch of
`zebra-state/src/service/finalized_state.rs:328-370`: before constructing the
`FinalizedBlock`, it updates note commitment trees and calls
`check::block_commitment_is_valid_for_chain_history(block.clone(), &self.network(), &history_tree)?`.
Only after that branch completes does `zebra-state/src/service/finalized_state.rs:438-443`
call `self.db.write_block(...)`.

The direct database write method is crate-private to the finalized-state service
module (`zebra-state/src/service/finalized_state/zebra_db/block.rs:432-438`), and
the production callers found in the sweep route through `commit_finalized()` or
`commit_finalized_direct()`:

- checkpoint finalized writer: `zebra-state/src/service/write.rs:306-309`
- non-finalized finalization: `zebra-state/src/service/write.rs:439-450`
- tests/helpers only: direct `commit_finalized_direct(...)` calls under
  `zebra-state/src/**/tests/**`

## NU5 Commitment Logic

`zebra-chain/src/block/merkle.rs:15-19` documents that the Bitcoin-inherited
transaction merkle root does not bind V5-onward authorizing data.

The separate auth-data root is defined in `zebra-chain/src/block/merkle.rs:230-239`.
For block contents, `AuthDataRoot` is computed from each transaction's
`auth_digest()`, using the ZIP-244 placeholder for pre-V5 transactions
(`zebra-chain/src/block/merkle.rs:302-324`). `Block::auth_data_root()` exposes
that calculation in `zebra-chain/src/block.rs:246-252`, and
`Transaction::auth_digest()` returns `Some(AuthDigest::from(self))` for V5
transactions in `zebra-chain/src/transaction.rs:274-289`.

For NU5 and later, `zebra-chain/src/block/commitment.rs:82-98` models the header
commitment as `ChainHistoryBlockTxAuthCommitment`. The hash is computed in
`zebra-chain/src/block/commitment.rs:290-320` from the previous history-tree root,
the current block's auth-data root, and the ZIP-244 terminator.

`zebra-state/src/service/check.rs:137-221` validates the block commitment. In the
NU5+ arm, `zebra-state/src/service/check.rs:184-219`:

- obtains the previous history-tree root,
- computes `let auth_data_root = block.auth_data_root();`,
- recomputes `ChainHistoryBlockTxAuthCommitmentHash::from_commitments(...)`,
- returns `InvalidChainHistoryBlockTxAuthCommitment` if the header commitment
  does not match.

## Full Verifier Comparison

The full semantic path reaches the same commitment validator earlier.
`zebra-state/src/service/write.rs:55-69` calls
`validate_and_commit_non_finalized()` for semantically verified blocks.
`zebra-state/src/service/non_finalized_state.rs:551-607` performs contextual
validation, constructs a `ContextuallyVerifiedBlock`, and then calls
`validate_and_update_parallel()`. That function runs
`check::block_commitment_is_valid_for_chain_history(...)` before returning the
updated chain (`zebra-state/src/service/non_finalized_state.rs:617-667`).

So the difference is timing:

- checkpoint path: auth-data binding is checked at finalized commit, before disk
  persistence;
- full path: auth-data binding is checked before non-finalized-chain acceptance.

## Replacement Reachability

The verifier-level replacement behavior is reachable by direct calls to the
checkpoint verifier while the range is incomplete, as shown by the local
`same_hash_checkpoint_auth_data_variant_replaces_queued_block_today` test.

Normal P2P block download paths add an important cap around that verifier
behavior. The sync downloader refuses a duplicate block hash while a
download/verify task is pending through `cancel_handles.contains_key(&hash)` in
`zebrad/src/components/sync/downloads.rs`. The inbound gossiped-block downloader
does the same duplicate-hash check in
`zebrad/src/components/inbound/downloads.rs` before queuing another block
download. So this does not currently look like a straightforward "two peers race
the same hash into checkpoint verification" issue through the standard P2P
downloaders.

## Triage

This does not need private disclosure as a fresh consensus vulnerability. The
current code rejects mutated-auth-data checkpoint blocks before persistence.

Useful local hardening and regression work:

- preserve the verifier-level replacement test that mutates NU5/V5 authorizing
  data while keeping the txid merkle root/header fixed, then shows the newer
  same-hash body replaces the older queued body and reaches the state-commit
  failure path;
- add a fuller state-backed regression if needed, using a valid NU5 checkpoint
  chain and asserting state rejects a mutated-auth-data body before DB write;
- keep the explicit comments in `CheckpointVerifiedBlock` and checkpoint queueing
  because they document an important deferred-validation boundary;
- continue to avoid any direct checkpoint import/restore path that calls
  `ZebraDb::write_block()` without `commit_finalized_direct()`.

## Duplicate Check

Read-only duplicate search refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra checkpoint verifier auth_data_root authorizing data commitment'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "checkpoint" "auth data" "merkle root"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "authorizing data hash" "checkpoint"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "hashBlockCommitments" "checkpoint"'
```

Relevant historical hits:

- closed #2633, "ZIP-221/244 auth data commitment validation in checkpoint
  verifier", explicitly records the chosen design: the check is done in
  finalized state because checkpoint verification lacks the history tree;
- closed #2697, "Security: Replace older duplicate queued checkpoint blocks with
  the latest block's data", records the duplicate-queued-block replacement
  behavior for same-hash blocks with different authorizing data;
- closed #2336, "Add hashAuthDataRoot to Block network messages, and
  semantically verify it", records the earlier desire to validate auth-data
  binding earlier in the pipeline.

These are historical design/fix issues rather than open duplicates. They confirm
the current lead is already known by design and remains eliminated as a new
security report.

Targeted verification rerun on 2026-05-09:

```sh
cargo test -p zebra-state all_upgrades_and_wrong_commitments_with_fake_activation_heights --lib
cargo test -p zebra-consensus same_hash_checkpoint_auth_data_variant_replaces_queued_block_today --lib
```

Result: both targeted tests passed.

The `zebra-state` property-style test corrupts commitments around Heartwood and
NU5 custom activation heights and expects finalized-state checkpoint-style
commits to reject the corrupted blocks.

The new `zebra-consensus` test also passed. It constructs two height-1 checkpoint
block bodies whose V5 transaction mined IDs and block hashes are identical while
the authorizing data differs, queues the good body first, queues the bad
same-hash body while the range is incomplete, then completes the range. Current
behavior returns `NewerRequest` to the older body and sends the newer replacement
body to state, where the mocked commit failure is surfaced as
`CommitCheckpointVerified`.
