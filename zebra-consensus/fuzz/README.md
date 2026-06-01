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
