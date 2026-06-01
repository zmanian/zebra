# zebra-consensus fuzz targets

This package contains `cargo-fuzz` targets for consensus verifier internals.

## Halo2 batch items

`halo2_batch_items` mutates real Orchard/Halo2 items extracted from Zebra's
local block test vectors, then checks that the fuzz-only batch summary agrees
with the item action counts and acceleration-candidate invariants.

```sh
cargo +nightly fuzz check --fuzz-dir zebra-consensus/fuzz halo2_batch_items
cargo +nightly fuzz run --fuzz-dir zebra-consensus/fuzz halo2_batch_items
```

To compile the target through the accelerated verifier facade:

```sh
cargo +nightly fuzz check --fuzz-dir zebra-consensus/fuzz halo2_batch_items --features halo2-accel-verify
```

## Halo2 invalid proofs

`halo2_invalid_proofs` extracts valid Orchard/Halo2 items from Zebra's local
block test vectors, verifies the source item once, mutates a proof byte,
binding-signature byte, or spend-authorization-signature byte, and checks that
the mutated auth data is rejected by Zebra's Orchard verifying key.

```sh
cargo +nightly fuzz check --fuzz-dir zebra-consensus/fuzz halo2_invalid_proofs
cargo +nightly fuzz run --fuzz-dir zebra-consensus/fuzz halo2_invalid_proofs
```
