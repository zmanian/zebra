# P2P Unsolicited Block Decode Hardening Note

Date: 2026-05-02

Last updated: 2026-05-09

Disposition: local-only. Do not post publicly without explicit
re-authorization and a fresh duplicate check.

Scope: follow-up on P2P full `block` and `tx` message handling in
`zebra-network/src/protocol/external/codec.rs`,
`zebra-network/src/peer/connection.rs`, and the inbound service stack.

## Finding

Zebra eagerly deserializes full `block` and `tx` messages in the network codec
before the peer connection state machine decides whether the message was
requested, useful, unsolicited, or mismatched. For unsolicited full blocks, this
means a peer can force bounded but potentially expensive block deserialization
work that is later ignored and does not pass through the inbound request
service's overload/backpressure path.

The same eager-decode shape also exists before a peer is fully admitted: during
handshake, Zebra waits through non-`version` and non-`verack` messages by asking
the same framed codec for the next `Message`. A peer can therefore send a full
`block` or `tx` frame before completing the handshake; Zebra decodes it, ignores
it as the wrong handshake message, and continues until the handshake timeout.

This is not consensus acceptance and not unbounded retained memory. It is public
P2P availability hardening.

## Evidence

- `zebra-network/src/protocol/external/codec.rs:656-658` decodes any incoming
  `block` frame into `Message::Block(Arc<Block>)` before the connection layer
  can decide if the block was requested.
- `zebra-network/src/protocol/external/codec.rs:724-727` similarly decodes any
  incoming `tx` frame into `Message::Tx(UnminedTx)`.
- `zebra-network/src/protocol/external/codec.rs:775-819` runs block and
  transaction deserialization through `tokio::task::block_in_place()` and Rayon,
  while the connection task waits for the result.
- `zebra-network/src/peer/handshake.rs:694-712` waits for a `Version` message
  by decoding messages from the same framed codec and ignoring non-`version`
  messages.
- `zebra-network/src/peer/handshake.rs:816-838` repeats that pattern while
  waiting for `Verack`.
- `zebra-network/src/peer/handshake.rs:1166-1168` bounds the whole handshake by
  `HANDSHAKE_TIMEOUT`, limiting the pre-admission subcase.
- `zebra-network/src/peer/connection.rs:1229-1231` treats unsolicited `Block`
  messages as `Unused` after they have already been decoded.
- `zebra-network/src/peer/connection/tests/vectors.rs` includes
  `unsolicited_block_message_is_unused_without_inbound_request_today`, which
  sends a full `Message::Block` through the connection loop and confirms it
  does not reach the inbound service or produce an outbound response.
- `zebra-network/src/peer/connection/tests/vectors.rs` also includes
  `mismatched_block_response_is_decoded_then_ignored_today`, which requests one
  block, sends a different already-decoded `Message::Block`, confirms the
  active response remains pending with no inbound-service routing, and then
  confirms the request completes when the requested block arrives.
- `zebra-network/src/peer/handshake/tests.rs` includes
  `handshake_decodes_and_ignores_pre_version_block_today`, which sends a real
  encoded `block` frame before the remote peer's `version` message and confirms
  the handshake still succeeds after Zebra decodes and ignores the non-handshake
  block.
- `zebra-network/src/peer/connection.rs:1278` maps unsolicited `Tx` messages
  into `Request::PushTransaction`, so they do at least enter the inbound service
  and mempool queue/verification limits.
- `zebra-network/src/peer/connection.rs:298-340` handles mismatched block
  responses by ignoring the decoded block and waiting for the requested block
  until the request completes or times out.
- `zebra-network/src/peer/connection.rs:1381-1434` applies inbound-service
  readiness, timeout, and overload handling only to messages mapped into
  internal `Request`s.
- `zebrad/src/commands/start.rs:168-175` wraps the inbound service in
  `load_shed`, a bounded buffer, and `MAX_INBOUND_RESPONSE_TIME`.
