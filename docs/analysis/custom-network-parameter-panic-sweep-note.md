# Custom Network Parameter Panic Sweep Note

Date: 2026-05-04

Scope: follow-up audit of configured Testnet and Regtest consensus parameter
handling, focused on parameters that are accepted as local configuration but
later flow into `assert!`, `expect()`, or unchecked division paths.

## Summary

Several custom-network parameters can still turn malformed local configuration
into panics instead of ordinary configuration errors. These are deployment-input
availability hardening issues, not remote mainnet consensus vulnerabilities.

The most concrete cases found in this pass are:

- configured Testnet `pre_blossom_halving_interval = 0`;
- configured Testnet `slow_start_interval` values that make the early block
  subsidy not exactly divisible by 5;
- configured Testnet `target_difficulty_limit = "0"`;
- configured Testnet or Regtest `lockbox_disbursements` containing invalid
  transparent addresses or individually-valid amounts whose sum is invalid.

## Evidence

Configured Testnet exposes both `target_difficulty_limit` and
`pre_blossom_halving_interval` as optional config fields:

- `zebra-network/src/config.rs:596`
- `zebra-network/src/config.rs:603`

`target_difficulty_limit` is parsed from a string and passed to
`ParametersBuilder::with_target_difficulty_limit()`:

- `zebra-network/src/config.rs:835-839`

The builder validates it by converting to compact difficulty first:

- `zebra-chain/src/parameters/network/testnet.rs:718-726`

But `ExpandedDifficulty::to_compact()` explicitly panics on zero:

- `zebra-chain/src/work/difficulty.rs:417-445`

So a configured Testnet target difficulty limit of `0` can panic during config
deserialization rather than returning `ParametersBuilderError::InvaildDifficultyLimits`.

The configured Testnet halving interval is passed to
`ParametersBuilder::with_halving_interval()`:

- `zebra-network/src/config.rs:855-857`

The builder does not reject zero. It stores the pre-Blossom interval and derives
the post-Blossom interval by multiplication:

- `zebra-chain/src/parameters/network/testnet.rs:746-756`

Subsidy code later divides by these network-provided intervals:

- `zebra-chain/src/parameters/network/subsidy.rs:432`
- `zebra-chain/src/parameters/network/subsidy.rs:440`

There is also an earlier configured-Testnet startup path: default funding
streams are validated in `ParametersBuilder::to_network()`, and funding-stream
address period calculation divides by
`network.funding_stream_address_change_interval()`, which is derived from the
configured post-Blossom halving interval:

- `zebra-chain/src/parameters/network/testnet.rs:855-862`
- `zebra-chain/src/parameters/network/subsidy.rs:276`
- `zebra-chain/src/parameters/network/subsidy.rs:294-295`

With a zero halving interval, the likely config-file manifestation is therefore
a startup/configuration panic during funding-stream validation. Programmatic
custom-network construction that clears funding streams could defer the same
zero interval to later subsidy callers, including `getblocksubsidy`:

- `zebra-rpc/src/methods.rs:2800-2808`

Configured Testnet also exposes `slow_start_interval`:

- `zebra-network/src/config.rs:595`
- `zebra-network/src/config.rs:827-830`

The builder accepts the interval without checking that all pre-Canopy slow-start
subsidies remain divisible by the founders reward denominator:

- `zebra-chain/src/parameters/network/testnet.rs:667-670`
- `zebra-chain/src/parameters/network/testnet.rs:836-837`

Changing `slow_start_interval` also changes `slow_start_shift`, which feeds the
custom-network first-halving height and funding-stream address-period
calculation. With default Testnet funding streams still present, a configured
Testnet using `slow_start_interval = 7` reaches a startup/configuration panic in
funding-stream address-period validation because the derived period requires
more recipient addresses than the default configuration provides:

- `zebra-chain/src/parameters/network/subsidy.rs:239-255`
- `zebra-chain/src/parameters/network/subsidy.rs:283-295`
- `zebra-chain/src/parameters/network/testnet.rs:313-325`
- `zebra-chain/src/parameters/network/testnet.rs:855-862`

There is a second deferred panic if a programmatic custom-network caller clears
funding streams. `block_subsidy()` floors
`MAX_BLOCK_SUBSIDY / slow_start_interval` and then multiplies that rate by the
early height:

- `zebra-chain/src/parameters/network/subsidy.rs:451-475`

