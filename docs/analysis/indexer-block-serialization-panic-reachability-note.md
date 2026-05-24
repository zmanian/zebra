# Indexer Block Serialization Panic Reachability Note

Date: 2026-05-04

Last updated: 2026-05-09

Disposition: eliminated as a current remote issue; no public post and no
low-severity queue entry unless new reachability evidence appears.

Scope: fresh follow-up on the optional indexer gRPC `BlockAndHash` stream and
Zebra's block-header version serialization checks.

## Finding

`BlockAndHash::new()` contains a real panic site: it serializes a `Block` with
`zcash_serialize_to_vec().expect("block serialization should not fail")`.

Current evidence eliminates this as a remotely reachable indexer-client
availability bug. An indexer subscriber can trigger the streaming path, but it
cannot supply the block being serialized. The block comes from Zebra's own
non-finalized state, and the relevant block version checks are shared by
deserialization and serialization.

## Evidence

The panic site is in the protobuf adapter:

- `zebra-rpc/src/indexer.rs:45-57`

The only live server call site is the optional `NonFinalizedStateChange` stream:

- `zebra-rpc/src/indexer/methods.rs:84-120`

That stream does not accept a block payload from the gRPC caller. It subscribes
to `ReadRequest::NonFinalizedBlocksListener`, then receives `(hash, Arc<Block>)`
pairs from state:

- `zebra-state/src/response.rs:218-264`

The listener walks the current non-finalized state and sends blocks already held
by Zebra. It does not stream arbitrary finalized history and does not deserialize
client-provided block bytes.

The block-header version check used during serialization rejects only versions
with the top bit set and versions below 4:

- `zebra-chain/src/block/serialize.rs:26-68`

The tempting historical edge is Mainnet block 434873, which is documented as a
"bad version field" vector. But the test suite proves that block is still
accepted and round-trips through serialization:

- `zebra-chain/src/block/tests/vectors.rs:138-158`

The known historical value described in the serialization comment is
`536870912`, which does not have the top bit set, so it does not hit the
serialization error path.

## Impact

Triage: eliminated as a current remote security issue.

The remaining risk is internal invariant hardening. If a future code path
constructs or retains a serialize-invalid `Block` in non-finalized state, an
indexer subscriber could cause the per-subscriber stream task to panic when it
tries to serialize that block. Current normal ingestion should reject such a
block before it reaches state.

This is also distinct from the existing `TrustedChainSync` validation-boundary
notes. `TrustedChainSync` is a consumer of the stream; it does not make the
server-side `BlockAndHash::new()` panic reachable.

## Suggested Fix Direction

Optional local hardening:

- Add `BlockAndHash::try_new(hash, block) -> Result<BlockAndHash, io::Error>`.
- Update `non_finalized_state_change()` to use the fallible constructor.
- On serialization failure, log an internal invariant error, send
  `Status::internal("failed to serialize non-finalized block")` if possible,
  and terminate only that subscriber stream.
- Keep `BlockAndHash::new()` for compatibility if the generated-type helper is
  treated as part of the public `zebra-rpc` API.

## Duplicate Check

Read-only duplicate search on 2026-05-07:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra BlockAndHash indexer block serialization panic'
```

No issue hits were returned.

Read-only duplicate search refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra BlockAndHash indexer block serialization panic'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "block serialization should not fail"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "NonFinalizedStateChange" "BlockAndHash"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "indexer" "serialization" "panic" "block"'
```

No direct duplicate issue hits were returned. The closest historical hit is
closed #9654, which introduced `NonFinalizedBlocksListener` and
`NonFinalizedStateChange`; it is feature provenance rather than a panic finding.

Targeted verification rerun on 2026-05-09:

```sh
cargo test -p zebra-chain round_trip_blocks --lib
cargo test -p zebra-chain blockheader_serialization --lib
cargo test -p zebra-chain block_commitment --lib
```

Result: all three commands passed. These are serialization/commitment backstops,
not a direct malformed-state indexer-panic reproducer.

## Confidence

Confidence: high that the `expect` is present and that the optional indexer
stream is the only in-repo server call site.

Confidence: high that ordinary remote indexer clients cannot provide the block
serialized by `BlockAndHash::new()`.

Confidence: medium-high that this is fully eliminated for current production
state inputs; that depends on the invariant that accepted non-finalized blocks
are parsed through the same version checks before entering state.
