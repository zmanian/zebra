# RPC getrawtransaction snapshot consistency note

Date: 2026-05-03

Last updated: 2026-05-09

Disposition: local-only residual note. Do not post publicly without explicit
re-authorization.

## Summary

`getrawtransaction(..., verbose=1)` can assemble one JSON response from multiple
independent read-state snapshots. On the no-`blockhash` path, Zebra reads the
transaction and its confirmations first, then separately asks for the best-chain
block hash at the transaction height. On the caller-supplied `blockhash` path,
Zebra first checks whether the caller's block currently contains the transaction
and whether that block is in the best chain, then separately fetches the
transaction by `txid`.

If the best chain changes between those reads, the response can mix transaction
data, `blockhash`, `confirmations`, and `in_active_chain` values that never all
belonged to the same local chain snapshot. This is not a consensus issue, but it
is RPC correctness hardening for clients that treat verbose transaction metadata
as an atomic chain-membership statement.

## Current Status After PR #10523

PR #10523 fixed the previously sharper caller-`blockhash` TOCTOU shape by
reusing the caller-provided block hash and the initial best-chain flag instead
of doing a later best-chain block-hash lookup for that response context.

Residual low-severity exactness/coherence concerns remain on current source:

- the no-`blockhash` verbose path still fetches `AnyChainTransaction(txid)` and
  then separately fetches `BestChainBlockHash(tx.height)`. This path now has a
  deterministic mock-service proof in
  `getrawtransaction_no_blockhash_can_mix_mined_tx_and_best_chain_blockhash_today`;
- the caller-`blockhash` path still validates membership by mined `txid`, then
  fetches the transaction body by global mined `txid`, which is covered more
  precisely in
  `docs/analysis/rpc-getrawtransaction-v5-blockhash-exactness-note.md`.

These residuals are local RPC hardening notes, not private disclosure material
on current evidence.

## Evidence

No caller-supplied `blockhash`:

- `zebra-rpc/src/methods.rs:1775-1783` checks state with
  `ReadRequest::AnyChainTransaction(txid)`.
- `zebra-rpc/src/methods.rs:1813-1818` matches `AnyTx::Mined(tx)` and then
  makes a separate `ReadRequest::BestChainBlockHash(tx.height)`.
- `zebra-rpc/src/methods.rs:1830-1841` builds the verbose response with the
  transaction height, confirmations, and block time from the first read, the
  block hash from the second read, and `in_active_chain = Some(true)`.
- `zebra-state/src/service.rs:1426-1431` handles `AnyChainTransaction` against
  the current `latest_non_finalized_state().chain_iter()` for that individual
  request.
- `zebra-state/src/service.rs:1591-1594` handles `BestChainBlockHash` against
  the current `latest_best_chain()` for that later individual request.
- `zebra-state/src/service/read/block.rs:164-203` computes `AnyTx::Mined`
  using the best-chain snapshot seen by that transaction read, including
  confirmations from that snapshot's tip height.

Caller-supplied `blockhash`:

- `zebra-rpc/src/methods.rs:1742-1766` first validates
  `ReadRequest::AnyChainTransactionIdsForBlock(block_hash.into())`, confirms the
  block currently contains the `txid`, and stores the returned `in_best_chain`
  flag.
- `zebra-rpc/src/methods.rs:1775-1783` then separately fetches
  `ReadRequest::AnyChainTransaction(txid)`.
- `zebra-rpc/src/methods.rs:1784-1810` uses the earlier caller block hash and
  earlier `in_best_chain` flag while taking transaction height, confirmations,
  and block time from the later transaction read when it is `AnyTx::Mined`.
- `zebra-state/src/service.rs:1443-1450` handles
  `AnyChainTransactionIdsForBlock` against the current
  `latest_non_finalized_state().chain_iter()` for that individual request.
- `zebra-state/src/service/read/block.rs:237-258` returns the `in_best_chain`
  flag from whichever chain snapshot is seen by the block-transaction-list read.

Response construction:

- `zebra-rpc/src/methods/types/transaction.rs:153-172` exposes
  `in_active_chain`, `height`, and `confirmations` in the verbose response.
- `zebra-rpc/src/methods/types/transaction.rs:290-303` exposes `blockhash` and
  `blocktime`.
- `zebra-rpc/src/methods/types/transaction.rs:673-681` accepts these fields as
  independent inputs to `TransactionObject::from_transaction`.
- `zebra-rpc/src/methods/types/transaction.rs:684-703` derives side-chain,
  mempool, and active-chain height/confirmation semantics from
  `in_active_chain` and `block_hash`.
- `zebra-rpc/src/methods/types/transaction.rs:929-946` stores the supplied
  `block_hash`, `block_time`, `txid`, and `in_active_chain` directly.

Existing tests check normal behavior and a prior confirmation undercount fix,
but do not cover block-hash or active-chain coherence under a state race:

- `zebra-rpc/src/methods/tests/vectors.rs:1016-1201`
- `zebrad/tests/acceptance.rs:3012-3075`

