# Indexer Idle Stream Disconnect Retention Note

Date: 2026-05-03

Last updated: 2026-05-09

Disposition: local-only. Do not post publicly without explicit
re-authorization and a fresh duplicate check.

## Summary

The optional indexer gRPC streaming methods spawn one task per subscriber, but
those tasks only detect a dropped client stream when the next source event makes
them attempt a send into the response channel. If a client opens a stream and
disconnects while the node is idle, the per-stream task can remain parked until
the next chain-tip, non-finalized-state, or mempool event. For
`NonFinalizedStateChange`, the parked RPC task also keeps a state-side
`NonFinalizedBlocksListener` task alive.

An adjacent stream-robustness edge exists in `MempoolChange`: broadcast lag is
treated the same as channel closure and terminates the subscriber stream with
`Unavailable`, while the internal transaction-gossip subscriber logs lag and
keeps running.

This is public indexer availability hardening, not private disclosure. The
indexer server is disabled by default and documented as unsafe to expose
publicly, but these stream-lifecycle shapes should be fixed before indexer
ports are treated as shared-network services.

## Evidence

- `zebra-rpc/src/indexer/server.rs:57-61` starts a plain tonic server with the
  indexer service and reflection, without an auth interceptor or explicit
  stream/subscriber limit.
- `zebra-rpc/src/indexer/methods.rs:36-81` implements `ChainTipChange` by
  creating an mpsc response channel, spawning a task, waiting on
  `chain_tip_change.best_tip_changed().await`, and only then calling
  `response_sender.try_send(...)`.
- `zebra-rpc/src/indexer/methods.rs:84-151` implements
  `NonFinalizedStateChange` by creating an mpsc response channel, spawning a
  task, obtaining a fresh `ReadRequest::NonFinalizedBlocksListener`, and then
  waiting on `non_finalized_state_change.recv().await` before calling
  `response_sender.try_send(...)`.
- `zebra-rpc/src/indexer/methods.rs:154-213` implements `MempoolChange` with
  the same shape: subscribe, wait on `mempool_change.recv().await`, then try to
  send the next message.
- `zebra-rpc/src/indexer/methods.rs` does not use
  `response_sender.closed().await`, `response_sender.is_closed()`, or a
  `tokio::select!` branch that races the source event against client
  disconnect.
- `zebra-rpc/src/indexer/methods.rs:163-165` uses
  `while let Ok(change) = mempool_change.recv().await`, so
  `broadcast::RecvError::Lagged(_)` exits the loop, logs
  `"mempool_change channel has closed"`, and sends terminal
  `Status::unavailable`.
- `zebrad/src/components/mempool.rs:282-285` creates the mempool change
  broadcast channel with capacity `gossip::MAX_CHANGES_BEFORE_SEND * 2`.
- `zebrad/src/components/mempool/gossip.rs:55-68` handles
  `RecvError::Lagged(skip_count)` by logging dropped transactions and
  continuing, treating only `RecvError::Closed` as terminal.
- `zebra-state/src/service.rs:1314-1327` creates a new
  `NonFinalizedBlocksListener` for each `ReadRequest::NonFinalizedBlocksListener`.
- `zebra-state/src/response.rs:219-279` shows that each listener has its own
  spawned task and mpsc channel. The listener task exits when its receiver is
  closed, but the RPC streaming task keeps that receiver alive while it waits
  for the next state-side event.

## Duplicate Check

