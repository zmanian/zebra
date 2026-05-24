# Mining RPC Verifier Timeout Security Note

Date: 2026-05-02

Last updated: 2026-05-09

Disposition: already had public context posted before the stop instruction via
#9301. Keep further work local unless explicitly re-authorized.

Scope: follow-up audit of the RPC `submitblock` and `getblocktemplate`
proposal-mode paths against Zebra's verifier timeout expectations.

## Finding

The RPC `submitblock` method and the `getblocktemplate` proposal-mode path call
the block verifier router directly and do not apply a timeout around verifier
readiness or verification. This is inconsistent with Zebra's own router
documentation and with the sync and inbound block verification paths, which wrap
the same verifier in `BLOCK_VERIFY_TIMEOUT`.

This is an availability concern for mining/RPC deployments. It is not a
consensus acceptance bug by itself.

## Evidence

`zebra-consensus/src/router.rs` explicitly warns:

> Block and transaction verification requests should be wrapped in a timeout
> ... Otherwise, verification of out-of-order and invalid blocks and
> transactions can hang indefinitely.

The sync path applies that timeout in `zebrad/src/components/sync.rs`:

```rust
let verifier = Timeout::new(verifier, BLOCK_VERIFY_TIMEOUT);
```

The inbound block download path applies that timeout in
`zebrad/src/components/inbound.rs`:

```rust
Timeout::new(block_verifier, BLOCK_VERIFY_TIMEOUT)
```

The RPC `submitblock` path in `zebra-rpc/src/methods.rs` does not:

```rust
let block_verifier_router_response = block_verifier_router
    .ready()
    .await
    .map_err(|error| ErrorObject::owned(0, error.to_string(), None::<()>))?
    .call(zebra_consensus::Request::Commit(Arc::new(block)))
    .await;
```

The `getblocktemplate` proposal-mode path also calls the verifier directly:

```rust
let block_verifier_router_response = block_verifier_router
    .ready()
    .await
    .map_err(|error| ErrorObject::owned(0, error.to_string(), None::<()>))?
    .call(zebra_consensus::Request::CheckProposal(Arc::new(block)))
    .await;
```

I added three local current-behavior repros:

- `rpc_submitblock_waits_without_timeout_when_block_verifier_hangs_today`
- `rpc_submitblock_waits_without_timeout_for_out_of_order_checkpoint_block_today`
- `rpc_getblocktemplate_proposal_waits_without_timeout_when_block_verifier_hangs_today`

The first test installs a block verifier service whose future never resolves,
calls `submitblock` with a structurally valid block, and confirms the RPC future
does not return before an outer test timeout.

The second test uses Zebra's real checkpoint verifier on an empty mainnet state.
It submits mainnet block 1 before genesis has verified. The checkpoint verifier
queues the block and waits for a contiguous checkpoint range; because the RPC path
does not add its own timeout, `submitblock` remains pending until the test's
outer timeout fires.

The third test installs a pending block verifier, calls `getblocktemplate` in
proposal mode with a structurally valid block, and confirms the RPC future does
not return before an outer test timeout.

Verification:

```sh
cargo test -p zebra-rpc rpc_submitblock_waits_without_timeout_when_block_verifier_hangs_today --lib
cargo test -p zebra-rpc rpc_submitblock_waits_without_timeout_for_out_of_order_checkpoint_block_today --lib
cargo test -p zebra-rpc rpc_getblocktemplate_proposal_waits_without_timeout_when_block_verifier_hangs_today --lib
```

Result: passed.

Re-verified on 2026-05-03:

```sh
cargo test -p zebra-rpc rpc_submitblock_waits_without_timeout_when_block_verifier_hangs_today --lib
cargo test -p zebra-rpc rpc_submitblock_waits_without_timeout_for_out_of_order_checkpoint_block_today --lib
cargo test -p zebra-rpc rpc_getblocktemplate_proposal_waits_without_timeout_when_block_verifier_hangs_today --lib
```

Result: passed.

Re-verified on 2026-05-07:

```sh
cargo test -p zebra-rpc rpc_submitblock_waits_without_timeout_when_block_verifier_hangs_today --lib
cargo test -p zebra-rpc rpc_submitblock_waits_without_timeout_for_out_of_order_checkpoint_block_today --lib
cargo test -p zebra-rpc rpc_getblocktemplate_proposal_waits_without_timeout_when_block_verifier_hangs_today --lib
```

Result: all three commands passed.

Re-verified on 2026-05-09:

```sh
cargo test -p zebra-rpc rpc_submitblock_waits_without_timeout_when_block_verifier_hangs_today --lib
cargo test -p zebra-rpc rpc_submitblock_waits_without_timeout_for_out_of_order_checkpoint_block_today --lib
cargo test -p zebra-rpc rpc_getblocktemplate_proposal_waits_without_timeout_when_block_verifier_hangs_today --lib
```

