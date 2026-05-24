# zebra-script FFI safety A6 revisit

Date: 2026-05-03

Status: eliminated as a fresh private vulnerability in the reviewed paths;
public defense-in-depth remains.

## Summary

I did not find a new attacker-reachable FFI panic, silent acceptance, or hidden
`libzcash_script` error in the current wrapper.

The current `CachedFfiTransaction::is_valid()` path checks input and previous
output alignment before calling into the C++ interpreter. The V5 sighash
callback allowlists canonical ZIP-244 hash bytes and returns a randomized dummy
digest for callback failures, so invalid callback cases fail signature
verification instead of relying on `None` propagation through
`libzcash_script`. Unknown `libzcash_script` errors are preserved as errors.

The main remaining A6 caveats are defense-in-depth:

- `SigHasher::sighash()` and `sighash_v4_raw()` still express precondition
  failures as `expect()` panics at the public wrapper boundary;
- `p2sh_sigop_count()` still has a release-mode truncation shape if a future
  caller passes misaligned `spent_outputs`, although the current consensus and
  mempool paths are already covered by the spent-output alignment review.

## Evidence

FFI wrapper input alignment:

- `zebra-script/src/lib.rs:147-153` returns `Error::TxIndex` if
  `all_previous_outputs[input_index]` is absent or if the previous-output vector
  length differs from `transaction.inputs().len()`.
- `zebra-script/src/lib.rs:164-172` rejects coinbase inputs before constructing
  the C++ script object.
- `zebra-consensus/src/script.rs:48-73` checks that the requested input exists
  before calling `cached_ffi_transaction.is_valid(input_index)`.

Callback failure behavior:

- `zebra-script/src/lib.rs:178-190` allowlists valid V5 hash bytes.
- `zebra-script/src/lib.rs:195-204` routes pre-V5 transactions through raw-byte
  sighash semantics.
- `zebra-script/src/lib.rs:207-218` maps valid V5 callback hash types into
  Zebra's typed `HashType`.
- `zebra-script/src/lib.rs:222-236` turns invalid callback computation into a
  random dummy digest rather than returning `None` to C++.
- `zebra-script/src/lib.rs:244-251` maps `verify_callback()` errors and false
  script results into Zebra errors.

Error mapping:

- `zebra-script/src/lib.rs:25-38` has explicit Zebra-side error variants,
  including `Unknown(libzcash_script::Error)`.
- `zebra-script/src/lib.rs:55-58` maps every `libzcash_script::Error` into
  `Error::Unknown(...)` rather than success.

Lower sighash preconditions:

- `zebra-chain/src/primitives/zcash_primitives.rs:297-335` centralizes
  internal sighash precondition errors.
- `zebra-chain/src/primitives/zcash_primitives.rs:392-430` still unwraps those
  precondition checks at the public `sighash()` / `sighash_v4_raw()` boundary.
- `zebra-chain/src/primitives/zcash_primitives.rs:441-493` validates input
  index, transparent bundle presence, previous output amount conversion, and
  transparent bundle input count before calling `signature_hash()`.

P2SH sigop caveat:

- `zebra-script/src/lib.rs:399-423` uses a `debug_assert_eq!()` plus `zip()`
  for `p2sh_sigop_count()`. A future misaligned caller would undercount in
  release mode, but the current live paths are covered by
  `docs/analysis/transparent-spent-output-alignment-note.md`.

## Commands

```sh
cargo test -p zebra-script is_valid_rejects_mismatched_previous_outputs_length --lib
cargo test -p zebra-script is_valid_rejects_out_of_range_input_index --lib
cargo test -p zebra-script stale_sighash_buffer_v5_two_checksig_rejected --lib
```

All three targeted tests passed. Two earlier incorrect filters selected 0 tests
(`mismatched_previous_outputs_returns_txindex` and
`out_of_range_input_index_returns_txindex`); the command list above is the
actual verification set.

## Triage

No new private disclosure item.

The FFI wrapper is still part of the private consensus-critical surface because
script verification can split Zebra from zcashd. But in the reviewed current
paths, malformed hash bytes, missing previous-output alignment, and unknown C++
errors fail closed. The already-disclosed V5 `SIGHASH_SINGLE` missing-output
issue remains separate: the callback and FFI wrapper run successfully, but the
lower ZIP-244 sighash computation is missing the corresponding-output rule.

## Suggested hardening

- Make `SigHasher::sighash()` return a typed `Result` for transparent
  signature-hash requests, then convert callback failures into the existing
  random-digest failure behavior at the FFI boundary.
- Replace `p2sh_sigop_count()`'s debug-only alignment assertion with a fallible
  or fail-closed API for non-coinbase transactions.
- Keep the V5 hash-byte allowlist and stale-buffer regression tests in the
  script crate.
