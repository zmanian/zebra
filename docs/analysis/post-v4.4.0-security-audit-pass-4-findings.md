# Post-v4.4.0 security audit pass 4 findings

Date: 2026-05-02

## Summary

This pass focused on waiter lifecycle and cleanup for state `AwaitUtxo` and
mempool `AwaitOutput` requests. The highest-signal result is a mempool
contract/retention mismatch: `AwaitOutput` says outdated requests are pruned
regularly, but the implementation only prunes abandoned pending-output waiters
on specific storage cleanup paths.

No cross-outpoint wakeup or stale-value reuse issue was found. Both state and
mempool waiters are keyed by exact transparent outpoint, and fulfillment removes
the sender before broadcasting.

## Finding 1: Mempool `AwaitOutput` abandoned waits are not pruned regularly

Status: confirmed availability / memory-retention hardening gap.

Impact: abandoned waits for distinct never-created outpoints can remain in
`pending_outputs` until an unrelated transaction-removal cleanup, full mempool
clear, mempool disable/drop, or shutdown. Duplicate waits for the same outpoint
share one map entry, so this is bounded by distinct missing outpoints rather
than by request count.

Evidence:

- `zebra-node-services/src/mempool.rs:58` to
  `zebra-node-services/src/mempool.rs:64` documents `AwaitOutput` as requiring
  timeout wrapping and says outdated requests are pruned on a regular basis.
- `zebrad/src/components/mempool/pending_outputs.rs:15` stores pending waits in
  a `HashMap<transparent::OutPoint, broadcast::Sender<transparent::Output>>`.
- `zebrad/src/components/mempool/pending_outputs.rs:20` to
  `zebrad/src/components/mempool/pending_outputs.rs:31` creates or reuses one
  sender per outpoint and subscribes a receiver for each waiter.
- `zebrad/src/components/mempool/pending_outputs.rs:46` to
  `zebrad/src/components/mempool/pending_outputs.rs:52` removes the sender only
  when the output is found and `respond()` is called.
- `zebrad/src/components/mempool/pending_outputs.rs:57` to
  `zebrad/src/components/mempool/pending_outputs.rs:63` has `prune()` and
  `clear()` primitives, but no timer or internal bound.
- `zebrad/src/components/mempool.rs:820` to
  `zebrad/src/components/mempool.rs:831` handles `Request::AwaitOutput` by
  queueing first, responding immediately only if `storage.created_output()`
  already has the outpoint, then returning the wait future.
- `zebrad/src/components/mempool/storage/verified_set.rs:172` to
  `zebrad/src/components/mempool/storage/verified_set.rs:178` calls
  `pending_outputs.respond()` only when a verified transaction is inserted and
  creates the requested output.
- `zebrad/src/components/mempool/storage.rs:607` prunes pending outputs from
  the transaction-removal path, and
  `zebrad/src/components/mempool/storage.rs:617` to
  `zebrad/src/components/mempool/storage.rs:621` clears them during full
  storage clear.
- `zebrad/src/components/mempool/queue_checker.rs:59` to
  `zebrad/src/components/mempool/queue_checker.rs:76` periodically sends
  `CheckForVerifiedTransactions`, but that path does not call
  `pending_outputs.prune()`.

Why it matters:

- If the caller times out or drops an `AwaitOutput` future for an outpoint that
  is never created, `receiver_count()` becomes zero, but the sender map entry
  stays until some later code explicitly calls `prune()` or `clear()`.
- The public service docs promise regular pruning. The implementation appears
  opportunistic rather than regular.
- This is availability-oriented, not consensus-critical. It does not cause a
  false transaction acceptance or rejection by itself.

Minimal regression test:

- Add a small test-only `len()` helper for `PendingOutputs`, mirroring
  `PendingUtxos::len()`.
- Enable the mempool and call `Request::AwaitOutput` for a unique nonexistent
  outpoint.
- Wrap the future in a short timeout and drop it.
- Drive `CheckForVerifiedTransactions` or let `QueueChecker` cadence pass.
- Assert `pending_outputs.len()` remains `1` under current behavior.
- Trigger a path that calls `storage.reject_and_remove_same_effects()` or
  `storage.clear()`, then assert the stale entry is removed.

Recommended fix:

- Add regular `pending_outputs.prune()` to the mempool poll path, likely near
  the existing `CheckForVerifiedTransactions` cleanup work.
- Consider adding an internal size metric for pending output waiters.
- Keep the current exact-outpoint fan-out behavior; the bug is cleanup cadence,
  not response routing.

## Finding 2: State `AwaitUtxo` read-path hits leave stale entries until prune

Status: confirmed low-severity retention gap.

Impact: a state `AwaitUtxo` request can successfully resolve via
`ReadRequest::AnyChainUtxo` while leaving behind a now-dead pending sender until
the state service's periodic prune runs. Dropped or timed-out missing-UTXO waits
also rely on the same prune cadence.

Evidence:

- `zebra-state/src/lib.rs:5` to `zebra-state/src/lib.rs:9` documents that
  `AwaitUtxo` and commit requests must be timeout-wrapped because they can hang.
- `zebra-state/src/request.rs:938` to `zebra-state/src/request.rs:945`
  repeats that `AwaitUtxo` requests should be timeout-wrapped and says outdated
  requests are pruned regularly.
- `zebra-state/src/service/pending_utxos.rs:12` stores pending waits in a
  `HashMap<transparent::OutPoint, broadcast::Sender<transparent::Utxo>>`.
