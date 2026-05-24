## Summary

Zebra is permissive for a few malformed P2P wire encodings:

- `headers` entries use `CountedHeader`, whose transaction-count field should be zero for header-only messages. Zebra deserializes that count but ignores nonzero values.
- `block` messages can contain a valid serialized block prefix followed by trailing bytes. Zebra parses and processes the valid block prefix and discards the suffix.
- `tx` messages can contain a valid serialized transaction prefix followed by trailing bytes. Zebra parses and processes the valid transaction prefix and discards the suffix.
- A `block` or `tx` body can be padded above the 2,000,000-byte object parser limit while remaining below the 2 MiB P2P message cap, as long as the valid object prefix completes within the object parser limit.

This looks like low-severity public P2P conformance and malformed-peer-accounting hardening. The parsed block or transaction object still goes through normal consensus or mempool validation, so I do not think this is invalid-object acceptance.

## Evidence

- `zebra-chain/src/block/header.rs` documents `CountedHeader` as a block header with a transaction-count field that is always zero and not stored in the struct.
- `zebra-chain/src/block/serialize.rs` serializes counted headers with a zero transaction count, but deserializes the count into `_transaction_count` and ignores the value.
- `zebra-network/src/protocol/external/codec.rs` dispatches a cursor over the full P2P body to the command-specific parser, then only logs `extra data after decoding message` if trailing bytes remain.
- `block` and `tx` deserialization limit the object parser to `MAX_BLOCK_BYTES`, but the outer P2P body can be larger up to `MAX_PROTOCOL_MESSAGE_LEN`.

Local current-behavior tests from my audit reproduce the permissive behavior:

```sh
cargo test -p zebra-chain counted_header_nonzero_transaction_count_is_accepted_today --lib
cargo test -p zebra-network accepted_today --lib
```

The network test coverage includes:

- `headers_message_nonzero_transaction_count_is_accepted_today`
- `block_message_with_trailing_bytes_is_accepted_today`
- `tx_message_with_trailing_bytes_is_accepted_today`
- `block_message_padded_past_max_block_bytes_is_accepted_today`
- `tx_message_padded_past_max_block_bytes_is_accepted_today`

## Impact

Expected impact is bounded:

- Malformed `headers` entries with nonzero per-header transaction counts are accepted and processed.
- Malformed `block` / `tx` frames with trailing junk are accepted at the P2P decode layer.
- Extra junk bytes are hidden behind a debug log rather than becoming a parse error or misbehavior signal.

This does not appear to cause consensus divergence, unbounded memory retention, or a default remote crash. It can make Zebra more permissive than strict peers and weaker at accounting for malformed P2P traffic.

## Suggested Fix

- In `CountedHeader::zcash_deserialize`, reject any transaction count other than zero.
- In the P2P codec, require exact body consumption for `block` and `tx` messages.
- Preserve generic extra-byte leniency for other commands only if that behavior is intentional.
- Add regression tests based on the current-behavior tests above, but expecting decode/deserialization failure after the fix.

Duplicate checks before filing found closed PR #1920 as parser-adjacent history and closed PR #2446 as unrelated `MSG_WTX` parsing work. Searches for nonzero `headers` transaction counts and exact extra-data-log handling returned no exact issue.
