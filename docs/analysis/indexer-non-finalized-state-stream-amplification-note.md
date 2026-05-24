# Indexer non-finalized-state stream amplification note

Date: 2026-05-03

Last updated: 2026-05-09

Disposition: local-only. Do not post publicly without explicit
re-authorization and a fresh duplicate check.

## Summary

The optional indexer gRPC method `NonFinalizedStateChange` creates a dedicated
state-side listener task for every subscriber. Each listener clones the current
non-finalized state, diffs it against that subscriber's previous snapshot, and
queues full non-finalized blocks for that subscriber.

This means subscriber count multiplies state clone/diff work and full-block
serialization work on each non-finalized-state update. The indexer server is
disabled by default and documented as unsafe to expose publicly, so this is a
public availability hardening issue rather than a private consensus issue.

## Evidence

- `zebra-rpc/src/config/rpc.rs:33-47` shows that the indexer RPC server is
  disabled by default and requires `rpc.indexer_listen_addr`.
- `zebrad/src/commands/start.rs:277-287` starts the indexer server whenever
  `config.rpc.indexer_listen_addr` is set.
- `zebra-rpc/src/indexer/server.rs:57-61` uses plain `Server::builder()`,
  reflection, and `IndexerServer::new(...)` without a subscriber cap or tonic
  concurrency/stream limit.
- `zebra-rpc/proto/indexer.proto:48-56` exposes
  `NonFinalizedStateChange(Empty) returns (stream BlockAndHash)`.
- `zebra-rpc/src/indexer/methods.rs:84-99` spawns a per-RPC task and obtains a
  fresh `ReadRequest::NonFinalizedBlocksListener`.
- `zebra-state/src/service.rs:1314-1327` special-cases that read request by
  calling `NonFinalizedBlocksListener::spawn(...)`.
- `zebra-state/src/response.rs:219-279` creates a per-listener task with a
  1,000-item state-side channel, starts each listener from an empty previous
  state, clones the watched `NonFinalizedState`, walks chains, checks each
  candidate block against the previous snapshot, and sends unseen blocks.
- `zebra-state/src/service/watch_receiver.rs:113-115` confirms that
  `cloned_watch_data()` borrows and clones the watched data.
- `zebra-state/src/service/non_finalized_state.rs:101-114` shows that cloning
  `NonFinalizedState` clones the chain set and invalidated-block map. Most
  block data is behind `Arc`, but the top-level collections are still copied per
  listener per update.
- `zebra-rpc/src/indexer.rs:45-57` serializes each streamed block into bytes
  for the gRPC `BlockAndHash` response.
- `zebra-state/src/constants.rs:96-104` limits tracked non-finalized chain forks
  to 10, and `zebra-state/src/service/write.rs:439-443` finalizes once the best
  chain exceeds `MAX_BLOCK_REORG_HEIGHT`.

## Duplicate Check

Read-only GitHub searches on 2026-05-07:

```sh
gh issue list --repo ZcashFoundation/zebra --search 'indexer idle stream disconnect retention in:title,body' --state all --limit 100
gh issue list --repo ZcashFoundation/zebra --search 'NonFinalizedStateChange subscriber amplification in:title,body' --state all --limit 100
gh issue list --repo ZcashFoundation/zebra --search 'indexer gRPC stream limit subscriber in:title,body' --state all --limit 100
gh issue list --repo ZcashFoundation/zebra --search 'mempool change broadcast lag indexer in:title,body' --state all --limit 100
```

Result: no hits returned.

Fresh overlap check on 2026-05-09 found the closed broad auth-architecture issue
`#10405` and the prior stream-backpressure advisory `GHSA-826r-gfq8-x79q`, but
no dedicated open issue for per-subscriber `NonFinalizedStateChange`
listener/diff amplification or subscriber limits.

## Local Proof

`zebra-rpc/src/indexer/tests/vectors.rs` now has a current-behavior proof named
`non_finalized_state_streams_request_one_listener_each_today`. It opens eight
`NonFinalizedStateChange` streams and confirms the mock read-state service
receives eight independent `ReadRequest::NonFinalizedBlocksListener` requests.

Focused proof run on 2026-05-09:

```sh
cargo test -p zebra-rpc non_finalized_state_streams_request_one_listener_each_today --lib
```

Result: passed.

## Impact

If the indexer port is exposed to untrusted clients, an attacker can open many
`NonFinalizedStateChange` streams. Every live stream gets its own state-side
clone/diff loop. On each non-finalized-state change, work scales roughly with:

- number of subscribers,
- number of tracked non-finalized chains and blocks,
- number of unseen blocks per subscriber,
- serialized size of queued block responses.

A fresh subscriber starts with an empty previous state, so its first pass treats
the current non-finalized state as unseen and can queue a catch-up burst of
current non-finalized blocks. Slow clients are eventually dropped when the
64-message RPC response channel fills, but clients that read slowly enough to
avoid a full channel can keep the per-subscriber state listener alive.

Existing mitigations:

- the indexer feature/config is opt-in;
- the documented example uses localhost;
- non-finalized chains and depths are bounded;
- the state-side listener channel is bounded to 1,000 block references;
- the RPC stream channel is bounded to 64 serialized messages and uses
  `try_send()` to drop slow consumers.

Those bounds reduce impact, but they do not prevent subscriber-count
amplification because each subscriber still receives an independent
`NonFinalizedBlocksListener` task.

## Suggested fix direction

- Add explicit indexer-level connection/subscriber limits, especially for
  `NonFinalizedStateChange`.
- Prefer one shared non-finalized-state diff/broadcast task over one diff task
  per subscriber.
- Bound or make configurable the initial catch-up behavior for new
  non-finalized-state subscribers.
- Set tonic server limits such as `max_concurrent_streams`,
  `concurrency_limit_per_connection`, request timeout, keepalive, and load
  shedding.
- Keep the indexer documented as localhost-only unless auth/TLS and explicit
  resource limits are added.

Disclosure triage: public hardening unless maintainers know of production
deployments that expose the indexer RPC server to untrusted networks.

Confidence: high on per-subscriber state-listener creation after the local proof
test; medium on practical severity because the indexer is opt-in and the
non-finalized state is bounded.
