# P2P Gossiped Address Services Unwrap Elimination Note

Date: 2026-05-03

Last updated: 2026-05-09

Disposition: local-only eliminated lead. Do not post publicly without explicit
re-authorization.

## Summary

`CandidateSet::send_addrs()` unwraps the result of
`MetaAddr::new_gossiped_change()` with the message "Received gossiped peers
always have services set". `AddrV1::from(MetaAddr)` also expects service bits
and a last-seen timestamp before serializing outbound `addr` messages. These
sites look attacker-adjacent because the input can originate from peer `addr` /
`addrv2` gossip or from the address book's `getaddr` response path.

I did not find a remote panic path. The P2P codec constructs all accepted
gossiped `MetaAddr`s with both service bits and a last-seen timestamp. The
address-book `getaddr` path sanitizes entries before serialization, filling
missing services with `NODE_NETWORK`, requiring a last-seen time, and dropping
entries that do not satisfy those invariants. Zebra does not send `addrv2`
messages in non-test code, so the `AddrV2::from(MetaAddr)` expects are not in
the live outbound P2P path.

This is an eliminated P2P panic lead, not a private disclosure candidate.

## Evidence: Gossiped Address Ingestion

- `zebra-network/src/peer_set/candidate_set.rs:345-352` maps incoming
  gossiped `MetaAddr`s through `MetaAddr::new_gossiped_change()` and unwraps.
- `zebra-network/src/meta_addr.rs:335-343` constructs gossiped `MetaAddr`s with
  `services: Some(untrusted_services)` and
  `untrusted_last_seen: Some(untrusted_last_seen)`.
- `zebra-network/src/meta_addr.rs:360-365` returns `None` if services are
  missing and only unwraps `untrusted_last_seen` after the constructor above has
  set it for gossiped entries.
- `zebra-network/src/protocol/external/addr/v1.rs:110-124` deserializes every
  v1 `addr` entry with service bits and last-seen time.
- `zebra-network/src/protocol/external/addr/v1.rs:87-94` converts v1 entries
  through `MetaAddr::new_gossiped_meta_addr()`, preserving those fields.
- `zebra-network/src/protocol/external/addr/v2.rs:270-309` deserializes
  supported IPv4/IPv6 `addrv2` entries with service bits and last-seen time,
  while unsupported network IDs become `AddrV2::Unsupported`.
- `zebra-network/src/protocol/external/addr/v2.rs:151-167` converts only
  supported `AddrV2::IpAddr` entries into `MetaAddr`.
- `zebra-network/src/protocol/external/codec.rs:635-649` filters unsupported
  `addrv2` entries with `filter_map(|addr| addr.try_into().ok())` before the
  address list becomes `Message::Addr`.

## Evidence: Outbound `addr` Serialization

- `zebra-network/src/protocol/external/codec.rs:274-283` serializes
  `Message::Addr` by converting every `MetaAddr` into `AddrV1`.
- `zebra-network/src/protocol/external/addr/v1.rs:66-77` expects
  `MetaAddr.services` and `MetaAddr::last_seen()` to be present before writing
  an `addr` entry.
- The inbound `getaddr` path maps a remote `Message::GetAddr` into
  `Request::Peers` (`zebra-network/src/peer/connection.rs:1345`), answers it
  from `CachedPeerAddrResponse` (`zebrad/src/components/inbound.rs:397-413`),
  and refreshes that cache through `AddressBook::fresh_get_addr_response()`
  (`zebrad/src/components/inbound/cached_peer_addr_response.rs:55-69`).
- `AddressBook::fresh_get_addr_response()` calls `AddressBook::sanitized()`
  (`zebra-network/src/address_book.rs:274-300`) before peers are cached for
  `getaddr` responses.
- `MetaAddr::sanitize()` requires a last-seen time and returns `None` if it is
  absent (`zebra-network/src/meta_addr.rs:707-719`). For entries that pass, it
  sets `services` to the existing value or `NODE_NETWORK` and sets
  `untrusted_last_seen: Some(last_seen)` (`zebra-network/src/meta_addr.rs:726-734`).
- The per-connection cached peer path also stores only `Message::Addr` entries
  decoded from the peer (`zebra-network/src/peer/connection.rs:1257-1264`) or
  alternate handshake addresses built with
  `MetaAddr::new_gossiped_meta_addr(..., NODE_NETWORK, DateTime32::now())`
  (`zebra-network/src/peer/handshake.rs:1113-1123`).
- `zebra-network/src/protocol/external/tests/prop.rs:110-123` has a property
  test that only serializes sanitized `MetaAddr`s and asserts that malicious
  gossiped/DNS-seeder addresses cannot make `AddrV1` serialization fail.

## Evidence: `addrv2` Serialization Is Test-Only

- `zebra-network/src/protocol/external/addr/v2.rs:117-143` gates
  `impl From<MetaAddr> for AddrV2` behind `#[cfg(test)]`.
- `zebra-network/src/protocol/external/addr/v2.rs:208-225` also gates
  `impl ZcashSerialize for AddrV2` behind `#[cfg(test)]`.
- The live message serializer always sends `addr` v1 messages for `Message::Addr`
  (`zebra-network/src/protocol/external/codec.rs:280-283`).

## Triage

Classification: eliminated as a remote P2P panic lead.

Residual hardening: the unwrap in `CandidateSet::send_addrs()` could still be
made self-documenting by filtering `None` from `new_gossiped_change()` instead
of using `expect()`, but current wire-decoding backstops make malformed remote
gossip unable to trigger it. Similarly, `AddrV1::from(MetaAddr)` could return a
typed error instead of panicking if it is ever called on unsanitized internal
state, but current live callers either receive decoded gossiped entries that
already carry both fields or use sanitized address-book responses.

## Local Verification

Focused tests rerun on 2026-05-09:

```sh
cargo test -p zebra-network addr_v1_sanitized_roundtrip --lib
cargo test -p zebra-network addr_v2_sanitized_roundtrip --lib
cargo test -p zebra-network parses_msg_addr_v1_ip --lib
cargo test -p zebra-network parses_msg_addr_v2_ip --lib
```

All four commands passed.

Duplicate checks on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra gossiped address services unwrap MetaAddr AddrV1 panic'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "Received gossiped peers always have services set"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "MetaAddrs should be sanitized before serialization"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "addrv2" "services" "panic"'
```

The exact panic-path searches returned no issue hits. The broader
`addrv2`/`services`/`panic` search returned only old closed PR #2976 ("Make
`services` field in `MetaAddr` optional"), which is historical context for the
optional-service invariant rather than a duplicate report of this reachability
question.