- `zebrad/src/components/inbound/downloads.rs:37-49` documents the block
  download/verification memory bound after blocks enter the inbound download
  path, including the 9 MB deserialized malicious-block estimate and
  `MAX_INBOUND_CONCURRENCY = 200`.
- `zebrad/src/components/mempool/downloads.rs:88-105` documents the corresponding
  transaction-side bounds, with `MAX_INBOUND_CONCURRENCY = 25`.

## Duplicate Check

Read-only GitHub searches on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "unsolicited block" "inbound"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "full block" "decoded" "unsolicited"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "handshake" "full block" "message"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "mismatched block" "decoded" "ignored"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "Message::Block" "Unused"'
```

Results:

- Direct unsolicited/full-block/mismatched-block searches returned no duplicate
  hits.
- The handshake search returned #10545, the generic untrusted-vector
  preallocation issue, which is adjacent deserialization hardening but not this
  unsolicited full-block routing path.
- Broader `Message::Block` / `Unused` searches returned unrelated or historical
  PRs such as #6662, #5257, #3295, and #2660.

Local proof status: strengthened current-behavior coverage. The connection tests
prove that an already-decoded unsolicited `Message::Block` is treated as unused
without inbound-service routing, and that a mismatched full block during an
active `BlocksByHash` request is ignored while the request remains pending for
the requested block. The handshake test proves the raw pre-version frame
subcase: Zebra decodes a full `block` frame before the remote peer's `version`
message, ignores it as a non-handshake message, and completes the handshake
after the peer sends normal `version` / `verack` messages.

## Impact

Expected impact is bounded CPU and memory churn on public P2P connections:

- a peer can send well-formed, protocol-size-limited `block` messages that Zebra
  fully deserializes and then ignores when no block was requested from that peer,
- before the handshake completes, a peer can send well-formed full `block` or
  `tx` frames that are fully decoded and then ignored as non-handshake messages,
  bounded by the handshake timeout,
- a peer can send mismatched full blocks during a pending block request; Zebra
  ignores them and keeps waiting for the requested hashes until timeout,
- this work happens before the inbound service can shed load, so it is not
  constrained by the normal inbound block-download queue,
- `tx` messages have the same eager decode property, but unsolicited
  transactions are forwarded into the bounded inbound/mempool path after decode.

The global 2 MiB wire message cap, peer connection limits, per-peer sequential
processing, and request timeouts keep this out of private disclosure territory.
But it is a clean hardening target because unsolicited full blocks are not useful
to Zebra's current protocol flow.

## Suggested Fix

- Track unexpected full `block` messages per connection and disconnect or reduce
  peer score after a small threshold, especially when no `BlocksByHash` request
  is pending.
- During handshake, reject unexpected full-body commands before expensive
  deserialization, or disconnect after the first non-handshake message rather
  than decoding arbitrary protocol frames until timeout.
- During `BlocksByHash`, count mismatched full blocks and fail the pending
  request or disconnect after a small threshold instead of waiting until
  `REQUEST_TIMEOUT` while continuing to parse more mismatches.
- Consider a lazy/raw message boundary for expensive body types so the
  connection state machine can reject unsupported or unsolicited full blocks
  before full block deserialization.
- Add regression tests for:
  - unsolicited full block is not routed to inbound service today,
  - repeated mismatched full blocks during `BlocksByHash` only cause bounded
    timeout and do not retain state,
  - any future disconnect/misbehavior threshold triggers before repeated
  unsolicited blocks consume sustained decode work.

## Verification

```sh
cargo test -p zebra-network handshake_decodes_and_ignores_pre_version_block_today --lib
cargo test -p zebra-network mismatched_block_response_is_decoded_then_ignored_today --lib
```

Result on 2026-05-09: both passed.

## Confidence

Confidence: medium-high as public availability hardening.

The eager decode and later-ignore paths are direct in the code. The exact impact
depends on peer count, block body shape, network bandwidth, and runtime CPU
capacity. The finding does not show invalid-block acceptance, unbounded retained
state, or a remotely reachable panic.
