# GetBlockTemplate Custom Pre-Canopy Panic Note

Date: 2026-05-03

Last updated: 2026-05-09

Disposition: local-only. Do not post publicly without explicit
re-authorization and a fresh duplicate check.

Scope: focused mining RPC panic sweep after the high-fee coinbase overflow
finding.

## Finding

On a custom Testnet or custom Regtest-style configuration where the next block
height is before Canopy activation, `getblocktemplate` can abort the process
instead of returning an RPC error.

The lower-level coinbase helper explicitly reports unsupported pre-Canopy
template generation as an error, but the RPC response constructor unwraps that
error:

- `zebra-rpc/src/methods/types/get_block_template.rs:340` calls
  `generate_coinbase_and_roots(...).expect(...)`.
- `zebra-rpc/src/methods/types/get_block_template.rs:832` returns
  `Err("Zebra does not support generating pre-Canopy coinbase transactions")`
  for pre-Canopy network upgrades.

Zebra's docs already say pre-Canopy block templates are unsupported, but the
current behavior is stronger than "doesn't work": with `panic = "abort"`, a
single mining RPC call can terminate the node.

## Reachability

This is not a default public-network issue:

- Default Mainnet and Testnet are long past Canopy.
- Default Regtest activates upgrades up to and including Canopy at height 1:
  `book/src/user/custom-testnets.md:169`.
- `ConfiguredActivationHeights::for_regtest()` fills omitted earlier upgrades so
  Canopy follows Heartwood at height 1 for default Regtest:
  `zebra-chain/src/parameters/network/testnet.rs:377-397`.

The reachable shape is custom-network mining:

- Custom Testnets can configure Canopy later than height 1, and the docs state
  that Zebra cannot produce pre-Canopy templates until Canopy activates:
  `book/src/user/custom-testnets.md:122`.
- The example custom Testnet configuration enables mining/RPC and disables proof
  of work: `book/src/user/custom-testnets.md:14-57`.
- `check_synced_to_tip()` returns success for test networks, so a custom
  low-height test network does not need to be near a public tip before reaching
  template generation: `zebra-rpc/src/methods/types/get_block_template.rs:691`.

The `generate` RPC reuses `getblocktemplate`, so PoW-disabled custom-network
test tooling inherits the same abort behavior when it attempts to generate a
pre-Canopy block.

## Duplicate / Overlap Check

Read-only GitHub searches refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra getblocktemplate pre-Canopy coinbase panic'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "pre-Canopy" "block templates" "panic"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "Zebra does not support generating pre-Canopy"'
gh api repos/ZcashFoundation/zebra/issues/8434
```

The searches did not find a panic/abort issue. Open issue #8434 already tracks
support for constructing pre-Canopy block templates on Regtest/custom Testnets.
That is the natural upstream home for this hardening detail, but #8434 does not
currently call out that the existing unsupported path can abort the process via
`BlockTemplateResponse::new_internal()` unwrapping
`generate_coinbase_and_roots()`.

## Impact

Availability impact for custom-network mining nodes or test automation. A caller
with access to mining RPC can abort the node when the local best tip is below
the configured Canopy activation height.

This is low-severity hardening, not private disclosure, because it requires a
non-default custom network configuration and a mining RPC surface. The docs
already warn that pre-Canopy templates are unsupported; the bug is the
panic/abort instead of a clean error.

## Local Reproducer

Added durable current-behavior coverage in
`zebra-rpc/src/methods/types/get_block_template/tests.rs`:

- `generate_coinbase_and_roots_rejects_pre_canopy_custom_network` confirms the
  lower-level helper returns
  `Zebra does not support generating pre-Canopy coinbase transactions` for a
  custom Testnet with Canopy at height 10 and the candidate block at height 1.
- `block_template_response_panics_for_pre_canopy_custom_network_today` confirms
  `BlockTemplateResponse::new_internal()` currently panics on that helper
  error via the `coinbase should be valid under the given parameters` unwrap.

Verification:

```text
cargo test -p zebra-rpc pre_canopy_custom_network --lib
```

Result on 2026-05-09: passed, 2 tests.

## Suggested Fix

- Make `BlockTemplateResponse::new_internal()` return a typed error or
  `RpcResult<Self>` instead of unwrapping `generate_coinbase_and_roots()`.
- Or add an early `getblocktemplate` guard that rejects next-block heights before
  Canopy activation with a JSON-RPC error.
- Ensure `generate` returns the same clean error because it depends on
  `getblocktemplate`.
- Add a custom Testnet regression where Canopy activates after the next block
  height and assert that the RPC returns an error rather than panicking.

## Confidence

Confidence: high for the abort in the unsupported pre-Canopy custom-network
path.

Confidence: low-medium for practical exploitability because it requires mining
RPC access on a deliberately custom activation schedule.
