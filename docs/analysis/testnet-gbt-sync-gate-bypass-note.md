# Testnet GBT sync-gate bypass note

Date: 2026-05-03

Last updated: 2026-05-09

Disposition: local-only. Do not post publicly without explicit
re-authorization and a fresh duplicate check.

## Finding

`getblocktemplate` and proposal validation use `check_synced_to_tip()` to reject
mining RPC work while Zebra is far from the chain tip. The helper's documentation
says it should return early when Proof-of-Work is disabled on the provided
network, but the implementation returns `Ok(())` for every test network.

That means default Testnet, where PoW is not disabled, bypasses the RPC
near-tip gate for both template mode and proposal mode.

## Evidence

- `zebra-rpc/src/methods/types/get_block_template.rs:679-691` documents the
  PoW-disabled exception, then returns early on `network.is_a_test_network()`.
- `zebra-rpc/src/methods.rs:2221-2290` calls `check_synced_to_tip()` inside the
  template-mode long-poll loop before fetching state and mempool data.
- `zebra-rpc/src/methods/types/get_block_template.rs:623-640` calls the same
  helper before proposal-mode validation.
- `zebra-chain/src/parameters/network.rs:277-280` defines
  `is_a_test_network()` as every non-Mainnet network.
- `zebra-chain/src/parameters/network/testnet.rs:505` sets default Testnet
  `disable_pow: false`.
- `zebra-chain/src/parameters/network/testnet.rs:1162-1167` exposes
  `Network::disable_pow()`, which is the narrower predicate matching the helper
  documentation.

## Local Verification

Durable current-behavior proof added in
`zebra-rpc/src/methods/types/get_block_template/tests.rs`:

```sh
cargo test -p zebra-rpc default_testnet_gbt_sync_check_ignores_unsynced_status_today --lib
```

Result on 2026-05-09: passed. The test sets a mock chain tip far from the
estimated network tip and a mock sync status that is not close to tip. The
Mainnet call returns an error, while the default Testnet call returns `Ok(())`.

Duplicate search refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra check_synced_to_tip getblocktemplate testnet disable_pow'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "check_synced_to_tip" "is_a_test_network"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "getblocktemplate" "disable_pow" "Testnet"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "GBT" "sync" "Testnet"'
```

The first three searches returned no hits. The broad `GBT sync Testnet` search
returned #6025, a manually triggered Testnet mining workflow issue that waits
for sync before mining; it is adjacent mining workflow history, not a duplicate
of the `check_synced_to_tip()` predicate mismatch.

## Impact

This is not a mainnet consensus issue and does not make Zebra accept invalid
blocks. It affects configured mining RPC users on default Testnet or custom
Testnet-like networks where PoW remains enabled.

On those networks, an unsynced Zebra instance can provide templates or evaluate
block proposals without the near-tip guard that Mainnet uses. That can mislead
mining pools, testnet miners, or automation that assumes `getblocktemplate`
readiness implies the node is close enough to the consensus tip.

The adjacent health endpoint has an explicit `enforce_on_test_networks` option
whose default makes `/ready` return `200 OK` on test networks. That health
behavior is documented in config and user docs; the stronger bug here is the
GBT helper's narrower PoW-disabled contract not matching its all-testnet
implementation.

## Suggested Fix

- Change `check_synced_to_tip()` to return early on `network.disable_pow()`,
  not `network.is_a_test_network()`.
- Add tests showing:
  - default Testnet unsynced template/proposal calls are rejected,
  - PoW-disabled Regtest/custom networks still bypass the near-tip check, and
  - Mainnet behavior is unchanged.
- Consider a startup warning if `mempool.debug_enable_at_height` is configured
  on a PoW-enabled network, because that separate debug knob can also force
  pre-sync mempool activation.

## Disclosure triage

Public mining RPC / operator-readiness hardening. This is opt-in RPC surface,
default Testnet/custom-network behavior, and not consensus acceptance.

Confidence: high on the code/doc mismatch and affected call paths; medium-low
on practical severity because exploitability depends on configured mining RPC
and testnet/custom-network automation.
