# Post-v4.4.0 security audit follow-ups

Date: 2026-05-02

This note records a local authorized audit pass after the Zebra 4.4.0 security
fixes. It avoids exploit construction details and focuses on verifier boundary
conditions, miner-facing liveness, and mempool/block parity.

## Executive summary

The strongest open consensus candidate remains the V5+ `SIGHASH_SINGLE` case
documented separately in
`docs/analysis/sighash-single-corresponding-output-finding.md`: a canonical
`SIGHASH_SINGLE` or `SIGHASH_SINGLE|ANYONECANPAY` transparent input appears to
avoid an explicit "corresponding output exists" validation check.

The next-highest-risk area is miner RPC liveness. `submitblock` and
`getblocktemplate` proposal mode call the consensus block verifier directly,
while the verifier's own documentation says callers should wrap block and
transaction verification in timeouts. This resembles the operational shape of
the Litecoin incident: a block can be invalid or unprocessable while still
tying up miner-facing validation paths.

Two lower-severity hardening items are worth tracking: `sendrawtransaction`
puts every parsed transaction into the retry queue before the node knows
whether the failure is transient, and the transparent UTXO collection code still
uses `flatten()` at the final boundary, silently shrinking a partially-filled
previous-output vector if a future regression leaves a slot empty.

## Finding 1: V5+ SIGHASH_SINGLE missing corresponding-output check

Status: confirmed candidate, consensus-critical.

Evidence:

- `zebra-script/src/lib.rs:147` starts per-input transparent script validation.
- `zebra-script/src/lib.rs:149` to `zebra-script/src/lib.rs:151` checks that a
  previous output exists for the input and that previous-output count equals
  input count. It does not check that `input_index < transaction.outputs().len()`.
- `zebra-script/src/lib.rs:209` maps `SignedOutputs::Single` to
  `HashType::SINGLE`.
- `zebra-script/src/lib.rs:217` calls `self.sighasher().sighash(...)`.
- `zebra-chain/src/primitives/zcash_primitives.rs:307` to
  `zebra-chain/src/primitives/zcash_primitives.rs:333` defines sighash errors
  for input-side bundle issues, but no missing-output variant.
- `zebra-chain/src/primitives/zcash_primitives.rs:480` to
  `zebra-chain/src/primitives/zcash_primitives.rs:492` constructs the
  transparent signable input and maps only input-count mismatch.
- `Cargo.lock:7510` uses `zcash_primitives 0.27.0`; `Cargo.lock:7603` uses
  `zcash_transparent 0.7.0`. In those crates, `SignableInput::from_parts` only
  rejects `index >= bundle.vin.len()`, and the v5 sighash path hashes an empty
  transparent output set when `SIGHASH_SINGLE` has no corresponding output.

Why it matters:

- ZIP-244 validation requires rejection when `SIGHASH_SINGLE` has no
  corresponding output for a V5 transparent input.
- Zebra's FFI callback now rejects undefined V5 raw hash-type bytes by returning
  a random digest, which is good, but canonical `SIGHASH_SINGLE` is not an
  undefined hash type. It needs its own precheck.

Recommended fix:

- Add a fallible V5+ sighash path that returns an error when
  `SignedOutputs::Single` and `input_index >= transaction.outputs().len()`.
- Use that fallible path inside `zebra-script`'s callback and route the failure
  to the existing random-digest rejection behavior.
- Add regressions for canonical `SIGHASH_SINGLE` and
  `SIGHASH_SINGLE|ANYONECANPAY` with too few transparent outputs.

## Finding 2: Mining RPC block-verifier calls are not deadline-wrapped

Status: likely availability vulnerability / miner DoS hardening gap.

Evidence:

- `zebra-consensus/src/router.rs:8` says block and transaction verification
  requests should be wrapped in a timeout.
- `zebra-consensus/src/router.rs:68` to `zebra-consensus/src/router.rs:70`
  repeats that block verification requests should be timeout-wrapped because
  out-of-order and invalid requests can hang indefinitely.
