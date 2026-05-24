# Sapling Validating-Key Panic Sweep Note

Date: 2026-05-03

Last updated: 2026-05-09

Disposition: local-only eliminated lead. Do not post publicly without explicit
re-authorization.

Scope: follow-up audit of whether malformed remote P2P `tx` or `block` bytes
can trigger a process-fatal panic while parsing Sapling spend authorization
validating keys (`rk`).

## Summary

This lead is eliminated as a current private security finding.

The suspicious site is `ValidatingKey::try_from(redjubjub::VerificationKey)` in
`zebra-chain/src/sapling/keys.rs`, which calls
`jubjub::AffinePoint::from_bytes(key.into()).unwrap()` before checking whether
the point is small order.

Malformed peer-controlled bytes do not reach that `unwrap()` directly. Both V4
Sapling spends and V5 spend prefixes first parse `rk` as:

```rust
rk: reader
    .read_32_bytes()?
    .try_into()
    .map_err(SerializationError::Parse)?,
```

The `[u8; 32]` conversion first calls
`redjubjub::VerificationKey::<SpendAuth>::try_from(value)`. In the locked
`reddsa` dependency, that constructor decodes the point with
`T::Point::from_bytes()` and returns an error unless the encoding is canonical.
Only after that succeeds does Zebra run the follow-up small-order check.

Classification: local hardening / eliminated lead, not private disclosure.

## Evidence

- `zebra-network/src/protocol/external/codec.rs:450` dispatches P2P `block`
  bodies to `read_block()`, and `:457` dispatches P2P `tx` bodies to
  `read_tx()`.
- `zebra-chain/src/sapling/spend.rs:217-220` parses V4 Sapling spend `rk`
  bytes through `TryFrom<[u8; 32]> for ValidatingKey` and maps failures to
  `SerializationError::Parse`.
- `zebra-chain/src/sapling/spend.rs:269-272` uses the same pattern for V5
  `SpendPrefixInTransactionV5`.
- `zebra-chain/src/sapling/keys.rs:371-374` first calls
  `redjubjub::VerificationKey::<SpendAuth>::try_from(value)`.
- `zebra-chain/src/sapling/keys.rs:355-364` contains the follow-up `unwrap()`
  while checking small-order points.
- `reddsa-0.5.1/src/verification_key.rs:96-112` validates canonical point
  decoding before returning `Ok(VerificationKey { point, bytes })`.

## Local Regression Evidence

Added targeted tests in `zebra-chain/src/sapling/keys.rs`:

- `validating_key_rejects_malformed_and_small_order_bytes_without_panicking`
  checks sampled malformed and small-order encodings return `Err` without
  unwinding.
- `redjubjub_validating_key_success_implies_affine_decode_success` is a
  property test over arbitrary `[u8; 32]` encodings. For every encoding accepted
  by `redjubjub::VerificationKey::<SpendAuth>::try_from`, the follow-up
  `jubjub::AffinePoint::from_bytes(key.into())` succeeds and
  `ValidatingKey::try_from(key)` does not panic.

Verification run:

```text
cargo test -p zebra-chain validating_key_rejects_malformed_and_small_order_bytes_without_panicking --lib
cargo test -p zebra-chain redjubjub_validating_key_success_implies_affine_decode_success --lib
```

Both targeted tests passed.

Verification rerun on 2026-05-09:

```text
cargo test -p zebra-chain validating_key_rejects_malformed_and_small_order_bytes_without_panicking --lib
cargo test -p zebra-chain redjubjub_validating_key_success_implies_affine_decode_success --lib
```

Both targeted tests passed.

Duplicate search refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra Sapling ValidatingKey malformed rk panic redjubjub'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "Sapling" "ValidatingKey" "panic"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "redjubjub" "ValidatingKey" "unwrap"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "rk" "redjubjub" "panic"'
```

Relevant adjacent hits:

- closed #3154 added Zcash-specific Sapling/Orchard key correctness checks;
- closed #10321 is a broader lazy-deserialization refactor touching Sapling
  `ValidatingKey` storage and validation timing.

Those are provenance or adjacent refactor context, not a duplicate live panic
finding.

## Remaining Hardening

The `unwrap()` is currently guarded by the upstream `redjubjub` / `reddsa`
invariant, and the new property test locks that invariant into Zebra's local
test suite. Still, this is consensus-critical parsing code and Zebra's binary
profiles use `panic = "abort"`, so replacing the `unwrap()` with explicit error
propagation would make the proof local and easier to audit.

Suggested hardening:

1. Replace the `unwrap()` in
   `TryFrom<redjubjub::VerificationKey<SpendAuth>> for ValidatingKey` with a
   parse-style error if affine decoding ever fails.
2. Keep the property test as a regression for dependency upgrades.
3. Optionally add full transaction fixtures for malformed Sapling V4 and V5
   `rk` bytes if maintainers want coverage at the `Transaction::zcash_deserialize`
   boundary rather than just the key conversion boundary.
