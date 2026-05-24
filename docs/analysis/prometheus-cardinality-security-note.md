# Prometheus Cardinality Security Note

Date: 2026-05-02

Scope: follow-up on the pass-5 observation that some Zebra metrics use
attacker-influenced values as Prometheus labels.

## Finding

Several metric call sites place remote-peer, invalid-transaction, or RPC-request
data directly into Prometheus labels. Prometheus creates a separate time series
for every unique metric name plus label set, so labels that an attacker can vary
can become an availability issue when metrics are enabled.

This is not a consensus bug and does not imply invalid block acceptance. It is
an operational resource-consumption risk for nodes that enable the Prometheus
metrics endpoint, especially if they accept inbound peer connections, run an
active mempool, or expose RPC with weak access controls.

## Plain-English Severity

The issue is that Zebra records some metrics using labels whose values come
from peers, invalid transactions, or RPC callers. Prometheus treats every new
label value as a separate time series and stores state for it. If an attacker
can cause many distinct values, they can make Zebra and/or the Prometheus stack
spend memory, CPU, disk, and scrape bandwidth on metrics that are not useful.

The three confirmed shapes have different attacker costs:

- In peer handshake and peer-message metrics, a remote peer can vary the
  user-agent string and create new transient address labels through connection
  churn. The user agent is length-limited and IPs are redacted in some labels,
  but the number of distinct label values is not normalized.
- In the mempool failure metric, an invalid transaction can produce a
  transaction-specific error string. Errors that include outpoints, hashes, or
  nullifiers are especially concerning because each crafted transaction can
  produce a fresh `reason` label.
- In RPC metrics, a caller can send arbitrary unknown JSON-RPC method names.
  Each distinct method name becomes a new `method` label if RPC is reachable.

This should be described as an observability-driven availability issue. It is
not expected to create coins, bypass consensus checks, or corrupt chain state.
It is lower severity in default Zebra deployments because metrics and RPC are
runtime-disabled by default, and RPC cookie authentication is enabled by
default when RPC is turned on. It becomes more important for public nodes,
copied Docker/observability deployments, shared RPC deployments, or operators
that expose metrics beyond localhost.

## Preconditions

- Zebra is compiled with Prometheus support. `zebrad` default release features
  include `prometheus` (`zebrad/Cargo.toml:52-58`, `zebrad/Cargo.toml:92`).
- Runtime metrics are enabled by configuring `metrics.endpoint_addr`; the
  default is `None` (`zebrad/src/components/metrics.rs:67-83`).
- The relevant surface is reachable:
  - peer surfaces require public or otherwise attacker-reachable P2P
    connectivity,
  - mempool surfaces require the mempool to be active and processing gossiped
    or pushed transactions,
  - RPC surfaces require RPC access with cookie auth disabled, bypassed, or
    legitimately available to the requester.
- Some shipped observability examples normalize wildcard metrics binding:
  `docker/observability/README.md:101-107` gives
  `ZEBRA_METRICS__ENDPOINT_ADDR=0.0.0.0:9999`,
  `docker/observability/prometheus/prometheus.yaml:24-31` scrapes `zebra:9999`,
  and `.github/workflows/test-docker.yml:145-148` tests the Prometheus feature
  with `0.0.0.0:9999`. These are not default runtime behavior, but they make
  copied deployments more likely to expose the metrics endpoint beyond
  localhost.

## Attacker-Influenced Label Paths

### Peer Handshake Labels

`zebra-network/src/peer/handshake.rs` records peer connection metrics with
labels including `remote_ip` and `user_agent`:

- `zcash.net.peers.obsolete`
- `zcash.net.peers.version.obsolete`
- `zcash.net.peers.connected`
- `zcash.net.peers.version.connected`

Evidence:

- `zebra-network/src/peer/handshake.rs:760-766` labels obsolete-version
  handshakes by `remote_ip`, `remote_version`, `min_version`, and
  `user_agent`.
- `zebra-network/src/peer/handshake.rs:770-774` labels the obsolete-version
  gauge by `remote_ip`.
- `zebra-network/src/peer/handshake.rs:799-806` labels successful handshakes by
  `remote_ip`, `remote_version`, `negotiated_version`, `min_version`, and
  `user_agent`.
- `zebra-network/src/peer/handshake.rs:810-814` labels the connected-version
  gauge by `remote_ip`.
- `zebra-network/src/protocol/external/codec.rs:518-531` enforces the
  256-byte user-agent decode limit.
- `zebra-network/src/meta_addr/peer_addr.rs:29-35` redacts the IP address but
  preserves the port in `PeerSocketAddr::Display`.

The user agent is limited to `MAX_USER_AGENT_LENGTH = 256`, but a remote peer
chooses its value. The label named `remote_ip` is generated from
`PeerSocketAddr::to_string()`, which redacts the IP address but preserves the
port, so it is still a transient socket label. A peer can repeatedly connect
with distinct user agents and source ports. Length limits and IP redaction bound
each label value, but not the number of distinct label values.

