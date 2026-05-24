# Post-v4.4.0 security audit pass 2 plan

Date: 2026-05-02

## Goal

Run a second, evidence-first security audit pass that turns the first pass'
highest-risk follow-ups into either reproducible findings or eliminated leads,
then broadens into adjacent parser, network, and RPC resource-exhaustion
surfaces.

This pass stays local and responsible: no public exploit payloads, no mainnet
testing, and no PR until the Zebra contribution gate is satisfied.

## Outputs

- A pass-2 findings document in `docs/analysis/`.
- One short reproducer or regression-test sketch for each confirmed issue.
- A table of eliminated leads with the exact code backstop that makes them safe.
- A fix-priority list split into consensus, miner availability, RPC/mempool
  availability, and defense-in-depth.

## Workstream 1: Miner RPC liveness reproduction

Question: can miner-facing RPCs hang on block verifier or state waits that are
already timeout-wrapped in sync/inbound paths?

Primary files:

- `zebra-rpc/src/methods.rs`
- `zebra-rpc/src/methods/types/get_block_template.rs`
- `zebra-rpc/src/methods/types/get_block_template/constants.rs`
- `zebra-consensus/src/router.rs`
- `zebra-consensus/src/block.rs`
- `zebra-state/src/service.rs`
- `zebra-state/src/service/pending_utxos.rs`
- `zebrad/src/components/sync.rs`
- `zebrad/src/components/inbound.rs`

Commands:

```bash
rg -n "Request::Commit|Request::CheckProposal|CheckBlockProposalValidity|AwaitUtxo|timeout\\(|BLOCK_VERIFY_TIMEOUT|response_fut.await" zebra-rpc/src zebra-consensus/src zebra-state/src zebrad/src/components -S
rg -n "wrapped in a timeout|hang indefinitely|out-of-order and invalid" zebra-consensus/src/router.rs zebrad/src/components/sync.rs -S
```

Evidence to collect:

- Exact RPC call sites that await `Request::Commit` or `Request::CheckProposal`
  without `tokio::time::timeout` or `tower::timeout::Timeout`.
- Exact sync/inbound call sites that do wrap the same verifier.
- The state wait path that can remain pending for missing UTXOs or queued
  non-finalized work.

Reproducer plan:

- Add a local-only test service that implements the block verifier trait and
  returns `std::future::pending()` for `call`.
- Exercise `submit_block()` and GBT proposal validation through their public
  method helpers.
- Confirm current behavior does not return under a small test timeout.
- The expected failing assertion before a fix is: "RPC method future did not
  complete before the test timeout."

Stop condition:

- Confirmed if a miner RPC method can await an unbounded verifier/state future
  and no RPC-local deadline exists.
- Eliminated only if jsonrpsee or an RPC middleware layer applies a request
  deadline that covers these futures.

## Workstream 2: SIGHASH matrix and callback failure behavior

Question: are all transparent sighash callback failure modes converted into
script rejection, especially V5+ `SIGHASH_SINGLE` with too few outputs?

Primary files:

- `zebra-script/src/lib.rs`
- `zebra-script/src/tests.rs`
- `zebra-chain/src/transaction/sighash.rs`
- `zebra-chain/src/primitives/zcash_primitives.rs`
- `Cargo.lock`

Commands:

```bash
rg -n "SignedOutputs::Single|raw_bits|sighash_v4_raw|sighash\\(|dummy|OsRng|verify_callback|catch_unwind" zebra-script/src zebra-chain/src -S
rg -n "name = \"zcash_primitives\"|name = \"zcash_transparent\"" Cargo.lock -S
cargo test -p zebra-script sighash_divergence --lib
```

Matrix to cover:

| Tx version | Hash type | Output index exists | Expected result |
| --- | --- | --- | --- |
| V5+ | `ALL` | not relevant | existing behavior |
| V5+ | `NONE` | not relevant | existing behavior |
| V5+ | `SINGLE` | yes | existing behavior |
| V5+ | `SINGLE` | no | reject without panic |
| V5+ | `SINGLE|ANYONECANPAY` | no | reject without panic |
| V5+ | invalid raw byte | not relevant | reject with random digest |
| V4 | raw `SINGLE` | no | compare against zcashd before changing |

Evidence to collect:

- Whether current code ever checks `input_index < transaction.outputs().len()`
  for V5+ `SIGHASH_SINGLE`.
- Whether downstream `zcash_primitives` treats missing V5+ `SINGLE` output as
  empty-output hash material.
- Whether the FFI callback can panic before it reaches the random-digest
  failure path.

Reproducer plan:

- Build a focused `zebra-script` regression with one transparent input and too
  few transparent outputs.
- Use canonical `SIGHASH_SINGLE`, not malformed hash-type bytes.
- Assert `CachedFfiTransaction::is_valid(0)` returns `Err` and does not panic.
- Add a second stale-buffer style test where a valid `CHECKSIG` runs before a
  canonical missing-output `SIGHASH_SINGLE` `CHECKSIG`.

Stop condition:

- Confirmed if current code produces a valid digest for missing-output V5+
  `SIGHASH_SINGLE` instead of callback failure.
