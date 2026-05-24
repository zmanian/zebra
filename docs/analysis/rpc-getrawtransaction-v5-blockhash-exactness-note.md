# RPC `getrawtransaction` V5 Blockhash Exactness Note

Date: 2026-05-03

Last updated: 2026-05-09

Disposition: local-only. Do not post publicly without explicit
re-authorization.

## Finding

`getrawtransaction(txid, verbose, blockhash)` validates the caller-supplied
block context by checking whether that block contains `txid`, but then fetches
the transaction body through a separate lookup keyed only by `txid`.

For V5 transactions, `txid` is the mined transaction ID: it identifies the
transaction effects, not the authorizing data. Two competing non-finalized
chains can therefore contain V5 transactions with the same mined ID but different
authorization digests and raw bytes. In that shape, Zebra can confirm that the
caller's side-chain block contains the `txid`, then return the raw transaction
from the best chain or from another earlier chain containing the same mined ID.

This is not a consensus issue. It is RPC exactness hardening for clients that
expect `getrawtransaction(..., blockhash)` to return the transaction as it
appears in the named block.

## Current Status After PR #10523

PR #10523 fixed an adjacent caller-`blockhash` TOCTOU race by reusing the
caller-provided block hash and the initial best-chain flag in the verbose
response. The residual V5 exactness issue remains distinct: current RPC code
still validates block membership by mined `txid`, then fetches the transaction
body via `ReadRequest::AnyChainTransaction(txid)`, which searches chains by mined
transaction hash rather than by the caller's block hash plus transaction
position.

## Preconditions

- JSON-RPC is enabled and reachable by the caller.
- The caller can authenticate if cookie auth is enabled.
- At least two non-finalized chains contain a V5 transaction with the same mined
  transaction ID but different authorization data.
- The caller supplies the `txid` and a `blockhash` for a block that is not the
  first chain returned by Zebra's `AnyChainTransaction(txid)` search.

This is most plausible for sibling chains or short reorg windows. It is not a
default steady-state main-chain issue.

## Evidence

Zebra's transaction ID types distinguish V5 mined IDs from witnessed IDs:

- `zebra-chain/src/transaction/hash.rs:1-24` documents that V5 `Hash`
  identifies transaction effects, while `WtxId` identifies effects plus
  authorizing data.
- `zebra-chain/src/transaction/hash.rs:203-235` stores a `WtxId` as a mined ID
  plus `AuthDigest`.

The caller-supplied `blockhash` path first checks whether that block contains
the mined `txid` and whether the block is in the best chain:

- `zebra-rpc/src/methods.rs:1742-1766`
- `zebra-state/src/service/read/block.rs:237-258`

After that validation, RPC fetches the transaction via
`ReadRequest::AnyChainTransaction(txid)`:

- `zebra-rpc/src/methods.rs:1775-1783`
- `zebra-state/src/service/read/block.rs:164-203`

That state helper searches chains by mined transaction hash and returns the first
matching transaction, with the first chain expected to be the best chain:

- `zebra-state/src/service/read/block.rs:173-192`

Each non-finalized chain indexes transactions by mined transaction hash:

- `zebra-state/src/service/non_finalized_state/chain.rs:434-445`
- `zebra-state/src/service/non_finalized_state/chain.rs:1552-1615`

The finalized database also stores `tx_loc_by_hash` by mined transaction hash,
but finalized state only has the finalized best chain, so the cross-chain
ambiguity here is specific to non-finalized competing chains:

- `zebra-state/src/service/finalized_state/zebra_db/block.rs:439-443`
- `zebra-state/src/service/finalized_state/zebra_db/block.rs:722-735`

Finally, the verbose response combines the raw transaction returned by
`AnyChainTransaction(txid)` with the earlier caller-provided block hash and
active-chain flag:

- `zebra-rpc/src/methods.rs:1784-1810`
- `zebra-rpc/src/methods/types/transaction.rs:673-681`
- `zebra-rpc/src/methods/types/transaction.rs:929-946`

## Impact

