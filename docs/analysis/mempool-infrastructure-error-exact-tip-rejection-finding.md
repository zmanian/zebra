# Mempool Infrastructure Error Exact-Tip Rejection Finding

Date: 2026-05-07

Status: reported publicly as low-severity hardening:
[#10559](https://github.com/ZcashFoundation/zebra/issues/10559).

## Summary

Zebra's mempool path can collapse verifier infrastructure failures into
transaction-invalid errors and then cache them as exact-tip failed-verification
rejections.

This does not accept invalid transactions and does not affect consensus. The
availability risk is that a transient verifier/state failure can suppress
retrying the same transaction until the exact-tip rejection cache is cleared by
tip movement or mempool reset.

## Evidence

Downloader/verifier boundary:

- `zebrad/src/components/mempool/downloads.rs:376-388` sends a mempool
  transaction to the transaction verifier.
- `zebrad/src/components/mempool/downloads.rs:393` maps any verifier service
  error into `TransactionDownloadVerifyError::Invalid { error: e.into(), ... }`.
- `zebra-consensus/src/error.rs:234-257` converts unknown boxed verifier errors
  into `TransactionError::InternalDowncastError`.

Mempool storage boundary:

- `zebrad/src/components/mempool.rs:641-662` treats every
  `TransactionDownloadVerifyError::Invalid` as failed verification and calls
  `storage.reject_if_needed(tx_id, error)`.
- `zebrad/src/components/mempool/storage.rs:843-869` stores invalid results as
  exact-tip failed-verification rejections.
- `zebrad/src/components/mempool/storage.rs:919-927` checks rejection state
  before allowing future download or verification.

Lower-level state lookup conversion:

- `zebra-consensus/src/transaction.rs:700-714` maps a state service failure while
  checking `UnspentBestChainUtxo` into `TransactionError::TransparentInputNotFound`.

## Local Proofs

State lookup failure collapse:

```sh
cargo test -p zebra-consensus mempool_request_with_state_lookup_error_is_currently_missing_input --lib
```

Result on 2026-05-07:

```text
test transaction::tests::mempool_request_with_state_lookup_error_is_currently_missing_input ... ok
```

Storage caching of the collapsed missing-input error:

```sh
cargo test -p zebrad transparent_input_not_found_is_exact_tip_rejected_today --lib
```

Result on 2026-05-07:

```text
test components::mempool::storage::tests::vectors::transparent_input_not_found_is_exact_tip_rejected_today ... ok
```

Service-level caching of an internal verifier error:

```sh
cargo test -p zebrad mempool_internal_verifier_error_is_exact_tip_rejected_today --lib
```

Result on 2026-05-07:

```text
test components::mempool::tests::vector::mempool_internal_verifier_error_is_exact_tip_rejected_today ... ok
```

That service-level test queues a direct pushed transaction through the normal
`Mempool` service, makes the mock verifier return
`TransactionError::InternalDowncastError`, polls the mempool, then queues the
same txid again. Current behavior returns
`MempoolError::StorageExactTip(ExactTipRejectionError::FailedVerification(TransactionError::InternalDowncastError(_)))`.

## Impact

The impact is temporary mempool availability/retry degradation for affected
transactions, not consensus failure:

- failed transactions are not accepted;
- exact-tip rejection is cleared by tip change or mempool reset;
- remote reachability depends on whether peer-supplied mempool work can trigger
  verifier/state infrastructure failures rather than ordinary consensus errors.

This is lower severity than the process-fatal panic reports and lower confidence
than the stale downloader timeout retention finding. It is still security-shaped
because mempool retry behavior should distinguish invalid transaction data from
service failures.

Disclosure routing update: after rerunning the local proofs and checking public
issue overlap on 2026-05-07, this was filed publicly rather than as a private
GHSA because the evidence establishes the error-classification/cache behavior,
but not a remotely reliable trigger for the underlying infrastructure failure.

## Suggested Fix Direction

- Split transaction verifier failures at the mempool downloader boundary into
  consensus-invalid versus infrastructure-failure categories.
- Only consensus-invalid errors should call `storage.reject_if_needed`.
- State service lookup failures should remain distinguishable from
  `TransparentInputNotFound`.
- Add post-fix tests proving internal verifier failures and state lookup
  failures do not poison exact-tip rejection state.
