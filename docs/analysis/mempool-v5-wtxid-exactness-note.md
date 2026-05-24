# Mempool V5 WTXID exactness note

Date: 2026-05-02

Last updated: 2026-05-08

Disposition: public issue filed after explicit re-authorization on 2026-05-08.
Issue: https://github.com/ZcashFoundation/zebra/issues/10565

## Summary

`TransactionsById` is documented as an exact unmined transaction lookup. For V5
transactions, that exact identifier is the witnessed transaction ID (`WtxId`),
which combines the mined transaction ID with the authorizing data digest.

The current mempool storage lookup only indexes by the mined transaction ID. A
request for a V5 transaction with the correct mined ID but a mismatched
authorizing data digest still returns the transaction currently held in the
mempool.

This is not a consensus issue and does not make Zebra accept an invalid
transaction or block. It is a P2P/mempool exactness and privacy hardening issue:
Zebra can return a V5 transaction that was not the exact `MSG_WTX` inventory item
requested by a peer.

## Evidence

- `zebra-chain/src/transaction/hash.rs` defines `WtxId` as `{ id,
  auth_digest }` and describes it as uniquely identifying unmined V5
  transactions.
- `zebra-chain/src/transaction/unmined.rs` represents V5 unmined transaction
  IDs as `UnminedTxId::Witnessed(WtxId)`.
- `zebra-network/src/protocol/external/inv.rs` maps `MSG_WTX` inventory entries
  to `UnminedTxId::Witnessed`.
- `zebrad/src/components/mempool/storage.rs` implements
  `transactions_exact(tx_ids)` by looking up `self.transactions().get(&tx_id.mined_id())`.
  It does not compare the full `UnminedTxId` after the mined-ID lookup.
- A local regression-style test,
  `transactions_exact_matches_v5_by_mined_id_not_wtxid_today`, mutates only the
  V5 auth digest in a requested `WtxId` and shows the stored transaction is still
  returned.

Command:

```sh
cargo test -p zebrad transactions_exact_matches_v5_by_mined_id_not_wtxid_today --lib
```

Result on 2026-05-07: passed.

## Impact

The practical impact appears limited:

- a peer that knows or guesses the mined transaction ID can request `MSG_WTX`
  with an arbitrary auth digest and still receive the mempool transaction;
- if same-effects V5 transaction variants exist, the response can be a different
  witnessed transaction than the one requested;
- remote peers that enforce request/response inventory exactness could consider
  the response unexpected, although the transaction itself remains subject to
  normal transaction validation.

This is most naturally treated as public protocol-hardening, not private
security disclosure.

## Suggested fix direction

Keep the mined-ID map for efficient lookup, but enforce exactness after lookup:

- look up by `tx_id.mined_id()` as today;
- return the transaction only if `stored.transaction.id == requested_tx_id`;
- add a regression test that a V5 request with the same mined ID but different
  auth digest returns no transaction and would therefore produce `notfound`.

## Duplicate Check

Read-only duplicate search performed on 2026-05-07:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra TransactionsById V5 WtxId exactness auth digest'
```

No issue hits were returned.

Confidence: high on the current behavior from the local test; medium-low on
security severity.
