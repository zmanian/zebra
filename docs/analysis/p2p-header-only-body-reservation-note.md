# P2P Header-Only Body Reservation Note

Date: 2026-05-03

Last updated: 2026-05-08

Disposition: public issue filed after explicit re-authorization on 2026-05-08.
Issue: https://github.com/ZcashFoundation/zebra/issues/10564

Scope: follow-up audit of P2P codec memory behavior for remote peer-controlled
wire headers.

## Finding

Zebra's P2P codec reserves buffer capacity for the entire declared message body
as soon as it has parsed a valid 24-byte message header. The body length is
bounded by the global 2 MiB protocol message limit, but the reservation happens
before any body bytes arrive.

A peer can therefore send a valid header declaring a maximum-sized body and then
stall. Zebra will keep the connection in `DecodeState::Body` waiting for the
missing body, while the `BytesMut` backing the framed read has already grown to
hold the declared body.

This is not a consensus issue and not an unbounded single-connection
allocation. It is a public P2P availability hardening issue: the attacker spends
24 bytes of application payload per connection to induce roughly 2 MiB of
application buffer capacity, bounded by connection limits and connection
timeouts/heartbeats.

## Evidence

- `zebra-network/src/protocol/external/codec.rs:63-68` sets the default codec
  body limit to `MAX_PROTOCOL_MESSAGE_LEN`.
- `zebra-chain/src/serialization/zcash_serialize.rs:10` defines
  `MAX_PROTOCOL_MESSAGE_LEN` as `2 * 1024 * 1024`.
- `zebra-network/src/protocol/external/codec.rs:393-398` rejects only declared
  body lengths above the configured maximum.
- `zebra-network/src/protocol/external/codec.rs:405-406` then calls
  `src.reserve(body_len + HEADER_LEN)` before receiving or validating the body.
- `zebra-network/src/protocol/external/codec.rs:422-425` waits for the full
  body once the decoder is in `DecodeState::Body`.
- `zebra-network/src/constants.rs:64-86` makes the default target peer count 25,
  with inbound and outbound multipliers of 5 and 3.
- `zebra-network/src/config.rs:222-240` derives default connection limits from
  those multipliers, giving 125 inbound plus 75 outbound connections by default.
- `zebra-network/src/peer/handshake.rs:1166-1169` wraps the initial handshake in
  a 3 second timeout.
- `zebra-network/src/peer/handshake.rs:1343-1346` waits one heartbeat interval
  after a completed handshake before sending the first heartbeat, and
  `zebra-network/src/peer/handshake.rs:1443` uses the heartbeat interval as the
  response timeout.

Local proof test added in this checkout:

```bash
cargo test -p zebra-network header_only_max_body_len_reserves_full_body_capacity_today --lib
```

The test builds a header-only frame with the correct network magic, `version`
command, a declared body length of `MAX_PROTOCOL_MESSAGE_LEN`, and no body. A
direct call to `Codec::decode()` returns `Ok(None)` and leaves the input buffer
empty but with capacity at least `MAX_PROTOCOL_MESSAGE_LEN + HEADER_LEN`.

## Duplicate Check

Read-only GitHub searches on 2026-05-07:

```sh
gh issue list --repo ZcashFoundation/zebra --search 'header-only body reservation in:title,body' --state all --limit 100
gh issue list --repo ZcashFoundation/zebra --search 'Codec reserve body_len HEADER_LEN in:title,body' --state all --limit 100
gh issue list --repo ZcashFoundation/zebra --search 'P2P header declared body length memory in:title,body' --state all --limit 100
gh issue list --repo ZcashFoundation/zebra --search 'MAX_PROTOCOL_MESSAGE_LEN reserve stalled peer in:title,body' --state all --limit 100
```

Results: no hits.

## Attack Shape

One shape is:

1. Open inbound peer connections up to Zebra's configured limit.
2. Complete or begin the P2P handshake.
3. Send a valid 24-byte Zcash message header with `body_len = 2 MiB`.
4. Send no body bytes, or send them very slowly.

Pre-handshake attempts are limited by the handshake timeout. Post-handshake
attempts can persist until heartbeat/request timeout machinery closes the
connection, and the first heartbeat is intentionally delayed by one heartbeat
interval after handshake completion.

The default retained reservation upper bound is roughly:

```text
(25 * (5 + 3)) connections * 2 MiB = about 400 MiB
```

This is an estimate for application buffer capacity, not an exact resident-set
increase.

## Impact

Suggested severity: public P2P availability hardening, medium-low to medium
depending on deployment limits.

The finding is weaker than a crash or consensus divergence because:

- the per-connection allocation is capped by the protocol message limit,
- connection counts are bounded,
- handshakes and heartbeats eventually close stalled peers,
- network-layer connection controls can further reduce exposure.

It is still worth fixing because the amplification ratio is poor: a tiny header
can force a maximum-body buffer reservation before the peer has committed to
sending the body.

## Suggested Fix

- Avoid reserving the entire declared body after parsing only the header.
  Reserve a small incremental read amount, or reserve only what is currently
  missing up to a bounded chunk size.
- Add a per-frame body receive timeout or progress timeout, especially after a
  header declares a large body.
- Consider lowering accepted body limits for message types with known smaller
  protocol caps before reserving, rather than using the global 2 MiB cap for
  every command.
- Keep the local repro test and invert it after the fix so header-only frames do
  not increase buffer capacity to the full declared body size.

## Confidence

Confidence: high that the pre-body reservation exists and is remote peer
influenced. Confidence: medium on practical impact because exact memory pressure
depends on connection-limit configuration, allocator behavior, and how quickly
handshake/heartbeat timeouts close the stalled connections.
