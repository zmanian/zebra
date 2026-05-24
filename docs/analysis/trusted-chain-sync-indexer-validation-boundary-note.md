# Trusted Chain Sync Indexer Validation Boundary Note

Date: 2026-05-03

Last updated: 2026-05-09

Disposition: local-only. Do not post publicly without explicit
re-authorization and a fresh duplicate check.

## Summary

`TrustedChainSync` imports non-finalized blocks from an indexer gRPC endpoint and
commits them into a read-only Zebra state mirror. This path is explicitly named
"trusted", so this is not a default full-node consensus vulnerability. But the
trust boundary is broader than the type names imply: the syncer turns streamed
`BlockAndHash` messages into `SemanticallyVerifiedBlock`s, accepts the streamed
hash without recomputing it, and calls lower-level non-finalized commit methods
without the normal `initial_contextual_validity()` check.

If a standalone read-state user points this syncer at a malicious, compromised,
or misconfigured indexer endpoint, the local read-state mirror can accept
non-finalized state that has not passed Zebra's normal recent-chain checks. That
can make indexer/read APIs answer from a poisoned non-finalized view even though
the primary full node path still verifies blocks normally.

## Evidence

`TrustedChainSync` connects to a caller-supplied plain HTTP tonic endpoint:

- `zebra-rpc/src/sync.rs:43-55`

The non-finalized sync loop receives `BlockAndHash` messages and constructs a
`SemanticallyVerifiedBlock` directly:

- `zebra-rpc/src/sync.rs:157-186`

The stream message decoder trusts the transmitted hash as long as it is 32 bytes
and separately deserializes the transmitted block bytes. It does not check that
`hash == block.hash()`:

- `zebra-rpc/src/indexer.rs:60-76`

The syncer then commits through `TrustedChainSync::try_commit()`:

- `zebra-rpc/src/sync.rs:217-225`

That method calls `NonFinalizedState::commit_new_chain()` or
`NonFinalizedState::commit_block()` directly. It does not call
`zebra-state/src/service/write.rs:55-62`, which is the normal state-service path
that runs `check::initial_contextual_validity(...)` before committing a
semantically verified block.

The skipped `initial_contextual_validity()` gate checks the recent-chain
context, including parent availability, parent height, block height sequencing,
difficulty threshold, and timestamp rules:

- `zebra-state/src/service/check.rs:396-415`
- `zebra-state/src/service/check.rs:50-130`

The lower-level non-finalized commit still performs important checks:

- transparent spend/value-balance checks,
- Sapling/Orchard/Sprout anchor checks,
- chain history block commitment checks,
- note commitment tree updates and nullifier/UTXO index updates.

But those lower-level checks do not appear to re-run the skipped
`block_is_valid_for_recent_chain()` gate. They also index the block using the
`SemanticallyVerifiedBlock.hash` value supplied by the stream:

- `zebra-state/src/service/non_finalized_state.rs:345-366`
- `zebra-state/src/service/non_finalized_state.rs:510-539`
- `zebra-state/src/service/non_finalized_state.rs:556-620`
- `zebra-state/src/service/non_finalized_state/chain.rs:1533-1542`
- `zebra-state/src/service/non_finalized_state/chain.rs:1200-1206`

## Duplicate Check

Read-only GitHub searches on 2026-05-07:

```sh
gh issue list --repo ZcashFoundation/zebra --search 'TrustedChainSync BlockAndHash hash mismatch in:title,body' --state all --limit 100
gh issue list --repo ZcashFoundation/zebra --search 'TrustedChainSync initial_contextual_validity in:title,body' --state all --limit 100
gh issue list --repo ZcashFoundation/zebra --search 'trusted indexer validation boundary in:title,body' --state all --limit 100
gh issue list --repo ZcashFoundation/zebra --search 'TrustedChainSync best tip forwarding in:title,body' --state all --limit 100
```

Result: no hits returned.

