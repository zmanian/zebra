# ZIP-235 Miner-Fee Share Intermediate Overflow Panic Note

Date: 2026-05-03

Scope: follow-up on the value-pool / amount-arithmetic audit pass, focused on
future NU7 / ZIP-235 miner-fee share validation.

## Finding

When Zebra is built with the unstable NU7 transaction-v6 path and the ZIP-235
fee-share check, high block miner fees can make consensus validation panic before
it reaches the normal subsidy/miner-fee equality check.

The check computes the minimum ZIP-233 amount with:

```rust
let minimum_zip233_amount = ((block_miner_fees * 6).unwrap() / 10).unwrap();
```

`Amount<NonNegative> * u64` returns an error if the intermediate product is
outside `0..=MAX_MONEY`. Therefore any valid `block_miner_fees` above
`MAX_MONEY / 6` makes `block_miner_fees * 6` return
`MultiplicationOverflow`, and the unwrap panics. The intended arithmetic is
`floor(block_miner_fees * 60 / 100)`, which can remain well within
`MAX_MONEY` even when the intermediate `* 6` does not.

This is not reachable in default mainnet/testnet builds today. It requires
building with the relevant unstable cfgs and transaction-v6 feature, and a
network height where NU7 is active.

## Preconditions

- Build-time cfgs include both `zcash_unstable = "nu7"` and
  `zcash_unstable = "zip235"`.
- Cargo feature `tx_v6` is enabled.
- The network has an NU7 activation height. The shipped mainnet and default
  testnet activation lists stop at NU6.1, but custom Testnet/Regtest parameters
  can configure `nu7`.
- A block or block-template path reaches aggregate miner fees above
  `MAX_MONEY / 6`.

## Evidence

- `zebra-consensus/src/block/check.rs:337-344` runs the ZIP-235 check at and
  after NU7 activation, and unwraps `((block_miner_fees * 6) / 10)`.
- `zebra-chain/src/amount.rs:377-394` implements `Amount * u64` as checked
  multiplication followed by re-constraining to the original `Amount`
  constraint; values above `MAX_MONEY` return `MultiplicationOverflow`.
- `zebra-chain/src/amount.rs:580-610` defines `Amount<NonNegative>` as
  `0..=MAX_MONEY`, where `MAX_MONEY = 21_000_000 * COIN`.
- `zebra-consensus/src/block.rs:292-341` sums per-transaction miner fees, maps
  sum overflow into `BlockError::SummingMinerFees`, then calls
  `miner_fees_are_valid()`. The sum itself can be any valid
  `Amount<NonNegative>` up to `MAX_MONEY`.
- `zebra-consensus/src/transaction.rs:545-567` calculates each non-coinbase
  miner fee from the remaining transaction value, subtracting the transaction's
  own ZIP-233 amount for V6 transactions. There is no `MAX_MONEY / 6` cap.
- `zebra-state/src/service/check/utxo.rs:231-267` checks that each
  non-coinbase transaction's remaining value is non-negative, not that the
  resulting block fee is below `MAX_MONEY / 6`.
- `zebra-consensus/src/block/check.rs:364-365` separately checks
  `expected_block_subsidy + block_miner_fees` for overflow, so fees above
  `MAX_MONEY / 6` but below `MAX_MONEY - subsidy` are otherwise representable.
- `zebra-chain/src/transaction/builder.rs:90-95` has the same intermediate
  unwrap when generating a V6 coinbase transaction without an explicit
  `zip233_amount`.
- `zebra-rpc/src/methods/types/get_block_template.rs:811-831` calculates
  selected mempool miner fees and passes them into V6 coinbase generation.
- `zebra-rpc/src/methods.rs:2512-2544` passes `None` for the ZIP-233 amount in
  current `getblocktemplate` construction, so the builder default computes the
  same panic-prone expression when ZIP-235 is compiled in.
- `Cargo.toml:183-184` and `Cargo.toml:304-305` set `panic = "abort"` for dev
  and release profiles.

Build reachability evidence:

- `Cargo.toml:327-329` allows `zcash_unstable` values including `nu7` and
  `zip235`.
- `zebrad/Cargo.toml:144` wires the top-level `tx_v6` feature through
  `zebra-chain`, `zebra-state`, `zebra-consensus`, and `zebra-rpc`.
- `.github/workflows/test-crates.yml:141-142` and
  `.github/workflows/lint.yml:229` use `--all-features`, but the checked CI
  snippets do not set the `zcash_unstable` cfgs, so ordinary all-feature CI does
  not necessarily exercise this combined unstable path.
- `zebrad/Cargo.toml:54-58` defines the default release binary features without
  `tx_v6`; `zebrad/Cargo.toml:144` keeps `tx_v6` as an explicit opt-in feature.
- `docker/Dockerfile:18` defaults `FEATURES` to `default-release-binaries`, and
  `docker/Dockerfile:162` passes only that feature string into `cargo build`.
- `.github/workflows/release-binaries.yml:34`,
  `.github/workflows/zfnd-ci-integration-tests-gcp.yml:144`, and
  `.github/workflows/zfnd-deploy-nodes-gcp.yml:263` take feature strings from
  GitHub repository variables. The checked-in workflow snippets do not set
  `RUSTFLAGS` or any `zcash_unstable` cfgs; the private repository variable
  values are not visible from this checkout.
- `zebra-chain/src/parameters/network_upgrade.rs:88-112` and
  `zebra-chain/src/parameters/network_upgrade.rs:123-136` list default Mainnet
  and Testnet activations only through NU6.1.

Path classification:

- Template mode enters the builder-side sink. `getblocktemplate` passes
  `zip233_amount = None`, selected mempool fees are summed into `miner_fee`, and
  `Transaction::new_v6_coinbase()` computes the default ZIP-233 amount with the
  overflow-prone expression.
