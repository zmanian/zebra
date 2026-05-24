# Post-v4.4.0 security audit pass 3 findings

Date: 2026-05-02

## Summary

This pass focused on verifier result taxonomy: cases where timeout,
cancellation, service failure, downcast failure, or unknown boxed errors are
collapsed into definitive consensus or RPC outcomes.

Two new hardening findings surfaced beyond the already-documented miner RPC
liveness gap:

1. Inbound block peer scoring appears to downcast verifier failures to the
   wrong type, so invalid gossiped blocks can avoid misbehavior scoring.
2. The mempool verifier boundary can convert infrastructure or unknown boxed
   verifier failures into exact-tip transaction rejections, temporarily
   poisoning the mempool rejection cache.

The sync downloader is a useful contrast: it already keeps typed verifier
failures separate from unknown validation-request failures.

## Finding 1: Inbound invalid-block scoring downcasts the wrong verifier error

Status: confirmed hardening gap.

Impact: peer-misbehavior under-classification for invalid gossiped blocks. This
does not make invalid blocks valid, but it can weaken peer eviction/scoring and
increase repeated invalid-block resource cost.

Evidence:

- `zebrad/src/components/inbound.rs:82` to
  `zebrad/src/components/inbound.rs:87` defines the inbound semantic block
  verifier as a buffered service whose error type is
  `zebra_consensus::router::RouterError`, then wraps it in
  `tower::timeout::Timeout`.
- `zebrad/src/components/inbound/downloads.rs:394` to
  `zebrad/src/components/inbound/downloads.rs:398` awaits the verifier and
  returns verifier errors as boxed errors with the advertiser address.
- `zebrad/src/components/inbound.rs:332` to
  `zebrad/src/components/inbound.rs:345` drains completed gossiped block
  downloads, but attempts `err.downcast::<VerifyBlockError>()`.
- `zebra-consensus/src/router.rs:100` to
  `zebra-consensus/src/router.rs:104` shows that verifier failures are wrapped
  in `RouterError::Checkpoint` or `RouterError::Block`.
- `zebra-consensus/src/router.rs:146` to
  `zebra-consensus/src/router.rs:152` already exposes
  `RouterError::misbehavior_score()`, which delegates to the wrapped verifier
  error.

Why it matters:

- A normal full-verifier invalid-block error reaches inbound cleanup as a boxed
  `RouterError`, not as a bare `VerifyBlockError`.
- The downcast at `zebrad/src/components/inbound.rs:337` fails for boxed
  `RouterError`, so the cleanup path hits `continue` and never sends a
  misbehavior score.
- This is the opposite of an infra-to-invalid collapse: consensus-invalid peer
  behavior is being treated like an unclassified internal error.

Minimal regression test:

- Use the existing inbound test setup in
  `zebrad/src/components/inbound/tests/real_peer_set.rs`, where the mock block
  verifier type already returns `RouterError`.
- Queue a gossiped block download with an advertiser.
- Have the mock block verifier return a boxed
  `RouterError::Block { source: VerifyBlockError::Equihash { ... } }` or any
  other non-zero-score invalid-block error.
- Poll inbound cleanup and assert a `(PeerSocketAddr, score)` item is sent on
  the misbehavior channel.
- The current behavior should fail by emitting no score.

Recommended fix:

- In `zebrad/src/components/inbound.rs`, downcast completed verifier failures
  to `RouterError` first and use `RouterError::misbehavior_score()`.
- Treat timeout and unknown downcast failures as infrastructure/unknown and do
  not score them.
- Remove the unused bare `VerifyBlockError` cleanup assumption once the
  `RouterError` path is covered.

## Finding 2: Mempool verifier infrastructure failures can become exact-tip rejections

Status: confirmed availability / mempool-retry hardening gap.

Impact: a transaction can be temporarily rejected at the mempool storage layer
because the verifier boundary turned a service failure, timeout-like failure, or
unknown boxed error into `TransactionError`. This can suppress retry until the
rejection is cleared by tip movement.

Evidence:

