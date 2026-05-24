# RepoPrompt script / FFI primitive slice

Date: 2026-05-09

Status: local-only. No public issue or private disclosure candidate promoted.

## Summary

I ran a focused RepoPrompt slice over Zebra's script verification FFI, P2SH
sigop accounting, and nearby primitive-boundary code. The main question was
whether the release-mode `zip()` truncation shape in
`zebra_script::p2sh_sigop_count()` can be reached by production block or mempool
verification with a mismatched `spent_outputs` list.

Result: no production-reachable consensus or process-fatal issue survived this
pass. The `p2sh_sigop_count()` mismatch remains a real library-boundary
hardening gap, but current verifier paths construct previous outputs by input
index and reject missing outputs before building the cached script transaction.

## RepoPrompt result

RepoPrompt's first pass selected the relevant block, transaction, script, and
primitive files and reached this conclusion:

- `SemanticBlockVerifier` sends each transaction through the transaction
  verifier and sums `Response::Block.sigops()`.
- The transaction verifier calls `spent_utxos()` before constructing
  `CachedFfiTransaction`.
- `spent_utxos()` allocates one slot per transaction input, fills slots by
  original `input_idx`, and returns an error before successful construction if a
  required output cannot be resolved.
- `CachedFfiTransaction::p2sh_sigops()` reaches
  `p2sh_sigop_count(tx, spent_outputs)`, but successful non-coinbase verifier
  construction currently gives it one spent output per transparent input.
- Coinbase is the only normal cached construction with
  `tx.inputs().len() == 1` and `spent_outputs.len() == 0`; it is benign for
  this specific path because `p2sh_sigop_count()` returns `0` before the
  `zip()` and script verification is skipped for coinbase inputs.

The skeptical follow-up also found no fresh disclosure-grade issue. It
classified direct `p2sh_sigop_count()` misuse, sighash precondition panics,
constructor error mapping, and FFI callback behavior as hardening or duplicate
routes unless a future source-to-sink shows peer/RPC-controlled bytes reaching a
production panic, silent acceptance, or materially unbounded work before
rejection.

## Local source check

Current source evidence:

- `zebra-consensus/src/transaction.rs:690-776` builds
  `spent_outputs: Vec<Option<transparent::Output>>`, fills by `input_idx`, fills
  later mempool `AwaitOutput` results back into the same index, and returns
  `TransparentInputNotFound` if missing mempool outputs cannot be resolved.
- `zebra-consensus/src/transaction.rs:483` constructs
  `CachedFfiTransaction` only after `spent_utxos()` succeeds.
- `zebra-consensus/src/transaction.rs:570-582` computes block-path sigops as
  `tx.sigops()` plus `cached_ffi_transaction.p2sh_sigops()`.
- `zebra-consensus/src/script.rs:48-73` bounds-checks the input index before
  calling `CachedFfiTransaction::is_valid(input_index)`.
- `zebra-script/src/lib.rs:147-153` makes `is_valid()` fail closed with
  `Error::TxIndex` if the requested input lacks a previous output or if the
  previous-output list length differs from `transaction.inputs().len()`.
- `zebra-script/src/lib.rs:399-423` documents the P2SH length invariant but
  still uses a release-mode `zip()` after only a `debug_assert_eq!()`.

Existing local notes already cover the adjacent ground:

- `docs/analysis/script-ffi-safety-a6-revisit-note.md` records the
  `p2sh_sigop_count()` caveat as defense-in-depth.
- `docs/analysis/transparent-spent-output-alignment-note.md` records why the
  current chain/mempool previous-output construction path preserves input
  order.
- `docs/analysis/sighash-single-corresponding-output-finding.md` and
  `docs/analysis/v5-sighash-parser-backstop-note.md` remain the source of truth
  for the already-reported V5 `SIGHASH_SINGLE` family.

## Classification

No new private disclosure item.

The remaining hardening work is worth doing if maintainers touch this area:

- Move previous-output length validation into `CachedFfiTransaction::new()`.
- Replace `p2sh_sigop_count()`'s debug-only invariant with a fallible or
  fail-closed checked helper.
- Remove or rewrite the stale `zebra-consensus/src/transaction.rs` TODO that
  still describes mempool outputs as appended out of order.
- Add a mixed chain-state plus mempool-output regression showing
  `spent_utxos()` preserves input order.

Future reports in this family should be treated as duplicates unless they show
a new production source-to-sink from peer/RPC-controlled bytes into a script
FFI/sighash panic, a silent acceptance path, or materially unbounded work before
ordinary rejection.
