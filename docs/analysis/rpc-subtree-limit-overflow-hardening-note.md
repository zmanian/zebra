# RPC Subtree Limit Overflow Hardening Note

Date: 2026-05-02

Last updated: 2026-05-09

Scope: follow-up on `z_getsubtreesbyindex` limit handling during the RPC/state
query-bounds audit.

Disposition: local-only. Do not post publicly without explicit
re-authorization and a fresh duplicate check.

## Finding

`z_getsubtreesbyindex` accepts a `start_index` and optional `limit`, both typed
as `NoteCommitmentSubtreeIndex`. The type is a transparent `u16` newtype, so RPC
callers cannot supply values outside `0..=65_535`. However, state currently
computes the exclusive end bound with `start_index.0.checked_add(limit.0)` and
uses the same `None` branch for both:

- omitted `limit`, which intentionally means "read all remaining subtrees", and
- explicit `limit` values where `start_index + limit` overflows `u16`.

That means an explicit overflowing `limit` is treated as an unbounded
`start_index..` range. This is not a consensus issue and is not a large exposure
expansion because the keyspace is already `u16`, but it is ambiguous and brittle
state-query behavior.

## Duplicate Check

Read-only GitHub searches refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "z_getsubtreesbyindex" "limit overflow"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "Subtrees" "limit overflow" "ReadRequest"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "NoteCommitmentSubtreeIndex" "checked_add" "limit"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "subtree_overflow_limit_matches_omitted_limit_today"'
```

Closest hit:

- #7436, closed/merged, implemented `z_getsubtreesbyindex`. It is provenance,
  not a duplicate of the explicit-overflow distinction.

No exact issue hits were returned.

## Evidence

- `zebra-chain/src/subtree.rs` defines
  `NoteCommitmentSubtreeIndex(pub u16)` with `serde(transparent)`.
- `zebra-rpc/src/methods.rs` exposes `z_getsubtreesbyindex` as
  `start_index: NoteCommitmentSubtreeIndex` and
  `limit: Option<NoteCommitmentSubtreeIndex>`.
- `zebra-rpc/src/methods.rs` forwards those values directly into
  `ReadRequest::SaplingSubtrees` or `ReadRequest::OrchardSubtrees`.
- `zebra-state/src/service.rs` uses `checked_add` to compute `end_index`; if the
  result is `None`, it calls `read::sapling_subtrees(..., start_index..)` or
  `read::orchard_subtrees(..., start_index..)`.
- `zebra-state/src/service/read/tree.rs` returns a `BTreeMap` for the requested
  range and returns empty if the starting subtree is missing.
- Finalized reads use RocksDB forward range iteration via
  `zebra-state/src/service/finalized_state/zebra_db/shielded.rs`, and
  non-finalized reads collect from in-memory `BTreeMap` ranges.
- Existing subtree tests cover normal bounded and omitted-limit behavior, but
  not the explicit overflow path.
- `zebra-state/src/service/tests.rs` now has a local current-behavior proof,
  `subtree_overflow_limit_matches_omitted_limit_today`, showing explicit
  overflowing Sapling and Orchard limits return the same suffix as omitted
  limits today.

Focused proof rerun on 2026-05-09:

```sh
cargo test -p zebra-state subtree_overflow_limit_matches_omitted_limit_today --lib
```

Result: passed.

## Impact

The practical impact is low:

- the RPC-visible type bounds both `start_index` and `limit` to `u16`,
- an overflowing explicit limit can only read the remaining suffix from
  `start_index` through the available subtree tail,
- omitted `limit` from index 0 can already intentionally request the full
  subtree suffix,
- RPC response size limits still apply.

The main value of fixing this is correctness, future-proofing, and avoiding
unbounded range construction for explicit bounded requests. It should be handled
as public hardening, not private disclosure.

## Suggested Fix

- Distinguish `limit=None` from explicit arithmetic overflow.
- Preserve omitted `limit` as the only intentionally unbounded case.
- Use widened arithmetic for explicit limits and clip overflowing ranges to the
  final representable subtree index, for example `start_index..=u16::MAX`.
- Add tests for `limit=None`, `limit=0`, normal bounded ranges, and overflow
  cases such as `start=1, limit=u16::MAX` and `start=u16::MAX, limit=1`.
- Update state/RPC docs to say that `limit` is a count and explicit values that
  extend past the representable index space are clipped, not treated as omitted.

## Confidence

Confidence: high on the implementation behavior and low practical severity. The
remaining uncertainty is compatibility preference: clipping explicit overflow is
the least disruptive fix, while rejecting overflow would be stricter but more
likely to surprise existing clients. Because the RPC-visible keyspace is `u16`,
the explicit-overflow path does not obviously return more subtrees than the
largest representable bounded query would return; this is correctness and
future-proofing hardening more than a meaningful availability issue.
