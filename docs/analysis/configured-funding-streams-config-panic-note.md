# Configured Funding Streams Config Panic Note

Date: 2026-05-03

Last updated: 2026-05-09

Disposition: local-only residual/adjacent to #10557. Do not post publicly
without explicit re-authorization. Related Regtest-specific bypass was already
posted as #10557.

Scope: audit of custom Testnet and Regtest funding stream configuration paths.

## Summary

Malformed configured funding streams can panic during Zebra configuration
deserialization or custom network construction instead of returning a
configuration error. This is an operator-local startup/restart availability
hardening issue, not a remote consensus vulnerability.

The panic is reachable through configured Testnet or Regtest parameters when a
funding stream has too few recipient addresses, recipient numerators whose sum
exceeds the denominator, or recipient addresses for the wrong network kind.

## Evidence

`ConfiguredFundingStreams::convert_with_default()` validates some user-provided
funding stream parameters with `assert!`:

- `zebra-chain/src/parameters/network/testnet.rs:246-249` asserts that the
  height range start is not above the end.
- `zebra-chain/src/parameters/network/testnet.rs:261-265` asserts that the sum
  of funding stream numerators does not exceed
  `FUNDING_STREAM_RECEIVER_DENOMINATOR`.

The final funding-stream validation also uses asserts:

- `zebra-chain/src/parameters/network/testnet.rs:310-328` asserts that each
  non-deferred recipient has enough addresses for the configured height range.
- `zebra-chain/src/parameters/network/testnet.rs:330-335` asserts that each
  recipient address is a Testnet address.
- `zebra-chain/src/parameters/network/testnet.rs:855-862` calls that validation
  from `ParametersBuilder::to_network()`.

These paths are used by real config deserialization:

- `zebra-network/src/config.rs:871-873` passes configured Testnet funding
  streams into `ParametersBuilder::with_funding_streams()`.
- `zebra-network/src/config.rs:884-885` can call `extend_funding_streams()` on
  configured Testnet parameters.
- `zebra-network/src/config.rs:898-903` calls `to_network()` while deserializing
  configured Testnet parameters and maps returned builder errors into config
  errors, but panics from asserts bypass that error path.
- `zebra-network/src/config.rs:754-787` converts Regtest `testnet_parameters`
  into `RegtestParameters` and then calls `Network::new_regtest()`.
- `zebra-chain/src/parameters/network.rs:172-177` calls
  `testnet::Parameters::new_regtest(params).expect(...)`, so any builder error
  in Regtest construction becomes a panic.

Existing tests document the behavior:

- `zebra-chain/src/parameters/network/tests/vectors.rs:433-493` uses
  `catch_unwind()` and expects panics for too few addresses, excessive
  numerators, and Mainnet recipient addresses.

Verification:

```sh
cargo test -p zebra-chain check_configured_funding_stream_constraints --lib
```

Result on 2026-05-09: passed. The test confirms that the three malformed
funding-stream configurations currently panic.

## Duplicate / Overlap Check

Known public overlap:

- #10557 covers the Regtest-specific funding-stream validation bypass and later
  runtime panic path.

This note also covers configured Testnet funding-stream assertion panics during
custom-network construction. Keep that broader local/deployment-input hardening
context local unless explicitly re-authorized.

## Impact

An operator, deployment system, or config-management path that writes invalid
funding stream parameters can make Zebra fail by panic at startup or restart.
This could matter in automated custom-network, Regtest, CI, or ephemeral testnet
deployments where configuration is assembled from templates or environment
inputs.

This is not attacker-reachable through normal P2P, mempool, or RPC traffic.
It does not allow invalid blocks or transactions to be accepted, and it does not
affect Mainnet or the default public Testnet without custom network parameters.

## Severity

Low. This is a local/deployment-input availability issue. It is useful hardening
because configuration deserialization should reject malformed user input with
ordinary errors rather than panicking, but it does not meet the bar for private
security disclosure by itself.

## Recommended Fix

Replace funding-stream configuration asserts with typed `ParametersBuilderError`
variants and return those errors through `ParametersBuilder::to_network()` and
`Parameters::new_regtest()`.

In particular:

- make `ConfiguredFundingStreams::convert_with_default()` return
  `Result<FundingStreams, ParametersBuilderError>`;
- make `check_funding_stream_address_period()` return
  `Result<(), ParametersBuilderError>`;
- remove the `expect("regtest parameters should always be valid")` in
  `Network::new_regtest()` or provide a fallible constructor for config paths;
- keep tests for the same malformed inputs, but assert returned errors instead
  of caught panics.

## Confidence

High that the panic behavior exists: it is encoded in both the production code
and the existing unit test expectations.

Low-to-medium that it is security-relevant beyond hardening: the input is local
configuration, so attacker reachability depends on an external config-injection
or deployment-control weakness.
