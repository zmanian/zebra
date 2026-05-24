# Post-v4.4.0 security audit pass 3 plan

Date: 2026-05-02

## Goal

Run a focused follow-up pass on validation outcomes that are incomplete,
timeout-driven, cancelled, or otherwise infrastructure-derived, and make sure
Zebra does not report them as definitive consensus results.

The pass uses the miner RPC liveness finding as the seed case. It then expands
to nearby RPC, verifier, state, and miner loop paths where "unknown" can be
accidentally collapsed into "accepted", "rejected", or "invalid".

This pass stays local and responsible: no public exploit payloads, no mainnet
testing, and no PR until the Zebra contribution gate is satisfied.

## Working theory

The most useful next audit question is:

> When Zebra starts validation work but cannot finish it, does every caller keep
> the result unknown, or does any path turn a liveness/infrastructure failure
> into a consensus/RPC judgment?

Seed evidence:

- `zebra-consensus/src/router.rs:8` documents that block and transaction
  verification requests should be timeout-wrapped because invalid or
  out-of-order work can hang indefinitely.
- `zebra-state/src/service.rs:1096` handles `Request::AwaitUtxo`; when no UTXO
  is found, it waits at `zebra-state/src/service.rs:1164`.
- `zebrad/src/commands/start.rs:250` to `zebrad/src/commands/start.rs:260`
  passes `block_verifier_router.clone()` directly into `RpcImpl::new`.
- `zebra-rpc/src/methods.rs:2573` to `zebra-rpc/src/methods.rs:2578` has
  `submitblock` await `Request::Commit` without an RPC-local timeout.
- `zebra-rpc/src/methods/types/get_block_template.rs:657` to
  `zebra-rpc/src/methods/types/get_block_template.rs:662` has GBT proposal
  validation await `Request::CheckProposal` without an RPC-local timeout.
- `zebra-rpc/src/methods.rs:2633` to `zebra-rpc/src/methods.rs:2637` maps
  non-duplicate or unknown verifier errors to `Rejected`.
- `zebrad/src/components/miner.rs:500` has the internal miner await
  `rpc.submit_block(...)`, so the same RPC liveness semantics can stall Zebra's
  internal miner loop.

## Outputs

- A pass-3 findings document in `docs/analysis/`.
- A classification table for every inspected path:
  - consensus-invalid,
  - duplicate or already-known,
  - side-chain / inconclusive,
  - timeout / cancellation / infrastructure failure,
  - caller abandoned result.
- A minimal regression-test sketch for each confirmed mismatch.
- A fix-priority list split into consensus risk, miner availability,
  RPC compatibility, and defense-in-depth.

## Workstream 1: Miner RPC timeout and result classification

Question: can `submitblock` or GBT proposal validation return, hang, or classify
errors in a way that misleads miners about block validity?

Primary files:

- `zebrad/src/commands/start.rs`
- `zebra-rpc/src/methods.rs`
- `zebra-rpc/src/methods/types/get_block_template.rs`
- `zebra-rpc/src/methods/types/submit_block.rs`
- `zebra-rpc/src/server.rs`
- `zebra-rpc/src/server/error.rs`
- `zebra-rpc/src/server/*.rs`

Commands:

```bash
rg -n "RpcImpl::new|block_verifier_router.clone\\(\\)|submit_block\\(|Request::Commit|Request::CheckProposal|SubmitBlockErrorResponse|BlockProposalResponse::rejected|LegacyCode|ErrorObject" zebrad/src zebra-rpc/src -S
rg -n "timeout\\(|Timeout::new|tower::timeout|request.*deadline|body_limit|max_request|middleware" zebra-rpc/src zebrad/src -S
```

Checks:

- Confirm whether production RPC receives a raw router or an already
  timeout-wrapped router.
- Confirm whether jsonrpsee or Zebra RPC middleware imposes any deadline that
  covers method futures.
- Separate `ready().await` timeout from `call(...).await` timeout; both are
  miner-facing liveness boundaries.
- Audit every `submitblock` mapping so infrastructure failures cannot become
  `Rejected`.
- Audit GBT proposal validation so verifier timeout cannot become
  `BlockProposalResponse::Rejected("invalid proposal", ...)`.

Reproducer plan:

- Build an RPC test with a mock block verifier that accepts `Request::Commit`
  and never responds.
- Build a GBT proposal test with a mock block verifier that accepts
  `Request::CheckProposal` and never responds.
- Assert current futures remain pending under a small test timeout.
- After a fix, assert `submitblock` timeout maps to `inconclusive` and GBT
  proposal timeout maps to a JSON-RPC server error, not a rejected proposal.

