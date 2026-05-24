# P2P getdata transaction request amplification note

Date: 2026-05-02

Last updated: 2026-05-08

Status: public issue filed after explicit re-authorization on 2026-05-08.
Issue: https://github.com/ZcashFoundation/zebra/issues/10566

## Summary

For block `getdata` requests, Zebra caps work before state lookups by only
checking up to `GETDATA_MAX_BLOCK_COUNT` block hashes. For transaction
`getdata` requests, Zebra forwards the whole requested transaction-ID set to
the mempool before applying the 1 MB outbound response byte cap.

This is protocol-valid but gives a peer a relatively cheap way to force many
mempool hash lookups and a large `notfound` response. The protocol message size
limit, inbound timeout, inbound buffer/load-shed layers, and per-connection
sequential handling are meaningful mitigations. The remaining issue is that the
transaction path lacks a direct pre-lookup count cap like the block path.

## Evidence

- The protocol allows inventory vectors up to `MAX_INV_IN_RECEIVED_MESSAGE =
  50_000`, additionally bounded by `MAX_PROTOCOL_MESSAGE_LEN = 2 * 1024 * 1024`.
  For legacy transaction IDs this can approach 50,000 IDs in one message; for
  `MSG_WTX` entries the byte limit lowers the practical count but still allows
  tens of thousands of entries.
- `zebra-network/src/peer/connection.rs` maps a peer `Message::GetData` with
  any transaction inventory entries into `Request::TransactionsById` containing
  all transaction IDs from the message.
- `zebrad/src/components/inbound.rs` handles `Request::TransactionsById` by
  cloning the whole requested set into `mempool::Request::TransactionsById`.
- `zebrad/src/components/mempool.rs` answers that request by cloning the set and
  running `storage.transactions_exact(ids.clone()).cloned().collect()`.
- The outbound `GETDATA_SENT_BYTES_LIMIT` is applied only after the mempool
  response returns, while building the available transaction response. Missing
  transaction IDs are preserved from the requested set.
- By contrast, the block path explicitly uses
  `hashes.iter().take(GETDATA_MAX_BLOCK_COUNT)` before state lookup.

Local proof added on 2026-05-07:

- `zebrad/src/components/inbound/tests/fake_peer_set.rs:
  large_transactions_by_id_request_returns_every_missing_id_today`

The test sends a 2,048-ID `Request::TransactionsById` to the inbound service
with an empty mempool and confirms current behavior returns a `Missing` response
for every requested transaction ID, preserving the entire large requested set.

Verification:

```sh
cargo test -p zebrad large_transactions_by_id_request_returns_every_missing_id_today --lib
```

Result:

```text
test components::inbound::tests::fake_peer_set::large_transactions_by_id_request_returns_every_missing_id_today ... ok
```

## Impact

This is not consensus-critical. It is a public P2P availability hardening lead.

A malicious peer can repeatedly send large `getdata` transaction requests full
of unknown IDs. Zebra will perform a large number of mempool lookups and then
prepare a correspondingly large `notfound` response. The global inbound timeout
limits the duration of each request, and load shedding can disconnect peers
under pressure, but the request still consumes CPU and buffer capacity.

## Suggested fix direction

- Add an explicit `GETDATA_MAX_TRANSACTION_COUNT` for transaction requests.
- Truncate transaction IDs before the mempool request, matching the block path's
  "cap before expensive lookup" pattern.
- Consider a smaller cap for unsolicited transaction `getdata` than the network
  deserialization cap, since the response byte cap already means Zebra will not
  return arbitrarily many transactions.
- Add a regression test that a transaction `getdata` request with more than the
  cap forwards only the capped set to mempool and returns `notfound` only for
  the capped set.

Disclosure triage: public hardening.

Confidence: medium-high on the missing pre-mempool count cap; medium on impact
because existing timeout/load-shed/message-size limits reduce exploitability.

Duplicate check already performed before the local-only switch:

```sh
gh issue list --repo ZcashFoundation/zebra --search 'getdata transaction mempool notfound cap in:title,body' --state all --limit 100
gh issue list --repo ZcashFoundation/zebra --search 'TransactionsById mempool in:title,body' --state all --limit 100
```

Closest hits were #1880, #1077, and #2608. Those are related to broad safe
preallocation or older inbound `TransactionsById` implementation work, not this
pre-mempool lookup cap.
