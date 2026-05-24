# Indexer MempoolChange privacy note

Date: 2026-05-03

Last updated: 2026-05-09

Disposition: local-only. Do not post publicly without explicit
re-authorization and a fresh duplicate check.

## Summary

The optional indexer gRPC method `MempoolChange` streams every verified mempool
change to each subscriber, including the mined transaction hash and the V5
authorization digest. The indexer server is feature-gated and disabled by
default, but when operators enable `rpc.indexer_listen_addr` and expose it to a
shared network, an unauthenticated client can observe the node's local mempool
change feed in real time.

This is not a consensus issue and does not affect block acceptance. It is a
privacy and deployment-hardening issue for indexer-enabled nodes, especially
nodes used as infrastructure for wallets, lightwalletd-style services, or other
systems where local mempool contents and timing are sensitive.

## Duplicate Check

Read-only GitHub searches refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "MempoolChange" "auth_digest" "privacy"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "indexer" "mempool timing" "privacy"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "UnminedTxId" "auth digest" "indexer"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "mempool change" "gRPC" "privacy"'
```

Result:

- The exact `MempoolChange` / `auth_digest` searches returned no hits.
- #10405, closed, is related broad prior art. It proposed RPC method access
  groups, says the gRPC indexer auth gap should be addressed, and calls out
  mempool timing feeds. It does not specifically document V5 authorization
  digest exposure through `MempoolChange`.

## Evidence

- `zebrad/Cargo.toml:52-63` keeps the `indexer` feature outside
  `default-release-binaries`.
- `zebra-rpc/src/config/rpc.rs:33-47` documents `indexer_listen_addr`, disables
  it by default, and warns that public binding lets anyone query node state.
- `zebrad/src/commands/start.rs:277-287` starts the indexer server whenever
  `config.rpc.indexer_listen_addr` is set, passing the live
  `mempool_transaction_subscriber`.
- `zebra-rpc/src/indexer/server.rs:57-61` builds a plain tonic server with
  reflection and `IndexerServer::new(indexer_service)`, without authentication
  or TLS.
- `zebra-rpc/proto/indexer.proto:25-45` defines `MempoolChangeMessage` with
  `change_type`, `tx_hash`, and `auth_digest`.
- `zebra-rpc/proto/indexer.proto:55-56` exposes `MempoolChange(Empty)` as a
  server-streaming RPC.
- `zebra-rpc/src/indexer/methods.rs:154-182` subscribes each RPC caller to the
  mempool-change broadcast channel and sends:
  - the change kind,
  - `tx_id.mined_id()`, and
  - `tx_id.auth_digest()` when present.
- `zebra-chain/src/transaction/unmined.rs:93-108` models V5 unmined
  transactions as `UnminedTxId::Witnessed(WtxId)`.
- `zebra-chain/src/transaction/unmined.rs:170-216` documents that the mined ID
  alone does not uniquely identify unmined V5 transactions and that
  `auth_digest()` returns the digest of authorizing data for V5 transactions.
- `zebra-chain/src/transaction/unmined.rs:111-130` deliberately redacts
  `UnminedTxId` in `Debug`/`Display`, with a comment that logging unmined
  transaction IDs can leak sensitive user information.
- `zebra-rpc/src/indexer/tests/vectors.rs` now has a local current-behavior
  proof, `mempool_change_stream_exposes_v5_auth_digest_today`, showing the
  streamed protobuf contains the V5 authorization digest bytes.

Focused proof rerun on 2026-05-09:

```sh
cargo test -p zebra-rpc mempool_change_stream_exposes_v5_auth_digest_today --lib
```

Result: passed.

## Impact

If the indexer gRPC port is exposed to untrusted clients, an observer can learn:

- which transaction effects enter, leave, or get invalidated in this node's
  mempool;
- timing information for local transaction propagation and mining/removal;
- V5 authorization digests for transactions where Zebra otherwise treats the
  unmined witnessed identifier as sensitive enough to redact in logs.

The stream does not reveal full transaction bytes by itself, and public network
gossip already leaks many transactions eventually. The sharper concern is local
vantage-point privacy: this feed can reveal what this specific node saw and when
it saw it, including transactions that may be invalidated or not broadly
propagated.

## Existing mitigations

- The indexer server is not part of default release features.
- `rpc.indexer_listen_addr` defaults to `None`.
- The config docs warn operators not to bind the indexer RPC port publicly.
- Each stream uses a bounded 64-message response channel and drops slow
  consumers, so this note is distinct from the existing streaming availability
  finding.

## Suggested fix direction

- Keep `MempoolChange` behind localhost-only or authenticated deployments.
- Document that `MempoolChange` exposes local mempool contents, timing, and V5
  authorization digests, not just generic node state.
- Consider splitting the API into a lower-privacy change notification stream and
  a privileged detailed stream.
- If remote indexer access is expected, add authentication/TLS and per-client
  authorization before exposing mempool-change data.
- Consider making `auth_digest` optional behind a capability or config flag if
  consumers only need mined transaction hashes.

## Disclosure triage

Public hardening. This is an opt-in, feature-gated indexer API, and Zebra's
config already warns that public indexer binding exposes node state. It is worth
calling out because the current warning does not make the mempool timing and V5
authorization-digest exposure explicit.

Confidence: high on the data exposed by the stream; medium on practical impact,
because it depends on indexer-enabled deployments exposing the port beyond a
trusted local client.