- Eliminated only if a hidden layer rejects the transaction before or during
  script verification for this exact condition.

## Workstream 3: Mempool versus block validation parity

Question: can a transaction be accepted into the mempool under assumptions that
would fail block validation, or can rejection caches suppress valid transactions
past their intended scope?

Primary files:

- `zebra-consensus/src/transaction.rs`
- `zebra-consensus/src/transaction/check.rs`
- `zebrad/src/components/mempool.rs`
- `zebrad/src/components/mempool/downloads.rs`
- `zebrad/src/components/mempool/storage.rs`
- `zebrad/src/components/mempool/storage/verified_set.rs`
- `zebrad/src/components/mempool/storage/policy.rs`
- `zebra-node-services/src/mempool.rs`

Commands:

```bash
rg -n "Request::Mempool|Request::Block|is_mempool\\(|VerifiedUnminedTx::new|spent_outputs|reject_if_needed|clear_tip_rejections|SameEffects|ExactTip|chain_rejected" zebra-consensus/src zebrad/src/components/mempool zebra-node-services/src -S
cargo test -p zebra-consensus transaction --lib
cargo test -p zebrad mempool --lib
```

Checks:

- Build a table of every branch guarded by `req.is_mempool()`.
- Verify coinbase maturity, locktime, expiry, branch ID, duplicate spends, and
  transparent input loading are either identical or intentionally different.
- Verify all tip-local rejection caches clear on best-tip growth, rollback, and
  network upgrade.
- Verify same-effects cache keys use mined IDs only where authorizing-data
  changes cannot alter the rejected effect.
- Verify `VerifiedUnminedTx.spent_outputs` order always matches transparent
  input order when inputs are resolved from both chain state and mempool.

Reproducer plan:

- Construct a mempool transaction with two transparent inputs:
  - input 0 resolved from best-chain state,
  - input 1 resolved from a parent transaction already in mempool.
- Assert the returned `VerifiedUnminedTx.spent_outputs` vector preserves input
  order.
- Add a negative test that a missing previous-output slot fails closed rather
  than reaching standardness as a shortened vector.

Stop condition:

- Confirmed if mempool verification can produce a `VerifiedUnminedTx` whose
  previous outputs are missing, shortened, or misordered.
- Eliminated if consensus verifier construction fails before storage policy and
  tests lock in mixed-source ordering.

## Workstream 4: Proposal validation side effects and state backstops

Question: does GBT proposal validation run all contextual backstops without
mutating canonical state or leaving pending waiters behind?

Primary files:

- `zebra-state/src/service.rs`
- `zebra-state/src/service/write.rs`
- `zebra-state/src/service/non_finalized_state.rs`
- `zebra-state/src/service/check/utxo.rs`
- `zebra-state/src/service/check/anchors.rs`
- `zebra-state/src/service/check/nullifier.rs`
- `zebra-state/src/service/pending_utxos.rs`
- `zebra-rpc/src/methods/types/get_block_template.rs`

Commands:

```bash
rg -n "CheckBlockProposalValidity|validate_and_commit_non_finalized|disable_metrics|latest_non_finalized_state|transparent_spend|EarlyTransparentSpend|pending_utxos|respond\\(" zebra-state/src zebra-rpc/src -S
cargo test -p zebra-state utxo --lib
```

Checks:

- Confirm proposal validation uses a cloned non-finalized state.
- Confirm transparent order, duplicate spend, immature coinbase spend,
  value-pool, anchor, and nullifier checks all run in proposal mode.
- Confirm proposal validation cannot call `pending_utxos.check_against_ordered`
  on canonical state in a way that wakes unrelated verifiers.
- Confirm metrics disabling does not skip validation.

Stop condition:

- Confirmed if `CheckBlockProposalValidity` can mutate canonical state, wake
  pending canonical waiters, or skip a contextual check that commit uses.
- Eliminated if cloned-state validation covers the same contextual checks and
  no canonical pending state is touched.

## Workstream 5: Parser, allocation, and network resource limits

Question: are attacker-controlled counts bounded before allocation across
network messages, blocks, transactions, and RPC request bodies?

Primary files:

- `zebra-chain/src/serialization/`
- `zebra-chain/src/transaction.rs`
- `zebra-chain/src/transparent/`
- `zebra-network/src/protocol/external/`
- `zebra-network/src/peer/`
- `zebra-rpc/src/server.rs`
- `zebra-rpc/src/server/http_request_compatibility.rs`

Commands:

```bash
rg -n "TrustedPreallocate|max_allocation|zcash_deserialize|Vec<|read_.*compact|compactsize|MAX_.*BYTES|Limited::new|max_request_body_size|max_response_body_size" zebra-chain/src zebra-network/src zebra-rpc/src -S
cargo test -p zebra-chain preallocate --lib
cargo test -p zebra-network preallocate --lib
```

Checks:

- Every vector length derived from serialized input has a trusted preallocation
  bound or consensus size cap.
- RPC body collection is bounded before JSON parsing and before authentication
  bypass paths.
