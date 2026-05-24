# GetBlockTemplate High-Fee Coinbase Overflow Panic Note

Date: 2026-05-03

Scope: adjacent value-arithmetic hardening issue in block-template coinbase
generation.

## Finding

`getblocktemplate` coinbase generation can panic if the selected mempool
transactions have a total miner fee that is individually representable as an
`Amount<NonNegative>`, but cannot be added to the current miner subsidy without
exceeding `MAX_MONEY`.

This is not a consensus acceptance issue. The block verifier handles the
corresponding `expected_block_subsidy + block_miner_fees` overflow as a subsidy
error. The issue is that the RPC/mining template path uses `expect()` while
constructing standard coinbase outputs, so the node can abort instead of
returning a clean block-template error.

## Evidence

- `zebra-rpc/src/methods/types/get_block_template.rs:811-812` sums selected
  mempool fees and passes the total into `standard_coinbase_outputs()`.
- `zebra-rpc/src/methods/types/get_block_template.rs:853-861` only asserts that
  the selected fee sum itself is at most `MAX_MONEY`.
- `zebra-rpc/src/methods/types/get_block_template.rs:873-881` computes
  `miner_subsidy(...) + miner_fee` and then unwraps the `Amount` result with
  `expect("reward calculations are valid for reasonable chain heights")`.
- `zebra-rpc/src/methods/types/get_block_template/zip317.rs:61-143` selects
  transactions by size, sigops, unpaid actions, dependencies, and fee weighting;
  it does not cap cumulative miner fees at `MAX_MONEY - miner_subsidy`.
- `zebra-rpc/src/methods/types/get_block_template/zip317.rs:378-410` confirms the
  per-transaction fit check updates only block bytes, sigops, and unpaid actions.
- `zebra-consensus/src/block/check.rs:364-365` maps the same arithmetic overflow
  into `SubsidyError::Overflow` on the validation path, rather than panicking.
- `Cargo.toml:183-184` and `Cargo.toml:304-305` set `panic = "abort"` for dev
  and release profiles.

## Local Proof

Added a durable current-behavior `zebra-rpc` unit test:
`zebra-rpc/src/methods/types/get_block_template/tests.rs:49`.

The test shape:

- custom Testnet/Regtest-like parameters with NU6 active at height 1,
- `miner_fee = MAX_MONEY`,
- call `standard_coinbase_outputs(...)`.

Command:

```sh
cargo test -p zebra-rpc standard_coinbase_outputs_panics_when_fee_plus_subsidy_exceeds_max_money_today --lib
```

Observed:

```text
test methods::types::get_block_template::tests::standard_coinbase_outputs_panics_when_fee_plus_subsidy_exceeds_max_money_today - should panic ... ok
```

The containing test module also passes:

```sh
cargo test -p zebra-rpc methods::types::get_block_template::tests --lib
```

## Impact

Availability impact for nodes using the mining RPC/template path.

Practical exploitability is low on ordinary public networks because an attacker
would need to get a very high-fee transaction into the node's mempool. That
requires spendable value and ordinary mempool admission checks, and the fee would
be economically extreme on mainnet.

The shape is more relevant on custom networks, Regtest, test harnesses, or
operator-controlled mining environments where large-value UTXOs and exposed RPC
are easier to create. It is also worth fixing because the verifier already has a
non-panicking error path for the same arithmetic class.

## Suggested Fix

- Make `standard_coinbase_outputs()` return `Result<_, SubsidyError>` or a
  getblocktemplate-specific typed error instead of panicking.
- During transaction selection, cap selected total fees at
  `MAX_MONEY - miner_subsidy(height, network, subsidy)` so the template path
  does not select an unmineable high-fee set.
- Preserve the block verifier's existing reject behavior for submitted blocks.
- Add a regression test with `miner_fee = MAX_MONEY` and nonzero subsidy.

## Triage

Public RPC/mining hardening. Not private disclosure on current evidence because
it requires mining RPC/template generation plus economically extreme or
custom-network mempool contents, and it does not affect consensus acceptance.

## Confidence

Confidence: high for the direct panic in `standard_coinbase_outputs()`.
Confidence: medium-low for realistic remote exploitability on mainnet/default
testnet deployments.
