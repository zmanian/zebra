# Serde Helper Unwrap Reachability Note

Date: 2026-05-04

Last updated: 2026-05-09

Disposition: local-only internal-format hardening. Do not post publicly without
explicit re-authorization.

Scope: follow-up on `zebra-chain/src/serialization/serde_helpers.rs`, which
contains Serde remote conversion helpers that call `unwrap()` when converting
32-byte encodings into Jubjub, Pallas, Sapling, and Orchard field/point types.

## Finding

The `unwrap()` sites are real panic sites, but I did not find a source-grounded
remote or RPC path where an attacker can feed arbitrary Serde bytes into them.
Current evidence classifies this as local disk/internal-format hardening rather
than a new remotely reachable Zebra vulnerability.

This is distinct from the already-documented RPC Sapling receiver panic: that
issue is reached through an RPC request parser. The Serde helpers reviewed here
appear to be reached by derived Serde formats used for internal disk formats,
tests/snapshots, local JSON serialization, and response/client compatibility
types, while peer, block, transaction, mempool, and raw RPC inputs use
`ZcashDeserialize` implementations that return typed parse errors.

## Panic Sites

`zebra-chain/src/serialization/serde_helpers.rs` defines remote Serde helpers
whose `From<Helper> for RealType` conversions unwrap failed decodes:

- `AffinePoint` unwraps `jubjub::AffinePoint::from_bytes()`
  (`serde_helpers.rs:11-14`).
- `Fq` unwraps `jubjub::Fq::from_bytes()` (`serde_helpers.rs:24-27`).
- `Affine` unwraps `pallas::Affine::from_bytes()` (`serde_helpers.rs:37-40`).
- `Scalar` unwraps `pallas::Scalar::from_repr()` (`serde_helpers.rs:50-53`).
- `Base` unwraps `pallas::Base::from_repr()` (`serde_helpers.rs:63-66`).
- `ValueCommitment` unwraps
  `sapling_crypto::value::ValueCommitment::from_bytes_not_small_order()`
  (`serde_helpers.rs:76-79`).
- `SaplingExtractedNoteCommitment` unwraps
  `sapling_crypto::note::ExtractedNoteCommitment::from_bytes()`
  (`serde_helpers.rs:89-92`).
- `Node` unwraps `sapling_crypto::Node::from_bytes()`
  (`serde_helpers.rs:108-111`).

Those conversions can panic if a Serde input contains length-valid but
non-canonical field elements, invalid curve encodings, small-order points, or
invalid node bytes.

## Helper Attachment Points

The helpers are attached to chain data types via `#[serde(with = ...)]`:

- Sapling roots, legacy tree nodes, commitments, outputs, and transmission keys
  (`sapling/tree.rs:48-49`, `sapling/tree/legacy.rs:20-21`,
  `sapling/commitment.rs:22-25`, `sapling/output.rs:27-63`,
  `sapling/keys.rs:251-253`).
- Orchard roots, nullifiers, note/value commitments, actions, and ephemeral
  public keys (`orchard/tree.rs:106-107`,
  `orchard/note/nullifiers.rs:10-11`, `orchard/commitment.rs:47-107`,
  `orchard/action.rs:23-32`, `orchard/keys.rs:168-169`).

These data types derive Serde because Zebra uses them in local/internal formats,
not because P2P consensus encoding is Serde-based.

## Eliminated Remote Paths

Network and raw chain-data inputs use `ZcashDeserialize`, not these Serde
helpers:

- Blocks are decoded through `Block::zcash_deserialize()` and bounded block
  readers (`zebra-chain/src/block/serialize.rs:149-163`).
- Transactions are decoded through `Transaction::zcash_deserialize()`
  (`zebra-chain/src/transaction/serialize.rs:768-1123`).
- Sapling and Orchard consensus fields in those decoders use typed
  `ZcashDeserialize` paths that return `SerializationError::Parse`, for example
  Sapling value/note commitments (`sapling/commitment.rs:89-126`), Sapling and
  Orchard roots (`sapling/tree.rs:137-140`, `orchard/tree.rs:175-178`), and
  Orchard commitments (`orchard/commitment.rs:223-226`).
- RPC raw transaction and block submission paths parse hex bytes and then call
  `ZcashDeserialize` (`zebra-rpc/src/methods.rs:1170`,
  `zebra-rpc/src/methods.rs:2556`, `zebra-rpc/src/methods.rs:641` for
  getblocktemplate proposals).