Result: all three commands passed.

Read-only duplicate refresh on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra submitblock getblocktemplate verifier timeout mining RPC'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "submitblock" "timeout" "verifier"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "getblocktemplate" "proposal" "timeout"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "BLOCK_VERIFY_TIMEOUT" "submitblock"'
```

Closest overlaps remain open #9301 ("DoS vulnerability in `getblocktemplate`
RPC") plus the historical implementation PRs #5526 (`submitblock`) and #5870
(`getblocktemplate` proposal mode). #9301 covers GBT mempool/proposal validation
parity and general GBT DoS risk, but it does not spell out the `submitblock`
timeout gap, the proposal-mode verifier wait gap, or the post-dispatch
error-taxonomy issue tracked here.

The checkpoint behavior is expected by design at the verifier layer:

- `zebra-consensus/src/router.rs` warns that checkpoint verification waits for
  previous blocks and requires caller-side timeouts.
- `zebra-consensus/src/checkpoint.rs` queues blocks, returns `WaitingForBlocks`
  when the queued range is not contiguous, and only sends the per-block result
  once a checkpoint range verifies or the verifier is dropped.
- Existing checkpoint tests also rely on pending futures for non-contiguous or
  bad checkpoint blocks until a good contiguous block resolves the range.

## Attack Shape

If an attacker or miner can submit a structurally valid block that causes the
real verifier/state path to wait indefinitely or for an excessive duration, then
the corresponding RPC task can remain occupied indefinitely. Multiple such
requests could tie up RPC worker capacity and create an operational DoS for
mining infrastructure.

Confirmed real-verifier shape:

- On a node that has not completed checkpoint verification, an out-of-order
  checkpoint-era block submitted over RPC can wait for missing previous
  checkpoint blocks.
- This is strongest as a startup/sync/RPC-exposure concern. It is less compelling
  against a normal fully synced mining node, because old checkpoint blocks are no
  longer routed through an active waiting checkpoint verifier once checkpoint
  verification has finished.

Related semantic-verifier shape:

- Above checkpoint height, full block verification waits for previous-block UTXOs.
  Transaction verification wraps UTXO lookup in its own `UTXO_LOOKUP_TIMEOUT`, so
  missing transparent inputs appear bounded by that inner timeout rather than
  indefinite. RPC still lacks the global block timeout used by sync and inbound.
- The block-context transparent UTXO path is still a miner-facing latency
  surface: `zebra-state/src/request.rs:930-945` documents `AwaitUtxo` as a
  request that should be timeout-wrapped, and `zebra-consensus/src/transaction.rs`
  performs block-context UTXO lookups through that state request. The timeout is
  local to the transaction verifier, not the RPC method, and multiple missing
  inputs in one transaction are resolved sequentially.

Verifier readiness/backpressure shape:

- `submitblock` and proposal-mode `getblocktemplate` both await
  `block_verifier_router.ready()` before calling the verifier. In production
  wiring this router is buffered, so a full verifier buffer or stalled verifier
  workers can make RPC wait before the block is even dispatched. This is a
  separate wait surface from the verifier future itself.

This is close in shape to the Litecoin-style incident class discussed earlier:
a malformed or contextually problematic submitted block does not need to be
accepted to harm mining operations if submission/verification tasks get stuck
instead of timing out and returning an error.

`getblocktemplate` proposal mode is part of the same miner-facing surface: a
pool or miner can ask Zebra to validate a candidate block before submission. The
same missing timeout means proposal validation can hang rather than returning a
bounded rejection.

`generate` and the experimental internal miner inherit the same submission
surface:

- `zebra-rpc/src/methods.rs:2946-2998` implements `generate` by calling
  `get_block_template(None)`, building a proposal block, then awaiting
  `submit_block`.
- `zebrad/src/components/miner.rs:489-518` submits solved internal-miner blocks
  by awaiting `rpc.submit_block(HexData(data), None)`.

The internal miner is feature-gated and off by default, but when enabled it does
not add a separate submission deadline around this reused RPC path.

## Result Classification

The liveness issue is paired with a miner-facing result-taxonomy issue.

`submitblock` currently maps results as follows:

| Condition | Current result |
| --- | --- |
| Block bytes fail structural deserialization | `Rejected` |
| Verifier `ready().await` fails before dispatch | JSON-RPC error |
| Verifier returns `Ok(hash)` and mined-block gossip send succeeds | `Accepted` |
| Verifier returns `Ok(hash)` but mined-block gossip send fails | JSON-RPC error after the block was already accepted by the verifier |
| Verifier error downcasts to `RouterError` and `is_duplicate_request()` is true | `Duplicate` |
| Any other downcastable `RouterError` | `Rejected` |
| Non-`RouterError` boxed verifier error | `Rejected` |

This means `DuplicateInconclusive` and `Inconclusive` are defined in
`zebra-rpc/src/methods/types/submit_block.rs:37-45`, but this path does not
currently emit them. Infrastructure or shutdown failures after verifier dispatch
can be reported as definitive rejection.

Proposal mode currently maps results as follows:

| Condition | Current result |
| --- | --- |
| Sync/tip precheck fails | JSON-RPC error |
| Proposal bytes fail structural deserialization | `Rejected(...)` |
| Verifier `ready().await` fails before dispatch | JSON-RPC error |
| Verifier returns `Ok(_)` | `Valid` |
| Any verifier error after dispatch | `Rejected(...)` string |

`BlockProposalResponse::rejected()` ignores its `_kind` argument and normalizes
the debug/error text into a kebab-case string. Therefore a transient state,
shutdown, cancellation, or verifier infrastructure error after dispatch is
surfaced through the same channel as an invalid block proposal.

There is also an accepted-but-erroring corner case: after `submitblock`
verification succeeds, `zebra-rpc/src/methods.rs:2590-2592` calls
`advertise_mined_block()`, which uses a bounded `try_send()` at
`zebra-rpc/src/methods/types/get_block_template.rs:548-555`. If that channel is
full or closed, RPC returns an error even though the verifier already accepted
the block. A miner or caller can then retry and see a duplicate-like result.

## Proposal State Effects

Proposal validation does not appear to mutate canonical state:

- `zebra-state/src/service.rs:1655-1660` handles
  `ReadRequest::CheckBlockProposalValidity` by cloning the latest non-finalized
  state.
- `zebra-state/src/service.rs:1678-1683` documents that the clone is dropped
  and the non-finalized state used by the rest of Zebra is not mutated.
- `zebra-state/src/service.rs:1685-1691` validates and commits the proposal
  only into that clone, then returns `ValidBlockProposal`.

So timeout or cancellation of proposal validation is not expected to commit a
proposal block to canonical state. The remaining risk is resource/liveness and
result classification, not unintended state mutation.

## Impact

Expected impact:

- stalled `submitblock` RPC calls,
- stalled `getblocktemplate` proposal-mode RPC calls,
- reduced or exhausted RPC request capacity under repeated submissions,
- mining/pool operational disruption if block submissions are made through
  Zebra RPC,
- harder incident handling because callers see a hang rather than a bounded
  reject/duplicate/inconclusive response.

The confirmed local repros prove the missing RPC timeout for both miner-facing
verifier entrypoints, including one real checkpoint-verifier wait path for
`submitblock`. They do not yet prove a high-impact current-chain attack against
a fully synced mining deployment.

## Disclosure Triage

Suggested triage: private maintainer heads-up if bundled with other
mining-availability concerns; otherwise public hardening.

Reasoning:

- The missing timeout is direct and high confidence.
- Exploitability against fully synced mining infrastructure still depends on
  finding a real current-chain verifier wait path reachable from `submitblock`,
  or on verifier/state stalls from operational conditions.
- RPC is disabled by default and cookie-authenticated by default, but mining
  deployments intentionally expose RPC to trusted pool/miner components.

## Suggested Fix

- Wrap the `submitblock` and `getblocktemplate` proposal verifier calls in
  `BLOCK_VERIFY_TIMEOUT`, or expose an equivalent timeout constant from a shared
  crate so RPC does not duplicate zebrad component internals.
- Apply the timeout to both `ready().await` and `call(...).await`.
- Map timeout, shutdown, verifier cancellation, state-service failure, and
  unknown boxed errors to an infrastructure/unknown result, not to definitive
  `Rejected` / invalid-proposal strings. For `submitblock`, prefer
  `Inconclusive` where compatible with zcashd semantics; for proposal mode,
  prefer a JSON-RPC server error for infrastructure failures.
- Treat a post-accept mined-block gossip send failure as a logging/notification
  failure after acceptance, or otherwise document and test that callers may see
  an RPC error after successful verifier acceptance.
- Add regression tests where a pending verifier causes both RPCs to return
  timeout-mapped responses rather than hanging.
- Add regression tests for:
  - verifier readiness/backpressure waits,
  - unknown boxed verifier errors collapsing to `Rejected` today,
  - proposal-mode unknown verifier errors becoming rejection strings today,
  - successful verifier acceptance followed by mined-block gossip channel
    failure.

## Confidence

Confidence: high that `submitblock` and `getblocktemplate` proposal mode lack a
timeout and can wait indefinitely if the verifier future waits indefinitely.

Confidence: high that the real checkpoint verifier can leave an RPC `submitblock`
future pending for out-of-order checkpoint-era blocks until missing earlier
blocks arrive or the verifier is dropped.

Confidence: medium-low on high-impact security severity for ordinary fully
synced mining deployments, because the confirmed real-verifier path is strongest
before checkpoint verification has completed.
