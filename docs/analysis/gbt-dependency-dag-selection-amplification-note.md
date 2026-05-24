# GetBlockTemplate Dependency DAG Selection Amplification Note

Date: 2026-05-04

Last updated: 2026-05-09

Disposition: local-only. Do not post publicly without explicit
re-authorization.

Scope: follow-up pass on mempool dependency depth and fanout behavior after the
post-v4.4.0 security audit plan.

## Finding

Zebra's mempool dependency graph has no explicit ancestor-depth or descendant
fanout policy. During `getblocktemplate`, ZIP-317 transaction selection walks
that attacker-influenced dependency graph and repeatedly scans the full
`selected_txs` vector to decide whether each dependent transaction's parents
have already been selected.

This is not consensus-critical and remains bounded by mempool and block-template
limits. It is a public mining-RPC availability hardening issue: a peer that can
shape the mempool with valid transparent parent/child transactions can increase
CPU spent by each template request beyond the independent-transaction
weighted-index cost already documented in
`gbt-zip317-selection-quadratic-note.md`.

## Evidence

`TransactionDependencies` stores direct dependencies and direct dependents, but
does not enforce a maximum chain depth, maximum descendants per transaction, or
maximum total dependency edges:

- `zebra-node-services/src/mempool/transaction_dependencies.rs:7-26`
- `zebra-node-services/src/mempool/transaction_dependencies.rs:40-63`
- `zebra-node-services/src/mempool/transaction_dependencies.rs:121-128`

Live insertion prevents arbitrary graph cycles by requiring each spent mempool
outpoint to already exist in `created_outputs` before adding the new
transaction's dependency entry:

- `zebrad/src/components/mempool/storage/verified_set.rs:148-170`

So the concerning shape is not an infinite dependency loop. The concerning
shape is repeated bounded work across a large acyclic dependency DAG.

`select_mempool_transactions()` partitions transactions by whether they have
mempool dependencies, then only the independent side enters the ZIP-317 weighted
candidate list:

- `zebra-rpc/src/methods/types/get_block_template/zip317.rs:84-94`
- `zebra-rpc/src/methods/types/get_block_template/zip317.rs:107-142`

After an independent transaction is selected, Zebra walks its direct dependents,
then deeper dependents. For every candidate dependent, it calls
`has_direct_dependencies()`:

- `zebra-rpc/src/methods/types/get_block_template/zip317.rs:315-359`

`has_direct_dependencies()` checks whether every direct dependency is already in
the block by scanning `selected_txs` from the start and doing a hash-set lookup
for each selected transaction. A local test-only scan counter now preserves this
current behavior as an explicit proof:

- `zebra-rpc/src/methods/types/get_block_template/zip317.rs:215-265`
- `zebra-rpc/src/methods/types/get_block_template/zip317/tests.rs`
- `multi_parent_dependency_check_repeatedly_scans_selected_transactions_today`

It returns early without scanning while `selected_txs.len() < deps.len()`, so
the repeated-scan shape requires either a small direct-dependency set or enough
already-selected unrelated transactions to make the selected vector at least as
large as the candidate's direct dependency set.

If a dependent transaction has multiple parents, it can be encountered once for
each parent as those parents are selected. Until the last required parent is
present, each encounter still scans the currently selected transaction vector
and returns `false`. The dependent is only removed from `dependent_txs` once all
direct dependencies are present:

- `zebra-rpc/src/methods/types/get_block_template/zip317.rs:326-354`

## Attack Shape

A peer can relay otherwise valid transparent transactions that form a wide or
multi-parent acyclic dependency graph, mixed with enough unrelated independent
transactions to keep the selected vector large:

1. Several independent parent transactions enter Zebra's mempool.
2. Additional unrelated independent transactions are selected into the template.
3. Many child transactions spend outputs from more than one of the parents.
4. Each `getblocktemplate` call selects parents over time.
5. After each parent is selected, Zebra revisits that parent's dependents.
6. For children whose other parents are not selected yet,
   `has_direct_dependencies()` scans the growing `selected_txs` vector and
   returns `false`.
7. The same children are revisited again when later parents are selected.

The total work is bounded by mempool cost, transparent input/output sizes, block
size, and block sigop/action limits. But it can concentrate avoidable CPU in the
mining RPC path, especially when an RPC client or pool calls `getblocktemplate`
repeatedly while the mempool shape remains hostile.

## Relationship To Existing Findings

This is related to, but distinct from,
`gbt-zip317-selection-quadratic-note.md`:

- that note covers repeated `WeightedIndex` rebuilds over the independent
  candidate vector;
- this note covers repeated dependency checks over selected transactions and
  dependents after an independent transaction has already been selected.

It is also separate from `gbt-mempool-dependency-template-note.md`, which covers
missing `depends` metadata in the serialized template response.

## Impact

Expected impact is CPU amplification on mining RPC deployments:

- default non-mining nodes do not expose the path because `getblocktemplate`
  requires mining configuration;
- the attacker still has to get valid mempool transactions accepted;
- the effect is bounded by existing mempool and block limits;
- there is no evidence of consensus acceptance failure or fund loss.

Treat as public hardening, not private disclosure, on current evidence.

## Suggested Fix

- Track selected transaction IDs in a `HashSet<transaction::Hash>` alongside
  `selected_txs`, and check direct dependencies against that set instead of
  scanning the full vector.
- Consider explicit mempool policy limits for unmined ancestor depth, descendant
  count, or total dependency edges if Zebra wants predictable mining RPC
  template cost.
- Avoid revisiting the same not-yet-ready dependent multiple times in a single
  template build; for example, keep a remaining-parent count or a ready queue.
- Add a stress regression with many multi-parent dependents and assert template
  selection stays near linear in the number of transactions plus dependency
  edges.

## Duplicate Check

Read-only duplicate search refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "getblocktemplate" "dependency DAG" "selection amplification"'
gh api repos/ZcashFoundation/zebra/issues/9301
gh api repos/ZcashFoundation/zebra/issues/9727
```

No exact issue hits were returned.

Closest broad overlap: #9301 ("DoS vulnerability in `getblocktemplate` RPC")
and #9727 ("Respond quickly to long-polled `getblocktemplate` RPC on new chain
tips") are open and cover general GBT DoS / long-poll themes, but not this
exact dependency-DAG selection cost shape.

Baseline and focused current-behavior tests rerun on 2026-05-09:

```sh
cargo test -p zebra-rpc getblocktemplate --lib
cargo test -p zebra-rpc multi_parent_dependency_check_repeatedly_scans_selected_transactions_today --lib
```

Result: passed. The focused proof constructs 12 unrelated selected transactions
and a four-parent dependent shape, then confirms each parent encounter scans the
entire selected transaction vector after the length gate is satisfied. This
upgrades the repeated selected-vector scan claim from source-evidence-only to a
small current-behavior proof. It is still not an end-to-end stress benchmark over
the full random ZIP-317 selector.

## Confidence

Confidence: high on the local repeated-scan behavior. The graph storage and
selection loops are direct, live insertion makes the graph acyclic rather than
unbounded, and the focused test measures the repeated selected-vector scan
shape.

Confidence: medium-low on practical severity. The path is mining-RPC only,
bounded by mempool and block limits, and requires the attacker to construct
valid transaction graphs with enough transparent inputs and outputs to create
the repeated-check shape.
