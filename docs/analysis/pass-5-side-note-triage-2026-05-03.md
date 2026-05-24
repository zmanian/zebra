# Pass 5 Side-Note Triage

Date: 2026-05-03

Scope: second-pass triage of remaining pass-5 side notes after the main finding
rollup had already promoted the high-confidence current-network issues.

## Summary

Only the NU7 / ZIP-235 miner-fee-share overflow deserves a conservative private
maintainer heads-up, and even that is a future-activation blocker rather than a
current default-network emergency.

The GBT ZIP-317 selection-complexity note, release/deploy feature-variable
note, and mempool/health configuration notes should stay as public hardening or
operator-footgun material unless additional evidence changes their reachability
or severity.

## Decisions

### ZIP-235 miner-fee-share intermediate overflow

Decision: include in the pass-5 rollup as a future-gated private maintainer
heads-up / future-activation blocker.

Reasoning:

- `zebra-consensus/src/block/check.rs:337-344` unwraps
  `((block_miner_fees * 6) / 10)` under `zcash_unstable = "zip235"` after NU7
  activation.
- `zebra-chain/src/transaction/builder.rs:90-95` has the same default V6
  coinbase ZIP-233 calculation when `zip233_amount` is omitted.
- `Amount * u64` checks the intermediate against `MAX_MONEY`, so fees above
  `MAX_MONEY / 6` can panic before the final 60% value is constrained.
- Default Mainnet/Testnet do not activate NU7 today, and source-controlled
  default release/Docker features do not enable the combined
  `nu7 + zip235 + tx_v6` path.

Remaining confidence check: ask maintainers whether private release variables,
deployment variables, or downstream builds ever combine `tx_v6` with
`zcash_unstable = "nu7"` and `zcash_unstable = "zip235"` on an NU7-active
network.

### GBT ZIP-317 selection complexity

Decision: keep as a side note / public performance hardening lead.

Reasoning:

- The structural cost is real: `getblocktemplate` fetches the full mempool and
  the ZIP-317 selector rebuilds weighted candidate state during selection.
- The pass-5 rollup already includes the stronger, source-backed GBT issues:
  missing dependency metadata, full-mempool long-poll polling, testnet sync-gate
  policy, high-fee coinbase overflow, and max-time long-poll behavior.
- No benchmark or profiler evidence yet shows the selector loop itself is a
  distinct severe attack surface under realistic RPC and mempool limits.

Promotion trigger: add benchmark evidence or a bounded worst-case repro showing
that ZIP-317 selection dominates template cost separately from the already
documented `FullTransactions` clone/polling work.

### Release/deploy feature variables

Decision: keep as operational release hardening unless maintainers confirm
dangerous private variable values.

Reasoning:

- `.github/workflows/release-binaries.yml:28-35` builds official Docker runtime
  images with `vars.RUST_PROD_FEATURES`.
- `.github/workflows/zfnd-deploy-nodes-gcp.yml:250-264` builds deployment
  runtime images with both `vars.RUST_PROD_FEATURES` and
  `vars.RUST_TEST_FEATURES`.
- The checked-in workflows prove possible release/deploy feature-surface
  divergence, but do not reveal the private variable values or any
  `RUSTFLAGS` that enable `zcash_unstable` cfgs.

Promotion trigger: promote only if private variables or downstream packaging
actually enable unstable/test-only behavior in runtime artifacts.

### Mempool debug and health testnet readiness

Decision: keep as public operator-footgun hardening, not private disclosure.

Reasoning:

- `zebrad/src/components/mempool/config.rs:35-43` exposes
  `debug_enable_at_height`, defaulting to `None`.
- `zebrad/src/components/mempool.rs:351-352` lets that debug flag bypass the
  close-to-tip mempool gate.
- `zebrad/src/commands/start.rs:119-125` forces the flag only on regtest.
- `zebrad/src/components/health/config.rs:21-37` defaults
  `enforce_on_test_networks` to `false`, and
  `zebrad/src/components/health.rs:225-227` returns `/ready` 200 on test
  networks unless enforcement is enabled.

These are explicit test-network/configuration behaviors. They can mislead
automation or copied deployments, but they do not form a hidden consensus or
default-production vulnerability.