Local proof added on 2026-05-09:

```sh
cargo test -p zebra-network handshake_connected_metric_uses_remote_user_agent_label_today --lib
```

Result: passed. The test completes two in-memory `version`/`verack` handshakes
through `negotiate_version()`, with each remote peer choosing a distinct user
agent string, and confirms both strings are used directly as `user_agent` label
values on `zcash.net.peers.connected`.

The connection/message metrics also use a per-connection `addr` label:

- `zcash.net.in.messages`
- `zcash.net.out.messages`
- `zebra.net.in.errors`
- `zebra.net.in.requests`
- `zebra.net.out.requests`
- `zebra.net.in.responses`
- `zebra.net.out.responses`
- `zebra.net.connection.state`
- byte counters in the external codec

Evidence:

- `zebra-network/src/peer/handshake.rs:1016-1020` labels outbound message
  counters by command and transient address.
- `zebra-network/src/peer/handshake.rs:1053-1057` labels inbound message
  counters by command and transient address.
- `zebra-network/src/peer/handshake.rs:1066-1070` labels inbound decode errors
  by formatted error string and transient address.
- `zebra-network/src/peer/connection.rs:993-1013`,
  `zebra-network/src/peer/connection.rs:1385-1391`, and
  `zebra-network/src/peer/connection.rs:1456-1462` label request/response
  counters by command and per-connection address.
- `zebra-network/src/peer/connection.rs:1689-1696` labels connection-state
  gauges by command/state string and per-connection address.
- `zebra-network/src/protocol/external/codec.rs:128-131` and
  `zebra-network/src/protocol/external/codec.rs:400-402` label byte counters by
  per-connection address.

These labels are less directly controllable than user agent strings, because
they are derived from connection addresses rather than the claimed address in
the peer's version message. They still include transient port values, scale with
peer churn, and can create high-cardinality series in public peer deployments.

`zebra.net.in.errors` also uses `err.to_string()` as an `error` label for
message decode failures. Most serialization error strings are static categories,
but using the formatted error as a label makes this path depend on error-display
stability rather than an explicit bounded taxonomy.

Peer-cache metrics are another bounded peer-address label source:

- `zcash.net.peers.initial`
- `zcash.net.peers.cache`

The cache update path writes and labels actual `IP:port` strings by calling
`remove_socket_addr_privacy()` before emitting `remote_ip`. This is capped to
`MAX_PEER_DISK_CACHE_SIZE = 75` peers per cache write, and the address book is
also capped. But peer addresses can be learned from proactive `getaddr`
responses, so this remains a bounded cardinality and metrics-privacy hardening
item rather than an unbounded memory issue.

### Mempool Failure Labels

`zebrad/src/components/mempool.rs` records failed mempool verification with:

```rust
metrics::counter!(
    "mempool.failed.verify.tasks.total",
    "reason" => error.to_string(),
)
```

Evidence:

- `zebrad/src/components/mempool.rs:641-661` handles transaction
  download/verification failures and emits the metric with `reason =>
  error.to_string()`.
- `zebra-consensus/src/error.rs:79-98` includes the transaction hash in
  maximum-expiry and expired-transaction display strings.
- `zebra-consensus/src/error.rs:162-172` includes duplicate transparent
  outpoints and shielded nullifiers in duplicate-spend display strings.
- `zebra-consensus/src/error.rs:190-203` includes transparent outpoint and
  height details in immature coinbase-spend display strings.

This label is high risk for cardinality because `TransactionError` display
strings can include transaction-specific data. For example,
`DuplicateTransparentSpend(transparent::OutPoint)` includes the outpoint in the
error string. A transaction with two identical transparent inputs can fail quick
checks before state lookup, and each crafted outpoint can produce a distinct
label value.

This gives an attacker a plausible low-cost way to create one metric series per
distinct invalid transaction error string, subject to mempool activation,
download/verification throttles, and peer/request limits.

Local proof added on 2026-05-09:

```sh
cargo test -p zebrad mempool_failed_verify_metric_reason_uses_raw_transaction_error_today --lib
```

Result: passed. The test installs a local metrics recorder, queues two direct
pushed transactions through the normal mempool verifier path, makes the verifier
return two `DuplicateTransparentSpend` errors with distinct transparent
outpoints, and confirms both complete downloader error strings are used
directly as `reason` label values on
`mempool.failed.verify.tasks.total`.

### RPC Method Labels

`zebra-rpc/src/server/rpc_metrics.rs` records request and duration metrics with
the JSON-RPC method name as a label:

- `rpc.requests.total{method=...}`
- `rpc.request.duration_seconds{method=...}`
- `rpc.errors.total{method=..., error_code=...}`

