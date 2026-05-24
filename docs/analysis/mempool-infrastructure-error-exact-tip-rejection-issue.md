# Mempool infrastructure failures can be cached as exact-tip transaction rejections

## Summary

The mempool path can collapse transaction verifier infrastructure failures into
transaction-invalid errors and then cache them as exact-tip failed-verification
rejections.

This does not accept invalid transactions and does not affect consensus. The
availability risk is narrower: a transient verifier or state-service failure can
suppress retrying the same transaction until the exact-tip rejection cache is
cleared by tip movement or mempool reset.

This looks like low-severity public hardening rather than a private disclosure
item, because the local proofs show the error-classification/cache behavior but
do not establish a remotely reliable way to trigger the underlying
infrastructure failure.

## Code Path

- The downloader sends a transaction to the transaction verifier:
  `zebrad/src/components/mempool/downloads.rs:376`
- The downloader maps verifier service errors into
  `TransactionDownloadVerifyError::Invalid`:
  `zebrad/src/components/mempool/downloads.rs:393`
- Unknown boxed verifier errors can become
  `TransactionError::InternalDowncastError`:
  `zebra-consensus/src/error.rs:234`
- The mempool service treats every
  `TransactionDownloadVerifyError::Invalid` as failed verification and calls
  `storage.reject_if_needed(tx_id, error)`:
  `zebrad/src/components/mempool.rs:641`
- Mempool storage stores invalid results as exact-tip failed-verification
  rejections:
  `zebrad/src/components/mempool/storage.rs:843`
- Later requests check rejection state before allowing future download or
  verification:
  `zebrad/src/components/mempool/storage.rs:919`
- A lower-level state lookup service failure while checking
  `UnspentBestChainUtxo` is also mapped into
  `TransactionError::TransparentInputNotFound`:
  `zebra-consensus/src/transaction.rs:700`

## Local Reproduction Tests

I added local proof tests for the three relevant layers:

```sh
cargo test -p zebra-consensus mempool_request_with_state_lookup_error_is_currently_missing_input --lib
cargo test -p zebrad transparent_input_not_found_is_exact_tip_rejected_today --lib
cargo test -p zebrad mempool_internal_verifier_error_is_exact_tip_rejected_today --lib
```

All three pass on the current tree.

The service-level test queues a direct pushed transaction through the normal
`Mempool` service, makes the mock verifier return
`TransactionError::InternalDowncastError`, polls the mempool, then queues the
same txid again. Current behavior returns
`MempoolError::StorageExactTip(ExactTipRejectionError::FailedVerification(TransactionError::InternalDowncastError(_)))`.

## Expected Behavior

Mempool retry/cache behavior should distinguish transaction-invalid consensus
failures from verifier or state-service infrastructure failures.

Infrastructure failures should be retriable and should not poison exact-tip
failed-verification rejection state for the transaction.

## Suggested Fix Direction

- Split transaction verifier failures at the mempool downloader boundary into
  consensus-invalid versus infrastructure-failure categories.
- Only consensus-invalid errors should call `storage.reject_if_needed`.
- State service lookup failures should remain distinguishable from
  `TransparentInputNotFound`.
- Add post-fix tests proving internal verifier failures and state lookup
  failures do not enter exact-tip rejection state.
