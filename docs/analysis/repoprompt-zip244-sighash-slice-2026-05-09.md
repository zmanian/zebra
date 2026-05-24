# RepoPrompt ZIP-244 Sighash Slice - 2026-05-09

Disposition: local-only. Do not post publicly without explicit direction and a
fresh duplicate check.

## Scope

RepoPrompt slice focused on ZIP-244 transparent sighash behavior after the
already-reported V5 `SIGHASH_SINGLE` missing-corresponding-output issue.

Goal: look for a fresh parity, panic, or hidden fallback issue where Zebra
accepts a transaction shape zcashd or librustzcash rejects, rejects one they
accept, panics, or silently falls back in the transparent script/sighash path.

## Result

No new confirmed vulnerability was found in this slice. The pass mostly
confirmed that the surrounding V5 hash-type matrix has already been hardened or
recorded:

- V5 undefined raw hash-type bytes such as `0x84` and `0x50` are rejected by the
  callback allowlist before digest construction.
- V4 continues to route through the raw-byte sighash path.
- The stale callback-buffer shape is covered by the per-call random fallback
  digest and regression coverage.
- Basic out-of-range input index and previous-output length mismatches are
  rejected at the script boundary.

The remaining actionable items are audit hardening and parity-test gaps, not a
new private disclosure item today.

## Fresh Audit Gaps

### Fallible sighash boundary

`zebra-chain/src/primitives/zcash_primitives.rs` already has internal
`SighashError`-style failure states, but public sighash wrappers convert those
to panics via `expect(...)`. Current script-call paths appear guarded, but a
fallible `SigHasher::try_sighash(...)` / `try_sighash_v4_raw(...)` API would let
`zebra-script` report internal invariant failures as structured transaction
rejection instead of relying on panic-only contracts.

This is hardening unless a consensus-reachable desynchronization path is found.

### OP_CODESEPARATOR and P2SH script-code parity

Zebra delegates `script_code` construction to the `zcash_script` callback. That
is probably the correct architecture, but we do not yet have explicit parity
fixtures for V5 transparent spends with:

- non-P2SH `OP_CODESEPARATOR` before `OP_CHECKSIG`;
- multiple separators where only the last executed separator should affect the
  script code;
- P2SH redeem scripts containing separators;
- invalid V5 raw hash-type bytes in separator/P2SH multi-`CHECKSIG` contexts.

These need zcashd or upstream `zcash_script` comparison before they should be
treated as findings.

### Mixed state/mempool previous-output ordering

The stale TODO in `zebra-consensus/src/transaction.rs` says mixed chain and
mempool UTXOs might be appended out of order. Current source review indicates
`spent_utxos()` fills `spent_outputs[input_idx]`, preserving input order, and a
prior local note eliminated this as a live vulnerability. A regression test
would still be useful because a future rework could reintroduce a subtle
input/output mispairing before `CachedFfiTransaction` script validation.

## Duplicate Control

This slice should not be used to re-report the V5 `SIGHASH_SINGLE` missing
corresponding output issue. The confirmed issue in this area remains that known
private-disclosure item. The surrounding malformed-hash-byte and stale-buffer
variants appear closed in current source and tests.

## Suggested Local Tests

- Add V5 `OP_CODESEPARATOR` and P2SH `script_code` parity fixtures using
  zcashd-captured outcomes.
- Add explicit tests for invalid V5 raw hash-type bytes in separator/P2SH
  multi-`CHECKSIG` scripts.
- Add a consensus-path regression proving mixed state plus mempool previous
  outputs remain aligned with transaction input order.
- Add API-level tests around any future fallible sighash methods so invariant
  failures are classified instead of panicking.

## Confidence

Confidence is high that this slice did not produce a new confirmed issue.
Confidence is medium that the remaining parity gaps are only test gaps, because
`zcash_script` owns the callback `script_code` semantics and direct zcashd
fixture comparison has not been performed in this pass.
