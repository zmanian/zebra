# P2P Block, Transaction, and Header Parse Strictness Note

Date: 2026-05-03

Last updated: 2026-05-09

Status: publicly covered by
[#10569](https://github.com/ZcashFoundation/zebra/issues/10569), filed on
2026-05-09 after explicit re-authorization.

Scope: P2P wire strictness for `headers` counted-header transaction counts,
`block` message bodies with trailing bytes after a syntactically complete block,
and `tx` message bodies with trailing bytes after a syntactically complete
transaction.

## Summary

This is public P2P hardening, not a consensus bug.

Two parser leniency cases are present in the current checkout:

- `CountedHeader` deserialization reads but ignores the per-header transaction
  count in `headers` messages. Header-only counts should be zero, but Zebra
  accepts nonzero values. This was already noted as a low-severity parser
  conformance item.
- `block` and `tx` message parsers accept a valid serialized object prefix even
  if the P2P message body has trailing bytes. That includes a body whose total
  length is above the 2,000,000-byte object parser limit but still below the 2
  MiB P2P message limit, as long as the parsed object prefix itself is valid.

The accepted block or transaction object is still the parsed prefix and later
consensus or mempool checks operate on that object. The extra bytes are
discarded, so this is not invalid block or transaction acceptance. The concern
is malformed-peer accounting, protocol conformance, and bounded extra
bandwidth/checksum/parser work.

## Evidence

Headers:

- `zebra-chain/src/block/header.rs:144-152` documents `CountedHeader` as a
  block header with a transaction-count field that is always zero and is not
  stored in the struct.
- `zebra-chain/src/block/serialize.rs:110-119` serializes counted headers with a
  zero transaction count.
- `zebra-chain/src/block/serialize.rs:124-134` deserializes the count into
  `_transaction_count` and ignores the value.
- `zebra-network/src/protocol/external/codec.rs:681-694` enforces the outer
  `headers` message count cap before deserializing counted headers.

Block and transaction trailing bytes:

- `zebra-chain/src/block/serialize.rs:149-163` limits the block parser to
  `MAX_BLOCK_BYTES` using `reader.take(MAX_BLOCK_BYTES)` and parses only a
  header plus transaction vector.
- `zebra-chain/src/block/serialize.rs:18-24` sets `MAX_BLOCK_BYTES` to
  2,000,000.
- `zebra-chain/src/serialization/zcash_serialize.rs:7-10` sets the P2P message
  body cap to `2 * 1024 * 1024`.
- `zebra-network/src/protocol/external/codec.rs:396-406` accepts message bodies
  up to the codec max and reserves for the full declared body.
- `zebra-network/src/protocol/external/codec.rs:440-457` dispatches a cursor over
  the full body to the command-specific parser, including `block` and `tx`.
- `zebra-network/src/protocol/external/codec.rs:478-489` computes remaining
  bytes after parsing but only logs `extra data after decoding message`, then
  returns the parsed message.
- `zebra-network/src/protocol/external/codec.rs:656-658` maps a parsed block body
  into `Message::Block`.
- `zebra-network/src/protocol/external/codec.rs:724-727` maps a parsed
  transaction body into `Message::Tx`.
- `zebra-network/src/protocol/external/codec.rs:777-797` deserializes
  transactions in the Rayon pool via `Transaction::zcash_deserialize(reader)`.
- `zebra-chain/src/transaction/serialize.rs:768-784` limits the transaction
  parser with `reader.take(MAX_BLOCK_BYTES)` and does not require the outer P2P
  body cursor to be exhausted after a valid transaction is parsed.

The padded-prefix case is limited to messages where the parsed block or
transaction itself completes within `MAX_BLOCK_BYTES`. Zebra does not accept an
object whose required serialized bytes exceed that object limit; the leniency is
that the P2P body may be longer, up to `MAX_PROTOCOL_MESSAGE_LEN`, if the suffix
is just ignored trailing data.

Existing coverage confirms nearby limits but not this strictness:

- `zebra-chain/src/block/tests/vectors.rs:270-305` and
  `zebra-chain/src/block/tests/vectors.rs:307-339` test direct block
  deserialization around the 2,000,000-byte object limit.
- `zebra-network/src/protocol/external/codec/tests/vectors.rs:657-715` tests
  the `headers` outer count cap, but not the per-header transaction count.

Local proof coverage now exercises the permissive behavior directly:

- `counted_header_nonzero_transaction_count_is_accepted_today` appends canonical
  `CompactSize(1)` after a valid header and confirms
  `CountedHeader::zcash_deserialize` still returns the header.
- `headers_message_nonzero_transaction_count_is_accepted_today` mutates an
  encoded one-header `headers` message from transaction count `0` to `1`,
  recomputes the checksum, and confirms Zebra still decodes it as `Headers`.
- `block_message_with_trailing_bytes_is_accepted_today` appends junk bytes to an
  encoded genesis `block` message, updates the body length and checksum, and
  confirms Zebra returns the parsed block prefix.
- `tx_message_with_trailing_bytes_is_accepted_today` does the same for an encoded
  genesis coinbase `tx` message.
- `block_message_padded_past_max_block_bytes_is_accepted_today` and
  `tx_message_padded_past_max_block_bytes_is_accepted_today` pad otherwise valid
  messages to `MAX_BLOCK_BYTES + 1` while staying within
  `MAX_PROTOCOL_MESSAGE_LEN`, then confirm Zebra still returns the parsed prefix.

## Impact

Expected impact is bounded P2P conformance and malformed-peer-accounting
weakness:

- A peer can send malformed `headers` entries with nonzero per-header
  transaction counts and still have the headers processed.
- A peer can send `block` messages with valid block bytes plus trailing junk and
  still have the parsed block processed.
- A peer can send `tx` messages with valid transaction bytes plus trailing junk
  and still have the parsed transaction processed.
- A peer can pad a valid block or transaction body above 2,000,000 bytes, up to
  the 2 MiB codec cap, and Zebra will still parse the valid prefix, as long as
  the prefix object itself completes within 2,000,000 bytes.

This does not create unbounded retained memory or consensus acceptance of the
junk suffix. It can make Zebra more permissive than strict peers and can hide
malformed traffic behind a debug-only "extra bytes" log instead of a parse error
or misbehavior path.

## Suggested Fix

Keep the change targeted:

- In `CountedHeader::zcash_deserialize`, reject any counted-header transaction
  count other than zero.
- In the codec, require exact body consumption for `block` and `tx` messages.
  Preserve the existing generic extra-byte leniency for other commands unless
  the team decides to tighten those separately.

Current-behavior proof tests:

- A chain-level `CountedHeader` test that appends `CompactSize(1)` after a valid
  header and confirms parse acceptance today.
- Codec-level `headers`, `block`, and `tx` tests that mutate the frame body,
  recompute the checksum, and confirm decode acceptance today.

Post-fix regression tests should use the same constructions but expect
`Codec::decode` or `CountedHeader::zcash_deserialize` to fail.

## Triage

Public hardening.

The `CountedHeader` behavior is duplicate of the earlier low-severity
conformance note in `docs/analysis/sighash-single-corresponding-output-finding.md`.
The `block` and `tx` trailing-byte shape is a fresh continuation finding, but it
should be handled publicly because the parsed object remains validated by the
normal block consensus or mempool path and the extra work is bounded by existing
message-size limits.

Confidence: high for current behavior from source inspection and local proof
tests; medium on practical exploitability because the main effect is stricter
malformed-peer handling rather than default remote crash or consensus
divergence.

Verification rerun on 2026-05-09:

```sh
cargo test -p zebra-chain counted_header_nonzero_transaction_count_is_accepted_today --lib
cargo test -p zebra-network accepted_today --lib
```

Result: the counted-header proof passed, and the `accepted_today` network test
run covered all five focused codec proofs listed above.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "counted header" "transaction count"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "headers" "nonzero transaction count"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "trailing bytes" "block" "tx" "message"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "extra data after decoding message"'
```

Results:

- The counted-header search returned merged PR #1920, which implemented trusted
  vector preallocation. It is parser-adjacent history, not a duplicate report of
  nonzero counted-header transaction-count acceptance.
- The trailing-bytes search returned merged PR #2446, which added ZIP-239
  `MSG_WTX` inventory parsing. It is unrelated to accepting trailing bytes after
  parsed `block` / `tx` message prefixes.
- The nonzero-headers and exact extra-data-log searches returned no hits.
