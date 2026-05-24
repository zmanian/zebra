# RPC getblock height-query snapshot consistency and panic note

Date: 2026-05-03
Updated: 2026-05-04
Last checked: 2026-05-07

Status: the public snapshot-consistency portion is tracked in
https://github.com/ZcashFoundation/zebra/issues/10550. The sharper
race-dependent `getblock <height> 2` negative-confirmations panic variant
remains a local/private-triage note; do not post further public detail without
explicit re-authorization.

## Summary

`getblock <height> 1` and `getblock <height> 2` can combine block-header
metadata resolved from one best-chain snapshot with transaction IDs or
transaction objects resolved from a later best-chain snapshot at the same
height.

This is a narrower and more concrete version of the RPC snapshot-consistency
class: the code comment says Zebra looks up by block hash so the hash,
transaction IDs, and confirmations are consistent, but for height-based
`getblock` requests the transaction request is built before the code shadows
`hash_or_height` with the resolved block hash. If a reorg or best-chain switch
changes the block at that height between those reads, the response can name one
block hash while listing transactions from a different block at that height.

There is also a sharper panic variant for `getblock <height> 2`: if the header
subcall resolves block `A`, then the depth subcall observes that `A` is no longer
in the best chain, `get_block_header()` returns the zcashd-compatible
`confirmations = -1` sentinel. The parent `get_block()` path then converts that
signed value to `u32` with `expect(...)` while constructing each verbose
transaction object. In Zebra release profiles where panics abort, that can
terminate the process.

## Evidence

- `zebra-rpc/src/methods.rs:1220-1233` starts `getblock` and, for verbosity
  `1` or `2`, creates a `get_block_header(...)` future using the caller's
  original hash-or-height string.
- `zebra-rpc/src/methods.rs:1260-1284` awaits that header future and extracts
  the resolved `hash`, `confirmations`, `height`, header roots, time, previous
  hash, and next hash.
- `zebra-rpc/src/methods.rs:1286-1289` then builds the transaction read request
  from the original `hash_or_height`. For height callers, that request remains
  height-based:
  - verbosity `1`: `ReadRequest::TransactionIdsForBlock(hash_or_height)`
  - verbosity `2`: `ReadRequest::BlockAndSize(hash_or_height)`
- `zebra-rpc/src/methods.rs:1292-1296` says Zebra looks up by block hash for
  consistency, but the shadowing `let hash_or_height = hash.into();` happens
  after the transaction request has already been constructed.
- `zebra-rpc/src/methods.rs:1319-1354` uses the transaction response to fill the
  `tx` list. In verbosity `2`, each transaction object is stamped with the
  earlier header `height`, `confirmations`, `block_time`, and resolved `hash`.
- `zebra-rpc/src/methods.rs:1341-1343` converts the header subcall's signed
  `confirmations` value to `u32` with `expect(...)`.
- `zebra-rpc/src/methods.rs:1411-1436` builds one `BlockObject` with the earlier
  header/provenance fields and the later `tx` vector.
- `zebra-state/src/service.rs:1385-1388` serves `BlockAndSize` against the
  current `latest_best_chain()` for that individual request.
- `zebra-state/src/service.rs:1434-1440` serves `TransactionIdsForBlock` against
  the current `latest_best_chain()` for that individual request.

`getblockheader` and the `getblock` header subcall also have related multi-read
behavior:

- `zebra-rpc/src/methods.rs:1455-1479` gets header/hash/height/next hash from
  `ReadRequest::BlockHeader`.
- `zebra-rpc/src/methods.rs:1484-1495` separately asks for
  `ReadRequest::SaplingTree(hash_or_height)`, still using the caller's original
  hash-or-height. For height callers, this can be a later block at the same
  height.
- `zebra-rpc/src/methods.rs:1497-1515` separately asks for
  `ReadRequest::Depth(hash)` and derives confirmations.
- `zebra-rpc/src/methods.rs:1507-1515` explicitly uses `-1` as the
  not-in-best-chain confirmations sentinel when the depth lookup returns `None`.
- `zebra-state/src/service.rs:1499-1505` serves Sapling/Orchard tree reads
  against the current `latest_best_chain()` for that individual request.
- `zebra-state/src/service.rs:1358-1363` serves `Depth(hash)` against the
  current `latest_best_chain()` for that individual request.
- `zebra-state/src/service/read/find.rs:151-154` returns `None` if the hash is
  not found in the best-chain snapshot used by the depth read.
- `Cargo.toml:184` and `Cargo.toml:305` set `panic = "abort"` for dev and
  release profiles.

Existing tests cover normal hash/height and verbosity behavior, but do not force
state answers to change between the header read and transaction/tree reads:

- `zebra-rpc/src/methods/tests/vectors.rs:196-723`
- `zebra-rpc/src/methods/tests/snapshot.rs:314-424`

