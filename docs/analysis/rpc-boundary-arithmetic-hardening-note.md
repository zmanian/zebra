# RPC Boundary Arithmetic Hardening Note

Date: 2026-05-04

Last updated: 2026-05-09

Disposition: local-only. Do not post publicly without explicit
re-authorization.

## Summary

Several RPC response paths assume ordinary chain heights and ordinary network
difficulty values. The assumptions are correct for current mainnet/testnet
operation, but the code uses panicking or non-finite floating-point arithmetic at
the boundary:

- `getblockchaininfo` and `getblocktemplate` compute the next block height with
  `(tip + 1).expect(...)`;
- `getblockchaininfo` computes `verificationprogress` as
  `tip_height / estimated_height`;
- `getdifficulty` / `getblockchaininfo` derive display difficulty by shifting
  256-bit targets down to 128 bits and dividing as `f64`.

This is not a consensus bug and is not an ordinary remote-triggerable process
crash in supported current deployments. It is a local hardening note for custom
networks, synthetic tests, future database-format changes, and operator-local
misconfiguration.

## Evidence

Next-height assumptions:

- `zebra-rpc/src/methods.rs:1094-1096` computes
  `getblockchaininfo`'s next consensus branch with
  `(tip_height + 1).expect("valid chain tips are a lot less than Height::MAX")`.
- `zebra-rpc/src/methods.rs:2501-2503` computes `getblocktemplate`'s next block
  height with `(chain_tip_and_local_time.tip_height + 1).expect(...)`.
- `zebra-rpc/src/methods/types/get_block_template.rs:288-290` repeats the same
  next-height assumption in `GetBlockTemplate::new_internal()`.
- `zebra-chain/src/block/height.rs:61-69` defines `Height::MAX` as
  `2^31 - 1`, and `zebra-chain/src/block/height.rs:249-257` makes
  `Height + 1` return `None` when the result exceeds `Height::MAX`.

Current storage bounds that make the max-height panic non-reachable in normal
current deployments:

- `zebra-state/src/service/finalized_state/disk_format/block.rs:29-36`
  documents the current 3-byte on-disk height limit and defines
  `MAX_ON_DISK_HEIGHT = 16_777_215`, far below `Height::MAX`.
- `zebra-state/src/service/finalized_state/zebra_db.rs:354-365` performs a
  quick database validity check that errors once the tip height exceeds half of
  `MAX_ON_DISK_HEIGHT`.

Finite-float assumptions:

- `zebra-rpc/src/methods.rs:1038-1057` estimates chain height and computes
  `verificationprogress` as `f64::from(tip_height.0) / f64::from(height.0)`.
  If both values are zero, this produces `NaN`.
- `zebra-rpc/src/config/rpc.rs:61-62` describes `debug_force_finished_sync` as a
  test-only option that makes Zebra say it is at the chain tip regardless of the
  estimate, and `zebra-rpc/src/config/rpc.rs:86-89` disables it by default.
- `zebra-rpc/src/methods.rs:4690-4705` computes displayed difficulty by shifting
  the target difficulty and expected difficulty right by 128 bits, converting to
  `f64`, and returning `pow_limit / difficulty`. Very small custom difficulty
  limits can make the shifted denominator zero, yielding `inf` or `NaN`.

## Reachability Notes

The next-height panics require a live chain tip exactly at `Height::MAX`. That
is not reachable through an RPC parameter. It would require internal/synthetic
state or future storage support for heights far above the current 3-byte
finalized database limit.

The `verificationprogress` zero-denominator case requires a chain-tip watcher at
height zero and a height estimate that is also forced to zero. Plausible examples
are operator-local conditions such as `debug_force_finished_sync = true` at
genesis on a non-regtest network, or a local clock earlier than the genesis tip
time. Regtest replaces progress with `1.0`.

The display-difficulty non-finite case requires non-default custom network
parameters with very small expanded difficulty values. Default mainnet/testnet
difficulty limits are far above the shifted-zero boundary.

## Impact

These are availability and response-correctness edges in RPC output
construction. They do not allow invalid blocks or transactions to pass
verification.

In ordinary default deployments:

- JSON-RPC is disabled by default and cookie-authenticated when enabled;
- current finalized storage bounds prevent a tip at `Height::MAX`;
- default network difficulty limits keep the display-difficulty denominator
  nonzero;
- `debug_force_finished_sync` defaults to `false`.

In custom/synthetic deployments, the likely failure mode is an RPC method error
or panic while constructing a response, depending on the exact boundary case and
build profile.

## Suggested Fix Direction

- Replace next-height `expect(...)` calls with explicit RPC errors or a stable
  "no next block" representation when the tip is `Height::MAX`.
- Compute `verificationprogress` with an explicit zero-height case, for example
  `0.0` when both actual and estimated heights are zero, and ensure the field is
  finite before serialization.
- In display difficulty, avoid losing the entire denominator by shifting before
  division. Use a wider fixed-point helper, or explicitly return an RPC error /
  clamped display value if the computed `f64` is not finite.
- Add focused tests for `Height::MAX` tip handling, genesis progress with
  `debug_force_finished_sync`, and custom difficulty values below `2^128`.

## Disclosure Triage

Local hardening. These are boundary-condition RPC availability issues for
non-default, synthetic, or future states, not private consensus vulnerabilities.

## Duplicate Check

Read-only duplicate search refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra RPC boundary arithmetic verificationprogress Height::MAX difficulty NaN'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "verificationprogress" "NaN"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "Height::MAX" "getblockchaininfo"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "getdifficulty" "NaN"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "getdifficulty" "Infinity"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "verificationprogress" "debug_force_finished_sync"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "valid chain tips are a lot less than Height::MAX"'
```

Relevant adjacent hits:

- closed #3143/#3891 implemented `getblockchaininfo`;
- closed #6081/#6099/#6105 implemented and revised `getdifficulty`; #6105
  documents the current high-128-bit division formula;
- closed #6330 is general height-difference refactoring provenance.

Those issues do not cover `Height::MAX` next-height response construction,
`verificationprogress` zero-height non-finiteness, or custom-difficulty
non-finite display values.

Sanity test run on 2026-05-09:

```sh
cargo test -p zebra-rpc rpc_getdifficulty --lib
```

Result: passed, 1 test. This covers ordinary `getdifficulty` behavior, not the
custom/boundary non-finite cases described here.

Focused boundary proof added and run:

```sh
cargo test -p zebra-rpc rpc_getdifficulty_custom_tiny_target_returns_nan_today --lib
```

Result on 2026-05-09: passed, 1 test. The test builds a custom Testnet with a
representable target difficulty limit of `1`, returns that same compact
difficulty from mocked `ChainInfo`, and confirms `chain_tip_difficulty()`
currently returns `NaN` because both shifted operands are zero before the final
floating-point division.

Confidence: high on the arithmetic shapes; low-medium on practical impact
because the dangerous preconditions are not remote-controlled in current
supported deployments.