Evidence:

- `zebra-rpc/src/server/rpc_metrics.rs:44-47` copies
  `request.method_name()` into an owned label value before dispatch.
- `zebra-rpc/src/server/rpc_metrics.rs:63-80` uses that value in request,
  duration, and error metrics.
- `zebra-rpc/src/server.rs:123-133` bounds the total HTTP request body before
  JSON-RPC parsing, so an individual unknown method name is size-bounded, but
  the number of distinct unknown method names is not normalized.
- `zebra-rpc/src/config/rpc.rs:13-31` documents that JSON-RPC is disabled by
  default, and `zebra-rpc/src/config/rpc.rs:66-97` enables cookie auth by
  default.

If RPC is reachable by an attacker, arbitrary unknown method names can produce
new label values. This is less exposed in default Zebra deployments because RPC
is disabled by default and cookie auth is enabled by default when RPC is
enabled.

The same middleware also increments `rpc.active_requests` before awaiting the
inner method future, then decrements it only after the future completes. If the
future is cancelled while the method is pending, for example because the client
disconnects, the async block is dropped before the decrement runs. That does not
create cardinality, but it can make the active-request gauge drift upward until
process restart. Use an RAII/drop guard for this gauge if the metric is meant to
remain reliable under cancellation.

Local proof added on 2026-05-09:

```sh
cargo test -p zebra-rpc rpc_active_requests_gauge_not_decremented_when_response_future_dropped_today --lib
```

Result: passed. The test uses a pending inner RPC service and a local recorder
that captures gauge operations. Current middleware increments
`rpc.active_requests` as soon as `call()` is invoked, but dropping the returned
pending response future does not emit the matching decrement.

### Lower-Risk Metric Families Checked

Several nearby metric families appear low-cardinality because they use fixed
variant names or bounded local enumerations:

- `state.requests` uses explicit `Request::variant_name()` and
  `ReadRequest::variant_name()` mappings rather than hashes or request
  arguments (`zebra-state/src/request.rs:1050-1082` and
  `zebra-state/src/request.rs:1440-1467`).
- sync and handshake duration labels use fixed result/reason sets, not
  formatted peer or block data
  (`zebrad/src/components/sync/downloads.rs:542-556` and
  `zebra-network/src/peer/handshake.rs:940-968`).
- mempool downloaded/pushed/verified transaction metrics label by transaction
  version, which is protocol-bounded rather than attacker-arbitrary
  (`zebrad/src/components/mempool/downloads.rs:359-398`).
- proof/signature batch histograms use fixed verifier and result labels, for
  example `zebra-consensus/src/primitives/sapling.rs:150-158`.
- RocksDB metrics label by column-family name and level number; both are local
  database/configuration values, not peer/RPC input
  (`zebra-state/src/service/finalized_state/disk_db.rs:648-677`).
- build-info labels are local package metadata, not network input
  (`zebrad/src/components/metrics.rs:21-36`).

## Impact

Expected impact is monitoring and node availability degradation, not consensus
divergence:

- larger in-process metrics recorder state,
- larger scrape responses,
- higher Prometheus memory/disk usage,
- slower or failing dashboard queries,
- noisy or unusable alerting around peer, mempool, and RPC metrics.

The shipped alert examples group RPC latency and error-rate alerts by `method`
(`docker/observability/prometheus/rules/zebra_alerts.yml:88-106`), so arbitrary
RPC method labels can also create unnecessary alert-query fan-out when RPC is
exposed to shared or untrusted callers.

Severity should be treated as public hardening unless a concrete deployment
profile shows that the in-process exporter can be exhausted remotely under
default or common settings.

## Suggested Fix

Keep metric labels low-cardinality and bounded:

- Replace `error.to_string()` labels with a stable error variant/category, for
  example `duplicate_transparent_spend`, `script`, `wrong_version`, or
  `timeout`.
- Replace arbitrary RPC method labels with known method names plus `unknown`.
- Remove peer `user_agent` from Prometheus labels, or map it to a small known
  client family set and `other`.
- Consider dropping per-peer `addr` labels from high-frequency message counters,
  or move per-peer details to logs/tracing rather than metrics.
- Use a drop guard for `rpc.active_requests` so cancellations and client
  disconnects decrement the gauge.
- Keep exact IP addresses, user agents, txids, outpoints, and full error strings
  in logs where needed, not in Prometheus labels.

## Confidence

Confidence: medium.

The code paths are direct and the Prometheus cardinality behavior is standard.
The remaining uncertainty is practical exploitability under real node configs:
metrics are runtime-disabled by default, RPC is disabled/authenticated by
default, and P2P/mempool paths have concurrency and rate limits. Even with those
limits, the pattern is worth fixing because it is cheap to harden and follows
Prometheus best practices.
