# Custom lockbox disbursement config can panic during NU6.1 validation

## Summary

Configured Testnet and Regtest `lockbox_disbursements` accept raw address
strings and individually valid amounts, but the builder does not parse the
addresses or validate that the total disbursement amount is valid before
returning a `Network`.

Later NU6.1 lockbox helpers parse each configured address with `expect()` and
sum configured amounts with another `expect()`. A malformed custom-network
configuration can therefore turn ordinary invalid config into a process panic
when NU6.1 lockbox validation/template/accounting code reaches those helpers.

This looks like low-severity public hardening rather than a private disclosure
item: it requires local custom-network or Regtest configuration and does not
affect default Mainnet or public Testnet parameters.

## Code Path

- `ConfiguredLockboxDisbursement` stores the configured address as `String`:
  `zebra-chain/src/parameters/network/testnet.rs:124`
- `ParametersBuilder::with_lockbox_disbursements()` stores the strings and
  amounts without parsing or total validation:
  `zebra-chain/src/parameters/network/testnet.rs:761`
- `Parameters::lockbox_disbursements()` later parses the address with
  `addr.parse().expect("hard-coded address must deserialize")`:
  `zebra-chain/src/parameters/network/testnet.rs:1134`
- `Parameters::lockbox_disbursement_total_amount()` sums amounts with
  `.expect("sum of configured amounts should be valid")`:
  `zebra-chain/src/parameters/network/testnet.rs:1125`
- The `Network` wrappers route custom Testnet parameters to these helpers at
  the NU6.1 activation height:
  `zebra-chain/src/parameters/network.rs:291` and
  `zebra-chain/src/parameters/network.rs:308`
- Block subsidy validation reaches `net.lockbox_disbursements(height)` for the
  NU6.1 activation block:
  `zebra-consensus/src/block/check.rs:261`

## Local Reproduction Tests

I added two local proof tests in
`zebra-chain/src/parameters/network/tests/vectors.rs`:

- `configured_lockbox_disbursement_invalid_address_panics_today`
- `configured_lockbox_disbursement_total_overflow_panics_today`

Both reproduce the current panic behavior:

```sh
cargo test -p zebra-chain configured_lockbox_disbursement_invalid_address_panics_today --lib
cargo test -p zebra-chain configured_lockbox_disbursement_total_overflow_panics_today --lib
```

Both pass as `#[should_panic]` tests on the current tree.

I also reran the adjacent existing sanity test:

```sh
cargo test -p zebra-chain sum_of_one_time_lockbox_disbursements_is_correct --lib
```

and formatting/diff hygiene:

```sh
cargo fmt -p zebra-chain
git diff --check
```

## Expected Behavior

Malformed configured lockbox disbursement addresses and invalid total amounts
should be rejected during custom-network construction or config deserialization,
returning a typed configuration/builder error instead of deferring to runtime
`expect()` panics.

## Suggested Fix Direction

- Parse and validate configured lockbox disbursement addresses in the builder.
- Validate that the configured disbursement total fits in
  `Amount<NonNegative>` before returning a `Network`.
- Keep the existing hard-coded-address invariant for default Mainnet/Testnet
  constants if desired, but avoid applying that invariant to operator-provided
  custom-network config.
