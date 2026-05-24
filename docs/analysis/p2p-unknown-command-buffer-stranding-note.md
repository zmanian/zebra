# P2P Unknown-Command Buffer Stranding Note

Date: 2026-05-07

Last updated: 2026-05-07

Status: public P2P availability hardening. This is already publicly tracked as
[#10553](https://github.com/ZcashFoundation/zebra/issues/10553).

## Summary

When `Codec::decode()` receives a complete frame with an unknown command, it
consumes the frame, verifies its checksum, resets to `DecodeState::Head`, logs
the unknown command, and returns `Ok(None)`.

For a `tokio_util::codec::Decoder`, `Ok(None)` means no complete frame is
available yet. If the same socket read already buffered another valid frame
after the unknown frame, `FramedRead` can wait for another read instead of
delivering the buffered valid frame immediately.

## Evidence

- `zebra-network/src/protocol/external/codec.rs:431-432` consumes the message
  body and resets the decoder state.
- `zebra-network/src/protocol/external/codec.rs:462-473` handles the unknown
  command by returning `Ok(None)`.
- Durable current-behavior test
  `unknown_command_before_valid_frame_strands_buffered_frame_today` writes an
  unknown frame followed by a valid `ping` frame into a `tokio::io::duplex`
  stream while keeping the writer open. `FramedRead::next()` times out instead
  of yielding the already-buffered ping; after EOF, the same buffered ping is
  decoded.

## Verification

```sh
cargo test -p zebra-network unknown_command_before_valid_frame_strands_buffered_frame_today --lib
cargo test -p zebra-network protocol::external::codec::tests::vectors --lib
```

Result: passed.

## Impact

This is not a consensus issue and not a process crash. It is a connection-level
availability/correctness issue: a peer can delay already-buffered valid messages
behind ignored unknown-command frames until another socket read or EOF wakes the
decoder.

The practical severity is bounded by normal peer connection limits, timeouts,
and the fact that any later read makes progress. The behavior is still worth
fixing because it violates the streaming decoder contract and can introduce
avoidable latency or stalls on otherwise valid peer traffic.

## Fix Direction

Either:

- return a sentinel `Message` variant for ignored unknown commands so the
  connection handler can discard it while the decoder still reports a completed
  frame; or
- restructure `decode()` so that after consuming an unknown frame it loops and
  attempts to decode the next buffered frame before returning `Ok(None)`.
