# Mempool Metrics Full-Scan Amplification Note

Date: 2026-05-03

Last updated: 2026-05-09

Disposition: local-only. Do not post publicly without explicit
re-authorization and a fresh duplicate check.

## Summary

The verified mempool set recomputes several aggregate metrics by scanning every
stored transaction after each successful insert, clear, and removal. This makes
attacker-fed mempool growth more expensive than the indexed insertion path alone:
building a mempool of `n` accepted transactions performs an extra
`1 + 2 + ... + n` metric-accounting walk over stored transactions.

This is not a consensus issue and does not imply invalid transaction acceptance.
It is an availability hardening lead for nodes whose mempool is active and
reachable through P2P transaction relay or trusted RPC transaction submission.

## Evidence

- `zebrad/src/components/mempool/storage/verified_set.rs:148-190` inserts a
  verified transaction into the indexed mempool structures and then calls
  `self.update_metrics()`.
- `zebrad/src/components/mempool/storage/verified_set.rs:288-308` removes a
  transaction/dependent set and then calls `self.update_metrics()`.
- `zebrad/src/components/mempool/storage/verified_set.rs:373-428` recomputes
  unpaid-action and weighted-size buckets by iterating over
  `self.transactions().values()`.
- `zebrad/src/components/mempool/storage/verified_set.rs:430-483` then emits the
  computed gauges.
- `zebrad/src/components/mempool/storage/tests/vectors.rs:96-175` has focused
  current-behavior proofs that five successful inserts call `update_metrics()`
  five times and perform `1 + 2 + 3 + 4 + 5` transaction visits, and that
  removing those five independent transactions calls `update_metrics()` five
  more times and performs `4 + 3 + 2 + 1 + 0` visits over the shrinking set.
- `zebrad/src/components/mempool/config.rs:52-65` sets the default
  `tx_cost_limit` to 80,000,000.
- `zebra-chain/src/transaction/unmined.rs:60-67` sets the minimum mempool
  transaction cost to 10,000 bytes, so the default configuration allows on the
  order of 8,000 minimum-cost transactions before eviction pressure.

The scan happens before emitting the gauges, so the extra loop is paid even if
the metrics recorder is otherwise cheap or the Prometheus endpoint is not
publicly scraped.

## Duplicate Check

Read-only GitHub searches refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "mempool metrics" "full scan"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "zcash.mempool.actions.unpaid" update_metrics'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "verified mempool" update_metrics scan'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "mempool metrics" amplification'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra mempool update_metrics'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "zcash.mempool.size.weighted"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "VerifiedSet::update_metrics"'
```

Results:

- The full-scan, amplification, and exact metric searches returned no direct
  duplicate.
- `mempool update_metrics` returned unrelated connectivity issue #4649 and
  historical metrics PR #2860. #2860 is useful provenance because it explicitly
  asked whether tracking serialized size incrementally was worthwhile to avoid
  iterating the whole mempool.
- `zcash.mempool.size.weighted` returned #6972, the PR that added the current
  bucketed mempool action/weighted-size metrics. That is implementation
  provenance, not a hardening follow-up.

## Local Proof Status

Proof-backed for both successful-insert growth and independent-removal shrink
shapes. A local test-only counter inside `VerifiedSet::update_metrics()`
confirms that inserting five accepted transactions into an eviction-free
storage calls `update_metrics()` once per insert and visits
`1 + 2 + 3 + 4 + 5` stored transactions while the mempool grows. The sibling
removal proof removes the same five independent transactions through
`Storage::remove_exact()` and confirms five further metric recomputations over
`4 + 3 + 2 + 1 + 0` remaining transactions.

Verification refreshed on 2026-05-09:

```sh
cargo test -p zebrad mempool_insert_recomputes_metrics_over_growing_set_today --lib
cargo test -p zebrad mempool_remove_recomputes_metrics_over_shrinking_set_today --lib
```

Result: both passed.

## Impact

For the default mempool cost limit, filling the mempool with minimum-cost
accepted transactions implies roughly tens of millions of additional
per-transaction metric-accounting visits. That is bounded and much cheaper than
cryptographic verification, but it is avoidable work on the mempool hot path.

The same recomputation shape can also amplify removals. `remove_all_that()` can
remove many transactions, and each root removal goes through `remove()`, which
recomputes the full aggregate metrics after the removal. Blocks and reorgs bound
how many transactions can be removed at once, but the work still scales with the
remaining mempool size.

Existing mitigations:

- only accepted verified transactions reach the successful insert path;
- the mempool cost limit bounds the number of stored transactions;
- transaction verification, download, and mempool activation limits still apply;
- the default cost limit keeps the worst case finite.

Those mitigations make this a low/medium availability hardening issue rather
than an emergency. The important part is that metrics should not add an
avoidable full-mempool scan to every hot-path mutation.

## Suggested Fix Direction

- Maintain the metric bucket totals incrementally during insert/remove instead
  of recomputing them from the whole `HashMap`.
- Or throttle aggregate recomputation to a periodic task rather than doing it
  after every mutation.
- Keep O(1) gauges such as transaction count, serialized size, and total cost on
  the hot path, because those are already tracked incrementally.
- Add a focused regression or benchmark with a synthetic mempool near the
  default cost limit to catch accidental full-set scans on insert/remove.

Disclosure triage: public hardening.

Confidence: high on the O(n) recomputation code shape; medium-low on practical
severity because the mempool size is bounded and accepted transactions are not
free to produce.
