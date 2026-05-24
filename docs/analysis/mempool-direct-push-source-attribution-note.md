# Mempool Direct-Push Source Attribution Note

Date: 2026-05-03

Last updated: 2026-05-09

Disposition: local-only. Do not post publicly without explicit
re-authorization.

Scope: peer-misbehavior attribution for unsolicited `tx` messages sent directly
over the P2P connection.

## Finding

Downloaded transaction gossip preserves the address of the peer that served the
transaction bytes, so score-bearing mempool verification failures can be reported
to the address-book misbehavior pipeline. Direct pushed transactions do not carry
that source address.

Current flow:

1. A peer sends a wire `tx` message.
2. `zebra-network/src/peer/connection.rs:1278` maps it to
   `Request::PushTransaction(transaction.clone())`.
3. `zebra-network/src/protocol/internal/request.rs:148` defines
   `PushTransaction(UnminedTx)` without source metadata.
4. `zebrad/src/components/inbound.rs:526-530` forwards it to the mempool queue
   using `transaction.into()`.
5. `zebra-node-services/src/mempool/gossip.rs:39-42` converts that into
   `Gossip::Tx(tx)`.
6. `zebrad/src/components/mempool/downloads.rs:365-370` handles
   `Gossip::Tx(tx)` by setting `advertiser_addr` to `None`.
7. `zebrad/src/components/mempool.rs:643-653` only reports invalid transactions
   when `TransactionDownloadVerifyError::Invalid` has
   `advertiser_addr: Some(advertiser_addr)`.

So a peer can directly push a transaction that fails with a score-bearing
`TransactionError`; Zebra rejects it, but the peer cannot be scored because the
connection address was dropped before verification.

## Impact

This is public P2P/mempool hardening, not a private consensus issue.

The invalid transaction is still rejected. The gap is enforcement: direct pushed
invalid transactions are treated differently from downloaded invalid
transactions. A peer can use direct `tx` messages to exercise mempool
verification without accruing address-book misbehavior score for invalid
payloads.

Bounds and mitigations:

- inbound service timeout/load-shed still applies to request work,
- mempool download/verify concurrency remains bounded,
- mempool rejection caches still avoid repeated work for exact/same-effect
  cases,
- only score-bearing transaction errors are relevant.

## Local Confidence Check

Added a direct downloader-level current-behavior test:

```sh
cargo test -p zebrad invalid_direct_pushed_transaction_has_no_advertiser_addr_today --lib
```

Result on 2026-05-09: passed. The test queues a direct `Gossip::Tx`, uses a
mock state service that says the transaction is not in the best chain, and uses
a verifier that returns `TransactionError::BadBalance`. `BadBalance` has a
nonzero mempool misbehavior score, but the resulting
`TransactionDownloadVerifyError::Invalid` has `advertiser_addr: None`.

Duplicate check refreshed on 2026-05-09:

```sh
gh issue list --repo ZcashFoundation/zebra --search 'direct pushed transaction advertiser addr misbehavior in:title,body' --state all --limit 100
gh issue list --repo ZcashFoundation/zebra --search 'PushTransaction source attribution mempool in:title,body' --state all --limit 100
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "PushTransaction" "advertiser_addr"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "invalid_direct_pushed_transaction_has_no_advertiser_addr_today"'
```

No hits were returned.

## Suggested Public Fix

- Change `Request::PushTransaction` to carry
  `{ transaction: UnminedTx, source: Option<PeerSocketAddr> }`.
- When `Message::Tx` is received from a peer connection, populate `source` from
  `connected_addr.get_transient_addr()`.
- Extend `Gossip::Tx` to carry `advertiser_addr: Option<PeerSocketAddr>`.
- Preserve that address in `mempool/downloads.rs` so
  `TransactionDownloadVerifyError::Invalid` can report it.
- Keep locally-originated pushes as `source: None`.

Regression idea: push a known score-bearing invalid transaction through the
inbound `Message::Tx` path and assert that the mempool invalid result sends a
misbehavior report for the source peer.
