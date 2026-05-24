# P2P mempool request enumeration note

Date: 2026-05-03

Last updated: 2026-05-09

Status: local-only per 2026-05-07 posting stop instruction.

## Summary

An unauthenticated peer can send the `mempool` P2P message, asking Zebra to
advertise transaction IDs from its local mempool. Zebra caps the outbound `inv`
response in the connection layer, but the cap is applied after the inbound and
mempool services have already materialized the full local mempool ID set.

This is bounded by Zebra's mempool size and by inbound timeout/load-shed layers,
but the source-side work is larger than the final response needs to be.

## Evidence

- `zebra-network/src/peer/connection.rs:1362` maps inbound `Message::Mempool`
  to `Request::MempoolTransactionIds`.
- `zebrad/src/components/inbound.rs:547-554` forwards that request to the
  mempool service as `mempool::Request::TransactionIds`.
- `zebra-node-services/src/mempool.rs:37-39` defines `TransactionIds` as a
  query for all `UnminedTxId`s in the mempool.
- `zebrad/src/components/mempool.rs:770-778` answers `TransactionIds` by
  collecting every `storage.tx_ids()` item into a `HashSet`.
- `zebra-network/src/peer/connection.rs:1537-1559` then truncates the
  `Response::TransactionIds` to `MAX_TX_INV_IN_SENT_MESSAGE` before sending the
  outbound `inv`.
- `zebra-network/src/protocol/external/inv.rs:192-201` sets
  `MAX_TX_INV_IN_SENT_MESSAGE` to `25_000`.

The connection-layer truncation is useful as defense in depth, but it is too
late to prevent the full mempool enumeration and allocation.

Existing focused tests rerun on 2026-05-09:

```sh
cargo test -p zebrad mempool_requests_for_transactions --lib
cargo test -p zebrad mempool_transaction_ids_request_forwards_full_set_before_connection_cap_today --lib
```

Result:

```text
test components::inbound::tests::fake_peer_set::mempool_requests_for_transactions ... ok
test components::inbound::tests::fake_peer_set::mempool_transaction_ids_request_forwards_full_set_before_connection_cap_today ... ok
```

The first test exercises the real inbound `MempoolTransactionIds` path and
confirms the network response matches the transaction IDs currently stored in
the mempool. The second test uses a mock mempool service to return
`MAX_TX_INV_IN_SENT_MESSAGE + 3` transaction IDs and confirms inbound forwards
the entire set as `Response::TransactionIds`; the connection-layer `inv` cap is
therefore applied only after inbound/mempool work has already materialized the
full ID set.

## Impact

This is a public P2P availability hardening issue.

A peer sends a tiny `mempool` request and Zebra performs work proportional to
the local mempool size:

- iterate every transaction ID in storage,
- allocate a `HashSet` for all IDs,
- convert the full set to the network response vector,
- only then truncate to the outbound protocol cap.

Default mempool settings bound this cost, and the request can time out or be
load-shed. The issue is still asymmetric because the peer's request size is
constant while Zebra's work scales with local mempool contents.

## Suggested Fix

- Add a bounded mempool request variant, for example
  `TransactionIdsLimited { max: usize }`.
- Have the mempool service return at most `max` IDs without first collecting all
  IDs.
- Use that bounded request from `Inbound::call(Request::MempoolTransactionIds)`.
- Keep the existing connection-layer truncation as defense in depth.

Disclosure triage: public hardening.

Confidence: high on current behavior; medium-low on practical severity because
the mempool and inbound service are bounded.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "mempool" "TransactionIds" "enumeration"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "MempoolTransactionIds" "MAX_TX_INV_IN_SENT_MESSAGE"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "mempool" "inv" "25,000"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "mempool message" "transaction ids" "cap"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra MempoolTransactionIds'
```

Results:

- The first four targeted searches returned no hits.
- The broad `MempoolTransactionIds` search returned historical test and
  implementation work, including #5384, #5706, #2214, #2726, and related PRs.
  Those cover flaky tests, general fanout limits, and `notfound` behavior, not
  this bounded-enumeration/cap-hardening concern.
