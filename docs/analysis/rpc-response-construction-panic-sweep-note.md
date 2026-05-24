# RPC Response-Construction Panic Sweep

Date: 2026-05-03

Last updated: 2026-05-09

Status: eliminated / local hardening only.

Disposition: local-only eliminated lead. Do not post publicly without explicit
re-authorization.

Confidence: medium-high for the eliminated candidates below.

## Summary

This pass looked for fresh remotely reachable panics in RPC response assembly,
after excluding the already documented RPC panic findings:

- `getblocktemplate` `longpollid` Unicode slicing panic
- `z_listunifiedreceivers` invalid Sapling receiver panic
- height-based `getblock` verbosity-2 confirmation conversion race

I did not confirm a new private-disclosure RPC panic in this sweep. The most
interesting sharp edge was the address-index ordering assertions in
`getaddresstxids` and `getaddressutxos`, but the current state indexes appear
to exclude the only normal on-chain sentinel collision: the genesis transparent
coinbase output.

## Address-Index Sentinel Assertions

`getaddresstxids` initializes its previous transaction location to the exact
genesis transaction location, then asserts each returned transaction is strictly
greater:

- `zebra-rpc/src/methods.rs:2066-2088`

`getaddressutxos` does the same with the exact genesis output location:

- `zebra-rpc/src/methods.rs:2114-2131`

Because Zebra builds release binaries with `panic = "abort"`, a reachable
assertion failure would be process-fatal:

- `Cargo.toml:184`
- `Cargo.toml:305`

The exploitable-looking question was whether an RPC client could request the
address receiving the genesis coinbase output and make state return a location
equal to the sentinel. Current evidence says no:

- Finalized block commit explicitly skips genesis UTXO, transparent address
  index, and value-pool updates.
  - `zebra-state/src/service/finalized_state/zebra_db/block.rs:641-658`
- The address UTXO reader defines the full address-height range as
  `Height(1)..=Height::MAX` because genesis coinbase transactions are not
  included in address indexes.
  - `zebra-state/src/service/read/address/utxo.rs:33-37`
- UTXO transaction-hash lookup also uses that non-genesis address-height range.
  - `zebra-state/src/service/read/address/utxo.rs:419-457`
- The finalized address-transaction iterator starts at the greater of the
  address's first indexed UTXO location and the query start height. Since the
  genesis transparent output is not indexed as an address location, normal
  address transaction queries should not return `(height 0, tx 0)`.
  - `zebra-state/src/service/finalized_state/disk_format/transparent.rs:527-558`
- Existing RPC vector tests intentionally skip genesis because its transaction
  is not indexed.
  - `zebra-rpc/src/methods/tests/vectors.rs:1051`
  - `zebra-rpc/src/methods/tests/vectors.rs:1156`
  - `zebra-rpc/src/methods/tests/vectors.rs:1166`
  - `zebra-rpc/src/methods/tests/vectors.rs:1183`

Suggested hardening:

- Replace the sentinel locals with `Option<TransactionLocation>` and
  `Option<OutputLocation>`, so the RPC layer does not rely on a state-storage
  invariant for process safety.
- Keep the ordering check, but return an internal RPC error rather than
  panicking if a state-service contract is violated.

## RPC Compatibility Middleware ID Panic

`rpc_call_compatibility.rs` rewrites invalid-parameter responses and expects the
response JSON to contain an `id`:

- `zebra-rpc/src/server/rpc_call_compatibility.rs:30-59`

This looked like a possible panic if a caller supplied a malformed JSON-RPC
identifier. It also has direct historical coverage: closed #9314 and #9421
report production `response json should have an id` panics in Zebra 2.2.0, and
the #9314 discussion identified numeric JSON-RPC IDs as the root trigger at the
time. The current checkout handles numeric IDs in this middleware.

For the remaining shape, jsonrpsee's request `Id` type only normalizes `null`,
unsigned integer, or string identifiers. Negative, fractional, object, or array
IDs are rejected during request deserialization before this middleware rewrites
the method response. I did not find a valid caller-controlled current response
shape that reaches this `expect()` without an `id`, but the historical issues
make this a good local hardening target rather than a fresh report.

Suggested hardening:

- Treat a missing `id` as `Id::Null` or leave the response unchanged instead of
  using `expect()`.

## GetBlockTemplate Config Panics

The reviewed `getblocktemplate` response assembly panics were local
configuration/operator hazards rather than remote caller-controlled panics. In
particular, miner-address and extra-coinbase-data invariants are derived from
the node's mining configuration or internal template construction, not from
untrusted RPC parameters in the current implementation.

Suggested hardening:

- Convert config-derived panics into startup validation errors where practical,
  so custom mining deployments fail before exposing RPC.

## Transaction Response Construction

I also checked transaction-derived `expect()` / defaulting sites in
`zebra-rpc/src/methods/types/transaction.rs`:

- `TransactionTemplate::from_coinbase()` calls
  `tx.sigops().expect("sigops count should be valid")`.
- `TransactionObject::from_transaction()` calls
  `input.coinbase_script().expect("we know it is a valid coinbase script")`.
- Sprout JoinSplit verbose fields default proof or ciphertext serialization
  failures to empty vectors.

I did not confirm a fresh attacker-reachable RPC panic from these sites.
`from_coinbase()` is fed by locally generated `getblocktemplate` coinbases; the
extra coinbase data is configuration-derived and length-checked before the
template path. The verbose coinbase-script path is sharper-looking, but coinbase
input deserialization enforces the consensus script length bounds and parses the
height/data representation that `coinbase_script()` reconstructs. The same
helper is also used in consensus sigop accounting with an invariant comment
that any successfully deserialized coinbase round-trips cleanly. The Sprout
JoinSplit defaults are response-quality hardening only: the proof and ciphertext
types are fixed-size, validated data, and serializing them into a `Vec` should
not fail in normal operation.

Suggested hardening:

- Replace the transaction-response `expect()` calls with explicit internal RPC
  errors if the local invariant is ever broken.
- Avoid silent `unwrap_or_default()` in verbose Sprout fields; failing loudly is
  easier to diagnose than returning empty proof/ciphertext bytes.

## Classification

No new private-disclosure item is confirmed by this sweep. The remaining
recommendations are local hardening / defense-in-depth, with the address-index
sentinel cleanup being the most worthwhile because it removes a process-fatal
assertion from an authenticated-but-often-deployed RPC surface.

## Duplicate Check

Read-only duplicate search refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra RPC response construction panic address index sentinel genesis'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "getaddresstxids" "genesis" "panic"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "response json should have an id"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "getrawmempool" "response json should have an id"'
```

No direct hits were returned for the address-index sentinel shape. The
JSON-RPC ID middleware subcase is historically covered by closed #9314, closed
#9421, and duplicate/autogenerated #9386.

Targeted RPC tests rerun on 2026-05-09:

```sh
cargo test -p zebra-rpc rpc_getaddresstxids_response --lib
cargo test -p zebra-rpc rpc_getaddressutxos_response --lib
cargo test -p zebra-rpc rpc_getaddresstxids_forwards_large_default_range_today --lib
```

Result: all three commands passed. These cover normal address-index response
construction and large default-range forwarding, not the internal missing-id
middleware hardening shape.