- `zebra-state/src/service/pending_utxos.rs:17` to
  `zebra-state/src/service/pending_utxos.rs:28` queues the sender before
  resolution work.
- `zebra-state/src/service/pending_utxos.rs:43` to
  `zebra-state/src/service/pending_utxos.rs:49` removes the sender only on
  `respond()`.
- `zebra-state/src/service/pending_utxos.rs:65` to
  `zebra-state/src/service/pending_utxos.rs:71` exposes prune and length
  behavior.
- `zebra-state/src/service.rs:1096` to `zebra-state/src/service.rs:1103`
  queues the `AwaitUtxo` response future before checking queued, sent, or read
  state.
- `zebra-state/src/service.rs:1107` to `zebra-state/src/service.rs:1123`
  removes the pending entry promptly for queued or sent non-finalized hits by
  calling `pending_utxos.respond()`.
- `zebra-state/src/service.rs:1137` to `zebra-state/src/service.rs:1158`
  returns `Ok(Response::Utxo(utxo))` directly when `ReadRequest::AnyChainUtxo`
  finds the output, without removing the sender entry.
- `zebra-state/src/service.rs:1142` to `zebra-state/src/service.rs:1153`
  has a TODO noting that responding all waiting requests from that path is not
  implemented.
- `zebra-state/src/service.rs:953` to `zebra-state/src/service.rs:965` prunes
  stale UTXO waiters from `poll_ready()` every `PRUNE_INTERVAL`, which is
  defined as 30 seconds at `zebra-state/src/service.rs:290`.

Why it matters:

- The stale-entry period is bounded under continued state-service activity, but
  an idle service can retain entries longer than the request future lifetime.
- The same queue-first behavior means dropped or timed-out missing-UTXO waiters
  rely on later `poll_ready()` activity to be pruned.
- This is lower severity than the mempool case because there is a documented
  periodic prune path and an existing `len()` hook that makes the behavior easy
  to test.

Minimal regression test:

- Seed state with a UTXO that is readable by `ReadRequest::AnyChainUtxo` but
  not found in queued/sent non-finalized paths.
- Call `Request::AwaitUtxo` for that outpoint and await success.
- Assert `pending_utxos.len()` remains elevated until prune runs.
- Force prune by advancing time or making `last_prune` old enough and polling
  service readiness, then assert the entry is removed.

Recommended fix:

- If practical, remove the pending sender on read-path success, or complete all
  same-outpoint waiters from that path as the existing TODO suggests.
- Keep the 30-second prune as a backstop for dropped or timed-out missing-UTXO
  waits.

## Eliminated lead: no cross-outpoint wakeup or stale-value reuse found

Status: eliminated as a correctness bug in this pass.

Evidence:

- State waiters are keyed by exact outpoint at
  `zebra-state/src/service/pending_utxos.rs:12`.
- Mempool waiters are keyed by exact outpoint at
  `zebrad/src/components/mempool/pending_outputs.rs:15`.
- State `respond()` removes the sender before sending at
  `zebra-state/src/service/pending_utxos.rs:43` to
  `zebra-state/src/service/pending_utxos.rs:49`.
- Mempool `respond()` removes the sender before sending at
  `zebrad/src/components/mempool/pending_outputs.rs:46` to
  `zebrad/src/components/mempool/pending_outputs.rs:52`.
- Mempool's immediate-hit `AwaitOutput` path queues, checks exact
  `created_output(&outpoint)`, then responds for the same outpoint at
  `zebrad/src/components/mempool.rs:820` to
  `zebrad/src/components/mempool.rs:831`.
- Mempool's verified-set insert path constructs exact output outpoints and
  responds only for those outpoints at
  `zebrad/src/components/mempool/storage/verified_set.rs:172` to
  `zebrad/src/components/mempool/storage/verified_set.rs:178`.

Conclusion:

- Same-outpoint fan-out is intentional.
- Different outpoints should not wake each other.
- Removing the sender before broadcasting prevents a later waiter from
  subscribing to a sender that already contains a buffered old value.

## Eliminated lead: waiter abandonment does not directly poison rejection caches

Status: eliminated as a direct rejection-cache bug in this pass.

Evidence:

- `zebrad/src/components/mempool/downloads.rs:412` to
  `zebrad/src/components/mempool/downloads.rs:450` shows cancellation and
  timeout paths for transaction verification; these can prevent output
  production, but they do not themselves insert a verified transaction or call
  `pending_outputs.respond()`.
- `zebrad/src/components/mempool/storage.rs:845` to
  `zebrad/src/components/mempool/storage.rs:869` only caches
  `TransactionDownloadVerifyError::Invalid` as failed verification.
- The previous pass already separately documents that some infrastructure
  errors can be misclassified as `Invalid`; this pass found no extra path where
  merely abandoning an `AwaitOutput` waiter causes a rejection-cache entry.

Conclusion:

- Waiter abandonment is a resource-retention issue.
- Rejection-cache poisoning remains the pass-3 verifier-taxonomy issue, not a
  direct property of pending-output cleanup.

## Prioritized next fixes

1. Add periodic `pending_outputs.prune()` and a test-only pending-output count.
2. Add a regression showing abandoned `AwaitOutput` waits are pruned by regular
   mempool polling.
3. Tighten state `AwaitUtxo` read-path cleanup so successful read hits do not
   leave stale senders until prune.
4. Add paired isolation tests for same-outpoint fan-out and different-outpoint
   non-wakeup behavior.
