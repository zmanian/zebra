# Mempool Empty Spent-Outputs Standardness Boundary

Date: 2026-05-09

## Summary

`Storage::reject_if_non_standard_tx()` currently treats an empty
`VerifiedUnminedTx.spent_outputs` vector as the shielded-only/no-previous-output
case. A synthetic `VerifiedUnminedTx` with transparent `PrevOut` inputs and an
empty `spent_outputs` vector therefore skips `are_inputs_standard()` and P2SH
sigop accounting before insertion.

This is proof-backed as an internal boundary gap, but not currently promoted as
a live remote vulnerability. The normal mempool verifier path appears to fill
one previous-output slot for every accepted `PrevOut` input or return
`TransparentInputNotFound` before constructing the `VerifiedUnminedTx`.

No public GitHub issue, comment, or advisory was posted from this finding.

## Proof

Added current-behavior test:

- `zebrad/src/components/mempool/storage/tests/vectors.rs`:
  `transparent_input_empty_spent_outputs_bypasses_input_standardness_today`

The test uses the same transparent-input transaction in two storage insertions:

- With one deliberately non-standard previous output per input, storage rejects
  the transaction with `NonStandardInputs`.
- With `spent_outputs = []`, storage accepts and stores the same transaction.

Verification:

```sh
cargo test -p zebrad transparent_input_empty_spent_outputs_bypasses_input_standardness_today --lib
```

Result on 2026-05-09: passed.

## Reachability

Current production reachability looks blocked:

- `zebra-consensus/src/transaction.rs::spent_utxos()` preallocates one
  `Option<transparent::Output>` slot per input.
- Successful best-chain, known-UTXO, and mempool-output branches fill the slot
  at the original input index.
- Missing mempool outputs return `TransparentInputNotFound`.
- Script verification fails closed if the previous-output vector length does
  not match the transaction input vector for the transparent input being
  verified.

The remaining weakness is that `VerifiedUnminedTx::new()` accepts a caller-owned
previous-output vector, and storage repeats only a conditional invariant check:
non-empty mismatches reject, but empty mismatches are treated as expected.

## Impact

If an internal or future caller can create a `VerifiedUnminedTx` with transparent
inputs and empty `spent_outputs`, Zebra can admit a transaction that bypasses
non-consensus mempool input standardness checks. This could include accepting a
transaction whose previous output script is non-standard, or undercounting P2SH
sigops for mempool policy.

This does not by itself show a consensus bypass. It is a mempool policy /
defense-in-depth boundary issue.

## Suggested Hardening

- In `Storage::reject_if_non_standard_tx()`, reject transactions with one or
  more transparent `PrevOut` inputs and an empty `spent_outputs` vector.
- In the verifier, replace the final `flatten()` over spent-output slots with an
  explicit conversion that errors if any expected `PrevOut` slot remains empty.
- Keep or add tests that prove shielded-only transactions can still use empty
  `spent_outputs` while transparent-input transactions cannot.