A caller asking for a V5 transaction in a specific side-chain block can receive:

- `blockhash`: the caller-supplied block,
- `in_active_chain`: the active-chain status of that caller-supplied block,
- `hex` / `authdigest`: a same-`txid` transaction from another chain.

For most wallet-style balance logic, same V5 mined ID means the effects are the
same. The main risk is to clients that use Zebra's RPC as an exact block
membership or block-commitment oracle and expect the returned raw transaction's
authorization digest to correspond to the named block's auth-data commitment.

This composes with the broader `getrawtransaction` snapshot-consistency note,
but it is a separate exactness issue: even without a chain race between two RPC
state reads, a block-specific validation by mined ID is not enough to select the
block-specific V5 raw transaction when competing chains contain auth-data
variants.

## Suggested Fix

For the caller-supplied `blockhash` path, fetch the transaction from the named
block rather than from the global mined-ID index:

- add a state read such as `AnyChainTransactionInBlock { block_hash, txid }`
  returning the exact transaction at the matching block location and the
  block's active-chain flag from one snapshot; or
- after `AnyChainTransactionIdsForBlock(block_hash)` finds the transaction
  position, fetch that block and return the transaction at that position.

Add a regression test with two non-finalized sibling chains containing V5
transactions that share the mined ID and differ only in auth data. Query the
side-chain `blockhash` and assert that verbose `hex` and `authdigest` match the
transaction in that block, not the best-chain sibling.

## Duplicate Check

Read-only duplicate searches refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "getrawtransaction" "V5" "blockhash" "authdigest"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "getrawtransaction" "blockhash" "V5" "same txid"'
```

No issue hits were returned.

Closest related artifact:

- PR #10523, merged, fixed the adjacent caller-`blockhash` TOCTOU advisory, but
  current code still does not fetch the transaction body from the named block.
- PR #9884, closed/merged, added side-chain support in `getrawtransaction`, but
  does not address V5 same-mined-ID/different-auth-data exactness for a named
  block.

## Local Confidence Check

Proof-backed. Rechecked on 2026-05-09. The current-behavior test
`getrawtransaction_blockhash_can_return_different_v5_auth_variant_today` in
`zebra-rpc/src/methods/tests/vectors.rs` constructs two V5 transactions with
the same mined ID and different authorizing data, mocks the named-block
membership check for one transaction, then mocks `AnyChainTransaction(txid)` to
return the other transaction.

The test confirms that Zebra can return a verbose `getrawtransaction` response
where `blockhash` and `in_active_chain` describe the caller-supplied block, but
`hex` and `authdigest` come from a different same-mined-ID V5 transaction.

Verification:

```sh
cargo test -p zebra-rpc getrawtransaction_blockhash_can_return_different_v5_auth_variant_today --lib
```

Supporting source evidence:

- `zebra-chain/src/transaction/hash.rs` documents that V5 `Hash` identifies
  effects while `WtxId` identifies effects plus authorizing data.
- `zebra-state/src/service/read/block.rs::any_transaction()` searches chains by
  mined transaction hash and returns the first match.
- `zebra-rpc/src/methods.rs::get_raw_transaction()` validates the caller block
  using `AnyChainTransactionIdsForBlock`, then fetches the body using
  `AnyChainTransaction(txid)`.

## Disclosure Posture

Classification: public hardening, but keep this note local-only unless the user
explicitly directs public posting.

This requires a non-finalized sibling-chain shape, RPC access, and a downstream
client that relies on exact auth-data membership for a named block. It does not
let an attacker make Zebra accept invalid consensus data, crash the node, or
forge a transaction effect.

## Confidence

Confidence: medium-high.

The source-to-sink code path is direct, and the V5 identifier distinction is
explicit in Zebra's transaction ID documentation. The local test demonstrates
the response mismatch with valid Zebra V5 transaction values and the same state
read sequence used by RPC. The remaining uncertainty is practical exploitability:
this needs a valid competing-chain auth-data variant and a client that cares
about exact `hex`/`authdigest` for the supplied `blockhash`.