A local proof test now forces the inconsistent mocked state sequence:

- `zebra-rpc/src/methods/tests/vectors.rs:834-928` makes `getblock <height> 2`
  resolve header/hash `A`, returns `Depth(A) = None`, then returns
  `BlockAndSize(height) = Some(block B)`. Current code panics at the
  `confirmations.try_into().expect(...)` conversion, and the test passes as
  `#[should_panic]`.
- Verification command:
  `cargo test -p zebra-rpc rpc_getblock_height_verbosity_2_panics_if_header_depth_reorgs_before_block_lookup_today --lib`

## Impact

For a height-based verbose `getblock` request, a possible race is:

1. The caller asks for `getblock "100" 2`.
2. The header subcall resolves height `100` to block hash `A` in chain snapshot
   `S1`.
3. The best chain reorgs or switches to a competing block at height `100`.
4. The transaction request is still height-based, so it returns block `B`'s
   transaction list from snapshot `S2`.
5. Zebra returns a block object whose `hash`, header fields, roots, previous
   hash, and confirmations describe block `A`, while `tx` describes block `B`.

For verbosity `2`, the transaction objects can also be stamped with block `A`'s
hash/height/confirmations while containing transaction bytes from block `B`.
If the depth read has returned `None` for block `A`, the same path can panic
before it returns a response because `-1i64.try_into::<u32>()` fails.

For `getblockheader <height>`, a similar but smaller inconsistency can mix a
header from one snapshot with a Sapling tree root or confirmations from a later
snapshot. The code already treats a missing tree after a reorg as possible, but
height-based tree lookup can also succeed for a different block at the same
height.

The practical security relevance is primarily downstream client confusion, with
a race-dependent availability edge for verbosity `2`. This does not let a peer
create invalid local state or make Zebra accept invalid blocks, but clients
using height-based `getblock` as a zcashd-compatible source of block contents
could receive a response that is not a real block. In the panic variant, an RPC
caller who can repeatedly issue `getblock <height> 2` during a non-finalized
reorg window can potentially crash a Zebra process.

## Existing mitigations

- JSON-RPC is disabled by default.
- Cookie authentication is enabled by default when RPC is enabled.
- The inconsistency is race-dependent and requires a reorg or best-chain switch
  affecting the queried height between separate read-state requests. A normal
  tip extension past a stable positive height is not enough.
- Hash-based `getblock <hash> ...` calls are less exposed because the original
  transaction request is already hash-based, and later tree/block-info requests
  use the resolved hash.
- The panic variant requires the header hash to fall out of the best chain
  between `BlockHeader` and `Depth`, while a later height-based `BlockAndSize`
  still returns a block with at least one transaction.

## Suggested fix direction

- Build the verbosity `1` and `2` transaction request after resolving the block
  hash, or rewrite the existing code so `TransactionIdsForBlock` and
  `BlockAndSize` always use `hash.into()` after the header subcall.
- In `getblockheader`, use the resolved `hash` for Sapling tree lookup instead
  of the original height query.
- Do not convert negative confirmations into `u32`; pass `None` or `0` for
  side-chain transaction objects, or avoid constructing transaction objects when
  the block is no longer in the best chain.
- Consider a single read-state request that returns header, transaction IDs or
  block bytes, trees, block info, and depth from one cloned best-chain snapshot.
- Add a mock read-state regression where a height query resolves header `A`,
  then transaction/tree/depth reads for the same height return block `B` or
  `Depth(None)` for `A`; assert Zebra uses the resolved hash, retries, or returns
  a typed RPC error without panicking.

## Independent cross-check

RepoPrompt builder chat `rpc-snapshot-panic-check-4B555C` independently
validated the code path and recommended narrowing the reachability language:

- The snapshot-mismatch claim is high confidence for reorgs or competing-chain
  switches affecting the queried height.
- The `getblock <height> 2` panic path is high confidence and straightforwardly
  unit-testable with mocked read-state response ordering.
- Practical exploitability is lower than deterministic RPC panics because it
  requires RPC access plus timing inside a non-finalized reorg window.
- A targeted fix is sufficient: use the resolved hash for the follow-up
  `getblock` transaction/block request and for `getblockheader`'s Sapling tree
  request, then add a defensive negative-confirmations guard.

## Disclosure triage

Private heads-up candidate, but not as urgent as the direct parameter-parsing
RPC panics. The availability impact is a process panic in release profiles, but
the trigger is race-dependent and requires RPC access plus a best-chain reorg
between subrequests. It is not a consensus failure or secret disclosure.

Confidence: high on the code/comment mismatch and the unit-testable
negative-confirmations panic path; medium-low on practical exploitability
because it requires timing a non-finalized reorg and depends on RPC exposure.