- `zebrad/src/components/mempool/downloads.rs:376` to
  `zebrad/src/components/mempool/downloads.rs:388` sends the transaction to the
  transaction verifier.
- `zebrad/src/components/mempool/downloads.rs:393` maps any verifier error with
  `TransactionDownloadVerifyError::Invalid { error: e.into(), ... }`.
- `zebra-consensus/src/error.rs:235` to
  `zebra-consensus/src/error.rs:257` implements `From<BoxError> for
  TransactionError`; unknown boxed errors become
  `TransactionError::InternalDowncastError`.
- `zebrad/src/components/mempool.rs:641` to
  `zebrad/src/components/mempool.rs:662` treats every
  `TransactionDownloadVerifyError::Invalid` as a failed verification, emits
  invalidation, and calls `storage.reject_if_needed(tx_id, error)`.
- `zebrad/src/components/mempool/storage.rs:845` to
  `zebrad/src/components/mempool/storage.rs:869` stores every `Invalid` result
  as `ExactTipRejectionError::FailedVerification(error)`.
- `zebrad/src/components/mempool/storage.rs:919` to
  `zebrad/src/components/mempool/storage.rs:927` checks rejection state before
  allowing future download or verification.
- Existing tests encode the intended distinction between verification failure
  and download failure:
  `zebrad/src/components/mempool/tests/vector.rs:710` rejects a verifier
  failure, while `zebrad/src/components/mempool/tests/vector.rs:796` confirms a
  download failure is not cached as rejected.

Additional lower-level coercions:

- `zebra-consensus/src/transaction.rs:649` to
  `zebra-consensus/src/transaction.rs:652` maps a state service failure while
  fetching `BestChainNextMedianTimePast` into
  `TransactionError::ValidateMempoolLockTimeError`.
- `zebra-consensus/src/transaction.rs:701` to
  `zebra-consensus/src/transaction.rs:708` maps a state service failure while
  checking `UnspentBestChainUtxo` into
  `TransactionError::TransparentInputNotFound`.
- `zebra-consensus/src/transaction.rs:742` to
  `zebra-consensus/src/transaction.rs:749` maps a mempool `AwaitOutput` timeout
  into `TransactionError::TransparentInputNotFound`.

Why it matters:

- The mempool storage layer is trying to do the right thing: it caches
  consensus verifier failures and avoids caching download, state, and
  cancellation failures.
- But the downloader/verifier boundary marks every boxed verifier error as
  `Invalid`, so the storage layer no longer knows whether the error was a true
  consensus rejection or infrastructure.
- This is especially relevant to `sendrawtransaction` and gossiped transaction
  retry behavior: transient service failures should not poison exact-tip
  rejection state.

Minimal regression tests:

- Extend the existing mempool vector tests with a third case:
  `mempool_failed_internal_verifier_error_is_not_rejected`.
- Follow `mempool_failed_verification_is_rejected`, but have the mock
  transaction verifier return a boxed internal error that is not
  `TransactionError`.
- Poll the mempool, then queue the same transaction ID again.
- Current behavior should return
  `MempoolError::StorageExactTip(ExactTipRejectionError::FailedVerification(
  TransactionError::InternalDowncastError(_)))`.
- Desired behavior is the same shape as
  `mempool_failed_download_is_not_rejected`: re-queue remains allowed.

Additional focused transaction-verifier tests:

- Mock `UnspentBestChainUtxo` service failure and assert it is not reported as
  `TransparentInputNotFound`.
- Mock `BestChainNextMedianTimePast` service failure and assert it is not
  treated as a lock-time consensus error.
- Mock `AwaitOutput` timeout and assert the caller can distinguish timeout from
  missing transparent input.

Recommended fix:

- Split transaction verifier failures at the mempool downloader boundary:
  `ConsensusInvalid(TransactionError)` versus
  `InfrastructureFailure(BoxError)` or an equivalent small enum.
- Only `ConsensusInvalid` should call `storage.reject_if_needed`.
- Keep outer timeout and cancellation behavior non-rejecting.
- Narrow the low-level transaction verifier conversions so service failures do
  not become `TransparentInputNotFound` or lock-time validation errors.

