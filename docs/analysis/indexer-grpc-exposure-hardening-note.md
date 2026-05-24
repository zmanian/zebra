# Indexer gRPC exposure hardening note

Date: 2026-05-02

Last updated: 2026-05-09

Disposition: local-only. Do not post publicly without explicit
re-authorization and a fresh duplicate check.

## Summary

Zebra's indexer gRPC server is disabled by default and must be explicitly enabled
with `rpc.indexer_listen_addr`. When enabled, the server exposes unauthenticated
tonic reflection and three server-streaming methods. The implementation does not
configure tonic's per-connection concurrency limit, request timeout, HTTP/2
stream limit, TLS, or an interceptor comparable to the cookie authentication used
by the JSON-RPC server.

This is not a consensus issue. It is an availability and data-exposure hardening
lead for deployments that bind the indexer RPC address to a public or shared
network.

## Duplicate Check

Read-only GitHub searches on 2026-05-07:

```sh
gh issue list --repo ZcashFoundation/zebra --search 'indexer gRPC auth stream limit in:title,body' --state all --limit 100
gh issue list --repo ZcashFoundation/zebra --search 'indexer_listen_addr public auth TLS in:title,body' --state all --limit 100
gh issue list --repo ZcashFoundation/zebra --search 'NonFinalizedStateChange stream subscriber limit in:title,body' --state all --limit 100
gh issue list --repo ZcashFoundation/zebra --search 'tonic reflection indexer unauthenticated in:title,body' --state all --limit 100
```

Result: no hits returned.

Fresh overlap check on 2026-05-09:

- `#10405` mentions that the gRPC indexer's auth gap should be addressed, but
  it was closed as too complex / not needed rather than fixed.
- `GHSA-826r-gfq8-x79q` reduced the gRPC stream buffer and changed slow
  subscribers to be dropped via `try_send()`. It does not add auth,
  connection/subscriber limits, or tonic server-level stream limits.
- Exact searches for `NonFinalizedStateChange subscriber limit` and indexer
  `max_concurrent_streams` did not find a dedicated open issue for this
  resource-limit shape.

## Evidence

- `zebra-rpc/src/config/rpc.rs` disables `indexer_listen_addr` by default and
  warns that binding the indexer RPC port to a public IP lets anyone query node
  state.
- `zebrad/src/commands/start.rs` starts the indexer server whenever
  `config.rpc.indexer_listen_addr` is set, using the same read-only state, chain
  tip, and mempool change handles used internally.
- `zebra-rpc/src/indexer/server.rs` builds the tonic server with plain
  `Server::builder()`, adds the reflection service, adds `IndexerServer`, and
  calls `serve_with_incoming(...)`.
- The same server builder call does not set `concurrency_limit_per_connection`,
  `load_shed`, `timeout`, `max_concurrent_streams`, TLS, or an authentication
  interceptor.
- The generated `IndexerServer` supports `max_decoding_message_size()` and
  `max_encoding_message_size()`, but Zebra uses `IndexerServer::new(...)`
  directly, leaving defaults in place. Tonic's default receive limit is 4 MiB and
  default send limit is effectively unlimited.
- `zebra-rpc/proto/indexer.proto` exposes three streaming methods:
  `ChainTipChange`, `NonFinalizedStateChange`, and `MempoolChange`.
- Each stream call in `zebra-rpc/src/indexer/methods.rs` creates a 64-message
  per-client response channel and spawns a task that stays alive until the client
  disconnects, the stream consumer falls behind, or the underlying watch/broadcast
  channel closes.
- `NonFinalizedStateChange` serializes full non-finalized blocks into response
  messages. The state-side listener has its own 1,000-item buffer of
  `Arc<Block>` references before the RPC method's 64-message serialized response
  buffer.
- `zebra-rpc/src/indexer/tests/vectors.rs` now has a local current-behavior
  proof, `indexer_accepts_many_unauthenticated_streams_today`, showing one
  unauthenticated client can open 32 streaming subscribers.
- `zebra-rpc/src/indexer/tests/vectors.rs` now also has
  `non_finalized_state_streams_request_one_listener_each_today`, showing each
  `NonFinalizedStateChange` subscriber requests its own state-side
  `NonFinalizedBlocksListener`.

Focused proof runs on 2026-05-09:

```sh
cargo test -p zebra-rpc indexer_accepts_many_unauthenticated_streams_today --lib
cargo test -p zebra-rpc non_finalized_state_streams_request_one_listener_each_today --lib
```

Result: both passed.

## Impact

If the indexer port is exposed to untrusted clients, an attacker can open many
long-lived streaming RPCs. Each stream consumes a spawned task and a bounded
response buffer. The non-finalized-state stream is the most expensive because
each queued response contains serialized block bytes, while the state-side
listener also maintains a block-reference buffer for that subscription.

Existing mitigations:

- the indexer server is disabled by default;
- the config example uses `127.0.0.1`;
- the config docs warn against binding the indexer RPC port to a public IP;
- per-stream response channels use `try_send()` and drop slow consumers once the
  64-message RPC buffer fills;
- tonic's default request decode limit bounds client request messages.

## Suggested fix direction

- Keep documenting the indexer as localhost-only unless it gains auth/TLS.
- Consider applying a tonic interceptor for auth, or explicitly require reverse
  proxy authentication for non-local binds.
- Set conservative `concurrency_limit_per_connection`,
  `max_concurrent_streams`, request timeout, and keepalive/connection-age values.
- Consider disabling reflection by default, or gate it behind a development
  option.
- Consider lowering or making configurable the per-client response buffer for
  full-block streams.

Disclosure triage: public hardening unless maintainers know of production
deployments that intentionally expose the indexer RPC server to untrusted
networks.

Confidence: high on missing server-level auth/concurrency/subscriber limits and
per-subscriber state-listener creation after the local proof tests; medium on
practical impact because the server is opt-in and documented as unsafe to bind
publicly.
