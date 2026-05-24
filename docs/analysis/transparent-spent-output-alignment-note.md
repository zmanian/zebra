# Transparent Spent-Output Alignment Note

Date: 2026-05-03

Scope: follow-up on the hypothesis that transparent previous outputs can become
missing or misaligned when a mempool transaction spends both best-chain and
mempool-created UTXOs.

## Finding

No exploitable spent-output misalignment was found in the current code. The
older concern appears to have been fixed by indexed slot filling in
`spent_utxos()`: both best-chain/known UTXOs and later mempool `AwaitOutput`
responses are written into `spent_outputs[input_idx]`, preserving transaction
input order before the final `flatten()`.

The remaining issue is defense-in-depth and stale documentation: the final
`flatten()` would silently shorten the vector if a future branch left a
non-coinbase `PrevOut` slot empty, and `Verifier::call()` still has a TODO that
says mixed chain/mempool outputs may be appended out of order. That TODO no
longer matches the current implementation. A later storage-layer proof confirms
why the invariant should be explicit: if a transparent-input `VerifiedUnminedTx`
is manufactured with `spent_outputs = []`, mempool storage currently skips input
standardness and P2SH sigop policy and accepts the transaction.

A nearby panic-looking site has the same invariant: mempool coinbase-maturity
checking calls `tx_transparent_coinbase_spends_maturity()`, which expects every
spent outpoint to be present in either the request's known block outputs or the
`spent_utxos` map. In the current mempool path, `spent_utxos()` either fills that
map for every `PrevOut` or returns `TransparentInputNotFound` before maturity
checking runs.

## Evidence

- `zebra-consensus/src/transaction.rs:409-418` rejects coinbase transactions
  from the mempool and rejects any non-coinbase transaction that has a coinbase
  input.
- `zebra-chain/src/transaction.rs:590-594` defines valid non-coinbase
  transactions as transactions whose inputs are all `PrevOut`.
- `zebra-consensus/src/transaction.rs:686-692` preallocates
  `Vec<Option<transparent::Output>>` with one slot per transaction input and
  stores missing best-chain mempool outpoints together with their original input
  index.
- `zebra-consensus/src/transaction.rs:697-730` fills the slot for known or
  best-chain UTXOs at `spent_outputs[input_idx]`.
- `zebra-consensus/src/transaction.rs:736-766` resolves mempool-only missing
  outputs through `mempool::Request::AwaitOutput(outpoint)`, fills the original
  `input_idx`, and returns `TransparentInputNotFound` if the mempool fallback is
  unavailable or times out.
- `zebra-consensus/src/transaction.rs:769-776` converts the filled slots with
  `flatten()` and returns the resulting outputs in input order.
- `zebra-consensus/src/transaction.rs:475-479` runs
  `check_maturity_height()` only for mempool requests, after `spent_utxos()`
  has returned successfully.
- `zebra-consensus/src/transaction.rs:277-282` makes
  `Request::Mempool::known_utxos()` empty, so the maturity path depends on the
  `spent_utxos()` result rather than extra block-context UTXOs.
- `zebra-consensus/src/transaction/check.rs:504-509` looks up every spent
  outpoint in request-known outputs or `spent_utxos`, then expects it to exist.
  Current callers satisfy that expectation because all successful `spent_utxos()`
  branches insert the resolved UTXO or return `TransparentInputNotFound`.
- `zebra-script/src/lib.rs:147-153` fails closed in script verification if
  `all_previous_outputs.len()` does not equal `transaction.inputs().len()` for
  the input being checked.
- `zebrad/src/components/mempool/storage.rs:281-290` rejects a
  `VerifiedUnminedTx` as non-standard if the transparent input count and stored
  spent-output count differ.
- `zebrad/src/components/mempool/storage/policy.rs:103-114` documents that its
  `zip()`-based standardness check depends on that length guard.
- `zebrad/src/components/mempool/storage/verified_set.rs:162-170` uses the
  separate `spent_mempool_outpoints` vector for dependency tracking and rejects
  missing mempool-created dependencies with `MissingOutput`.
- `zebrad/src/components/mempool/storage/tests/vectors.rs` now has
  `transparent_input_empty_spent_outputs_bypasses_input_standardness_today`,
  which proves the storage boundary accepts a transparent-input transaction
  with empty `spent_outputs` even though the same transaction rejects when a
  non-standard previous-output vector is supplied.

## Classification

Status: eliminated as a live vulnerability; public defense-in-depth remains.

For accepted non-coinbase mempool transactions, every input is a `PrevOut`, and
every successful current branch either fills the corresponding previous-output
slot or returns an error before constructing `VerifiedUnminedTx`. For coinbase
transactions, the empty previous-output vector is expected and does not reach
the mempool path.

If a future edit introduced a successful branch that left a `PrevOut` slot as
`None`, the current `flatten()` would make the failure mode less explicit.
Downstream layers are still fail-closed, but the consensus-adjacent verifier
boundary should express the invariant directly.

## Suggested Hardening

- Replace the final `flatten()` with an explicit conversion that errors if any
  expected `PrevOut` slot is missing.
- Reject transparent-input transactions with empty `spent_outputs` in
  `Storage::reject_if_non_standard_tx()`, while preserving the shielded-only
  empty-vector case.
- Replace the maturity-check `expect()` with
  `ok_or(TransactionError::TransparentInputNotFound)?` so an invariant
  regression remains a transaction rejection rather than a panic.
- Replace the stale TODO above `VerifiedUnminedTx::new()` with an invariant note
  saying `spent_utxos()` fills previous outputs by original input index.
- Add a regression test for a transaction with at least two transparent inputs:
  one resolved from known/best-chain UTXOs and one resolved from mempool
  `AwaitOutput`. Assert `VerifiedUnminedTx.spent_outputs` preserves input order
  and `spent_mempool_outpoints` contains only the mempool-resolved outpoint.

## Confidence

Confidence: medium-high.

The live control flow and downstream guards are direct. The remaining gap is
test coverage for the mixed best-chain plus mempool ordering case; existing
tests cover single-input `AwaitOutput` acceptance and mempool standardness
length guarding, but not the two-source multi-input alignment scenario.