- Proposal mode, `submitblock`, and ordinary block sync enter the consensus-side
  sink. Caller-supplied block bytes are verified semantically, miner fees are
  summed, and `miner_fees_are_valid()` computes the minimum ZIP-233 amount with
  the same overflow-prone expression.
- The ZIP-317 fake coinbase used during transaction selection is not a direct
  overflow trigger. `zebra-rpc/src/methods/types/get_block_template/zip317.rs`
  hardcodes `miner_fee = 1` for the fake transaction before calling
  `Transaction::new_v6_coinbase()`. The real template-generation risk is later,
  when the selected transactions' aggregate fee is passed to
  `generate_coinbase_and_roots()`.

## Local Reproduction

First, I confirmed the unstable combined build reaches the existing ZIP-233
miner-fee tests:

```sh
RUSTFLAGS='--cfg zcash_unstable="nu7" --cfg zcash_unstable="zip235"' \
  cargo test -p zebra-consensus \
  miner_fees_validation_succeeds_when_zip233_amount_is_correct \
  --features tx_v6 --lib
```

Observed:

```text
test block::tests::miner_fees_validation_succeeds_when_zip233_amount_is_correct ... ok
```

There is now a durable targeted `#[should_panic]` regression at
`zebra-consensus/src/block/tests.rs:702`:

- `block_miner_fees = MAX_MONEY / 2`,
- `zip233_amount = 3 * MAX_MONEY / 10`,
- transparent coinbase output value = `MAX_MONEY / 5`,
- expected block subsidy and deferred amount = zero,
- custom Testnet with NU7 active at height 1.

The amounts are internally consistent for the later equality check:

```text
transparent output + zip233 amount = MAX_MONEY / 5 + 3 * MAX_MONEY / 10
                                  = MAX_MONEY / 2
                                  = expected subsidy + miner fees
```

But validation panics earlier while computing `block_miner_fees * 6`.

Command:

```sh
RUSTFLAGS='--cfg zcash_unstable="nu7" --cfg zcash_unstable="zip235"' \
  cargo test -p zebra-consensus \
  miner_fees_validation_panics_when_zip233_fee_share_intermediate_overflows_today \
  --features tx_v6 --lib
```

Observed:

```text
test block::tests::miner_fees_validation_panics_when_zip233_fee_share_intermediate_overflows_today - should panic ... ok
```

The surrounding miner-fee test filter also passes under the same unstable
configuration:

```sh
RUSTFLAGS='--cfg zcash_unstable="nu7" --cfg zcash_unstable="zip235"' \
  cargo test -p zebra-consensus miner_fees_validation --features tx_v6 --lib
```

Observed: six miner-fee tests passed, including the durable should-panic proof
and the existing ZIP-233 zero/correct/incorrect amount cases.

## Impact

If this code is deployed on an NU7/ZIP-235-active network, a high-fee otherwise
valid block can abort the node during block verification. The same arithmetic is
also in V6 coinbase generation, so a mining/RPC path that selects high-fee
mempool transactions and leaves `zip233_amount = None` can abort while building a
template.

Current severity is limited because the path is future/unstable gated:

- default mainnet and default testnet do not activate NU7;
- default release and Docker builds in source do not enable the combined
  unstable cfgs and `tx_v6`;
- exploiting the block-validation path requires a high-fee valid block on an
  NU7/ZIP-235-active network.

## Suggested Fix

Avoid overflow-prone intermediate `Amount` multiplication:

- compute the ratio in a wider integer type using raw zatoshis, then constrain
  the final value back to `Amount<NonNegative>`;
- or divide/reduce before multiplying in a way that preserves the intended
  consensus rounding semantics;
- return a typed subsidy error instead of unwrapping, even if the arithmetic is
  expected to be in range.

The same helper should be used by both `miner_fees_are_valid()` and
`Transaction::new_v6_coinbase()` so validation and generation stay identical.

Add tests with `block_miner_fees > MAX_MONEY / 6`, including at least
`MAX_MONEY / 2`, under the combined `nu7 + zip235 + tx_v6` configuration.

An independent RepoPrompt builder pass agreed with the reachability conclusion:
the panic is not enabled by repo-default features or default Mainnet/Testnet
activation lists, but it is reachable in opt-in experimental/custom NU7 builds
because `block_miner_fees` is bounded only by `0..=MAX_MONEY` before the ZIP-235
check.

## Triage

Conservative private maintainer heads-up / future-activation blocker.

This is not an emergency current-mainnet disclosure item on the evidence above.
If no shipped binary, Docker image, CI artifact, or downstream deployment enables
the combined `nu7 + zip235 + tx_v6` configuration on an NU7-active network, this
can be handled as a normal public bugfix. The source-controlled release and
Docker workflows do not show that combined configuration, but maintainer
confirmation is needed for private GitHub repository variables and downstream
packaging. Since it is consensus-path code for a future network upgrade, it is
still worth sending privately to maintainers now and fixing before any
NU7/ZIP-235-capable build is shipped or used on a network where NU7 can
activate.

## Confidence

Confidence: high for the panic in the combined unstable build and direct
consensus helper.

Confidence: medium for full block-validation exploitability on a future
NU7/ZIP-235-active network, because the repro targeted the consensus helper
directly rather than constructing a fully verified high-fee chain. The source
trace shows no `MAX_MONEY / 6` cap before the helper, and the test amounts are
consistent with the later miner-fee equality rule.

Remaining release-confidence check: ask maintainers whether any non-public
release variables or downstream builds set `RUSTFLAGS` with
`zcash_unstable = "nu7"` / `zcash_unstable = "zip235"` while also enabling
`tx_v6`.
