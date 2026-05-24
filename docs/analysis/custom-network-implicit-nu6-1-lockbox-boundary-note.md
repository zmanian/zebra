# Custom Network Implicit NU6.1 Lockbox Boundary Note

Date: 2026-05-03

Last updated: 2026-05-09

Disposition: local-only residual/adjacent to #10557/#10558 and implementation
history in #9526/#9710. Do not post publicly without explicit re-authorization.

Scope: follow-up activation-boundary pass after the Zebra 4.4.0 security
fixes, focused on custom Regtest/Testnet upgrade configuration and NU6.1
lockbox accounting.

## Summary

`NetworkUpgrade::activation_height()` intentionally falls forward to the next
configured upgrade when a prior upgrade has no explicit height. That is useful
for custom networks that activate multiple upgrades at the same height, but it
also means an omitted `nu6_1` height can inherit a later configured upgrade
height such as `nu7`.

The NU6.1 lockbox helpers and subsidy checks use
`NetworkUpgrade::Nu6_1.activation_height(network)` directly. On a custom
Regtest/Testnet where a later upgrade is configured and `nu6_1` is omitted,
that later activation height is also treated as the NU6.1 one-time lockbox
disbursement height.

For Regtest, `lockbox_disbursements` default to empty. In that configuration,
the inherited NU6.1 height can make the activation block fail consensus with
`missing lockbox disbursements for NU6.1 activation block`. If custom lockbox
disbursements are configured, `getblocktemplate` and block validation can
instead include and require those one-time outputs at the later upgrade height,
even though `nu6_1` was not explicitly configured.

This is not a mainnet/default-testnet consensus split. It is a custom-network
configuration footgun and availability/economics hardening issue.

## Evidence

`NetworkUpgrade::activation_height()` first looks for the exact upgrade in the
network activation list, then falls forward recursively to the next upgrade:

- `zebra-chain/src/parameters/network_upgrade.rs:340-360`

The existing activation-height unit test demonstrates that setting only a later
upgrade height causes earlier upgrades to inherit that height:

- `zebra-chain/src/parameters/network/tests/vectors.rs:100-137`

Regtest defaulting only fills Overwinter through Canopy. It leaves `nu5`,
`nu6`, `nu6_1`, and `nu7` as configured:

- `zebra-chain/src/parameters/network/testnet.rs:374-412`

Regtest parameters also default missing lockbox disbursements to an empty list:

- `zebra-chain/src/parameters/network/testnet.rs:986-1008`

The lockbox amount and output helpers are gated by equality with the possibly
fallback-derived NU6.1 activation height:

- `zebra-chain/src/parameters/network.rs:289-304`
- `zebra-chain/src/parameters/network.rs:307-324`

Block subsidy validation uses the same activation-height check and rejects an
NU6.1 activation block if the expected lockbox disbursements are empty:

- `zebra-consensus/src/block/check.rs:256-267`

The mining RPC template path includes `network.lockbox_disbursements(height)` in
the standard coinbase outputs:

- `zebra-rpc/src/methods/types/get_block_template.rs:867-902`

## Local Verification

Targeted tests passed:

```sh
cargo test -p zebra-chain activates_network_upgrades_correctly --lib
cargo test -p zebra-chain omitted_nu6_1_inherits_nu7_lockbox_boundary_today --lib
```

Result on 2026-05-09: both passed. The existing activation-height test confirms
the key fallback primitive: a custom network with only `nu7: Some(1)` makes the
earlier upgrades report activation height `Height(1)`. The added Regtest-focused
test confirms that omitted `nu6_1` inherits the later configured NU7 height and
that default Regtest lockbox disbursements are empty at that inherited boundary.

## Duplicate Check

Read-only duplicate searches refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra NU6.1 lockbox omitted nu7 activation fallback'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra custom network nu6_1 omitted lockbox disbursements'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "NU6.1" "NU7" "lockbox" "Regtest"'
gh api repos/ZcashFoundation/zebra/issues/10557
gh api repos/ZcashFoundation/zebra/issues/10558
gh api repos/ZcashFoundation/zebra/pulls/9526
gh api repos/ZcashFoundation/zebra/pulls/9710
```

Closest hits:

- #10557 covers the adjacent Regtest funding-stream validation bypass.
- #10558 covers custom lockbox disbursement address/amount panics.
- PR #9526 introduced the `Nu6_1` network upgrade variant.
- PR #9710 added Regtest funding-stream and activation-height configurability.

No exact tracker was found for omitted `nu6_1` inheriting a later configured
NU7 height for NU6.1 lockbox logic.

## Impact

Likely severity: low.

Preconditions:

- custom Regtest or configured Testnet parameters,
- a later upgrade height such as NU7 is set,
- `nu6_1` is omitted,
- the node reaches that height through block validation or template generation.

Potential effects:

- Regtest/custom-network block production can become unexpectedly unmineable at
  the inherited NU6.1 height when lockbox disbursements are empty.
- If custom lockbox disbursements are present, coinbase templates and consensus
  checks can require one-time disbursements at a later upgrade height that was
  not intended to activate NU6.1.

This does not affect shipped Mainnet or default Testnet activation lists.

## Suggested Fix Direction

- Make NU6.1 lockbox logic require an explicit NU6.1 activation entry on custom
  networks, rather than accepting a fallback-derived activation height.
- Alternatively, validate custom activation configs so a later configured
  upgrade cannot silently imply NU6.1 lockbox activation without disbursements.
- Add a focused Regtest regression test for `nu7` configured with `nu6_1`
  omitted and empty lockbox disbursements.

## Disclosure Triage

Public hardening.

This is not currently private-disclosure-worthy because it is limited to custom
network configuration and does not create an invalid-block acceptance path on
Mainnet or default Testnet.

## Confidence

Confidence: medium-high for the source-level behavior and custom-network
availability footgun. Confidence: low for broader operational severity because
the path requires non-default activation configuration.
