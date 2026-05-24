# Sync Concurrency Config Overflow Note

Date: 2026-05-09

Status: local-only. Do not post publicly without explicit user direction.

## Summary

Zebra lower-bounds sync concurrency configuration values, but does not
upper-bound them. Extremely large local values can flow into unchecked sizing
math in the syncer.

The concrete proof is `full_verify_concurrency_limit = usize::MAX`. When the
syncer is just crossing from checkpoint verification into full verification,
`ChainSync::lookahead_limit()` computes:

```rust
self.full_verify_concurrency_limit + checkpoint_hashes
```

In debug/test builds, this overflows and panics. The workspace dev profile uses
`panic = "abort"`, so this is process-fatal in that profile. In ordinary release
builds, Rust integer overflow checks are not enabled by default, so the same
expression is more likely to wrap and produce an unexpectedly small lookahead
limit.

## Evidence

The sync config exposes three `usize` concurrency knobs:

- `zebrad/src/components/sync.rs:238`
- `zebrad/src/components/sync.rs:260`
- `zebrad/src/components/sync.rs:266`

`ChainSync::new()` only raises values that are too small; it does not cap values
that are too large:

- `zebrad/src/components/sync.rs:440-469`

The crossing-boundary lookahead calculation adds the configured full verifier
limit to the number of checkpoint hashes after the configured checkpoint
boundary:

- `zebrad/src/components/sync.rs:1109-1124`

A nearby downloader path also performs unchecked arithmetic on the effective
lookahead limit before converting it to `HeightDiff`:

- `zebrad/src/components/sync/downloads.rs:402-411`

and, when there is no best tip yet, converts `lookahead_limit - 1` to `u32` with
`expect()`:

- `zebrad/src/components/sync/downloads.rs:417-421`

These source paths suggest the issue is a small family of unbounded sync sizing
math, not just a single test-only expression. A top-level oversized
`checkpoint_verify_concurrency_limit` or `full_verify_concurrency_limit` can
become the `Downloads::new(..., lookahead_limit, ...)` value through
`max(checkpoint_verify_concurrency_limit, full_verify_concurrency_limit)`.

Local proof tests added:

- `zebrad/src/components/sync/tests/vectors.rs`
- `huge_full_verify_concurrency_limit_can_overflow_lookahead_limit_today`
- `zebrad/src/components/sync/downloads.rs`
- `huge_lookahead_limit_can_panic_downloader_height_filter_today`

Verification:

```sh
cargo test -p zebrad huge_full_verify_concurrency_limit_can_overflow_lookahead_limit_today --lib
cargo test -p zebrad huge_lookahead_limit_can_panic_downloader_height_filter_today --lib
```

Result on 2026-05-09: both passed. The second is a lower-level downloader proof:
with `lookahead_limit = usize::MAX` and no best tip, the download task reaches
the `u32::try_from(lookahead_limit - 1).expect("fits in u32")` path and panics;
the stream then panics while unwrapping the task `JoinError`.

## Impact

Likely severity: low.

This requires local/deployment configuration control, not remote P2P or RPC
input. The practical impact is:

- debug/test/profile builds can abort from oversized sync config;
- release builds can wrap the sizing arithmetic and make the effective
  lookahead limit different from the operator's configured value;
- extremely large accepted config values can also drive oversized Tower
  concurrency limits, channels, or bookkeeping before the node reaches normal
  sync behavior.

This is best treated as configuration validation and resource-hardening debt,
not a private-disclosure-worthy vulnerability by itself.

## Recommended Fix Direction

- Add upper bounds for sync concurrency config values.
- Use checked or saturating arithmetic for derived lookahead limits.
- Treat overflow and impossible conversions as typed configuration errors, not
  `expect()`-guarded invariants.
- Add tests for too-small, normal, and too-large sync concurrency settings.

## Duplicate Check

Local search on 2026-05-09 did not find an existing dedicated note for sync
concurrency upper-bound overflow:

```sh
rg -n "checkpoint_verify_concurrency_limit|full_verify_concurrency_limit|download_concurrency_limit|lookahead_limit \\+|usize::MAX|too high|upper bound|fits in u32|fits in HeightDiff" docs/analysis zebrad/src/components/sync.rs zebrad/src/components/sync/downloads.rs zebrad/src/components/sync/tests -g '*.md' -g '*.rs'
```

Related existing notes discuss sync and block-download limits as security
bounds, but not this local oversized-config overflow shape.

## Confidence

High for the debug/test panic: it is backed by a direct local test.

Medium for the release-build behavior: it follows Rust's default overflow
semantics, but was not separately exercised in a release test.