Stop condition:

- Confirmed if any miner RPC verifier wait is unbounded or any timeout-like
  error is mapped as a definitive rejection.
- Eliminated only if the production server stack applies a deadline and the RPC
  methods preserve timeout errors as unknown/inconclusive results.

## Workstream 2: Internal miner amplification path

Question: does Zebra's internal miner inherit RPC liveness bugs, and can one
stuck `submitblock` call prevent future template work or solved blocks from
being processed?

Primary files:

- `zebrad/src/components/miner.rs`
- `zebrad/src/commands/start.rs`
- `zebra-rpc/src/methods.rs`
- `zebra-rpc/src/methods/types/get_block_template.rs`

Commands:

```bash
rg -n "submit_block\\(|get_block_template\\(|spawn_init|tokio::spawn|cancel_fn|submit_old|template_sender|solver_id|any_success" zebrad/src/components/miner.rs zebra-rpc/src/methods.rs -S
```

Checks:

- Trace the internal miner loop around solved block submission.
- Check whether a stuck `rpc.submit_block(...).await` blocks all later mined
  blocks in the same solver result batch.
- Check whether template cancellation or `submit_old` logic can bypass the
  submission await.
- Check whether the central RPC timeout fix fully protects the internal miner,
  or whether the miner loop also needs its own outer deadline.

Reproducer plan:

- Prefer a unit test around `submit_block` because Equihash solving is not a
  useful test dependency.
- If a miner-loop test is practical, inject a fake `RpcServer` implementation
  whose `submit_block` future never resolves and assert the loop stops making
  progress.
- Otherwise document the inherited impact with call-chain evidence and make the
  RPC method timeout the primary regression boundary.

Stop condition:

- Confirmed if internal mining has no independent liveness boundary around
  `rpc.submit_block`.
- Eliminated if the miner loop has an outer cancellation or timeout that
  triggers while submission is pending.

## Workstream 3: State `AwaitUtxo` waiter lifecycle and cancellation

Question: when a verifier future is dropped or timed out while waiting on
`AwaitUtxo`, does state cleanup remain bounded and non-observable to unrelated
validation work?

Primary files:

- `zebra-state/src/service.rs`
- `zebra-state/src/service/pending_utxos.rs`
- `zebra-state/src/request.rs`
- `zebra-consensus/src/transaction.rs`
- `zebra-consensus/src/router.rs`

Commands:

```bash
rg -n "AwaitUtxo|pending_utxos|queue\\(|respond\\(|prune\\(|response_fut\\.await|oneshot\\(zebra_state::Request::AwaitUtxo" zebra-state/src zebra-consensus/src -S
```

Checks:

- Confirm that dropping a verifier future drops the pending UTXO receiver.
- Confirm `PendingUtxos::prune()` eventually removes orphaned senders.
- Confirm timeout/cancellation cannot wake unrelated waiters or satisfy a later
  verifier with stale data.
- Confirm no metric or queue grows without bound if many invalid proposals are
  timed out or abandoned.

Reproducer plan:

- Create a state-service test that starts an `AwaitUtxo`, drops the waiting
  future, runs prune or the normal cleanup path, and asserts no waiter remains.
- If internal fields are private, add a focused observable test using a missing
  UTXO request followed by a real response for the same outpoint.

Stop condition:

- Confirmed if dropped verifier waits leave persistent pending UTXO state or
  cross-talk into future waits.
- Eliminated if receiver drop plus prune cleanup is bounded and regression
  covered.

## Workstream 4: Consensus verifier error taxonomy sweep

Question: where do timeout, cancellation, channel closure, readiness failure, or
downcast failure get collapsed into ordinary validation errors?

Primary files:

- `zebra-consensus/src/router.rs`
- `zebra-consensus/src/block.rs`
- `zebra-consensus/src/transaction.rs`
- `zebra-consensus/src/error.rs`
- `zebrad/src/components/sync.rs`
- `zebrad/src/components/inbound.rs`
- `zebrad/src/components/mempool/downloads.rs`
- `zebra-rpc/src/methods.rs`

Commands:

```bash
rg -n "downcast|BoxError|Elapsed|timeout|Timeout|map_err|unwrap_or_else|Rejected|rejected|invalid|duplicate|is_duplicate_request|ready\\(\\)\\.await" zebra-consensus/src zebrad/src/components zebra-rpc/src -S
```

Checks:

- Build an error taxonomy table for verifier callers:
  - sync,
  - inbound,
  - mempool,
  - RPC `submitblock`,
  - RPC GBT proposal,
  - internal miner through RPC.
