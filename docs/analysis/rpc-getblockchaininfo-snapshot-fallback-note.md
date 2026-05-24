# RPC getblockchaininfo snapshot and fallback note

Date: 2026-05-03

Last updated: 2026-05-09

Disposition: local-only. Do not post publicly without explicit
re-authorization.

## Summary

`getblockchaininfo` assembles one JSON response from several independently served
state reads and chain-tip watcher reads. The `blocks`, `bestblockhash`,
`chain_supply`, `value_pools`, `headers`, `upgrades`, and `consensus` fields are
based on `TipPoolValues`; `difficulty` is based on a separate `ChainInfo` read;
and `estimatedheight` / `verificationprogress` are based on the latest chain-tip
watcher. Those values are not tied to one atomic state snapshot.

There is also a fail-open fallback: if `TipPoolValues` fails, Zebra still
returns a successful response with genesis height/hash and zero value pools. If
`ChainInfo` fails in this path, `chain_tip_difficulty(..., should_use_default =
true)` returns default/minimum difficulty rather than propagating an RPC error.
That behavior is tested, so it appears intentional, but it can make a transient
state-read failure look like a real successful genesis-like chain state to
automated clients.

This is not a consensus bug. It is public RPC client-safety hardening for
monitoring, wallet middleware, orchestration, and services that treat
`getblockchaininfo` as a coherent snapshot of local chain state.

## Evidence

- `zebra-rpc/src/methods.rs:1000-1011` runs `UsageInfo`, `TipPoolValues`, and
  `chain_tip_difficulty(...)` concurrently with `tokio::join!`.
- `zebra-rpc/src/methods.rs:1021-1029` maps `TipPoolValues` failure to
  `(Height::MIN, network.genesis_hash(), Default::default())`.
- `zebra-rpc/src/methods.rs:1031-1034` accepts the difficulty returned by the
  separate `chain_tip_difficulty(...)` call.
- `zebra-rpc/src/methods.rs:1037-1059` separately reads
  `latest_chain_tip.best_tip_height_and_block_time()` for estimated height and
  verification progress.
- `zebra-rpc/src/methods.rs:1067-1107` derives upgrade statuses and consensus
  branch IDs from the `tip_height` value produced by `TipPoolValues` or the
  genesis fallback.
- `zebra-rpc/src/methods.rs:1109-1119` builds one response by combining
  `tip_height`, `tip_hash`, `value_balance`, `difficulty`, and watcher-derived
  sync progress.
- `zebra-state/src/service.rs:1340-1350` serves `TipPoolValues` from the
  current `latest_best_chain()` and finalized DB for that individual request.
- `zebra-state/src/service.rs:1596-1615` serves `ChainInfo` through
  `read::difficulty::get_block_template_chain_info(...)`, using a separate
  non-finalized-state/db view for that individual request.
- `zebra-state/src/service/read/difficulty.rs:45-75` retries and then returns a
  single internally consistent `GetBlockTemplateChainInfo`.
- `zebra-state/src/service/read/difficulty.rs:145-194` explicitly errors if the
  tip before and after the chain-info subqueries differs.
- `zebra-rpc/src/methods.rs:4643-4660` shows that
  `chain_tip_difficulty(..., should_use_default = true)` returns default
  difficulty on `ChainInfo` read failure.
- `zebra-rpc/src/methods/tests/prop.rs:370-430` tests the no-chain-tip behavior.
- `zebra-rpc/src/methods/tests/prop.rs:536-607` tests that
  `getblockchaininfo` succeeds with genesis-like fields when `TipPoolValues` and
  `ChainInfo` fail.

## Impact

Mixed-snapshot response:

1. `TipPoolValues` observes tip `A` and returns `blocks = hA`,
   `bestblockhash = A`, and value pools for `A`.
2. The best chain advances or reorganizes.
3. `ChainInfo` observes tip `B` and returns expected difficulty for `B`.
4. The chain-tip watcher used for `estimatedheight` observes tip `C`.
5. Zebra returns one JSON object with chain-state fields from `A`, difficulty
   from `B`, and sync-progress fields from `C`.

