# Metrics Endpoint Connection Hardening Note

Date: 2026-05-02

Last updated: 2026-05-09

Disposition: local-only. Do not post publicly without explicit
re-authorization and a fresh duplicate check.

Scope: follow-up on the Prometheus metrics findings, focused on the exporter
HTTP listener rather than metric label cardinality.

## Finding

Zebra's Prometheus metrics endpoint is disabled by default, but when
`metrics.endpoint_addr` is configured it uses
`metrics_exporter_prometheus::PrometheusBuilder::new().with_http_listener(addr).install()`
without Zebra-side allowlisting, connection limits, or request/header timeouts.

The upstream `metrics-exporter-prometheus` 0.16.2 HTTP listener accepts TCP
streams in a loop and spawns one task per stream with Hyper
`serve_connection()`. I did not find a semaphore or request/header timeout in
that listener. Its `idle_timeout()` builder option applies to stale metric
recency cleanup, not accepted HTTP connections. If the metrics endpoint is
bound to an attacker-reachable interface, slow or idle clients can hold exporter
connection tasks open. This compounds the separate high-cardinality-label
finding because an exposed scrape endpoint is also where large metric
registries are rendered.

## Evidence

- `zebrad/src/components/metrics.rs` enables the endpoint only when
  `config.endpoint_addr` is `Some`, then calls `PrometheusBuilder::new()`,
  `.with_http_listener(addr)`, and `.install()`.
- `zebrad/src/components/metrics.rs` does not call the builder's
  `add_allowed_address()` API, and does not wrap the exporter in an application
  connection semaphore or timeout.
- The config default for `metrics.endpoint_addr` is `None`, so this is opt-in.
- `book/src/user/metrics.md` recommends `127.0.0.1:9999` for normal use, which
  is the right deployment posture.
- `zebrad/src/commands/generate.rs` includes an environment-variable example
  with `ZEBRA_METRICS__ENDPOINT_ADDR=0.0.0.0:9999`, so it is easy for operators
  to bind the endpoint broadly.
- `Cargo.lock` pins `metrics-exporter-prometheus` 0.16.2.
- In `metrics-exporter-prometheus` 0.16.2, `HttpListeningExporter::serve_tcp()`
  accepts every stream and immediately calls `process_tcp_stream()`.
- `process_tcp_stream()` clones the metrics handle, builds a Hyper service, and
  `tokio::spawn`s `HyperHttpBuilder::new().serve_connection(...)` directly.
- That listener has an optional IP allowlist mechanism, but the default is
  allow-all and Zebra does not configure it.
- The dependency's `PrometheusBuilder::idle_timeout()` stores a metric recency
  timeout, which is passed into `Recency::new(...)` when the recorder is built;
  it is not a connection timeout around `serve_connection()`.

## Duplicate Check

Read-only GitHub searches refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "metrics endpoint" "connection timeout"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "Prometheus endpoint" "connection limit"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "metrics exporter" "slow client"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "with_http_listener" "add_allowed_address"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra Prometheus allowlist metrics endpoint'
```

Results:

- Exact phrase searches for the metrics endpoint, Prometheus connection limit,
  metrics exporter slow client, and `with_http_listener`/`add_allowed_address`
  returned no hits.
- The broader `Prometheus allowlist metrics endpoint` search also returned no
  hits.
- A broad unquoted `metrics endpoint connection timeout` search returned only
  unrelated release, sync, lightwalletd, and connectivity issues; none described
  the exporter listener connection-retention path.

## Local Proof Status

Partial current-behavior coverage added on 2026-05-09, with dependency-source
evidence. I did not add an in-process test through Zebra's production
`MetricsEndpoint::new()` path because it calls `PrometheusBuilder::install()`,
which mutates the process-global metrics recorder and can make tests
order-dependent. Instead, the focused proof uses the dependency's
`PrometheusBuilder::build()` path, which returns a recorder and exporter future
without installing the global recorder.

The proof test,
`prometheus_metrics_connection_waits_without_request_timeout_today`, builds a
local exporter, opens a TCP connection, sends no request bytes for 250 ms, then
sends a normal scrape request on the same connection and receives a successful
metrics response. This proves the current dependency listener keeps an idle
accepted connection open across that short request-header idle window. It does
not prove that no timeout exists at any larger duration, and the finding should
not be worded that way.

Local dependency-source inspection was also refreshed against the resolved
`metrics-exporter-prometheus` 0.16.2 source in the local cargo registry:

- `src/exporter/http_listener.rs` has `serve_tcp()` accept each TCP stream and
  immediately call `process_tcp_stream()`.
- `process_tcp_stream()` checks the optional allowlist, builds a Hyper service,
  and `tokio::spawn`s `HyperHttpBuilder::new().serve_connection(...)` directly.
- `check_tcp_allowed()` returns `true` when `allowed_addresses` is `None`.
- `src/exporter/builder.rs` defaults `allowed_addresses` to `None`.
- `PrometheusBuilder::idle_timeout()` configures metric recency cleanup, which
  is passed to `Recency::new(...)`; it is not a connection/request timeout.

A fully deterministic proof of "no timeout", "no connection cap", or "no
allowlist" would require Zebra to own the listener or expose explicit testable
timeout/cap configuration. Under the current dependency-owned listener, bounded
black-box waits should be treated as current-behavior evidence only.

## Impact

Expected impact is public operational availability hardening, not consensus:

- idle or slow TCP clients can consume exporter tasks and file descriptors,
- large metric registries can make scrapes more expensive,
- if high-cardinality labels are also present, scrape rendering and response
  size become easier to amplify.

The endpoint is disabled by default and the user docs show a localhost binding,
so this is not private-disclosure material. It becomes more relevant in
container, cloud, or orchestration setups where `0.0.0.0` binding is convenient.

## Suggested Fix

Public fix direction:

- keep documenting localhost-only binding as the safe default,
- add a warning near any `0.0.0.0` examples,
- configure `add_allowed_address()` when Zebra can infer a safe local allowlist,
- consider replacing the dependency-managed listener with a Zebra-owned listener
  that applies a connection semaphore and short request/header timeout,
- consider setting a metrics idle timeout for high-cardinality metric families if
  labels cannot be bounded immediately.

## Confidence

Confidence: medium-high on the missing connection and timeout guards in the
enabled endpoint path. Practical exploitability is medium-low because metrics
are runtime-disabled by default and usually bound to localhost.