- `zebrad/src/components/sync.rs:173` defines `BLOCK_VERIFY_TIMEOUT` as eight
  minutes, and sync wraps the verifier at `zebrad/src/components/sync.rs:496`.
- Inbound verification also wraps the block verifier at
  `zebrad/src/components/inbound.rs:278`.
- `zebra-rpc/src/methods/types/get_block_template.rs:661` calls
  `Request::CheckProposal` with no visible timeout.
- `zebra-rpc/src/methods.rs:2577` calls `Request::Commit` from `submitblock`
  with no visible timeout.
- `zebra-state/src/service.rs:1096` implements `Request::AwaitUtxo`; when the
  UTXO is absent, it reaches `response_fut.await` at
  `zebra-state/src/service.rs:1164`, waiting for a future state response.

Why it matters:

- Block verification legitimately waits on state for missing UTXOs. The sync
  and inbound paths account for that with timeouts; miner RPC paths currently do
  not.
- If a submitted or proposed block reaches a verifier/state wait that never
  resolves, the RPC request can remain pending. That is operationally dangerous
  for miners and pool software even if consensus acceptance is still correct.
- A follow-up pass also found that post-dispatch infrastructure failures collapse
  into `Rejected` / invalid-proposal strings, and that `generate` plus the
  experimental internal miner reuse the same no-deadline `submit_block()` path.
- Proposal validation itself appears state-safe because it validates against a
  cloned non-finalized state rather than canonical state.

Recommended fix:

- Add miner-RPC-local timeouts around:
  - `submitblock` verifier commit.
  - `getblocktemplate` proposal validation.
  - state and mempool fetches used while building block templates.
- On timeout, return an RPC error or BIP22/BIP23 "inconclusive" style response,
  not `Rejected`. A timed-out commit may still complete after the caller stops
  waiting, so `Rejected` would be a misleading result.
- Add tests with a verifier service that never resolves.

## Finding 3: sendrawtransaction retry queue loses retryability semantics

Status: bounded availability / resource-churn hardening gap.

Evidence:

- `zebra-rpc/src/methods.rs:1177` sends every parsed transaction to the retry
  queue before the immediate mempool result is known.
- `zebra-rpc/src/methods.rs:1180` then sends the same transaction to mempool
  verification.
- `zebra-rpc/src/methods.rs:1195` to `zebra-rpc/src/methods.rs:1205` awaits and
  flattens the mempool queue result.
- `zebrad/src/components/mempool/downloads.rs:443` sends verifier failure back
  to the caller as a formatted string in `BoxError`.
- `zebra-node-services/src/mempool.rs:167` models `Queued` as
  `Vec<Result<oneshot::Receiver<Result<(), BoxError>>, BoxError>>`, so RPC
  does not receive typed retryability information.
- The retry queue itself is bounded:
  `zebra-rpc/src/queue.rs:42` sets capacity to 20, and
  `zebra-rpc/src/queue.rs:91` evicts the oldest entry when full.

Why it matters:

- This is not as serious as the consensus or miner-RPC issues because the queue
  is small and lossy.
- But it still causes permanent-invalid, non-standard, or already-invalidated
  transactions to be retried as if they might become valid later.
- The existing mempool rejection taxonomy already knows the difference between
  exact-tip, same-effects-tip, and same-effects-chain failures; that information
  is discarded before it reaches RPC.

Recommended fix:

- Introduce a small internal queue result type with `Retryable` and `Rejected`
  categories.
- Queue a raw transaction for background retry only after the immediate mempool
  response says the failure can plausibly become valid at a later tip or when a
  missing mempool parent appears.
- Keep the queue capacity and lossy behavior; the main fix is preserving typed
  failure semantics.

## Finding 4: spent-output collection should fail closed instead of flattening

Status: eliminated as a live vulnerability; defense-in-depth remains.