Fail-open fallback:

1. `UsageInfo` succeeds, so the node can still report live disk usage.
2. `TipPoolValues` fails because the state is empty or because finalized-state
   consistency retries are exhausted during heavy state churn.
3. Optionally, `ChainInfo` also fails and the difficulty helper returns a
   default difficulty.
4. Zebra returns success with `blocks = 0`, `bestblockhash = genesis`, zero
   value pools, genesis-derived consensus/upgrades, live `size_on_disk`, and
   possibly watcher-derived progress.

The first issue can make monitoring or wallet middleware read a response as more
atomic than it is. The second is sharper because success responses are often not
retried: an orchestrator can interpret the fallback as an actual database reset,
catastrophic rollback, or unsynced node rather than a transient state-read error.

## Existing mitigations

- JSON-RPC is disabled by default.
- Cookie authentication is enabled by default when RPC is enabled.
- The mixed-snapshot issue is most visible during sync, finalization churn, or
  non-finalized reorgs.
- The fallback behavior is intentional enough to have tests, and the no-chain-tip
  case is useful before the state has initialized.
- `ChainInfo` itself has an internal before/after tip consistency check; the
  issue is cross-request response assembly in `getblockchaininfo`, not the
  internals of `ChainInfo`.

## Suggested fix direction

- For an initialized node, return a transient RPC error when `TipPoolValues`
  fails for consistency/retry reasons instead of fabricating a genesis snapshot.
- If the genesis fallback is kept for empty state compatibility, distinguish
  empty state from retry-exhaustion/state-churn errors.
- Use a single read-state request for the coherent `getblockchaininfo` fields:
  tip height/hash, value pools, current difficulty, and any consensus/upgrades
  inputs.
- Include the observed tip hash/height in the difficulty helper result and retry
  if it does not match the `TipPoolValues` tip.
- Add regression tests where mocked `TipPoolValues` and `ChainInfo` return
  different tips, and where retry-exhaustion errors are distinct from empty state.

## Duplicate Check

Read-only duplicate searches refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "getblockchaininfo" "genesis fallback" "TipPoolValues"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "getblockchaininfo" "snapshot consistency" "ChainInfo" "TipPoolValues"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "getblockchaininfo" "returns genesis" "state fails"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "get_blockchain_info_returns_genesis_when_tip_pool_fails"'
```

No hits were returned.

## Local Confidence Check

Existing focused behavior test rerun on 2026-05-09:

```sh
cargo test -p zebra-rpc get_blockchain_info_returns_genesis_when_tip_pool_fails --lib
```

Result: passed. The test confirms a `TipPoolValues` failure plus `ChainInfo`
failure currently returns a successful genesis-like `getblockchaininfo`
response.

## Eliminated adjacent candidates

- `getblocktemplate` uses one `ReadRequest::ChainInfo` for state tip/template
  data and checks mempool `last_seen_tip_hash` against the state tip hash before
  including mempool transactions.
- `z_gettreestate` resolves the requested block first, then uses the resolved
  block hash for Sapling and Orchard tree reads. Reorg fallout becomes a missing
  tree/error shape rather than silent substitution by height.
- `z_getsubtreesbyindex` issues one state request per pool, and the state
  subtree merge logic is explicitly written to avoid inconsistent mixed lists
  across non-finalized/finalized overlap.
- Hash-pinned `getblock` block-info/value-pool reads use resolved block hashes;
  missing reads omit optional pool fields or return errors rather than silently
  replacing the block by height.

## Disclosure triage

Public hardening. This is an RPC response-coherence and fallback-semantics issue
in a disabled-by-default/authenticated-by-default endpoint. It can mislead
operators and downstream clients, but it is not a consensus failure, process
crash, or secret disclosure.

Confidence: high on the code shape and tested fallback behavior; medium on
practical impact, which depends on clients treating a successful
`getblockchaininfo` response as authoritative and atomic.