- Response compatibility middleware does not duplicate unbounded responses.
- Network message parsing cannot allocate based on inventory or compact-block
  counts before applying protocol caps.

Stop condition:

- Confirmed if an unauthenticated peer or RPC caller can force allocation or CPU
  work above documented caps before rejection.
- Eliminated if the relevant parser uses `TrustedPreallocate`, message size
  limits, or request-body caps before allocation.

## Workstream 6: RPC exposure, auth, and concurrency limits

Question: can authenticated or accidentally exposed RPC endpoints be used to
starve consensus, mempool, or JSON-RPC worker resources?

Primary files:

- `zebra-rpc/src/config/rpc.rs`
- `zebra-rpc/src/server.rs`
- `zebra-rpc/src/server/http_request_compatibility.rs`
- `zebra-rpc/src/methods.rs`
- `zebrad/src/commands/start.rs`

Commands:

```bash
rg -n "listen_addr|enable_cookie_auth|check_credentials|max_request_body_size|max_response_body_size|Server::builder|rpc_middleware|parallel_cpu_threads|timeout|rate" zebra-rpc/src zebrad/src -S
cargo test -p zebra-rpc server --lib
```

Checks:

- Confirm cookie auth is checked before body collection.
- Confirm unauthenticated large requests are rejected before expensive parsing.
- Identify whether jsonrpsee has configured per-connection, per-method, or
  global concurrency limits in this server setup.
- Identify RPC methods that can await consensus/state/mempool services without
  per-method deadlines.

Stop condition:

- Confirmed if an RPC method can hold unbounded server resources with no auth,
  no request size cap, no concurrency cap, or no method-level deadline.
- Eliminated if auth, size caps, and concurrency settings all apply before
  expensive work.

## Workstream 7: Dependency drift against upstream Zcash behavior

Question: do Zebra's local wrappers rely on upstream `zcash_primitives`,
`zcash_transparent`, or `zcash_script` behavior that differs from current zcashd
consensus or policy?

Primary files:

- `Cargo.lock`
- `zebra-chain/src/primitives/zcash_primitives.rs`
- `zebra-script/src/lib.rs`
- `zebrad/src/components/mempool/storage/policy.rs`

Commands:

```bash
cargo tree -i zcash_primitives
cargo tree -i zcash_transparent
cargo tree -i zcash_script
rg -n "zcashd|v6\\.11\\.0|policy.cpp|standard.cpp|SIGHASH|from_bits|raw_bits" zebra-chain/src zebra-script/src zebrad/src -S
```

Checks:

- Compare all local "match zcashd" comments to the cited upstream versions.
- For consensus behavior, prefer Zcash ZIPs and zcashd behavior over
  dependency defaults when they disagree.
- Revalidate pre-NU5 `SIGHASH_SINGLE` missing-output semantics before changing
  V4 raw handling.

Stop condition:

- Confirmed if a local wrapper delegates a consensus edge case to a dependency
  whose behavior is policy-compatible but not consensus-compatible.
- Eliminated if the wrapper explicitly checks Zebra's intended consensus rule
  before calling the dependency.

## Workstream 8: Proof/signature batch verifier failure semantics

Question: do batch verifier panics, dropped tasks, or channel closures become
node crashes or hidden acceptance failures?

Primary files:

- `zebra-consensus/src/primitives/halo2.rs`
- `zebra-consensus/src/primitives/groth16.rs`
- `zebra-consensus/src/primitives/redjubjub.rs`
- `zebra-consensus/src/primitives/redpallas.rs`
- `zebra-consensus/src/primitives/ed25519.rs`

Commands:

```bash
rg -n "spawn|spawn_blocking|watch::|oneshot|panic!|expect\\(|unwrap\\(|flush|batch|Verifier" zebra-consensus/src/primitives -S
cargo test -p zebra-consensus primitives --lib
```

Checks:

- Panics in verifier worker tasks are converted into verification errors, not
  hidden success.
- Dropped batch senders fail closed.
- `expect()` and `panic!()` sites are unreachable under attacker-controlled
  inputs.
- No verifier future can wait forever after a batch service is dropped.

Stop condition:

- Confirmed if an attacker-controlled proof/signature can trigger process abort
  or indefinite wait rather than a verification error.
- Eliminated if each panic is restricted to shutdown or internal invariant
  failure outside untrusted data.

## Triage order

1. Finish `SIGHASH_SINGLE` reproducer and fix sketch.
2. Reproduce miner RPC no-timeout behavior.
3. Fail-closed `spent_outputs` invariant and ordering regression.
4. Classify `sendrawtransaction` retry queue failures.
5. Run parser/allocation and RPC concurrency scans.
6. Run proof/signature batch verifier failure scan.
7. Do dependency drift comparison last, because it may require current upstream
   source checks.

## Completion criteria

This pass is complete when:

- Every workstream has either a confirmed finding or an eliminated-lead note.
- Confirmed findings include file/line evidence and a minimal regression plan.
- No finding relies on stale local assumptions where upstream behavior is the
  source of truth.
- Any external disclosure text avoids exploit-ready payloads and describes
  impact, affected path, and recommended fix at a defensive level.
