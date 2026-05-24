# ZIP-244 sighash hash-type matrix A1 revisit

Date: 2026-05-03

Status: one already-disclosed private consensus finding remains; adjacent
malformed-hash-byte and stale-buffer shapes are eliminated in the current
checkout.

## Summary

The remaining confirmed A1 issue is still the V5 `SIGHASH_SINGLE` /
`SIGHASH_SINGLE|ANYONECANPAY` missing-corresponding-output case: Zebra accepts
the script verification path when the verified input index is valid but
`input_index >= tx.outputs().len()`.

The adjacent undefined-hash-byte concern is no longer live in this checkout.
The FFI callback now allowlists the six valid ZIP-244 transparent hash bytes
before converting into Zebra's `HashType`, and returns a randomized dummy digest
when the hash byte is invalid. That makes the C++ script interpreter fail the
signature instead of accidentally reusing a stale callback buffer.

## Evidence

The V5 callback allowlist is explicit:

- `zebra-script/src/lib.rs:178-190` checks V5 raw hash bytes against
  `{0x01, 0x02, 0x03, 0x81, 0x82, 0x83}` before constructing Zebra's typed
  sighash input.
- `zebra-script/src/lib.rs:207-218` maps the already-allowlisted C++ hash-type
  shape into Zebra's `HashType`.
- `zebra-script/src/lib.rs:222-236` avoids returning `None` to
  `libzcash_script`; invalid callback cases return a fresh random digest so the
  checked ECDSA signature fails.
- `zebra-chain/src/transaction/sighash.rs:37-49` only converts the six
  canonical typed hash combinations into `zcash_transparent::SighashType`.

The lower ZIP-244 sighash call still does not enforce the corresponding-output
rule:

- `zebra-chain/src/primitives/zcash_primitives.rs:480-493` constructs a
  `SignableInput::from_parts(...)` with a valid input index and previous output.
- `zebra-chain/src/primitives/zcash_primitives.rs:498-505` then computes the
  `zcash_primitives` signature hash. In the current linked library, this path
  does not reject `SIGHASH_SINGLE` when the transparent output at the same index
  is missing.

Local repro tests in the current worktree show the split:

- `zebra-script/src/tests.rs:480-487` accepts canonical V5 `SIGHASH_SINGLE`
  with no corresponding output.
- `zebra-script/src/tests.rs:491-501` accepts canonical V5
  `SIGHASH_SINGLE|ANYONECANPAY` with no corresponding output.
- `zebra-script/src/tests.rs:511-522` rejects malformed V5 hash byte `0x84`.
- `zebra-script/src/tests.rs:532-540` rejects malformed V5 hash byte `0x50`.
- `zebra-script/src/tests.rs:1188-1230` covers the two-`CHECKSIG` stale-buffer
  bypass shape and expects rejection.

## Commands

```sh
cargo test -p zebra-script sighash_divergence_v5_p2pkh_malformed_0x84_rejected --lib
cargo test -p zebra-script sighash_divergence_v5_p2pkh_malformed_0x50_rejected --lib
cargo test -p zebra-script stale_sighash_buffer_v5_two_checksig_rejected --lib
cargo test -p zebra-script sighash_single_v5_p2pkh_missing_corresponding_output_is_accepted_today --lib
cargo test -p zebra-script sighash_single_anyonecanpay_v5_p2pkh_missing_corresponding_output_is_accepted_today --lib
```

All five targeted tests passed. The first three are elimination tests for the
invalid-hash-byte / stale-buffer side. The final two are proof tests for the
already-disclosed missing-corresponding-output issue.

## Triage

No new private disclosure item beyond the existing V5 `SIGHASH_SINGLE`
corresponding-output finding.

Keep the already-sent disclosure focused on the consensus divergence:

- zcashd rejects V5 `SIGHASH_SINGLE` and `SIGHASH_SINGLE|ANYONECANPAY` when the
  input lacks a corresponding output;
- Zebra's current script-verification path accepts that shape;
- the missing guard belongs before or during V5 transparent sighash
  calculation, not only in transaction parsing.

The invalid raw hash byte and stale-buffer cases should stay in the public test
or hardening bucket unless a regression removes the callback allowlist or the
random dummy digest behavior.

## Suggested fix direction

- Add an explicit V5 guard for `SIGHASH_SINGLE` and
  `SIGHASH_SINGLE|ANYONECANPAY` requiring
  `input_index < tx.outputs().len()` before computing the digest.
- Return a deterministic script-verification failure or typed sighash error
  through Zebra's wrapper, while preserving the existing random-digest fallback
  for callback failures that cannot be propagated through `libzcash_script`.
- Keep the invalid-hash-byte allowlist tests and stale-buffer regression test
  as defense in depth.
