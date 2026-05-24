# Sapling TransmissionKey Public API Panic Note

Date: 2026-05-03

Last updated: 2026-05-09

Disposition: local-only residual/adjacent to #5476. Do not post publicly
without explicit re-authorization.

Scope: follow-up on cryptographic point-decoding unwraps in `zebra-chain`,
separate from the already documented RPC `z_listunifiedreceivers` invalid
Sapling receiver panic.

## Finding

`zebra_chain::sapling::keys::TransmissionKey::try_from([u8; 32])` can panic on
malformed Jubjub bytes even though its `TryFrom` contract says malformed,
non-canonical, or non-subgroup encodings should return `Err`.

This is a public library API panic/hardening issue. I did not find a current
Zebra node, consensus, mempool, or RPC path that calls this constructor on
attacker-controlled network input.

## Evidence

- `zebra-chain/src/sapling/keys.rs:210-226` implements
  `TryFrom<[u8; 32]> for TransmissionKey`.
- `zebra-chain/src/sapling/keys.rs:213-215` documents that malformed,
  non-canonical, or non-prime-subgroup byte encodings should fail.
- `zebra-chain/src/sapling/keys.rs:219` calls
  `jubjub::AffinePoint::from_bytes(bytes).unwrap()` before checking whether the
  `CtOption` is present.
- The nearby Sapling ephemeral public key parser handles the same shape
  defensively: `zebra-chain/src/sapling/keys.rs:299-308` checks
  `possible_point.is_none()` before any unwrap.
- The prior Sapling validating-key sweep covers
  `ValidatingKey::try_from(...)`, not this `TransmissionKey` constructor.
- This audit added a durable current-behavior proof at
  `zebra-chain/src/sapling/keys.rs:399`.

The in-tree proof is:

```rust
#[test]
#[should_panic]
fn transmission_key_panics_on_malformed_bytes_today() {
    let _ = TransmissionKey::try_from([0xff; 32]);
}
```

Command:

```sh
cargo test -p zebra-chain transmission_key_panics_on_malformed_bytes_today --lib
```

Result on 2026-05-09: passed.

## Duplicate Check

Read-only duplicate searches refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra TransmissionKey malformed bytes panic Sapling'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra Sapling TransmissionKey TryFrom unwrap panic'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "TransmissionKey::try_from"'
gh api repos/ZcashFoundation/zebra/issues/5476
```

Closest hit:

- #5476, broad cleanup for unused shielded key/address code and known key
  parsing/conversion panics. It is adjacent historical context, but not an exact
  tracker for this remaining malformed-byte `TransmissionKey::try_from()`
  panic.

No exact issue hit was returned.

## Reachability

I do not currently see node-level attacker reachability:

- `zebra-chain/src/primitives/address.rs:67-75` and
  `zebra-chain/src/primitives/address.rs:100-109` validate Sapling address
  receiver bytes through `sapling_crypto::PaymentAddress::from_bytes(&data)`,
  not Zebra's `TransmissionKey::try_from`.
- `zebra-rpc/src/methods.rs:2891-2894` reaches the address conversion path for
  `z_listunifiedreceivers`; the fatal Sapling receiver issue there is already
  documented separately in
  `docs/analysis/rpc-z-listunifiedreceivers-invalid-sapling-panic-finding.md`.
- A source search for `TransmissionKey::try_from`,
  `sapling::keys::TransmissionKey`, and `keys::TransmissionKey` found no live
  validator/RPC call site beyond the type's definition and Sapling `Note` field.

That means this should not be treated like the RPC panic finding unless a new
call site is found or a downstream user of the `zebra-chain` crate accepts
untrusted Sapling transmission-key bytes.

## Impact

Potential impact is denial of service for library consumers that use
`TransmissionKey::try_from()` as a fallible parser for untrusted bytes. In
`zebrad`, current impact appears to be none or very low because the constructor
does not appear to sit on a reachable transaction, peer, mempool, or RPC path.

This still violates the local API expectation and the module-level Sapling claim
that canonical Jubjub point encodings are rejected rather than panicking.

## Suggested Fix

Mirror the `EphemeralPublicKey` pattern:

1. Store `jubjub::AffinePoint::from_bytes(bytes)` in a `possible_point`.
2. Return `Err("Invalid jubjub::AffinePoint value for Sapling TransmissionKey")`
   if `possible_point.is_none()`.
3. Unwrap only after the presence check.
4. Add a regression test that wraps
   `TransmissionKey::try_from([0xff; 32])` in `catch_unwind` and asserts it
   returns `Ok(Err(_))`, not a panic.

## Classification

Public hardening / library API panic. Not a private consensus disclosure based
on current reachability evidence.

Confidence: high that the public API panics; medium-high that it is not currently
reachable from Zebra's node surfaces, based on source search.
