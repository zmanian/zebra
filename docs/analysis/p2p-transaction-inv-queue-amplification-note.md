# P2P transaction inv mempool queue amplification note

Date: 2026-05-03

Last updated: 2026-05-09

Status: local-only per 2026-05-07 posting stop instruction.

## Summary

A peer can send a protocol-valid `inv` message containing many transaction
inventory entries. Zebra deduplicates the transaction IDs to unique
`UnminedTxId`s, forwards the full unique set to the mempool queue, and the
mempool creates per-entry response bookkeeping before the 25-entry inbound
download concurrency cap rejects excess work.

The work is bounded by the received inventory-message cap and by the mempool
download queue, so this is public DoS hardening rather than a private
consensus/security disclosure.

## Evidence

- `zebra-network/src/protocol/external/inv.rs:182-210` allows up to `50_000`
  inventory entries in a received inventory message, subject to the overall
  protocol message-size limit.
- `zebra-network/src/protocol/external/inv.rs:84-93` maps both legacy `Tx` and
  ZIP-239 `Wtx` inventory variants to `UnminedTxId`.
- `zebra-network/src/peer/connection.rs:1279-1295` maps any inbound `inv` with
  transaction inventory to `Request::AdvertiseTransactionIds(...)`.
- `zebra-network/src/peer/connection.rs:1815-1822` filters transaction IDs out
  of the full inventory list. The connection-layer request collects those IDs
  into a `HashSet`, so duplicate IDs in one `inv` are removed before the
  mempool call.
- `zebrad/src/components/inbound.rs:534-541` converts the whole set into
  mempool gossip entries and calls `mempool::Request::Queue`.
- `zebrad/src/components/mempool.rs:862-883` iterates every queued gossip item,
  creates a oneshot response channel for each item, checks storage, invokes the
  downloader, and collects one response result per input.
- `zebrad/src/components/mempool/downloads.rs:82-105` sets
  `MAX_INBOUND_CONCURRENCY` to `25`.
- `zebrad/src/components/mempool/downloads.rs:276-307` rejects already pending
  or over-cap downloads with `AlreadyQueued` / `FullQueue`, but that happens
  after the mempool queue path has already entered per-item bookkeeping. For a
  single large `inv` of new unique IDs, same-request overflow is primarily
  `FullQueue`; `AlreadyQueued` needs prior queue state for the same
  `UnminedTxId`.
- `zebrad/src/components/inbound/tests/fake_peer_set.rs` includes
  `advertise_transaction_ids_forwards_full_set_to_mempool_before_download_cap_today`,
  which sends `MAX_INBOUND_CONCURRENCY + 3` advertised transaction IDs to
  inbound and confirms the mock mempool receives the entire set in
  `mempool::Request::Queue`.
- `zebrad/src/components/mempool/tests/vector.rs` includes
  `mempool_queue_reports_every_gossiped_id_before_download_cap`, which queues
  `MAX_INBOUND_CONCURRENCY + 3` unique gossiped IDs against an enabled mempool,
  observes a full-length `Response::Queued` vector, and confirms only 25
  downloads remain in flight while the overflow entries return `FullQueue`.

The inbound service ignores the per-item queue response for transaction
advertisements:

- `zebrad/src/components/inbound.rs:536-540`

## Impact

This is a public P2P/mempool availability hardening issue.

An attacker can send one valid large transaction `inv` of unique transaction
IDs and make Zebra:

- build a large internal transaction-id set,
- allocate a large `Vec<Gossip>`,
- create response-channel bookkeeping per advertised ID,
- collect a large `Response::Queued` vector,
- discard that response at the inbound layer.

Only a small number of downloads can actually be started, so most of the
per-entry work is avoidable. Existing timeout/load-shed behavior, the inventory
count cap, the protocol message-size cap, and connection/message serialization
bound the damage.

If the mempool is disabled, Zebra still returns a full-length per-input
`Response::Queued` vector, but it takes the cheaper disabled path and does not
create downloader oneshots. The more interesting bookkeeping path requires an
enabled mempool.

## Suggested Fix

- Cap transaction IDs accepted from one inbound `inv` before forwarding them to
  the mempool queue.
- Prefer the downloader's `MAX_INBOUND_CONCURRENCY` or a small multiple as the
  first hardening cap.
- Add a queue API for fire-and-forget advertisements that does not create
  response channels when the caller ignores them.
- Penalize or disconnect peers that repeatedly send huge mostly-new transaction
  advertisements.

Disclosure triage: public hardening.

Confidence: high on current behavior, with local proof coverage at both the
inbound forwarding boundary and the mempool queue boundary; medium-low on
practical severity because the request is bounded and the downloader has a small
concurrency cap.

Verification rerun on 2026-05-09:

```sh
cargo test -p zebrad advertise_transaction_ids_forwards_full_set_to_mempool_before_download_cap_today --lib
cargo test -p zebrad mempool_queue_reports_every_gossiped_id_before_download_cap --lib
```

Result:

```text
test components::inbound::tests::fake_peer_set::advertise_transaction_ids_forwards_full_set_to_mempool_before_download_cap_today ... ok
test components::mempool::tests::vector::mempool_queue_reports_every_gossiped_id_before_download_cap ... ok
```

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "AdvertiseTransactionIds" "FullQueue"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "transaction inv" "mempool" "queue" "amplification"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "mempool" "MAX_INBOUND_CONCURRENCY" "inv"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra AdvertiseTransactionIds'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "transaction downloader" "FullQueue"'
```

Results:

- Targeted `AdvertiseTransactionIds` / `FullQueue`, transaction-inv queue
  amplification, and transaction-downloader `FullQueue` searches returned no
  hits.
- The `mempool` / `MAX_INBOUND_CONCURRENCY` / `inv` search returned #10565,
  #4500, and #2679. #10565 is the separate V5 pending-limit/exactness issue,
  while #4500 and #2679 are older mempool gRPC/downloader work.
- The broad `AdvertiseTransactionIds` search returned #6911 and PR #6625,
  among older implementation/test issues. PR #6625 is the closest overlap: it
  added sent-message inventory caps and gossip rate limiting, but it does not
  cap inbound peer advertisements before `mempool::Request::Queue` bookkeeping.
