# NU7 Custom Activation V5 Serialization Panic Note

Date: 2026-05-03

Last updated: 2026-05-09

Disposition: local-only. Do not post publicly without explicit
re-authorization.

Scope: follow-up on pass-5 Workstream A2, focused on custom Regtest/Testnet
activation boundaries and mining RPC behavior around `NetworkUpgrade::Nu7`.

## Finding

In a normal non-test build without `tx_v6`, a configured Regtest/Testnet that
activates NU7 can make Zebra construct a V5 coinbase transaction whose
`network_upgrade` is `Nu7`. Serializing that transaction then panics because the
NU7 consensus branch ID is only compiled for tests or the `zebra-test` feature.

This can make `getblocktemplate` or internal mining abort the process if an
operator configures NU7 activation on a custom network and an RPC/miner path
tries to generate the NU7-height coinbase transaction.

This is not a mainnet/testnet consensus issue: the shipped mainnet and default
testnet activation lists do not include NU7.

## Evidence

- `zebra-rpc/src/methods/types/get_block_template.rs:815-821` generates a V5
  coinbase for `NetworkUpgrade::Nu7` when not built with both
  `zcash_unstable = "nu7"` and `tx_v6`.
- `zebra-chain/src/transaction/builder.rs:172-178` sets the V5 coinbase
  transaction's `network_upgrade` to `NetworkUpgrade::current(network, height)`,
  so a custom NU7 activation height produces a V5 transaction tagged `Nu7`.
- `zebra-chain/src/parameters/network_upgrade.rs:229-231` only includes the NU7
  placeholder branch ID under `#[cfg(any(test, feature = "zebra-test"))]`.
- `zebra-chain/src/transaction/serialize.rs:682-686` serializes V5
  `nConsensusBranchId` via `network_upgrade.branch_id().expect(...)`, so a
  normal-build `Nu7` transaction panics during serialization.
- `zebra-chain/src/transaction/unmined.rs:268-270` computes
  `zcash_serialized_size()` when converting a generated `Transaction` into
  `UnminedTx`, which is exactly what GBT coinbase generation does after
  constructing the transaction.
- `Cargo.toml:183-184` and `Cargo.toml:304-305` set `panic = "abort"` for dev
  and release profiles.

## Local Probe

A temporary normal-build example was added, run, then removed. It constructed:

- `Network::new_regtest` with `ConfiguredActivationHeights { nu7: Some(1) }`,
- `Transaction::new_v5_coinbase(&network, Height(1), ...)`,
- `tx.zcash_serialize_to_vec()`.

Command:

```sh
cargo run -p zebra-chain --example nu7_v5_serialize_probe
```

Observed result:

```text
thread 'main' panicked at zebra-chain/src/transaction/serialize.rs:686:26:
valid transactions must have a network upgrade with a branch id
```

No probe source file remains in the worktree.

## Impact

Availability impact for custom-network deployments only:

- RPC must be enabled or the internal miner must be generating templates.
- The operator must configure NU7 activation on Regtest or a custom Testnet.
- A caller who can invoke `getblocktemplate`, or local miner operation, can hit
  the NU7-height template path.

Default mainnet and default testnet do not activate NU7, so this is public
hardening rather than private consensus disclosure.

## Suggested Fix

- Do not allow `ConfiguredActivationHeights::nu7` unless the build has a usable
  NU7 branch ID and matching transaction-version support.
- Or make `generate_coinbase_and_roots()` return a clean RPC error for `Nu7`
  when `tx_v6` / NU7 support is not enabled.
- Replace the V5/V6 serialization `expect()` on `branch_id()` with a typed
  serialization error if illegal constructed transactions can exist internally.

## Duplicate Check

Refreshed on 2026-05-09 with read-only GitHub searches for:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra NU7 custom activation V5 serialization branch_id panic'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "NU7" "branch_id" panic'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "valid transactions must have a network upgrade with a branch id"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "new_v5_coinbase" "NU7"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "ConfiguredActivationHeights" "NU7" "getblocktemplate"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "ConsensusBranchId" "Nu7" "zebra-test"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "NU7" "tx_v6" "coinbase"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "NU7" "V5" "panic"'
```

Relevant hits:

- open #10210, "Complex conditions exist between zcash_unstable=nu7 and
  feature=tx_v6 flags", explicitly calls out the inverted GBT match arm and the
  state where `Nu7` exists as an upgrade but uses V5 coinbase transactions. That
  issue covers the root configuration/feature-gating concern and is the natural
  upstream home for this concrete panic evidence.
- closed #2075 is historical V5 consensus-branch-ID serialization provenance.
- open #10534 is a separate future V6 transaction hash panic when `tx_v6` is
  enabled; it is not the same V5 custom-NU7 serialization path.

Do not file this as a separate public issue without explicit maintainer/user
direction, because #10210 already covers the underlying shape closely enough.

## Confidence

Confidence: high for the panic in normal builds with custom NU7 activation.
Confidence: low for practical default deployment impact because NU7 is not
activated by default and RPC/mining access is required.
