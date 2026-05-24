# Mempool Tip Rejection Cache Thrash Note

Date: 2026-05-04

Last updated: 2026-05-09

Disposition: local-only. Do not post publicly without explicit
re-authorization and a fresh duplicate check.

## Summary

Zebra bounds the two tip-local mempool rejection maps by clearing the whole map
when it grows past `MAX_EVICTION_MEMORY_ENTRIES = 40_000`. This is memory-safe,
but it is a cache-thrash shape: enough unique tip-local rejected transactions can
erase the cache and make previously rejected transactions eligible for download
and verification again.

This is not a consensus issue and does not let invalid transactions enter the
mempool. It is public availability hardening for active mempool deployments.

## Evidence

- `zebrad/src/components/mempool/storage.rs:51` sets
  `MAX_EVICTION_MEMORY_ENTRIES` to 40,000.
- `zebrad/src/components/mempool/storage.rs:639-648` clears
  `tip_rejected_exact` or `tip_rejected_same_effects` entirely once the selected
  map length exceeds that limit.
- `zebrad/src/components/mempool/storage.rs:771-797` calls the limiter after
  every rejection insertion.
- `zebrad/src/components/mempool/storage.rs:805-821` uses those maps as the
  settled rejection cache checked by `should_download_or_verify()`.
- `zebrad/src/components/mempool/storage.rs:843-869` records verifier-invalid
  transaction results as exact-tip failed-verification rejections.
- Existing property tests assert the current clearing behavior:
  `reject_lists_are_limited_insert_conflict` and
  `reject_lists_are_limited_reject` both expect the tip-local rejection count to
  drop to zero after the limit is exceeded.

The chain-wide same-effects rejection cache uses `EvictionList` instead, so it
removes older entries while staying at the configured size. The all-or-nothing
clear is specific to the current-tip exact and current-tip same-effects maps.

## Duplicate Check

Read-only GitHub searches refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "mempool tip rejection cache clear"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "tip_rejected_exact" "MAX_EVICTION_MEMORY_ENTRIES"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "rejection cache thrash" mempool'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "exact_tip_rejection_cache_clear_makes_old_reject_retryable_today"'
```

Closest hit:

- #10559, open, is adjacent but not duplicate. It covers infrastructure
  failures being cached as exact-tip rejections; this note covers all-or-nothing
  cache clearing once the tip-local rejection maps exceed the memory cap.

The other searches returned no hits.

Focused current-behavior tests rerun on 2026-05-09:

```sh
cargo test -p zebrad exact_tip_rejection_cache_clear_makes_old_reject_retryable_today --lib
cargo test -p zebrad reject_lists_are_limited --lib
```

Result: passed. The focused test confirms that an exact-tip rejection suppresses
retry before the cache exceeds the cap, then becomes retry-eligible after enough
distinct exact-tip rejections clear the entire map. The broader property test
confirms current over-limit behavior for the tip-local rejection maps and the
chain-wide eviction list.

## Attacker Influence

Remote peers can influence these maps through the ordinary mempool queue path
when the mempool is active:

- invalid transactions that reach verifier consensus failure are stored under
  exact `UnminedTxId` in `tip_rejected_exact`;
- standardness failures are also cached as exact-tip rejections after the
  transaction has otherwise reached storage insertion;
- spend conflicts or missing mempool-created outputs are cached under mined ID
  in `tip_rejected_same_effects`.

An attacker who can cause more than 40,000 unique entries in one of the
tip-local maps before the next block can flush that map. After the flush, older
bad transaction IDs no longer hit the rejection cache, so repeated inv/push/RPC
submissions can make Zebra redo state lookups, downloads, script/proof
verification, or standardness checks that the cache was meant to suppress.

## Limits and Severity

Important mitigations:

- memory remains bounded;
- the mempool is active only near the tip;
- each accepted block clears tip-local rejections anyway;
- the attacker needs many unique rejected transaction identities before a tip
  change;
- normal peer, queue, and verifier limits still apply.

That makes this lower severity than unbounded memory growth. The practical risk
is repeated wasted work under sustained invalid-transaction churn, especially
combined with V5 same-effects variants where exact witnessed IDs can vary by
authorization digest.

## Suggested Fix Direction

- Replace whole-map clearing with bounded FIFO/LRU eviction, similar to
  `EvictionList`, for `tip_rejected_exact` and `tip_rejected_same_effects`.
- Preserve the current memory cap but drop only the oldest entry or oldest small
  batch when inserting over the limit.
- Add a regression test that over-limit tip-local rejection insertion keeps the
  newest rejection and at least one earlier unrelated rejection, rather than
  clearing the entire map.
- Track cache-hit and cache-eviction metrics separately so operators can see
  invalid-transaction churn without high-cardinality labels.

## Disclosure Triage

Public hardening / low-to-medium availability.

Confidence: high on the code shape and existing test coverage for current
clearing behavior. Practical exploitability is medium-low because the attacker
must generate many unique rejected transactions under one tip, and all normal
mempool admission limits still apply.
