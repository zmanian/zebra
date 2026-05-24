# RPC Solution-Rate Window Bounds Note

Date: 2026-05-02

Last updated: 2026-05-09

Scope: follow-up on RPC parameter bounds and state-query amplification after
the post-v4.4.0 security pass.

Disposition: local-only known follow-up. Do not post publicly without explicit
re-authorization and a fresh duplicate check.

## Finding

`getnetworksolps` and its deprecated alias `getnetworkhashps` accept a
caller-supplied `num_blocks` value and do not impose a method-level maximum.
Positive values are converted from `i32` to `usize` and forwarded to the state
service. The state service then iterates ancestor block headers until it has
taken `num_blocks + 1` headers or reaches genesis.

In normal use this method defaults to 120 blocks, and zero or negative inputs
use the 17-block proof-of-work averaging window. But an RPC caller can pass
`2_147_483_647`, which makes Zebra attempt a full-chain solution-rate scan from
the selected height. That is bounded by the current chain length and by RPC
authentication/exposure defaults, but it is still a caller-controlled CPU and
database-read amplifier on configured RPC endpoints.

This is not a novel finding. The same broad issue was previously tracked in
ZcashFoundation/zebra#6688, and PR #7647 fixed the original worst-case behavior
by switching the scan from full blocks to headers. The remaining local concern
is the explicit range-limit TODO that PR #7647 left out of scope.

## Duplicate Check

Read-only GitHub searches refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "getnetworksolps" "num_blocks" "limit"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "SolutionRate" "num_blocks" "RPC"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "getnetworkhashps" "window bounds"'
gh api repos/ZcashFoundation/zebra/issues/6688
```

Result:

- #6688, closed, "`getnetworksolps` & `getnetworkhashps` RPCs hang with large
  num_blocks", is a direct prior report.
- #6688 notes that large `num_blocks` could hang RPCs and says a range limit
  was a possible quick fix.
- PR #7647, merged 2023-10-11, fixed the high-cost full-block-read behavior and
  explicitly says it "doesn't limit the height range".
- #7403 tracked longer-term optimization by caching difficulties and times, and
  is also closed.
- No distinct open issue was found for an explicit post-#7647 request-count or
  `num_blocks` cap.

## Evidence

- `zebra-rpc/src/methods.rs` documents `getnetworksolps` as estimating network
  solutions per second over the last `num_blocks` before `height`.
- `zebra-rpc/src/methods.rs` parses `num_blocks: Option<i32>`, defaults `None`
  to 120, maps values below 1 to `POW_AVERAGING_WINDOW`, then converts any
  positive `i32` to `usize` without an upper cap.
- `getnetworkhashps` directly calls `get_network_sol_ps()` with the same
  parameters, so it shares the same bound behavior.
- `zebra-state/src/service.rs` receives `ReadRequest::SolutionRate {
  num_blocks, height }`, chooses the requested start hash or tip hash, and calls
  `read::difficulty::solution_rate(...)`.
- `zebra-state/src/service/read/difficulty.rs` builds an
  `any_chain_ancestor_iter::<block::Header>(...)` and applies
  `.take(num_blocks.checked_add(1).unwrap_or(num_blocks))`.
- `zebra-state/src/service/block_iter.rs` implements that iterator by reading a
  header by height on every `next()`, stepping backward until genesis or the
  `take()` limit.
- `zebra-rpc/src/methods/tests/vectors.rs` already exercises
  `Some(i32::MAX)` as an accepted `num_blocks` input in the small vector-state
  test.
- `zebra-rpc/src/methods/tests/vectors.rs` now has a local current-behavior
  proof, `rpc_getnetworksolps_forwards_i32_max_window_today`, showing
  `Some(i32::MAX)` is forwarded to state as
  `ReadRequest::SolutionRate { num_blocks: i32::MAX as usize, ... }`.
- `zebra-rpc/src/methods/tests/snapshot.rs` has a TODO to add tests for
  excessive `num_blocks` and `height`.

Focused proof rerun on 2026-05-09:

```sh
cargo test -p zebra-rpc rpc_getnetworksolps_forwards_i32_max_window_today --lib
```

Result: passed.

## Impact

This is not a consensus bug and does not create invalid block acceptance. The
impact is availability/operational:

- a single authenticated or exposed RPC caller can force an expensive full-chain
  header walk instead of the normal 120-block window,
- repeated calls can contend for RPC/state capacity and database cache,
- the deprecated `getnetworkhashps` alias doubles the reachable method surface,
- batch requests can amplify the same method if JSON-RPC batching remains
  enabled without a batch-count cap.

The practical impact is lower than P2P-facing issues because JSON-RPC is
disabled by default, cookie authentication is enabled by default, and jsonrpsee
has request/connection guards. It is still worth fixing because it is a small,
obvious bound and the intended operational window is already much smaller than
the accepted maximum.

## Suggested Fix

- Define a maximum solution-rate window for RPC, such as a multiple of the
  default window or a zcashd-compatible cap if one exists.
- Reject `num_blocks` values above that cap with a JSON-RPC invalid-parameter
  error before calling state.
- Apply the same cap to `getnetworkhashps`.
- Add regression tests for excessive `num_blocks`, including `i32::MAX`, and for
  high `height` values that are clamped to the tip.
- Consider documenting the maximum in the RPC docs if compatibility permits.

## Confidence

Confidence: high that the missing cap exists and that `i32::MAX` reaches the
state request path today. Confidence on practical severity is low because the
scan is now header-only after PR #7647, bounded by chain height, and RPC is
disabled/authenticated by default. This should be treated as a known,
low-severity RPC availability-hardening follow-up rather than a new report.
