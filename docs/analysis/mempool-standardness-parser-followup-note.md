# Mempool Standardness Parser Follow-Up

Date: 2026-05-09

## Summary

This pass looked for a fresh vulnerability in Zebra's mempool standardness
checks around P2SH scriptSig parsing, push counting, previous-output alignment,
and sigop accounting.

I did not find a new live vulnerability. The main candidate shapes are either
covered by existing notes or collapse into rejection / consensus-verifier
failure before `Storage::insert()` can admit the transaction.

Follow-up proof on 2026-05-09 confirmed a synthetic storage-layer boundary gap:
a transparent-input `VerifiedUnminedTx` with `spent_outputs = []` skips input
standardness and P2SH sigop policy. This remains local-only because the normal
production verifier path appears to fill or fail every `PrevOut` previous-output
slot before storage insertion.

No public GitHub issue, comment, or advisory was posted from this pass.

## Evidence

Relevant Zebra code:

- `zebrad/src/components/mempool/storage.rs` applies scriptSig size and
  push-only checks for every transparent input before input standardness and
  sigop policy.
- `zebrad/src/components/mempool/storage.rs` only calls
  `policy::are_inputs_standard()` when `spent_outputs` is non-empty; this is
  the already-recorded synthetic bypass shape.
- `zebrad/src/components/mempool/storage/policy.rs` counts scriptSig stack
  items with `count_script_push_ops()` and extracts the P2SH redeem script from
  the last parsed `Opcode::PushValue`.
- `zebra-consensus/src/transaction.rs` fills previous outputs by original input
  index before constructing `CachedFfiTransaction`.
- `zebra-script/src/lib.rs` rejects script verification when
  `all_previous_outputs.len()` differs from `transaction.inputs().len()`.

Relevant `zcash_script` code:

- `zcash_script::opcode::PossiblyBad::parse()` maps `OP_1` through `OP_16` to
  `Opcode::PushValue(PushValue::SmallValue(_))`.
- `zcash_script::script::Code::is_push_only()` intentionally accepts
  `PushSize(_)` parse errors and `OP_RESERVED`, matching zcashd's push-only
  behavior around opcodes that are not greater than `OP_16`.
- Script evaluation still executes the scriptSig before the scriptPubKey, so a
  push-only-but-invalid scriptSig is rejected by consensus verification before
  mempool storage sees a `VerifiedUnminedTx`.

## Candidate Checks

### Small-number push undercount

Hypothesis: `count_script_push_ops()` might count direct pushdata opcodes but
miss `OP_1` through `OP_16`, allowing stack-depth mismatches in standard P2SH
or multisig inputs.

Result: eliminated.

`zcash_script` represents `OP_1` through `OP_16` as
`Opcode::PushValue(PushValue::SmallValue(_))`, so Zebra's
`matches!(..., Opcode::PushValue(_))` count includes them.

### Oversize-push parser error undercount

Hypothesis: `Code::is_push_only()` accepts `PushSize(_)`, while
`count_script_push_ops()` ignores parse errors, so an oversized push could make
Zebra's stack-depth policy count differ from actual script execution.

Result: not promoted.

This can change the local policy count for a synthetic `VerifiedUnminedTx`, but
it does not look remotely reachable through normal mempool validation. The
transaction verifier runs the transparent script before constructing the
verified transaction for storage, and script evaluation rejects malformed or
oversized pushes. In the P2SH path, ignored parse errors also prevent redeem
script extraction, producing rejection rather than acceptance.

### Empty previous-output vector bypass

Hypothesis: a transparent transaction with `spent_outputs = []` can skip
`are_inputs_standard()` and P2SH sigop accounting in
`reject_if_non_standard_tx()`.

Result: proof-backed as local-only defense-in-depth.

`VerifiedUnminedTx::new()` accepts a caller-supplied previous-output vector, so
a synthetic storage unit test can manufacture this shape. The current-behavior
test
`transparent_input_empty_spent_outputs_bypasses_input_standardness_today`
confirms that storage rejects the same transaction when non-standard previous
outputs are supplied, but accepts it when `spent_outputs` is empty.

The production transaction-verification path should not expose this to remote
peers: it preallocates one previous-output slot per input, fills them by input
index from state or mempool, and returns `TransparentInputNotFound` if a
previous output cannot be resolved.

The source of truth remains:

- `docs/analysis/mempool-empty-spent-outputs-standardness-boundary-note.md`
- `docs/analysis/transparent-spent-output-alignment-note.md`
- `docs/analysis/repoprompt-wide-search-2026-05-09.md`

## Triage

No private disclosure candidate from this slice.

Recommended hardening only:

- In `Storage::reject_if_non_standard_tx()`, reject any transaction that has
  transparent `PrevOut` inputs and an empty `spent_outputs` vector, unless the
  transaction is coinbase.
- Consider making `count_script_push_ops()` explicitly return an error on
  parser errors rather than silently counting only successfully parsed pushes.
  This would make the standardness code easier to audit, even if consensus
  verification already protects the live path.
