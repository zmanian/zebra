# P2P BIP37 Filter Message Hardening Note

Date: 2026-05-02

Last updated: 2026-05-09

Status: publicly covered by
[#10568](https://github.com/ZcashFoundation/zebra/issues/10568), filed on
2026-05-09 after explicit re-authorization.

Scope: follow-up on P2P protocol message parsing in
`zebra-network/src/protocol/external/codec.rs` and
`zebra-network/src/peer/connection.rs`.

## Finding

Zebra ignores BIP37 bloom-filter messages because it does not advertise or
implement `NODE_BLOOM`, but the codec still parses `filterload`, `filteradd`,
and `filterclear` into normal `Message` variants. Those variants are consumed
inside the peer connection without going through the inbound request service.

This creates a public P2P availability hardening lead:

- `filteradd` accepts any protocol-body length up to the global 2 MiB message
  limit, truncates the parsed data to 520 bytes, and silently treats the
  remaining body bytes as extra data.
- `filterload` bounds total body size, but does not enforce the documented
  `hash_functions_count <= 50` rule because the message is ignored later.
- bodyless messages such as `mempool`, `getaddr`, `filterclear`, and `verack`
  also accept and discard extra body bytes under the codec's general
  extra-fields policy. Their downstream handling differs: `getaddr` and
  `mempool` still become inbound service requests, `verack` is handled as a
  duplicate handshake, and BIP37 `filterclear` is consumed with the other
  unsupported filter messages.
- ignored filter messages are marked `Consumed`, so they do not exercise the
  inbound service overload/load-shed path that normally helps disconnect peers
  generating too much inbound request work.

This is not a consensus issue. It is also not an unbounded allocation issue,
because the global protocol message size still caps each decoded body. The
practical risk is bounded per-peer CPU, bandwidth, buffer reservation, and
connection scheduling pressure from messages Zebra does not need to support.

## Evidence

- `zebra-network/src/protocol/external/codec.rs:63-68` sets the default codec
  body limit to `MAX_PROTOCOL_MESSAGE_LEN`.
- `zebra-network/src/protocol/external/codec.rs:396-406` accepts any body length
  up to that limit and reserves `body_len + HEADER_LEN` bytes before body
  decoding.
- `zebra-network/src/protocol/external/codec.rs:480-488` allows extra bytes
  after decoding most messages and only logs them.
- `zebra-network/src/protocol/external/message.rs:243-285` documents that
  `filterload` and `filteradd` are ignored; it also documents a 36,000 byte
  filter limit, a 50 hash-function limit, and a 520 byte `filteradd` data
  limit.
- `zebra-network/src/protocol/external/codec.rs:733-758` validates the
  `filterload` body length, but parses `hash_functions_count` as an unrestricted
  `u32`.
- `zebra-network/src/protocol/external/codec.rs:761-768` parses `filteradd` by
  taking `min(body_len, 520)`, leaving any remaining body bytes as accepted
  extra data.
- `zebra-network/src/protocol/external/codec.rs:729-730` parses `mempool`
  without requiring an empty body.
- `zebra-network/src/peer/connection.rs:1237-1249` consumes BIP37 messages
  after logging that they arrived without `NODE_BLOOM`; it does not forward them
  to `drive_peer_request()`.
- `zebra-network/src/peer/connection.rs:1381-1434` applies inbound-service
  readiness, timeout, and overload handling only to messages mapped into
  `Request`s.
- `zebra-network/src/protocol/external/types.rs` defines advertised peer
  services without a `NODE_BLOOM` bit, and the `VersionMessage::relay`
  documentation says Zebra does not implement BIP37 bloom filters.

Local proof tests:

```bash
cargo test -p zebra-network filteradd_message_too_large_is_truncated_and_accepted_today --lib
cargo test -p zebra-network accepted_today --lib
cargo test -p zebra-network message_with_body_is_accepted_today --lib
cargo test -p zebra-network bip37_filter_messages_are_consumed_without_inbound_request_today --lib
```

The proof coverage constructs a 2048-byte `filteradd` and confirms truncation
plus acceptance, a `filterload` with `hash_functions_count = 51` and confirms
acceptance, `filterclear` / `mempool` / `getaddr` / `verack` messages with body
bytes and confirms codec-level acceptance, and BIP37 `filterload` / `filteradd`
/ `filterclear` messages sent through the peer connection and confirms they are
consumed without inbound service requests.

## Impact

Expected impact is bounded availability degradation:

- a peer can send large ignored `filteradd` bodies that are decoded, checksumed,
  partially copied, and logged as extra data,
- those ignored messages do not consume inbound-service permits and therefore do
  not directly trigger the normal inbound overload disconnect path,
- `getaddr` and `mempool` messages with extra body bytes are accepted by the
  codec, but unlike BIP37 filters they still enter the inbound service path,
- continuous ignored-message traffic can delay Zebra-originated work on that
  peer connection until other timeout or keepalive mechanisms intervene.

This should be handled publicly. It does not create invalid-block acceptance,
unbounded retained memory, or a high-confidence default-configuration remote
crash.

## Suggested Fix

- Reject BIP37 messages at the codec/message boundary when Zebra has not
  negotiated `NODE_BLOOM`, or disconnect/count them through an explicit
  misbehavior path rather than treating them as harmless consumed messages.
- Make exact-size validation local for empty-body messages that Zebra treats as
  active requests, especially `mempool` and `getaddr`.
- Change `filteradd` parsing to reject `body_len > 520` instead of truncating
  and accepting extra bytes.
- Enforce `filterload.hash_functions_count <= 50` even if the message is
  ignored, so the parser matches the documented protocol shape.
- Add regression tests for oversized `filteradd`, non-empty `mempool`, non-empty
  `filterclear`, non-empty `getaddr` / `verack`, and `filterload` with too many
  hash functions.

## Confidence

Confidence: medium as public hardening.

The code evidence is direct. The practical severity is lower because Zebra
ignores BIP37 semantics, message size is globally capped, peer counts are
bounded, and operators can rely on network-layer limits. The finding is still
worth fixing because Zebra does not need to spend normal peer-processing work on
unsupported filter messages.

Verification rerun on 2026-05-09:

```sh
cargo test -p zebra-network accepted_today --lib
cargo test -p zebra-network bip37_filter_messages_are_consumed_without_inbound_request_today --lib
```

Result: all matching codec current-behavior tests passed, including the focused
`filteradd`, `filterload`, `filterclear`, `mempool`, `getaddr`, and `verack`
cases listed above. The peer-connection proof also passed.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "BIP37" "filterload" "filteradd" "filterclear"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "filteradd" "520"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "mempool" "getaddr" "verack" "extra bytes"'
```

Results:

- The BIP37 and `filteradd` searches returned open #10315, a broad one-line
  tracker to review network conformance with ZIP-204. This is relevant
  background but not a duplicate of the concrete unsupported-BIP37 and
  extra-body acceptance paths.
- The `filteradd` search also returned closed PR #520, an unrelated dependency
  bump.
- The `mempool` / `getaddr` / `verack` extra-bytes search returned no hits.
