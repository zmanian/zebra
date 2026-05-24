# Halo2 Batch Queue Weight Admission Note

Date: 2026-05-09

Status: local-only. Do not post publicly without explicit re-authorization.

## Summary

`tower-batch-control` uses `RequestWeight` to decide when a batch is full, but
its admission semaphore reserves one permit per request. That is fine for the
unit-weight primitive verifiers. It is a weaker bound for Halo2, where one
request can contain many Orchard actions and
`halo2::Item::request_weight()` returns `bundle.actions().len()`.

The result is an availability/backpressure gap: a remote block, mempool
transaction, or RPC-submitted transaction with heavyweight Orchard bundles can
consume much more verifier work than the queue admission bound accounts for.
This does not make invalid transactions valid, and the work is still bounded by
transaction/block size, primitive verifier concurrency, and outer verifier
timeouts. It is a current resource-hardening issue rather than a private
consensus disclosure.

## Evidence

`tower-batch-control` documents `max_items_weight_in_batch` as the work budget
for a batch, but the queue semaphore capacity is
`max_items_weight_in_batch * max_batches_in_queue` permits:

- `tower-batch-control/src/service.rs:104-115`
- `tower-batch-control/src/service.rs:172-189`

Each successful `poll_ready()` acquires a single `OwnedSemaphorePermit`, and
`call()` attaches that single permit to the queued message:

- `tower-batch-control/src/service.rs:234-293`
- `tower-batch-control/src/message.rs:8-15`

The worker observes the dynamic request weight only after the request has
already been admitted:

- `tower-batch-control/src/worker.rs:145-153`
- `tower-batch-control/src/worker.rs:250-257`

The Halo2 verifier explicitly makes one item heavier when it has more Orchard
actions:

- `zebra-consensus/src/primitives/halo2.rs:34-35`
- `zebra-consensus/src/primitives/halo2.rs:60-67`
- `zebra-consensus/src/primitives/halo2.rs:140-144`

Therefore a queue that looks like at most `64 * max_batches_in_queue` requests
can represent far more than `64 * max_batches_in_queue` Orchard actions. A
single heavyweight item can also exceed the nominal batch size before the worker
gets a chance to flush it.

RepoPrompt's async/backpressure slice independently selected this as its
strongest fresh candidate.

## Current-Behavior Proof

Added focused test:

```sh
cargo test -p tower-batch-control weighted_requests_only_consume_one_queue_permit_today --test worker
```

Result on 2026-05-09: passed.

The test uses a synthetic `WeightedRequest` whose `request_weight()` equals the
configured `max_items_weight_in_batch`. With `max_items_weight_in_batch = 2`
and `max_batches = 1`, two full-weight requests both obtain readiness and enter
the queue before a third request is backpressured. If admission were weighted
against the same budget as flushing, the second full-weight request would not be
admitted while the first full-weight request still occupied the queue.

## Duplicate Check

Local docs mention Halo2 batch/fallback behavior in the primitive verifier
taxonomy, but that note focuses on correctness, fallback CPU, metrics drift, and
error taxonomy. It does not record the weight-insensitive queue admission shape:

- `docs/analysis/primitive-verifier-failure-taxonomy-note.md`

GitHub issue search on 2026-05-09 found no exact open duplicate:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "RequestWeight" "Halo2"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "batch" "Halo2" "queue"'
```

The second query returned older batch/performance history, including #4750,
#4752, #4789, #9308, and #10179, but not this admission-bound issue.

## Impact

Suggested severity: low-to-medium availability hardening.

The main security shape is verifier work amplification and queue unfairness
under attacker-influenced Orchard action counts. A hostile peer or RPC client
cannot bypass semantic verification through this path, but it can make the
primitive verifier queue admit more Halo2 work than the configured batch budget
suggests.

The practical bound is meaningful:

- transaction and block serialization limits cap input size;
- global primitive verifier concurrency and batch size cap execution fanout;
- block verification is wrapped in sync timeouts;
- mempool and RPC paths have their own admission and exposure constraints.

This still deserves hardening because the code already has a weight abstraction;
the admission path just does not use an equivalent bound.

## Suggested Fix Direction

- Extend `RequestWeight` with a request-type admission weight, for example a
  defaulted `queue_permit_weight()` associated function.
- Make `Batch::poll_ready()` reserve that many permits per queued request.
- Override the new queue admission weight for `halo2::Item`, conservatively
  treating one item as one full Halo2 batch.
- Keep `request_weight(&self)` for dynamic worker-side flush timing.
- Add a focused `tower-batch-control` test proving a high queue-permit-weight
  request type cannot obtain more ready permits than the queue budget allows.

## Confidence

Confidence is high that current admission is request-count based while Halo2
flush weight is action-count based; the focused `tower-batch-control` test now
exercises that behavior directly. Confidence is medium on operational impact
because the attacker still has to supply large validly encoded Orchard bundles,
and the amplification is bounded by normal transaction/block and verifier
limits.