- The indexer gRPC decoder for client-supplied blocks uses
  `zcash_deserialize_into()` and logs malformed client data rather than Serde
  (`zebra-rpc/src/indexer.rs:60-78`). The server stream serializes state-held
  blocks with `ZcashSerialize` (`zebra-rpc/src/indexer.rs:45-57`), which is
  covered separately in the indexer block serialization reachability note.

I also did not find a production JSON/RON entrypoint that deserializes these
Serde chain objects directly from an attacker-controlled request body. The
JSON-RPC compatibility middleware parses generic request/response wrappers
(`zebra-rpc/src/server/http_request_compatibility.rs:127-175`), and the
`z_gettreestate`/`z_getsubtreesbyindex` RPC structs use hex strings/bytes in
response/client-compatibility types rather than deserializing note-commitment
tree internals from RPC requests (`zebra-rpc/src/methods/trees.rs:14-200`).

## Remaining Trusted/Internal Paths

The credible Serde decode paths are local disk formats:

- Finalized Sapling and Orchard note commitment trees are encoded with
  `bincode::DefaultOptions` and decoded with `expect(...)`
  (`zebra-state/src/service/finalized_state/disk_format/shielded.rs:105-163`).
- History tree parts also use Serde/bincode and `expect(...)`
  (`zebra-state/src/service/finalized_state/disk_format/chain.rs:38-96`).
- RocksDB reads call `FromDisk::from_bytes` on values returned by local database
  reads and iterators (`zebra-state/src/service/finalized_state/disk_db.rs:424-442`,
  `zebra-state/src/service/finalized_state/disk_db.rs:792-807`).
- Non-finalized backup restore is a useful contrast: it reads local backup files,
  but it uses `zcash_deserialize_into()` for blocks and turns malformed bytes into
  warnings instead of Serde helper panics
  (`zebra-state/src/service/non_finalized_state/backup.rs:177-199`,
  `zebra-state/src/service/non_finalized_state/backup.rs:217-269`).

Malformed RocksDB data or manually crafted local disk formats can therefore
still panic Zebra on startup, RPC tree reads, or format-upgrade reads. That is
already the general disk-format hardening class documented in
`state-format-migration-b5-revisit-note.md`, not a remote peer/RPC exploit by
itself.

Elasticsearch support serializes blocks to JSON for export
(`zebra-state/src/service/finalized_state.rs:506-520`); this path uses local
state as the source and is serialization-only for the reviewed helpers.

## Impact

No new private disclosure item from this Serde-helper surface alone.

If a future source trace finds a production endpoint that calls Serde
deserialization on one of these chain types using attacker-controlled input, the
severity would change to a panic-on-untrusted-input DoS candidate. On current
evidence, the failure precondition is corrupted/trusted local state or a local
operator/test/client compatibility decode path.

## Suggested Hardening

- Replace the helper `unwrap()` conversions with Serde `Error::custom(...)` so
  malformed Serde data becomes a decode error instead of a panic.
- Keep the remote consensus parser separation clear: untrusted peer/RPC hex data
  should continue to use `ZcashDeserialize`.
- For disk reads, consider returning structured corruption errors at the
  `FromDisk` boundary so operators get a clean rebuild/reindex instruction rather
  than a process panic.
- Avoid adding new Serde deserialization entrypoints for consensus chain objects
  at RPC or indexer boundaries unless invalid curve/field encodings are converted
  into request-scoped errors.

## Duplicate Check

Read-only duplicate search on 2026-05-07:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra serde_helpers unwrap bincode malformed disk panic'
```

No issue hits were returned.

Read-only duplicate search refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra serde_helpers unwrap bincode malformed disk panic'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "serde_helpers" "unwrap"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "from_bytes" "unwrap" "bincode"'
```

No direct duplicate issue hits were returned. The closest broad hit was closed
#2185 for Orchard nullifier storage, which is historical state-format work rather
than malformed-Serde panic handling.

Targeted disk-format roundtrip tests rerun on 2026-05-09:

```sh
cargo test -p zebra-state roundtrip_sapling_tree_root --lib
cargo test -p zebra-state roundtrip_sapling_subtree_data --lib
cargo test -p zebra-state roundtrip_orchard_tree_root --lib
cargo test -p zebra-state roundtrip_orchard_subtree_data --lib
```

Result: all four commands passed. These are valid-data disk-format backstops;
they do not eliminate malformed local-state panics.

## Confidence

Confidence: medium-high for no current remote reachability.

The reviewed source shows direct remote inputs using `ZcashDeserialize` and the
Serde helpers used by local/internal formats. Remaining uncertainty is that a
future feature or less obvious downstream consumer could deserialize these
Serde-derived chain types directly. That would need a separate source-to-sink
trace before severity changes.