Fresh duplicate check on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "block_and_hash_decode_accepts_mismatched_hash_today"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "TrustedChainSync" "BlockAndHash" "hash mismatch"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "TrustedChainSync" "initial_contextual_validity"'
```

Result: no hits returned.

## Local Proof

Added focused current-behavior unit tests and end-to-end syncer proofs:

- `zebra-rpc/src/indexer/tests/vectors.rs::block_and_hash_decode_accepts_mismatched_hash_today`
- `zebra-state/src/service/non_finalized_state/tests/vectors.rs::lower_level_commit_skips_recent_chain_height_check_today`
- `zebra-rpc/src/sync.rs::trusted_chain_sync_stream_propagates_transmitted_hash_today`
- `zebra-rpc/src/sync.rs::trusted_chain_sync_stream_accepts_nonsequential_child_of_genesis_today`

The first test decodes a valid serialized genesis block paired with a different
transmitted hash and confirms `BlockAndHash::decode()` returns both values
without requiring `decoded_block.hash() == decoded_hash`.

The lower-level non-finalized-state test constructs a block whose header points
at finalized genesis but whose coinbase height is 2. The normal state write
helper rejects the block as `ValidateContextError::NonSequentialBlock`, while a
direct `NonFinalizedState::commit_new_chain()` call accepts it and records it as
the best tip at height 2. This proves the concrete recent-chain height check is
outside the lower-level commit path that `TrustedChainSync::try_commit()` calls.

The end-to-end `TrustedChainSync` tests stand up a local tonic indexer server,
instantiate the syncer directly, stream `BlockAndHash` messages through the
actual `non_finalized_state_change` subscription, and observe the mirror's
non-finalized watch channel. One test streams the height-2 child-of-genesis
fixture with a deliberately mismatched transmitted hash and confirms the mirror
publishes the transmitted hash rather than the block header hash. The other
streams the same non-sequential child with its actual header hash and confirms
the syncer publishes a height-2 non-finalized tip.

Focused command:

```sh
cargo test -p zebra-rpc block_and_hash_decode_accepts_mismatched_hash_today --lib
cargo test -p zebra-state lower_level_commit_skips_recent_chain_height_check_today --lib
cargo test -p zebra-rpc trusted_chain_sync_stream_propagates_transmitted_hash_today --lib
cargo test -p zebra-rpc trusted_chain_sync_stream_accepts_nonsequential_child_of_genesis_today --lib
```

Result on 2026-05-09: all four passed.

The validation-boundary claim is now end-to-end proof-backed for the live gRPC
stream-to-syncer path. The component tests still provide the sharper decoder
and normal-state-helper contrast, while the syncer tests prove those behaviors
compose through `TrustedChainSync::sync()` and `try_commit()` into the published
mirror state.

## Impact

This does not let a remote peer make a normal Zebra node accept an invalid best
chain. It affects the optional standalone read-state/indexer sync path that is
documented and named as trusted.

The risk is for deployments or downstream users that treat the indexer endpoint
as merely "remote Zebra" rather than fully trusted infrastructure:

- a malicious indexer endpoint can stream blocks with a mismatched external hash
  and serialized block body;
- it can attempt to populate the read-state non-finalized chain without the
  normal recent-chain PoW/difficulty/time/height gate;
- read APIs built on that mirror can then report a non-finalized tip, block
  locations, address indexes, or transaction lookups derived from that poisoned
  mirror.

The severity is lower because the feature is opt-in/library-style and already
called `TrustedChainSync`, but the current code makes the trust assumption
implicit in behavior rather than enforced or clearly documented at the API
boundary.

## Suggested Fix Direction

- In `BlockAndHash::decode()`, recompute the block hash and reject the message if
  it does not match the transmitted hash.
- In `TrustedChainSync::try_commit()`, route through the same validation helper
  as the normal state-service path, or explicitly call
  `check::initial_contextual_validity()` before the lower-level commit.
- Document `TrustedChainSync` as accepting only authenticated/local/trusted Zebra
  indexer endpoints, not arbitrary remote endpoints.
- Consider TLS/auth or an integrity check if this syncer is meant for networked
  deployments.

## Triage

Public hardening. Not private disclosure on current evidence because the API and
type names already describe the source as trusted, and this does not affect the
default full-node validation path.

Confidence: high on `BlockAndHash::decode()` accepting mismatched hash/body
messages after the focused proof; high on the lower-level commit path skipping
the recent-chain height check after the normal-helper/direct-commit contrast
test; medium on practical severity because the intended deployment model may
already require a fully trusted endpoint.
