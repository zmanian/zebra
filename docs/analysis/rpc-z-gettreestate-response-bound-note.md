# RPC `z_gettreestate` Response Bound Note

Date: 2026-05-02

Last updated: 2026-05-09

Disposition: local-only eliminated lead. Do not post publicly without explicit
re-authorization.

## Question

Can a caller use `z_gettreestate` on a mature Sapling or Orchard tree to force
Zebra to build a response proportional to the total number of note commitments?

## Result

Eliminated as a large-response or whole-tree serialization issue.

`z_gettreestate` does fetch the full block first, then asks state for the
Sapling and Orchard trees at that block hash, and serializes each tree into the
legacy RPC `finalState` format. That looked risky at first because each tree can
contain up to `2^32` note commitments.

The actual serialized format is bounded by Merkle depth, not by leaf count.
Zebra stores these trees as incremental frontiers, and the RPC conversion writes
a legacy `CommitmentTree` made from:

- one optional left node,
- one optional right node,
- a vector of optional parent nodes.

For Sapling and Orchard, `MERKLE_DEPTH = 32`, and
`CommitmentTree::from_frontier()` fills the parent vector from `1..DEPTH`, so
there are at most 31 parent entries. `write_commitment_tree()` serializes each
optional node as a one-byte tag plus a 32-byte node when present, and the parent
vector has a one-byte CompactSize length at this size.

Worst-case raw bytes per tree:

```text
left option      33 bytes
right option     33 bytes
parent length     1 byte
31 parent slots  31 * 33 bytes
total          1090 bytes
```

The JSON `finalState` hex encoding doubles that to at most 2180 characters per
pool, plus small JSON/root overhead. This is far below Zebra's default
50 MiB RPC response limit and does not scale with chain age.

## Evidence

- `zebra-rpc/src/methods.rs:1874-1968` implements `z_gettreestate`, including
  the full-block lookup and Sapling/Orchard tree serialization.
- `zebra-chain/src/sapling/tree.rs:158-175` documents Sapling storage as a
  `Frontier<_, MERKLE_DEPTH>` with depth 32.
- `zebra-chain/src/sapling/tree.rs:465-473` converts the frontier to a legacy
  `CommitmentTree` and writes it with `write_commitment_tree()`. Orchard uses
  the same pattern.
- `incrementalmerkletree-0.8.2/src/frontier.rs:446-449` shows
  `CommitmentTree` contains only `left`, `right`, and `parents`.
- `incrementalmerkletree-0.8.2/src/frontier.rs:561-572` builds `parents` from
  `1..DEPTH`, so the depth-32 trees have at most 31 parent slots.
- `zcash_primitives-0.27.0/src/merkle_tree.rs:200-208` writes only those fields.

## Residual Hardening

`z_gettreestate` still fetches the full block before deriving hash, height, and
time. The code already has a TODO to fetch only the block header if this RPC is
called heavily. That is ordinary RPC efficiency hardening, not a security issue
of the same class as an unbounded `finalState` response.

There are two other small correctness hardening items:

- If the first block lookup succeeds and a reorg removes that block before the
  active-pool tree lookups, `ReadRequest::SaplingTree` or
  `ReadRequest::OrchardTree` can return `None`. Today the RPC maps that to empty
  commitments for an active pool. Returning an internal/state-race error would
  be clearer because active pools should have a tree for every canonical block.
- The RPC method docs say negative heights are unsupported, but the shared
  `HashOrHeight::new()` parser accepts negative heights relative to the current
  tip when a tip height is available. That is a doc/contract mismatch, not a
  security issue.

## Duplicate Check

Read-only duplicate search refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra z_gettreestate finalState response bound whole tree'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "z_gettreestate" "finalState"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "z_gettreestate" "response" "large"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "CommitmentTree" "finalState"'
```

No duplicate hits were returned for the large-response/whole-tree bound shape.
Relevant adjacent historical hits are closed #3990 for the original RPC
implementation and closed #9445/#9451 for optional `finalState` compatibility.

Targeted verification rerun on 2026-05-09:

```sh
cargo test -p zebra-rpc test_z_get_treestate --lib
```

Result: passed, 1 test.