## Impact

During active sync, finalization overlap, or a reorg, a verbose
`getrawtransaction` response can become internally inconsistent.

On the no-`blockhash` path, a possible sequence is:

1. `AnyChainTransaction(txid)` returns a mined transaction at height `H` with
   confirmations from chain snapshot `S1`.
2. The best chain changes.
3. `BestChainBlockHash(H)` returns the block hash at height `H` in snapshot
   `S2`.
4. Zebra returns the transaction from `S1`, the block hash from `S2`, and
   `in_active_chain = true`.

If the transaction was reorged out, the response can name a best-chain block
that never contained the returned transaction.

On the caller-`blockhash` path, a possible sequence is:

1. `AnyChainTransactionIdsForBlock(caller_hash)` says the caller's block
   contains the transaction and is in the best chain.
2. The best chain changes.
3. `AnyChainTransaction(txid)` now returns a side-chain view, no longer the best
   chain view.
4. Zebra still passes the earlier `in_best_chain = true` to the response.

Depending on the exact reorg and duplicate-inclusion shape, the response can
carry stale `in_active_chain`, stale `blockhash`, or height/confirmation fields
from a different block than the one reported by `blockhash`.

The practical security relevance is downstream client confusion, not node
compromise. A wallet, bridge, swap service, indexer, or monitoring system that
uses Zebra's zcashd-compatible RPC as a chain-membership oracle could make a
low-confirmation decision from metadata that is not tied to one coherent chain
snapshot.

## Existing mitigations

- JSON-RPC is disabled by default.
- Cookie authentication is enabled by default when RPC is enabled.
- The inconsistency is race-dependent and most visible while the best chain is
  changing.
- The response is still derived from locally verified state; this does not let
  an attacker make Zebra accept invalid blocks or transactions.
- Transaction hashes bind the raw transaction bytes, so the concern is
  provenance metadata (`blockhash`, `height`, `confirmations`,
  `in_active_chain`), not arbitrary transaction substitution.

## Suggested fix direction

- Return the containing block hash alongside `AnyTx::Mined` from the same
  read-state snapshot, instead of doing a later `BestChainBlockHash(height)`
  lookup in RPC code.
- For the caller-`blockhash` path, fetch the transaction and active-chain flag
  from one state request keyed by `(block_hash, txid)`, or retry if the block's
  active-chain status changes between validation and response assembly.
- Keep the mock read-state proofs that deliberately change answers between
  `AnyChainTransaction`, `BestChainBlockHash`, and
  `AnyChainTransactionIdsForBlock` calls, then make the desired behavior assert
  Zebra either retries or returns a response whose `blockhash`, `height`,
  `confirmations`, and `in_active_chain` come from one snapshot.

## Duplicate Check

Read-only duplicate searches refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "getrawtransaction snapshot consistency"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "getrawtransaction" "AnyChainTransaction" "BestChainBlockHash"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "AnyChainTransactionIdsForBlock" "AnyChainTransaction" "blockhash"'
```

No exact issue hits were returned.

Closest related artifacts:

- PR #10523, merged, fixed the adjacent RPC advisory for the caller-`blockhash`
  TOCTOU class. It does not add a block-specific transaction-body lookup keyed
  by `(blockhash, txid)`.
- PR #9884, closed/merged, added side-chain support in `getrawtransaction`. It
  is provenance for the multi-read side-chain behavior, not a tracker for the
  residual snapshot-consistency issue.

## Local Confidence Check

Focused behavior tests rerun on 2026-05-09:

```sh
cargo test -p zebra-rpc rpc_getrawtransaction --lib
cargo test -p zebra-rpc getrawtransaction_no_blockhash_can_mix_mined_tx_and_best_chain_blockhash_today --lib
```

Result: both passed. The broader `rpc_getrawtransaction` test confirms the
normal current `getrawtransaction` paths still work. The no-`blockhash` proof
mocks the exact state-read sequence used by the verbose path:
`TransactionsByMinedId` misses the mempool, `AnyChainTransaction(txid)` returns
a mined transaction from one block, and `BestChainBlockHash(height)` returns a
different block hash for the same height. Zebra currently returns one verbose
object containing the transaction bytes, height, confirmations, and block time
from the mined transaction metadata, but the mismatched block hash from the
later best-chain lookup with `in_active_chain = true`.

## Disclosure triage

Public hardening. This is a disabled-by-default/authenticated-by-default RPC
snapshot-consistency gap, not a consensus failure, crash, or secret disclosure.
It is worth fixing because downstream systems can treat verbose transaction
metadata as a chain-membership signal.

Confidence: high on the RPC method's ability to assemble internally
inconsistent metadata from independent read-state responses; medium on exploit
impact. The no-`blockhash` inconsistency is proof-backed at the RPC boundary,
but practical impact still needs active chain movement or reorg/finalization
timing and depends on how the RPC consumer uses `getrawtransaction` metadata.
