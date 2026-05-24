# P2P Addr/AddrV2 Count-Cap Regression Triage

Date: 2026-05-09

Scope: follow-up on the apparent late `addr` / `addrv2` count-cap shape after
the post-v4.4.0 deserialization audit.

## Summary

This does not look like a fresh vulnerability in the current tree. The
`read_addr()` and `read_addrv2()` helpers still perform a visible
`len() > MAX_ADDRS_IN_MESSAGE` check after deserializing a `Vec`, but the
effective preallocation guard now fires earlier in the shared
`Vec<T: TrustedPreallocate>` deserializer.

Classification: duplicate/regression-verification of the already fixed
`AddrV1` / `AddrV2` GHSA shape, with optional local hardening if maintainers
want the message readers to mirror `read_headers()` more explicitly.

## Evidence

- `Codec::decode()` dispatches inbound `addr` and `addrv2` frames to
  `read_addr()` and `read_addrv2()` after enforcing the outer message body
  length cap.
- `read_addr()` currently calls `reader.zcash_deserialize_into::<Vec<AddrV1>>()`
  before the redundant post-deserialize `MAX_ADDRS_IN_MESSAGE` check.
- `read_addrv2()` does the same for `Vec<AddrV2>`.
- `Vec<T>::zcash_deserialize()` reads the compact count and then calls
  `zcash_deserialize_external_count(count, reader)`.
- `zcash_deserialize_external_count()` rejects
  `external_count > T::max_allocation()` before `Vec::with_capacity`.
- `AddrV1::max_allocation()` and `AddrV2::max_allocation()` both return
  `MAX_ADDRS_IN_MESSAGE as u64`, with comments pointing to
  GHSA-xr93-pcq3-pxf8.
- `AddrV2` additionally caps each `addr` byte field at
  `MAX_ADDR_V2_ADDR_SIZE` before allocating the per-entry byte vector.

Important reachability note: the decode path is reachable from unauthenticated
remote peers as soon as the framed codec is active. During handshake,
`negotiate_version()` decodes and ignores non-`version` and non-`verack`
messages while waiting for the expected handshake message. After handshake,
unsolicited `Message::Addr` is consumed by `Connection::handle_message_as_request`
and stored in the per-connection address cache, which is bounded by
`MAX_ADDRS_IN_MESSAGE`.

## Bounds Today

- Outer body length is bounded by the codec's `max_len`
  (`MAX_PROTOCOL_MESSAGE_LEN` by default).
- Vector count is bounded before preallocation by
  `TrustedPreallocate::max_allocation() == MAX_ADDRS_IN_MESSAGE`.
- `addrv2` per-entry address bytes are bounded at 512 bytes.
- Retained unsolicited address state is bounded by the per-connection cache
  truncation.

The remaining attacker-controlled work is bounded parsing and cache churn for
valid or near-valid messages, not the old oversized upfront heap allocation.

## Duplicate And Related Public Coverage

Read-only issue searches found:

- PR #10563: shared `zcash_deserialize_external_count` reservation hardening,
  explicitly referencing GHSA-xr93-pcq3-pxf8 and the `AddrV1` / `AddrV2`
  sibling.
- Issue #10545: generic `Vec::with_capacity` preallocation from untrusted
  varints, also referencing the `AddrV1` / `AddrV2` fix.
- PR #10570: protocol-level caps for `block::Hash` and `CountedHeader`, with
  context that `AddrV1` / `AddrV2` were already capped by PR #10494.
- Closed PR #3022: original addrv2 parsing work and older count-cap history.

No new public issue should be opened for this shape unless a separate bypass is
found.

## Verification

```sh
cargo test -p zebra-network poc_remote_addrv2_resource_exhaustion --lib
cargo test -p zebra-network addr_v --lib
```

Result on 2026-05-09: both passed.

The second test filter runs the existing `AddrV1` / `AddrV2` max-allocation
property tests along with related address-vector tests.

## Optional Hardening

If maintainers want clearer reader-local invariants, `read_addr()` and
`read_addrv2()` could be refactored to mirror `read_headers()`:

1. read `CompactSizeMessage`,
2. reject `count > MAX_ADDRS_IN_MESSAGE`,
3. call `zcash_deserialize_external_count(count, reader)`.

That would be a maintainability/regression-proofing change rather than a fresh
security fix.
