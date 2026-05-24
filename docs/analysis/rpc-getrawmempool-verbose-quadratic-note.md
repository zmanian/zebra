# RPC getrawmempool verbose quadratic assembly note

Date: 2026-05-03

Last updated: 2026-05-09

Disposition: local-only. Do not post publicly without explicit
re-authorization.

## Summary

`getrawmempool(true)` asks the mempool service for every verified mempool
transaction, then builds one verbose object per transaction. The helper used for
each object rebuilds a full transaction-id lookup map from the complete mempool
slice, so a mempool with `n` transactions performs `n` full `HashMap`
constructions of size `n`.

This is bounded by Zebra's mempool size and RPC configuration, but it is a
reachable CPU/allocation amplification path for deployments that expose RPC to
shared or weakly trusted callers.

## Evidence

- `zebra-rpc/src/methods.rs:1618-1621` maps verbose `getrawmempool` to
  `mempool::Request::FullTransactions`.
- `zebrad/src/components/mempool.rs:834-848` responds to `FullTransactions` by
  cloning every stored `VerifiedUnminedTx` and cloning the dependency graph.
- `zebra-rpc/src/methods.rs:1637-1650` iterates every returned transaction and
  calls `MempoolObject::from_verified_unmined_tx()`.
- `zebra-rpc/src/methods/types/get_raw_mempool.rs:61-68` calls the lookup-map
  builder once per verbose object.
- `zebra-rpc/src/methods/types/get_raw_mempool.rs:120-134` rebuilds
  `transactions_by_id` by iterating the entire `transactions` slice.
- `zebra-rpc/src/methods/types/get_raw_mempool.rs:217-253` has a focused
  current-behavior proof that eight verbose rows trigger eight lookup-map builds
  over eight inputs each.
- `zebrad/src/components/mempool/config.rs:57-64` defaults the mempool cost
  limit to `80_000_000`, and
  `zebra-chain/src/transaction/unmined.rs:43-67` gives the minimum mempool
  transaction cost as `10_000`, so the default configuration still allows
  thousands of transactions.

## Impact

This is a public RPC availability hardening issue, not a consensus failure.

An RPC client can repeatedly request the verbose mempool and make Zebra:

- clone all verified mempool transactions,
- clone mempool dependency metadata,
- rebuild an all-transaction lookup table once per transaction,
- serialize a full verbose JSON response.

The main bug is the quadratic lookup construction before serialization. JSON
serialization of the full verbose response is expected to be linear in response
size, but the repeated `HashMap` construction is avoidable internal work.

Existing mitigations:

- JSON-RPC is disabled by default;
- cookie authentication is enabled by default when RPC is enabled;
- the cost is bounded by configured mempool limits;
- the RPC server has normal request body and response-size controls.

## Suggested Fix

- Build `transactions_by_id` once in `get_raw_mempool()` before iterating the
  transactions.
- Pass the precomputed map into `MempoolObject::from_verified_unmined_tx()`.
- Preserve the existing response schema and field semantics.
- Add a regression test or micro-benchmark shape that would fail if verbose
  object construction rebuilds the full transaction map per transaction.

Optional product hardening:

- add an RPC-side maximum for verbose full-mempool dumps, or
- return an explicit error when verbose output would exceed configured
  operational limits.

Disclosure triage: public hardening.

Confidence: high on the quadratic code shape; medium-low on severity because
RPC is disabled/authenticated by default and the mempool size is bounded.

## Duplicate Check

Read-only duplicate searches refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "getrawmempool" "verbose" "quadratic"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "MempoolObject" "transactions_by_id" "full mempool"'
```

No hits were returned.

## Local Confidence Check

Existing focused behavior test and lookup-build proof rerun on 2026-05-09:

```sh
cargo test -p zebra-rpc mempool_transactions_are_sent_to_caller --lib
cargo test -p zebra-rpc verbose_mempool_object_rebuilds_lookup_for_each_transaction_today --lib
```

Result: passed. This confirms the verbose `getrawmempool` path returns full
mempool transaction data. The focused lookup-build proof constructs eight
non-coinbase mempool transactions, builds one verbose object per transaction,
and confirms the helper rebuilds the full lookup map eight times over eight
inputs each. The practical RPC severity remains bounded by RPC auth/config and
mempool limits.
