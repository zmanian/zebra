# Regtest Funding Stream Validation Bypass Panic Note

Date: 2026-05-04

Public issue: <https://github.com/ZcashFoundation/zebra/issues/10557>

Scope: follow-up audit of custom funding-stream configuration, focused on the
difference between configured Testnet construction and Regtest construction.

## Summary

Configured Testnet funding streams go through a final address-period validation
step before `Network` construction. Regtest funding streams do not. As a
result, malformed Regtest funding stream parameters can survive startup and
later panic in ordinary subsidy consumers such as `getblocksubsidy`,
`getblocktemplate`/`generate`, or block subsidy validation.

This is a Regtest/custom-network hardening issue. It does not affect Mainnet or
the default public Testnet.

## Evidence

Configured Testnet construction calls `ParametersBuilder::to_network()`:

- `zebra-chain/src/parameters/network/testnet.rs:855-862`

That final pass calls `check_funding_stream_address_period()` for every
configured funding stream:

- `zebra-chain/src/parameters/network/testnet.rs:310-335`

Those checks assert that non-deferred recipients have enough addresses for the
height range and that addresses are Testnet addresses.

Regtest construction takes a different path. `Network::new_regtest()` delegates
to `testnet::Parameters::new_regtest()`:

- `zebra-chain/src/parameters/network.rs:173-177`

`Parameters::new_regtest()` accepts configured funding streams and then returns
`parameters.finish()` directly:

- `zebra-chain/src/parameters/network/testnet.rs:986-1017`

It does not call `to_network()`, so it skips
`check_funding_stream_address_period()`.

The existing tests cover the two behaviors separately:

- `zebra-chain/src/parameters/network/tests/vectors.rs:339-493` expects
  configured Testnet construction to panic for too few addresses, excessive
  numerators, and Mainnet recipient addresses.
- `zebra-chain/src/parameters/network/tests/vectors.rs:498-538` confirms
  Regtest preserves configured funding streams, but does not exercise malformed
  address-count or address-network cases.
- `zebra-consensus/src/block/subsidy/tests.rs` now includes the
  current-behavior proof
  `regtest_funding_stream_address_panics_after_skipped_validation_today`.

Once malformed Regtest streams are retained, runtime address selection can
panic. `funding_stream_address_index()` computes a one-based funding stream
address index and asserts it is within the recipient address vector length:

- `zebra-consensus/src/block/subsidy.rs:18-37`

For example, a non-deferred recipient with an empty address list can pass
Regtest construction, but at an active funding-stream height the computed index
is positive while `num_addresses` is zero, so the assertion fails.

Runtime consumers include:

- `zebra-rpc/src/methods.rs:2800-2844` for `getblocksubsidy`;
- `zebra-rpc/src/methods/types/get_block_template.rs:867-902` for standard
  coinbase output construction used by mining/template paths;
- `zebra-consensus/src/block/check.rs:286-296` for block subsidy validation.

Focused proof command:

```sh
cargo test -p zebra-consensus regtest_funding_stream_address_panics_after_skipped_validation_today --lib
```

Result on 2026-05-07:

```text
test block::subsidy::tests::regtest_funding_stream_address_panics_after_skipped_validation_today - should panic ... ok
```

The test constructs Regtest with an active ECC funding stream whose recipient
has an empty address list. Construction succeeds, then
`funding_stream_address(Height(1), ...)` panics.

## Impact

Likely severity: low.

Preconditions:

- the node is running on Regtest/custom parameters;
- Regtest funding streams are supplied through local config or programmatic
  construction;
- a non-deferred recipient is malformed, for example with too few addresses for
  the configured height range;
- the node reaches or is queried for an active funding-stream height.

Potential effects:

- `getblocksubsidy` can panic when asked for an affected height;
- `getblocktemplate` or `generate` can panic while constructing coinbase
  outputs;
- block validation can panic while checking required funding-stream outputs.

This is not remotely reachable on a normally configured node. A remote RPC
caller can only trigger the later panic if the node operator already launched
Regtest with malformed custom funding-stream parameters and exposed the
affected RPC.

## Recommended Fix Direction

- Make `Parameters::new_regtest()` use the same final funding-stream validation
  as configured Testnet before returning.
- Prefer converting the current assertion-based funding-stream checks into
  `ParametersBuilderError` variants, so malformed Regtest/Testnet configuration
  is rejected with typed errors rather than panics.
- Add Regtest regression coverage for malformed funding streams:
  - empty non-deferred recipient address list;
  - too few recipient addresses for the configured height range;
  - wrong-network recipient address.

## Disclosure Triage

Public hardening. This is custom Regtest/local-configuration availability, not
private-disclosure-worthy by itself.

## Confidence

High that Regtest skips the final validation and that the runtime assertion
exists. Medium that the most important practical trigger is RPC/mining, because
it depends on operators configuring malformed Regtest funding streams and then
querying or mining at the affected height.
