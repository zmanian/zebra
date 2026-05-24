# RPC address-index query bounds note

Date: 2026-05-02

Last updated: 2026-05-09

## Summary

The `getaddresstxids`, `getaddressutxos`, and `getaddressbalance` JSON-RPC
methods accept caller-supplied transparent address lists. `getaddresstxids` also
accepts an optional height range, defaulting to the whole best chain. Zebra
validates that addresses parse and deduplicates them into a `HashSet`, but it
does not appear to enforce a maximum address count, maximum height-range width,
or maximum returned item count before dispatching to state.

This is not a consensus issue. It is an RPC availability hardening lead for
deployments that expose RPC access to untrusted or semi-trusted clients.

## Evidence

- The RPC docs for `getaddresstxids` explicitly recommend that callers choose
  `start` / `end` heights such that the response cannot be too large.
- `GetAddressBalanceRequest`, `GetAddressUtxosRequest`, and
  `GetAddressTxIdsRequest` all deserialize `addresses: Vec<String>` without a
  method-level count cap.
- `ValidateAddresses::valid_addresses()` parses the entire address list and
  collects it into `HashSet<Address>`, deduplicating valid addresses but not
  bounding the original vector length or final set size.
- `get_address_tx_ids()` defaults a missing `start` to height `0` and a missing
  or zero `end` to the current chain tip, then forwards the full address set and
  range to `ReadRequest::TransactionIdsByAddresses`.
- `get_address_utxos()` forwards the full address set to
  `ReadRequest::UtxosByAddresses` and then materializes every returned UTXO into
  the JSON-RPC response vector.
- The finalized state address-index helpers iterate all requested addresses and
  collect all matching transaction locations or UTXOs into `BTreeMap` /
  `BTreeSet` results. There is no `LIMIT`-style cap at the state helper layer.

## Local Confidence Check

Reran and expanded current-behavior proofs on 2026-05-09:

```sh
cargo test -p zebra-rpc rpc_getaddresstxids_forwards_large_default_range_today --lib
cargo test -p zebra-rpc rpc_getaddress --lib
cargo test -p zebra-state address_utxos_queries_chain_tx_history_even_for_empty_utxos_today --lib
```

Result: passed. The `getaddresstxids` test generates 256 unique valid
transparent addresses, calls `get_address_tx_ids()` with no `start` or `end`,
and confirms the RPC forwards all 256 addresses to state as
`ReadRequest::TransactionIdsByAddresses { height_range: Height(0)..=Height(1000), ... }`.

Two sibling tests now cover the other address-index methods:

- `rpc_getaddressbalance_forwards_large_address_set_today` confirms
  `get_address_balance()` forwards all 256 parsed addresses as
  `ReadRequest::AddressBalance`.
- `rpc_getaddressutxos_forwards_large_address_set_today` confirms
  `get_address_utxos()` forwards all 256 parsed addresses as
  `ReadRequest::UtxosByAddresses`.

Together, these proofs cover the missing method-level address-count cap across
the three address-index RPC methods. The response-size and returned-item count
cap concerns remain source-backed.

### Specific `getaddressutxos` Txid-History Subcase

A state-level proof covers a sharper `getaddressutxos` subcase that is distinct
from the generic missing request caps:

- `lookup_tx_ids_for_utxos()` first derives the exact transaction locations
  needed for the returned live UTXOs.
- It then calls
  `chain.partial_transparent_tx_ids(addresses, ADDRESS_HEIGHTS_FULL_RANGE)`,
  which asks the non-finalized chain for the full transparent txid history of
  the requested addresses.
- The test
  `address_utxos_queries_chain_tx_history_even_for_empty_utxos_today` adds a
  test-only counter around that call and confirms it happens even when the
  live UTXO map is empty and no txids are needed.

This means `getaddressutxos` can do broader non-finalized txid-history work
than required for the returned live UTXO set. The behavior is proportional to
non-finalized address activity history rather than to the number of returned
UTXOs. The existing helper-level proof is enough for this audit note because it
instruments the exact call site; a new end-to-end RPC test would mainly prove
reachability already covered by the RPC forwarding tests above.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "address index RPC bounds"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "getaddresstxids" "address count limit"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "getaddressutxos" "response limit"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "rpc_getaddresstxids_forwards_large_default_range_today"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "lookup_tx_ids_for_utxos"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "getaddressutxos" "partial_transparent_tx_ids"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "address UTXO" "txid" "history"'
```

No hits were returned.

## Impact

An RPC client with access to the JSON-RPC endpoint can ask Zebra to scan many
address index ranges and construct large response objects. For
`getaddresstxids`, omitting `start` and `end` requests the full indexed chain.
For `getaddressutxos`, the response can include all current UTXOs for all
requested addresses.

Existing mitigations:

- JSON-RPC is disabled by default;
- cookie authentication is enabled by default;
- the HTTP request body limit bounds the serialized request size;
- the JSON-RPC response body limit bounds what can be returned to the client;
- address queries use indexes rather than scanning every block.

These mitigations do not prevent a trusted or compromised RPC client from
causing large indexed reads and response construction before the response body
limit is hit. For the specific `getaddressutxos` txid-history subcase, the
evidence proves avoidable internal work, not a benchmarked or demonstrated
exploitable remote denial of service.

## Suggested fix direction

- Add explicit per-method limits for address count, height-range width, and
  returned item count.
- Return a clear JSON-RPC error when a request exceeds the caps, instead of
  relying on deployment advice or response serialization limits.
- Consider requiring `start` and `end` for `getaddresstxids` when more than one
  address is requested.
- Narrow `lookup_tx_ids_for_utxos()` so it resolves txids only for the returned
  UTXO transaction locations. Avoid calling
  `partial_transparent_tx_ids(addresses, ADDRESS_HEIGHTS_FULL_RANGE)` when the
  UTXO set is empty or sparse.
- Add regression tests for over-limit address lists, over-wide ranges, and
  response truncation/error behavior.

Disclosure triage: local-only public RPC availability hardening.

Confidence: medium-high on the missing method-level caps and on the
`getaddressutxos` helper behavior; low-medium on practical impact because
JSON-RPC is disabled by default, cookie-authenticated by default, and the
resource-exhaustion magnitude has not been benchmarked.
