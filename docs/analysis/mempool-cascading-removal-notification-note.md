# Mempool Cascading Removal Notification Note

Date: 2026-05-03

Last updated: 2026-05-09

Disposition: local-only. Do not post publicly without explicit
re-authorization and a fresh duplicate check.

## Finding

Mempool dependency removals can remove more transactions than the caller reports
or rejects. The sharpest shape is insertion-time ZIP-401 eviction:

1. a new transaction is verified and inserted,
2. the mempool is now above `tx_cost_limit`,
3. random eviction chooses one of the new transaction's unmined ancestors,
4. `VerifiedSet::remove()` removes that ancestor and all direct or indirect
   dependents, including the newly inserted transaction,
5. `Storage::insert()` still returns `Ok(new_txid)` because the selected
   eviction victim was the ancestor, not the newly inserted dependent, and
6. the mempool service publishes `MempoolChange::added(new_txid)` for a
   transaction that has already been removed from storage.

This is not a consensus bug. It is mempool consistency and availability
hardening around dependency-chain eviction, RPC/indexer notifications, and
transaction re-download churn.

## Preconditions

- Zebra's mempool is enabled.
- An attacker can get a dependency chain of otherwise valid transactions into
  the mempool.
- Inserting another dependent transaction pushes total mempool cost above
  `tx_cost_limit`.
- ZIP-401 random eviction selects an ancestor of the newly inserted transaction,
  not the new transaction itself.

The random selection makes this probabilistic, but the attacker can increase the
chance by shaping transaction costs and can retry while the mempool is near its
cost limit.

## Evidence

`TransactionDependencies::remove_all()` walks from a removed transaction through
all direct and indirect dependents and returns the dependent hashes:

- `zebra-node-services/src/mempool/transaction_dependencies.rs:84-118`

`VerifiedSet::remove()` removes those dependents plus the requested key from the
stored transaction map:

- `zebrad/src/components/mempool/storage/verified_set.rs:288-310`

`VerifiedSet::evict_one()` calls `remove(key_to_remove)` but returns only the
randomly selected transaction, dropping any removed dependents from the return
value:

- `zebrad/src/components/mempool/storage/verified_set.rs:218-238`

`Storage::insert()` inserts the verified transaction first, then repeatedly
calls `evict_one()` while the pool is above `tx_cost_limit`. It records only the
returned victim as `RandomlyEvicted`, and only changes the insertion result to
an error if that victim is the newly inserted transaction:

- `zebrad/src/components/mempool/storage.rs:430-449`
- `zebrad/src/components/mempool/storage.rs:463-495`

The mempool service treats `Ok(inserted_id)` as an added transaction and later
broadcasts `MempoolChange::added(send_to_peers_ids)`:

- `zebrad/src/components/mempool.rs:615-631`
- `zebrad/src/components/mempool.rs:718-727`

`MempoolChange` says its `tx_ids` are the affected transaction IDs for a change,
and `invalidated()` is the change kind for transactions invalidated or rejected
from the mempool:

- `zebra-node-services/src/mempool/mempool_change.rs:25-42`
- `zebra-node-services/src/mempool/mempool_change.rs:71-78`

A similar dependent-loss shape exists in expiry cleanup. `remove_expired_transactions()`
collects only transactions whose own expiry height has been reached, then calls
`remove_all_that()`, which can remove non-expired dependents. It returns and
notifies only the originally expired transaction IDs:

- `zebrad/src/components/mempool/storage.rs:882-912`
- `zebrad/src/components/mempool.rs:698-715`

## Duplicate Check

Read-only GitHub searches refreshed on 2026-05-09:

```sh
gh issue list --repo ZcashFoundation/zebra --search 'mempool cascading removal notification in:title,body' --state all --limit 100
gh issue list --repo ZcashFoundation/zebra --search 'VerifiedSet evict_one dependents in:title,body' --state all --limit 100
gh issue list --repo ZcashFoundation/zebra --search 'MempoolChange added evicted dependent in:title,body' --state all --limit 100
gh issue list --repo ZcashFoundation/zebra --search 'ZIP-401 eviction dependent transaction notification in:title,body' --state all --limit 100
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "remove_expired_transactions" "dependents"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "expired_parent_removes_unreported_non_expired_dependent_today"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "MempoolChange" "dependents" "expired"'
```

Results: no hits.

## Local Verification

Deterministic expiry-side current-behavior proof added and run on 2026-05-09:

```sh
cargo test -p zebrad expired_parent_removes_unreported_non_expired_dependent_today --lib
```

Result: passed. The test inserts an expiring parent and a non-expired dependent
that is recorded as spending the parent's mempool output. When
`remove_expired_transactions(Height(1))` runs, storage removes both
transactions, but the returned expired-ID set contains only the parent.

The insertion-time ZIP-401 false-added case is now proof-backed as of
2026-05-09:

```sh
cargo test -p zebrad evicted_parent_reports_dependent_inserted_today --lib
```

Result: passed. The test adds a test-only eviction-key hook, inserts a parent,
lowers the mempool cost limit, then inserts a dependent while forcing ZIP-401
eviction to select the parent. The hook only replaces random victim selection;
the real `Storage::insert()` eviction loop still calls through
`VerifiedSet::evict_one()` and `VerifiedSet::remove()`. That loop returns
`Ok(child_id)`, while the parent eviction cascades and removes the child from
storage. The selected parent is cached as `RandomlyEvicted`; the removed child
is neither stored nor cached as rejected.

## Impact

Expected impact is bounded consistency and availability degradation:

- peers or local subscribers can receive an "added" event for a transaction that
  is no longer in the mempool;
- RPC/indexer clients that track mempool state from `MempoolChange` streams can
  miss dependent invalidations and temporarily diverge from `FullTransactions`;
- the gossip task can announce a transaction ID that Zebra no longer serves from
  its mempool;
- removed dependents are not cached as randomly evicted unless they were the
  selected victim, so reintroduced descendants can consume more download,
  verification, or missing-output rejection work before being suppressed by
  other caches.

The impact is bounded by mempool cost limits, download concurrency, normal
rejection caches, and the fact that all affected transactions were already
verified or still fail normal verification. It does not let an attacker create
invalid blocks or make Zebra accept invalid transactions.

## Suggested Fix

- Change `VerifiedSet::evict_one()` to return the selected victim and the full
  set of removed dependents.
- Have `Storage::insert()` reject or otherwise report every transaction removed
  by eviction, not just the selected victim.
- After insertion-time eviction, only return `Ok(unmined_tx_id)` if the newly
  inserted transaction is still present in `verified`.
- Make expiry cleanup return every transaction removed by cascading dependency
  removal, not just transactions whose own expiry height was reached.
- Add regression coverage for a parent/child pair where inserting the child can
  evict the parent and cascade-remove the child. The test should assert that the
  child is not reported as added unless it remains in storage.

## Disclosure Posture

Treat as public hardening.

This is a mempool notification and retry-churn issue, not a consensus split or
node-crash issue. It becomes operationally more relevant for deployments using
Zebra's mempool change stream for indexer state or transaction relay decisions.

## Confidence

Confidence: medium-high on the code path and medium-low on practical severity.

The source path is direct, and both concrete removal-reporting shapes now have
focused tests. The insertion-time case still depends in production on random
eviction choosing an ancestor at the right time, but the post-selection storage
behavior is proof-backed. The expiry dependent-loss case is deterministic once a
non-expired dependent hangs off an expired ancestor. Both remain RPC/indexer
consistency issues rather than consensus failures.
