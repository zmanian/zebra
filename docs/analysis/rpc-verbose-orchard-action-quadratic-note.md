# RPC verbose Orchard action serialization quadratic note

Date: 2026-05-03

Last updated: 2026-05-09

Disposition: local-only. Do not post publicly without explicit
re-authorization.

## Summary

Verbose transaction RPC output rebuilds Orchard action data by iterating the
transaction's actions and, for each action, searching the same transaction's
authorized-action list to recover the matching spend authorization signature.
That makes verbose serialization of one transaction with `n` Orchard actions
perform `O(n^2)` action comparisons.

This is bounded by the Zcash block-size limit and by RPC access controls. It is
not consensus-relevant, but it is avoidable CPU work in exposed RPC paths such
as `getrawtransaction(..., verbose=1)` and `getblock(..., verbosity=2)`.

## Evidence

- `zebra-rpc/src/methods.rs:1718-1730` returns verbose mempool transactions via
  `TransactionObject::from_transaction()`.
- `zebra-rpc/src/methods.rs:1783-1790` also uses
  `TransactionObject::from_transaction()` for verbose mined transactions.
- `zebra-rpc/src/methods.rs:1329-1352` maps every transaction in a
  `getblock(..., verbosity=2)` response through `TransactionObject`.
- `zebra-rpc/src/methods/types/transaction.rs:864-902` builds the Orchard
  action response by collecting `tx.orchard_actions()` into a vector, iterating
  that vector, then calling `.find()` over `shielded_data.actions.iter()` for
  each action.
- `zebra-chain/src/orchard/action.rs:23-42` shows that `Action` equality
  compares a large action description, including ciphertext fields.
- `zebra-chain/src/orchard/shielded_data.rs:185-204` caps Orchard action
  counts by the maximum block size and the 2^16 consensus bound.
- `zebra-chain/src/block/serialize.rs:24` sets `MAX_BLOCK_BYTES` to
  `2_000_000`.

With `AUTHORIZED_ACTION_SIZE = 884` bytes, a maximally dense valid block or
transaction can still carry thousands of Orchard actions. The exact maximum is
bounded, but the repeated search is unnecessary because `ShieldedData` already
stores each `AuthorizedAction` as the paired action plus signature.

## Impact

This is a public RPC availability hardening issue.

A caller with RPC access can request verbose output for a transaction or block
that contains many Orchard actions, causing Zebra to:

- allocate an intermediate vector of action references,
- scan the authorized-action list once per action,
- compare large action descriptions repeatedly,
- then serialize the full verbose JSON response.

The response itself is legitimately large, so some work is expected. The
quadratic signature lookup is the avoidable part.

Existing mitigations:

- JSON-RPC is disabled by default;
- cookie authentication is enabled by default when RPC is enabled;
- valid block and transaction sizes bound the action count;
- response-size limits can cap what is returned to clients.

## Suggested Fix

- When Orchard shielded data exists, iterate `shielded_data.actions.iter()`
  directly and build each RPC `OrchardAction` from the paired
  `AuthorizedAction`.
- Avoid collecting `tx.orchard_actions()` and then searching back into
  `shielded_data.actions`.
- Preserve the current fallback behavior for transactions without Orchard data.
- Add a regression test with multiple Orchard actions that verifies the
  signature is copied from the corresponding authorized action without a nested
  search.

Disclosure triage: public hardening.

Confidence: high on the code shape; medium-low on practical severity because
the action count is block-size bounded and RPC is disabled/authenticated by
default.

## Duplicate Check

Read-only duplicate searches refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "verbose Orchard action" "quadratic"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "TransactionObject" "Orchard actions" "find signature"'
```

No hits were returned.

## Local Confidence Check

Proof-backed as of 2026-05-09. A test-only comparison counter around the
authorized-action search confirms the repeated-scan shape on the first Testnet
V5 transaction with two Orchard actions and no other transfers:

```sh
cargo test -p zebra-rpc verbose_transaction_searches_authorized_orchard_actions_repeatedly_today --lib
```

Result: passed.

The test deserializes `BLOCK_TESTNET_1842467_BYTES`, selects the two-action
Orchard transaction, builds `TransactionObject::from_transaction()`, and
confirms verbose output does three authorized-action comparisons for two
actions. That is the triangular `1 + 2 + ... + n` search pattern produced by
collecting Orchard actions and then scanning `shielded_data.actions` from the
start for each action.