## Finding 3: RPC miner-facing result taxonomy remains too lossy

Status: already identified in earlier pass; confirmed by taxonomy pass.

Impact: miner-facing RPCs can report infrastructure or unknown verifier
failures as definitive rejection, or flatten them into proposal-invalid strings.

Evidence:

- `zebra-rpc/src/methods.rs:2573` to `zebra-rpc/src/methods.rs:2578` awaits
  `Request::Commit` in `submitblock`.
- `zebra-rpc/src/methods.rs:2597` to `zebra-rpc/src/methods.rs:2612`
  downcasts verifier errors to `RouterError`.
- `zebra-rpc/src/methods.rs:2615` to `zebra-rpc/src/methods.rs:2637` maps only
  duplicates separately; non-duplicate `RouterError` and unknown downcast
  failures both become `SubmitBlockErrorResponse::Rejected`.
- `zebra-rpc/src/methods/types/get_block_template.rs:657` to
  `zebra-rpc/src/methods/types/get_block_template.rs:662` awaits
  `Request::CheckProposal` in proposal mode.
- `zebra-rpc/src/methods/types/get_block_template.rs:664` to
  `zebra-rpc/src/methods/types/get_block_template.rs:674` maps every verifier
  error into `BlockProposalResponse::rejected("invalid proposal", ...)`.
- `zebra-rpc/src/methods/types/get_block_template/proposal.rs:41` to
  `zebra-rpc/src/methods/types/get_block_template/proposal.rs:58` turns a
  boxed error into a single kebab-case rejection string.
- `zebra-consensus/src/block.rs:59` to
  `zebra-consensus/src/block.rs:95` shows `VerifyBlockError` mixes consensus
  invalidity with infrastructure variants such as `Depth`, `StateService`, and
  `ValidateProposal`.
- `zebra-consensus/src/checkpoint.rs:960` to
  `zebra-consensus/src/checkpoint.rs:1009` shows `VerifyCheckpointError` mixes
  invalidity with control-flow and infrastructure variants such as `Dropped`,
  `CommitCheckpointVerified`, `Tip`, `CheckpointList`, and `ShuttingDown`.

Recommended fix:

- Add a small verifier-disposition classifier over `RouterError`,
  `VerifyBlockError`, and `VerifyCheckpointError`.
- Use it before building external RPC responses.
- `submitblock` timeout, shutdown, service failure, and unknown boxed errors
  should become `inconclusive` or JSON-RPC server errors, not `rejected`.
- GBT proposal infrastructure failures should be JSON-RPC errors, not rejected
  proposal strings.

## Eliminated lead: sync downloader preserves unknown verifier failures

Status: eliminated as a primary bug in this pass.

Evidence:

- `zebrad/src/components/sync/downloads.rs:122` to
  `zebrad/src/components/sync/downloads.rs:137` has separate
  `Invalid { error: RouterError, ... }` and
  `ValidationRequestError { error: BoxError, ... }` variants.
- `zebrad/src/components/sync/downloads.rs:562` to
  `zebrad/src/components/sync/downloads.rs:568` downcasts verifier errors to
  `RouterError`; downcast success becomes `Invalid`, downcast failure becomes
  `ValidationRequestError`.
- The same file also has explicit cancellation and timeout variants around the
  verification pipeline.

Conclusion:

- Sync is the model to copy for RPC, inbound, and mempool boundaries: typed
  verifier failure and unknown infrastructure failure should stay distinct.

## Prioritized next fixes

1. Fix inbound cleanup to downcast to `RouterError` and test that invalid
   gossiped blocks produce misbehavior scoring.
2. Split mempool verifier errors into consensus-invalid and infrastructure
   outcomes before calling `storage.reject_if_needed`.
3. Add miner RPC verifier timeout and classification, preserving
   `inconclusive`/server-error semantics for unknown commit status.
4. Add proposal-mode classification so infrastructure failures do not become
   `invalid proposal`.
5. Add additive consensus error classifier helpers to make these boundaries
   less ad hoc.