But `founders_reward()` assumes every pre-Canopy first-halving subsidy divides
exactly by 5 and calls the panicking `Amount<NonNegative>::div_exact(5)`:

- `zebra-chain/src/amount.rs:79-86`
- `zebra-chain/src/parameters/network/subsidy.rs:539-549`

A configured Testnet with `slow_start_interval = 7` and cleared funding streams
builds successfully, but `founders_reward(&network, Height(1))` panics because
the floored slow-start subsidy is not divisible by 5. This can be reached
through block subsidy RPC formatting and through block-validation paths that
compute the expected founders reward on pre-Canopy custom Testnets:

- `zebra-rpc/src/methods.rs:2800-2855`
- `zebra-consensus/src/block/check.rs:200-210`

Lockbox disbursements are accepted from configured Testnet/Regtest config as
raw address strings plus individually-validated `Amount<NonNegative>` values:

- `zebra-chain/src/parameters/network/testnet.rs:124-128`
- `zebra-network/src/config.rs:604`
- `zebra-network/src/config.rs:877`
- `zebra-chain/src/parameters/network/testnet.rs:761-768`

They are not parsed or summed by the builder. Later, `Parameters` parses each
configured address with an `expect()`:

- `zebra-chain/src/parameters/network/testnet.rs:1134-1142`

The total amount helper also sums configured amounts and expects the sum to be
valid:

- `zebra-chain/src/parameters/network/testnet.rs:1125-1130`

Those helpers are reached by NU6.1 lockbox consensus/template paths:

- `zebra-chain/src/parameters/network.rs:291-303`
- `zebra-chain/src/parameters/network.rs:308-321`
- `zebra-consensus/src/block/check.rs:262`
- `zebra-rpc/src/methods/types/get_block_template.rs:902`

Local proof tests added in
`zebra-chain/src/parameters/network/tests/vectors.rs`:

- `configured_lockbox_disbursement_invalid_address_panics_today`
- `configured_lockbox_disbursement_total_overflow_panics_today`
- `configured_slow_start_interval_can_make_funding_stream_validation_panic_today`
- `configured_slow_start_interval_can_make_founders_reward_panic_today`

Verification on 2026-05-07:

```sh
cargo test -p zebra-chain configured_lockbox_disbursement_invalid_address_panics_today --lib
cargo test -p zebra-chain configured_lockbox_disbursement_total_overflow_panics_today --lib
cargo test -p zebra-chain sum_of_one_time_lockbox_disbursements_is_correct --lib
cargo fmt -p zebra-chain
git diff --check
```

All commands completed successfully.

Additional verification on 2026-05-09:

```sh
cargo test -p zebra-chain configured_slow_start_interval_can_make --lib
```

The command completed successfully.

## Impact

Likely severity: low.

These issues require local custom-network or Regtest configuration, or a
programmatic caller constructing custom `Parameters`. They do not affect
Mainnet or the default public Testnet without custom parameters.

Practical effects:

- a malformed custom Testnet config can panic at startup/restart;
- a custom network with malformed lockbox disbursements can panic when NU6.1
  lockbox helpers are reached by block validation, template construction, or
  related accounting;
- automated testing or deployment systems that assemble custom-network configs
  from templates can turn ordinary invalid input into process aborts.

This does not create an invalid-block acceptance path, and it is not reachable
from normal P2P, mempool, or RPC input unless the node was already launched with
the malformed custom parameters.

## Recommended Fix Direction

- Reject zero `pre_blossom_halving_interval` in
  `ParametersBuilder::with_halving_interval()`.
- Make `with_target_difficulty_limit()` reject zero before calling
  `to_compact()`, returning `ParametersBuilderError` instead of panicking.
- Parse and validate configured lockbox disbursement addresses in the builder.
- Validate that the configured lockbox disbursement total is a valid
  `Amount<NonNegative>` before returning a `Network`.
- Convert the existing lockbox `expect()` calls into typed configuration or
  consensus errors where the address is not hard-coded.

## Disclosure Triage

Public hardening. These are local/deployment-input availability issues, not
private-disclosure-worthy vulnerabilities on their own.

Reported publicly:

- [#10558](https://github.com/ZcashFoundation/zebra/issues/10558):
  custom lockbox disbursement config can panic during NU6.1 validation.

## Confidence

Medium-high for the panic paths: the source-level control flow is direct, and
the panicking operations are explicit.

Low for broader security severity: exploitation depends on a separate way to
influence the node's custom-network configuration or programmatic construction
of network parameters.