Read-only GitHub searches refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "indexer idle stream disconnect retention"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "NonFinalizedStateChange" "subscriber amplification"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "indexer gRPC" "stream limit" "subscriber"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "mempool change" "broadcast lag" "indexer"'
```

Result: no hits returned.

## Local Proof Status

The idle-disconnect shape now has focused local proofs for all three stream
methods:

- `zebra-rpc/src/indexer/tests/vectors.rs`
- `dropped_chain_tip_change_stream_retains_task_until_tip_event_today`
- `dropped_mempool_change_stream_retains_subscription_today`
- `dropped_non_finalized_state_stream_retains_state_listener_today`
- `lagged_mempool_change_stream_ends_as_unavailable_today`

The chain-tip test opens a `ChainTipChange` stream, waits for the spawned
server-side stream task to start, drops the client response stream, and confirms
the task remains active while no tip event has occurred. It then sends a tip
event and confirms the task exits once the event lets it observe the closed
response channel.

The mempool test opens a `MempoolChange` stream, waits for the spawned stream
task to subscribe to the mempool broadcast channel, drops the client response
stream, and confirms the broadcast sender still has one receiver after a short
yield. That proves the disconnected stream does not promptly release its
server-side mempool subscription while idle.

The non-finalized-state test opens a `NonFinalizedStateChange` stream, answers
the state service's `NonFinalizedBlocksListener` request with a test listener,
drops the client response stream, and confirms the state listener receiver is
still alive after a short yield. That proves the disconnected stream does not
promptly release its state-side listener while idle.

The lag test opens a `MempoolChange` stream, waits for the spawned stream task to
subscribe to the capacity-one test broadcast channel, sends two changes without
yielding, and confirms the stream exits with `Code::Unavailable` and the same
`mempool_change channel has closed` status text used for upstream channel
closure. That proves lag is terminal for indexer clients today.

Verification refreshed on 2026-05-09:

```sh
cargo test -p zebra-rpc dropped_chain_tip_change_stream_retains_task_until_tip_event_today --lib
cargo test -p zebra-rpc dropped_mempool_change_stream_retains_subscription_today --lib
cargo test -p zebra-rpc dropped_non_finalized_state_stream_retains_state_listener_today --lib
cargo test -p zebra-rpc lagged_mempool_change_stream_ends_as_unavailable_today --lib
```

Result: passed.

Adjacent tests also passed earlier on 2026-05-09:

- `indexer_accepts_many_unauthenticated_streams_today` proves the server accepts
  many unauthenticated stream subscribers.
- `non_finalized_state_streams_request_one_listener_each_today` proves each
  `NonFinalizedStateChange` subscriber requests a separate state-side listener.

Broadcast-lag behavior is now proof-backed for the indexer stream.

## Impact

If the indexer gRPC port is reachable by untrusted clients, an attacker can
open many server-streaming RPCs and disconnect before any source event occurs.
Each disconnected-but-idle stream can retain:

- one spawned indexer RPC task for all three stream methods;
- one cloned `ChainTip` watcher, mempool broadcast receiver, or state-side
  listener receiver depending on the method;
- for `NonFinalizedStateChange`, one additional state-side listener task and
  its 1,000-item state-side channel until a non-finalized-state event lets the
  RPC task observe the closed response channel.

The retained work is eventually released when the relevant source event fires
and the next `try_send()` sees the closed response receiver. That bounds
duration on active nodes, but a quiet node or quiet stream type can retain many
parked tasks longer than expected. This composes with the broader missing
indexer auth/concurrency/stream limits documented separately.

For `MempoolChange`, a subscriber that falls behind the broadcast channel also
loses the stream completely rather than receiving a gap signal and continuing.
That is not a node DoS by itself, but it weakens indexer reliability under
bursty mempool conditions and gives clients a less robust feed than the internal
gossip task.

## Suggested Fix Direction

- In each stream task, use `tokio::select!` to race source-event waits against
  `response_sender.closed()`.
- For `NonFinalizedStateChange`, avoid creating a state-side listener until the
  client stream is known to still be open, and drop it immediately if
  `response_sender.closed()` fires.
- Add explicit indexer subscriber/connection limits in tonic server
  configuration.
- Handle `broadcast::RecvError::Lagged(skip_count)` in `MempoolChange` by
  sending a gap/unavailable warning if the wire API supports it, or logging and
  continuing like the internal gossip task.
- Add regression tests that create each stream, drop the response stream before
  any source event, assert the server-side task/listener is released without
  waiting for a chain or mempool event, and assert broadcast lag does not look
  like upstream channel closure.

## Triage

Public availability hardening.

Confidence: high for chain-tip task retention, mempool-change subscription
retention, non-finalized-state listener retention after client stream drop, and
mempool-change lag being terminal for indexer clients. Practical severity
remains medium because the indexer server is opt-in and event cadence bounds
idle retention on active nodes.
