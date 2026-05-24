# GetBlockTemplate Mempool Dependency Metadata Note

Date: 2026-05-02

Last updated: 2026-05-09

Disposition: local-only. Do not post publicly without explicit
re-authorization.

Scope: follow-up on the pass-5 mempool dependency and block-template
construction lead.

## Finding

Zebra's mempool can now track transaction dependencies and the ZIP-317 block
template selection path can intentionally include a dependent transaction after
its mempool parent has been selected. But the serialized `getblocktemplate`
transaction object still sets every non-coinbase transaction's `depends` field
to an empty list and documents that Zebra's mempool does not support
dependencies.

This is not a consensus acceptance bug in Zebra. It is a mining RPC correctness
and compatibility issue: the template can contain a child transaction that must
not be mined unless an earlier template transaction is also mined, while the RPC
metadata says there are no such dependencies.

## Evidence

- `zebra-node-services/src/mempool/transaction_dependencies.rs` stores direct
  dependencies and dependents for mempool transactions. `add()` records an
  entry for every spent mempool outpoint and explicitly says the structure is
  used during block template construction.
- `zebrad/src/components/mempool/storage/verified_set.rs` calls
  `transaction_dependencies.add(tx_id, spent_mempool_outpoints)` when inserting
  a verified transaction.
- `zebrad/src/components/mempool.rs` returns both cloned transactions and cloned
  `transaction_dependencies` for `Request::FullTransactions`.
- `zebra-rpc/src/methods.rs` passes those transactions and dependencies into
  `zip317::select_mempool_transactions()` before building the response.
- `zebra-rpc/src/methods/types/get_block_template/zip317.rs` partitions
  independent and dependent mempool transactions, then after selecting an
  independent transaction walks `direct_dependents()` and selects eligible
  dependents when `has_direct_dependencies()` sees all direct dependencies in
  `selected_txs`.
- The existing unit test
  `includes_tx_with_selected_dependencies` confirms that selection returns both
  an independent transaction and one dependent transaction with dependency depth
  1.
- `zebra-rpc/src/methods/types/transaction.rs` converts every
  `VerifiedUnminedTx` to `TransactionTemplate` with `depends: Vec::new()` and
  says "Zebra's mempool does not support transaction dependencies, so this list
  is always empty."
- `zebra-rpc/src/methods/types/get_block_template/constants.rs` includes
  `"transactions"` in the `mutable` field, so clients are told they may mutate
  the transaction set.

The current selection order appears to keep selected parents before selected
children in production builds: independent transactions are pushed into
`selected_txs` before any eligible direct dependents, and deeper dependents are
walked level by level. Therefore a miner that includes the entire returned
transaction list in order should not be broken by this by itself.

The missing `depends` values still matter for clients or mining pool software
that use `getblocktemplate` as designed: template transactions are optional
unless marked `required`, and the `depends` list tells the client which earlier
transactions must be kept if a later transaction is kept. Returning an empty
list for a dependent transaction can cause clients that drop, filter, reprioritize,
or reorder template transactions to build invalid block candidates.

## Reproducer Shape

1. Put a parent transaction and a child transaction spending the parent's
   transparent output into Zebra's mempool.
2. Call `getblocktemplate` with mining RPC enabled.
3. Observe that ZIP-317 selection can include both transactions.
4. Observe that the child transaction's `depends` field is serialized as `[]`
   instead of the parent's 1-based index in the `transactions` array.

The existing selection unit test verifies step 3. A stronger regression test
would construct a block-template response from selected dependent transactions
and assert that the child template records the expected parent index.

## Impact

Expected impact is miner/pool reliability, not chain consensus failure:

- miners or pool proxies can be told a dependent transaction has no dependencies,
- clients that mutate the transaction set can create invalid block candidates,
- attackers who can feed dependent transaction chains into the mempool can make
  this more likely in templates returned to miners.

The practical severity depends on how miners consume Zebra's GBT output. Miners
that include all transactions in order are likely unaffected. Miners or proxies
that filter or reorder transactions based on fee policy, template size, or local
rules are more exposed.

## Local Verification

Added a focused current-behavior response test on 2026-05-09:

```sh
cargo test -p zebra-rpc selected_dependent_transaction_has_empty_template_depends_today --lib
```

Result: passed.

The test lives at
`zebra-rpc/src/methods/types/get_block_template/tests.rs:175-251`. It builds a
synthetic mempool dependency where one selected transaction depends on an
earlier selected parent, confirms ZIP-317 selection returns the child with
dependency depth 1, constructs a `BlockTemplateResponse`, and observes that the
child's serialized template still has `depends: []`.

Focused existing selection test rerun on 2026-05-09:

```sh
cargo test -p zebra-rpc includes_tx_with_selected_dependencies --lib
```

Result: passed. This verifies the selection side can include a dependent
transaction after its parent.

Read-only duplicate search refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra getblocktemplate transaction depends mempool dependencies'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "depends" "getblocktemplate" "mempool"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "TransactionTemplate" "depends"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "mempool dependencies" "block template"'
```

Closest adjacent hits:

- closed #9645 tracks block-proposal validation of mempool transactions and
  dependency-aware template construction internals.
- closed #8857 adds mempool verification for unmined inputs and updates GBT
  selection to include dependencies when their parents are selected.
- closed #5496/#5554 added the `TransactionTemplate` fields and conversion
  plumbing, including the current empty `depends` behavior.
- closed #6195 fixed duplicate transparent spends in GBT responses, and its
  historical logs show `depends: []` in templates, but it does not track
  dependency metadata correctness.

Those issues do not cleanly cover the serialized GBT transaction `depends`
metadata remaining empty after dependency-aware selection.

## Suggested Fix

Fix direction:

- compute `depends` when converting selected mempool transactions into
  `TransactionTemplate`,
- use 1-based indexes into the final `transactions` vector, matching the GBT
  field semantics,
- only include dependencies that are present in the same template,
- update the stale comment saying Zebra's mempool does not support dependencies,
- add a regression test where a child transaction selected with its parent gets
  `depends: [parent_index]`.

If Zebra intentionally wants clients to treat dependencies as unknown, the field
should be omitted rather than serialized as an empty list. But because Zebra
already has the dependency graph at template-construction time, returning precise
indexes is the cleaner compatibility fix.

## Confidence

Confidence: medium-high on the metadata mismatch. The source paths are direct
and the existing unit test confirms dependent transaction selection.

Confidence: medium-low on practical exploitability. The likely bad outcome is
invalid miner work under particular GBT client behavior, and the RPC is disabled
by default.
