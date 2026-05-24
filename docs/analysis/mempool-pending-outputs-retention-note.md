# Mempool PendingOutputs Retention Note

Date: 2026-05-02

Last updated: 2026-05-09

Disposition: local-only. Do not post publicly without explicit
re-authorization and a fresh duplicate check.

Scope: follow-up on the abandoned `AwaitOutput` waiter cleanup behavior in the
active mempool.

## Finding

Zebra's mempool keeps a `PendingOutputs` map from missing transparent outpoints
to broadcast senders. A mempool verifier that spends an output missing from the
best chain waits on `mempool::Request::AwaitOutput(outpoint)` for up to 60
seconds. If the wait times out or the verifier task is cancelled, the receiver is
dropped, but the sender entry remains in the map until an explicit cleanup path
runs.

This is an availability hardening issue, not a consensus divergence. It does not
make Zebra accept invalid blocks or transactions. It can let repeated orphan
mempool candidates grow small retained waiter entries over time when the mempool
is active.

## Code Paths

- `zebra-consensus/src/transaction.rs:64-71` sets
  `MEMPOOL_OUTPUT_LOOKUP_TIMEOUT` to 60 seconds.
- `zebra-consensus/src/transaction.rs:736-749` sends
  `mempool::Request::AwaitOutput(outpoint)` for missing best-chain transparent
  inputs and maps timeout to `TransactionError::TransparentInputNotFound`.
- `zebrad/src/components/mempool.rs:820-831` handles `Request::AwaitOutput` by
  calling `storage.pending_outputs.queue(outpoint)`.
- `zebrad/src/components/mempool/pending_outputs.rs:20-40` stores or reuses a
  `broadcast::Sender` keyed by exact `transparent::OutPoint`.
- `zebrad/src/components/mempool/pending_outputs.rs:55-59` removes stale senders
  only when `PendingOutputs::prune()` is called.
- `zebrad/src/components/mempool/storage.rs:607` calls `prune()` during
  mined/conflicting transaction cleanup on tip growth.
- `zebrad/src/components/mempool/storage.rs:615-620` clears pending outputs when
  the whole storage is cleared.
- `zebrad/src/components/mempool/storage/verified_set.rs:172-178` removes and
  notifies matching waiters when an output is inserted into the verified set.

No direct cleanup was found on verifier timeout, verifier cancellation, outer
download task timeout, `storage.reject_if_needed()`, or ordinary
`CheckForVerifiedTransactions` polling without a tip-grow cleanup event.

## Bounds And Practical Impact

Active verifier pressure is bounded:

- `zebrad/src/components/mempool/downloads.rs:105` caps inbound download/verify
  tasks at `MAX_INBOUND_CONCURRENCY = 25`.
- `zebrad/src/components/mempool/downloads.rs:412-414` wraps each task in
  `RATE_LIMIT_DELAY`.
- `zebrad/src/components/mempool/crawler.rs:84` sets that delay to 73 seconds.
- The verifier awaits missing mempool outputs serially, so the failure path
  effectively creates one unresolved waiter per verifier task before returning.

Retained stale sender growth is different from active pressure. The map is keyed
by unique outpoint, so duplicate requests for the same outpoint share one sender,
but unique missing outpoints can accumulate until `respond()`, `prune()`, or
`clear()` runs.

The practical bound is therefore closer to "unique missing outpoints since the
last matching output, prune event, or mempool clear" than to "currently active
verification tasks."

## Duplicate Check

Read-only GitHub searches refreshed on 2026-05-09:

```sh
gh issue list --repo ZcashFoundation/zebra --search 'PendingOutputs retention AwaitOutput in:title,body' --state all --limit 100
gh issue list --repo ZcashFoundation/zebra --search 'mempool pending outputs waiter prune in:title,body' --state all --limit 100
gh issue list --repo ZcashFoundation/zebra --search 'AwaitOutput timeout pending output in:title,body' --state all --limit 100
gh issue list --repo ZcashFoundation/zebra --search 'TransparentInputNotFound pending_outputs in:title,body' --state all --limit 100
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "PendingOutputs" "AwaitOutput"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "mempool_dropped_await_output_waiter_survives_poll_until_pruned_today"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "pending_outputs.prune"'
```

Results: no hits.

## Local Verification

Added focused tests in `zebrad/src/components/mempool/pending_outputs.rs`:

- `dropped_waiters_remain_until_pruned`
- `duplicate_outpoints_share_pending_sender`

Added a service-level current-behavior test in
`zebrad/src/components/mempool/tests/vector.rs`:

- `mempool_dropped_await_output_waiter_survives_poll_until_pruned_today`

Command:

```sh
cargo test -p zebrad pending_outputs --lib
cargo test -p zebrad mempool_dropped_await_output_waiter_survives_poll_until_pruned_today --lib
```

Result on 2026-05-09: both commands passed.

These tests confirm the retention primitive directly:

- dropping the waiter future does not remove the sender entry immediately,
- `prune()` removes the entry after all receivers are gone,
- duplicate waiters for the same outpoint share one sender and are removed only
  after the final receiver is dropped,
- an abandoned `Request::AwaitOutput` queued through the normal `Mempool`
  service remains after ordinary `CheckForVerifiedTransactions` polling, then
  disappears after an explicit `pending_outputs.prune()`.

## Suggested Fix Direction

Use the existing cleanup primitive more regularly:

- add a `Storage::prune_pending_outputs()` helper,
- call it from enabled `Mempool::poll_ready()` so stale entries are pruned by the
  normal queue-check cadence,
- optionally add global and per-outpoint waiter caps,
- consider exposing a low-cardinality internal metric for current pending-output
  waiter count.

With the queue checker polling every 5 seconds, pruning in `poll_ready()` would
bound stale sender lifetime to the next active mempool poll rather than the next
tip-growth cleanup event.
