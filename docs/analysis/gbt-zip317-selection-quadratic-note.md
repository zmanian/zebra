# GetBlockTemplate ZIP-317 Selection Quadratic Work Note

Date: 2026-05-03

Last updated: 2026-05-09

Disposition: local-only. Do not post publicly without explicit
re-authorization.

Scope: focused mining RPC resource sweep after the high-fee coinbase overflow
finding.

## Finding

When mining RPC is enabled, `getblocktemplate` can spend quadratic CPU rebuilding
ZIP-317 weighted transaction indexes while selecting transactions from the
mempool.

The template path fetches every verified mempool transaction and the dependency
graph:

- `zebra-rpc/src/methods/types/get_block_template.rs:777-789` requests and
  unwraps `mempool::Request::FullTransactions`.
- `zebrad/src/components/mempool.rs:834-845` clones all stored verified
  transactions and the dependency graph into that response.
- `zebra-rpc/src/methods.rs:2513` passes those transactions into
  `select_mempool_transactions()`.

`select_mempool_transactions()` partitions candidates, builds a
`WeightedIndex`, chooses one candidate, removes it, and then rebuilds the whole
index over the remaining vector:

- `zebra-rpc/src/methods/types/get_block_template/zip317.rs:62-143` runs the
  ZIP-317 selection loops.
- `zebra-rpc/src/methods/types/get_block_template/zip317.rs:201-207` builds a
  fresh `Vec<f32>` and `WeightedIndex<f32>` for all candidates.
- `zebra-rpc/src/methods/types/get_block_template/zip317.rs:417-427` samples one
  transaction with `swap_remove()`, then calls `setup_fee_weighted_index()` again
  for the whole remaining list.

For `n` independent candidate transactions, that produces `n + (n - 1) + ... +
1` weighted-index construction work. The path also keeps rebuilding after
candidates fail the remaining block byte, sigop, or unpaid-action limits.

## Bounds

This is bounded by the mempool size and block size limits, not unbounded memory
growth:

- Default `tx_cost_limit` is 80,000,000:
  `zebrad/src/components/mempool/config.rs:52-65`.
- Minimum transaction cost is 10,000:
  `zebra-chain/src/transaction/unmined.rs:67`.
- So the default mempool candidate count is on the order of 8,000 minimum-cost
  transactions.

That is still enough for avoidable CPU work on every template request if a
mining setup exposes or shares RPC access.

## Impact

Mining-RPC availability hardening. A peer or RPC caller cannot trigger
this on a default non-mining node, because `getblocktemplate` requires a
configured miner address. But an attacker who can influence the mempool shape
and repeatedly call `getblocktemplate` can amplify the cost of block-template
construction on mining nodes, pools, or custom-network test infrastructure.

This is not private disclosure on current evidence: it does not affect consensus
acceptance, has clear operational preconditions, and is bounded by existing
mempool limits.

## Duplicate Check

Read-only duplicate search refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra getblocktemplate ZIP-317 quadratic WeightedIndex'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "ZIP-317" "WeightedIndex"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "select_mempool_transactions" "weighted"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "getblocktemplate" "quadratic" mempool'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "ZIP-317" "getblocktemplate"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "fee_weight_ratio"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "block template" "ZIP-317"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "transaction selection" "getblocktemplate"'
```

Relevant adjacent hits:

- closed #5473 and #5724 implemented ZIP-317 transaction selection;
- closed #6006 fixed duplicate transaction selection by removing candidates
  from candidate collections during selection;
- closed #8857 added dependent unmined-input handling to the mempool and GBT
  selector.

Those are provenance, but they do not cover the repeated `WeightedIndex`
rebuild / quadratic CPU work angle.

Focused proof added on 2026-05-09:

```sh
cargo test -p zebra-rpc independent_transaction_selection_rebuilds_weighted_index_after_each_candidate_today --lib
```

Result: passed.

The proof adds test-only instrumentation around `setup_fee_weighted_index()`,
then runs `select_mempool_transactions()` over eight independent non-coinbase
test transactions. It confirms the selector rebuilds the weighted index over
candidate counts `n, n - 1, ..., 1` across the conventional-fee and low-fee
candidate partitions.

Baseline ZIP-317 module test run on 2026-05-09:

```sh
cargo test -p zebra-rpc methods::types::get_block_template::zip317::tests --lib
```

Result: passed, 4 tests.

## Suggested Fix

- Avoid rebuilding `WeightedIndex` over the full remaining candidate list after
  every selection.
- Prefilter candidates that cannot fit the current byte, sigop, or unpaid-action
  budget before weighting.
- Stop early once all remaining candidates exceed hard limits.
- Preserve one selection path so optimized behavior does not diverge from the
  current ZIP-317 policy.
- Add a stress/regression test with many independent conventional-fee candidates
  and many non-fitting candidates.

## Confidence

Confidence: high for the quadratic code shape. The repeated rebuild claim is
now covered by a focused current-behavior unit test, not only source inspection.

Confidence: medium-low for practical severity because the path requires enabled
mining RPC and remains bounded by mempool limits.