- Check whether `tower::timeout::error::Elapsed` is recognized anywhere.
- Check whether channel closure or dropped verifier tasks can masquerade as
  block invalidity.
- Check whether duplicate detection remains separate from queue replacement,
  cancellation, or backpressure.

Reproducer plan:

- Use mock services that return:
  - `RouterError`,
  - boxed timeout elapsed,
  - boxed arbitrary internal error,
  - pending forever.
- Assert each caller's externally visible mapping matches the taxonomy table.

Stop condition:

- Confirmed if any caller reports a non-consensus infrastructure failure as
  consensus-invalid.
- Eliminated if all inspected callers preserve the difference or are
  intentionally non-user-facing.

## Workstream 5: Proposal validation side effects after timeout

Question: if GBT proposal validation times out or is cancelled, can it leave
state, metrics, pending UTXOs, or gossip notifications in a misleading state?

Primary files:

- `zebra-rpc/src/methods/types/get_block_template.rs`
- `zebra-state/src/service.rs`
- `zebra-state/src/service/write.rs`
- `zebra-state/src/service/non_finalized_state.rs`
- `zebra-state/src/service/check/*.rs`
- `zebra-rpc/src/methods/types/submit_block.rs`

Commands:

```bash
rg -n "CheckBlockProposalValidity|validate_and_commit_non_finalized|disable_metrics|latest_non_finalized_state|pending_utxos|advertise_mined_block|SubmitBlockChannel|respond\\(" zebra-rpc/src zebra-state/src -S
```

Checks:

- Confirm proposal validation continues to use cloned state and does not mutate
  canonical state on success, failure, timeout, or cancellation.
- Confirm timeout does not call `advertise_mined_block`.
- Confirm metrics disabling does not skip contextual checks or hide repeated
  timeout behavior.
- Confirm proposal validation and commit validation have matching contextual
  checks before the result classification layer diverges.

Reproducer plan:

- Add a mock proposal verifier timeout test first.
- Only add deeper state tests if the call graph shows proposal validation can
  mutate shared state before returning.

Stop condition:

- Confirmed if timed-out proposal validation can leave canonical side effects.
- Eliminated if all side effects are either cloned-state-local or only happen
  after a successful commit.

## Workstream 6: SIGHASH failure-path integration with result taxonomy

Question: after the V5+ `SIGHASH_SINGLE` missing-output candidate is fixed, do
all callback failure paths become script rejection rather than panic, stale
digest reuse, or infrastructure error?

Primary files:

- `zebra-script/src/lib.rs`
- `zebra-script/src/tests.rs`
- `zebra-chain/src/transaction/sighash.rs`
- `zebra-chain/src/primitives/zcash_primitives.rs`

Commands:

```bash
rg -n "SignedOutputs::Single|sighash\\(|sighash_v4_raw|verify_callback|catch_unwind|dummy|OsRng|input_index|outputs\\(\\)" zebra-script/src zebra-chain/src -S
```

Checks:

- Keep canonical missing-output `SIGHASH_SINGLE` separate from malformed raw
  sighash bytes.
- Confirm callback failure always returns the random-digest rejection path, not
  a panic that aborts verification.
- Confirm a failed callback cannot reuse a previous valid sighash buffer.
- Confirm V4 behavior is not changed without zcashd parity evidence.

Reproducer plan:

- Add canonical V5+ missing-output `SIGHASH_SINGLE` and
  `SIGHASH_SINGLE|ANYONECANPAY` regressions.
- Add a two-input stale-buffer regression: valid transparent input first,
  missing-output `SIGHASH_SINGLE` second.

Stop condition:

- Confirmed if missing-output canonical `SIGHASH_SINGLE` can still produce a
  valid digest or non-rejection failure mode.
- Eliminated if the fixed path reliably rejects and tests cover stale-buffer
  ordering.

## Workstream 7: Disclosure-ready writeup and patch boundary

Question: what is the smallest responsible patch set and writeup boundary if
this pass confirms the timeout/result-classification issue?

Checks:

- Keep miner RPC timeout changes local to `zebra-rpc` unless production wiring
  proves a broader timeout wrapper is already intended.
- Do not change template-mode longpoll behavior.
- Use existing `submitblock` serialized values when possible:
  - timeout or unknown commit status should be `inconclusive`,
  - consensus invalidity should remain `rejected`,
  - duplicate should remain `duplicate`.
- GBT proposal timeout should be a JSON-RPC server error, not an
  `invalid proposal` response.
- Include test evidence and AI disclosure if this ever becomes a PR.

Stop condition:

- Produce one local findings doc with:
  - evidence,
  - minimized impact statement,
  - no exploit payload,
  - suggested tests,
  - suggested patch boundary.