Evidence:

- `zebra-consensus/src/transaction.rs:690` preallocates
  `Vec<Option<transparent::Output>>` by input index.
- `zebra-consensus/src/transaction.rs:770` converts it with
  `spent_outputs.into_iter().flatten().collect()`.
- `zebra-consensus/src/transaction.rs:590` to
  `zebra-consensus/src/transaction.rs:591` passes these spent outputs into
  `VerifiedUnminedTx::new`.
- `zebrad/src/components/mempool/storage.rs:287` rejects a length mismatch as
  non-standard, but this is a later mempool policy boundary, not the consensus
  verifier boundary.

Updated conclusion:

- The current slot-filling logic should preserve input order when chain and
  mempool UTXO sources are mixed.
- But if a future edit leaves one expected slot as `None`, `flatten()` silently
  shortens the vector. That is a risky failure mode for a consensus-adjacent
  invariant.
- `zebra-script/src/lib.rs:147-153` fails closed if the previous-output vector
  length does not match the transaction input count for script verification.
- `zebrad/src/components/mempool/storage.rs:281-290` also rejects mismatched
  spent-output vectors before mempool policy checks.
- See `docs/analysis/transparent-spent-output-alignment-note.md`.

Recommended fix:

- Replace the final `flatten()` with an explicit check: every transparent
  `PrevOut` input must have a filled previous-output slot.
- Return `TransactionError::TransparentInputNotFound` or a more specific error
  if any expected slot is missing.
- Add a mixed chain-plus-mempool previous-output ordering regression.

## Eliminated or lower-priority leads

### Intra-block out-of-order transparent spends

`zebra-consensus` supplies all same-block outputs as `known_utxos`, including
later transaction outputs, so semantic verification may do script work before
order is known. However, state contextual validation rejects later-output
spends:

- `zebra-state/src/service/check/utxo.rs:126` starts
  `transparent_spend_chain_order`.
- `zebra-state/src/service/check/utxo.rs:143` rejects when the output's
  transaction index is greater than or equal to the spending transaction index.
- Proposal validation goes through cloned state contextual validation at
  `zebra-state/src/service.rs:1655` and
  `zebra-state/src/service.rs:1685`.

Conclusion: this is verifier-cost exposure, not an acceptance bug, unless the
state backstop is bypassed in a future path.

### Mempool same-effects rejection caches

The cache categories are directionally sound:

- exact-tip for authorizing data and standardness,
- same-effects-tip for mempool conflicts and missing mempool outputs,
- same-effects-chain for mined/expired/duplicate/random eviction.

Tip-local rejections are cleared on chain growth, and the lists are bounded.
No consensus-acceptance issue was found in this pass.

### Longpoll ID and template loop

The longpoll loop waits on mempool polling, best-tip changes, or maximum block
time. It may intentionally hold a miner RPC request, but this is distinct from
the unbounded verifier waits in proposal and submit paths.

### Stale sighash buffer for invalid V5 hash types

The current random-digest workaround in `zebra-script/src/lib.rs:227` to
`zebra-script/src/lib.rs:238` addresses the known stale-buffer issue for
callback failures that are detected and handled. The remaining problem is that
canonical `SIGHASH_SINGLE` missing-output is not currently classified as a
failure before the sighash is computed.

## Next checks

1. Add a minimal regression for V5+ canonical `SIGHASH_SINGLE` with too few
   outputs and confirm it fails on current code.
2. Add a fake never-resolving verifier/state service test for `submitblock` and
   GBT proposal mode.
3. Audit `zebra-script` callbacks with `catch_unwind` in mind: no callback-time
   panic should cross the FFI boundary.
4. Confirm pre-NU5 / V4 `SIGHASH_SINGLE` missing-output behavior against zcashd
   before changing raw V4 semantics.
5. If fixing `sendrawtransaction`, keep it local and typed: do not leak
   zebrad-internal error enums into public JSON-RPC responses.
