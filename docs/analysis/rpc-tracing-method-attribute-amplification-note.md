# RPC tracing method attribute amplification note

Date: 2026-05-03

Last updated: 2026-05-09

Disposition: local-only. Do not post publicly without explicit
re-authorization and a fresh duplicate check.

Scope: follow-up on the pass-5 observability sweep, focused on RPC tracing
span attributes and sibling Prometheus metric labels.

## Finding

Zebra's RPC tracing middleware copies the JSON-RPC method name from each request
and records it directly as the `rpc.method` span attribute. Unknown method names
are therefore attacker-influenced when an attacker can reach JSON-RPC. The
request body limit bounds the size of any single method string, but Zebra does
not normalize unknown methods to a bounded value before creating tracing spans.

This is a public observability hardening issue, not a consensus issue. It is
closely related to the Prometheus RPC method-label finding, but the sink is an
OpenTelemetry or Sentry trace/log export path rather than the in-process metrics
recorder.

## Evidence

- `zebra-rpc/src/server/rpc_tracing.rs:54-67` calls
  `request.method_name().to_owned()` and records it as `rpc.method = %method`
  on every `rpc_request` span.
- `zebra-rpc/src/server/rpc_metrics.rs:44-80` also calls
  `request.method_name().to_owned()` and records the raw method string in
  `rpc.requests.total`, `rpc.request.duration_seconds`, and `rpc.errors.total`
  labels.
- `zebra-rpc/src/server/rpc_metrics.rs` test
  `rpc_metrics_method_label_uses_raw_unknown_method_today` installs a local
  metrics recorder, sends two different unknown JSON-RPC method names through
  `RpcMetricsMiddleware`, and confirms both unknown names are used directly as
  `method` label values on the request counter and duration histogram.
- `zebra-rpc/src/server/rpc_tracing.rs` test
  `rpc_tracing_method_attribute_uses_raw_unknown_method_today` installs a local
  tracing subscriber layer, sends two different unknown JSON-RPC method names
  through `RpcTracingMiddleware`, and confirms both unknown names are used
  directly as `rpc.method` span attributes.
- `zebra-rpc/src/server.rs:138-142` installs `RpcTracingMiddleware` in the RPC
  middleware stack after `RpcMetricsMiddleware`.
- `zebra-rpc/src/server.rs:123-125` sets a whole HTTP request body cap of
  `2 * MAX_BLOCK_BYTES + 1024`.
- `zebra-rpc/src/server/http_request_compatibility.rs:127-136` enforces that
  cap with `http_body_util::Limited` before JSON-RPC parsing.
- `zebrad/src/sentry.rs:137-140` maps WARN/ERROR/INFO tracing levels into
  Sentry exports when `SENTRY_DSN` is configured. OpenTelemetry export is also
  runtime-gated by `tracing.opentelemetry_endpoint` or
  `OTEL_EXPORTER_OTLP_ENDPOINT`.

The body cap is an important mitigation: this is not an unlimited single-span
payload bug. The remaining issue is cardinality and payload churn across many
distinct unknown method names in deployments where RPC and telemetry export are
both reachable/configured.

## Impact

An RPC caller can send many invalid requests with unique method names. Each one
can create a distinct `rpc.method` attribute value in span/log data, increasing:

- in-process tracing allocation work,
- OpenTelemetry export payload size and backend cardinality,
- Sentry log/breadcrumb attribute diversity if the span context is included in
  exported events.

Default Zebra posture limits exposure: JSON-RPC is disabled by default, cookie
authentication is enabled by default when RPC is configured, and off-host
telemetry export requires explicit runtime configuration. The risk is mainly
for copied Docker/observability deployments or shared services that expose RPC
and tracing backends together.

## Duplicate Check

Read-only GitHub searches refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "RPC tracing" "method attribute"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "rpc.method" OpenTelemetry cardinality'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "RPC method" "metrics cardinality"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "unknown RPC method" "Prometheus label"'
```

Results:

- `RPC tracing` / `method attribute` returned closed PR #10174, which is
  provenance for adding OpenTelemetry RPC spans and the `rpc.method` attribute,
  not a hardening issue.
- The `rpc.method` OpenTelemetry cardinality search returned no hits.
- The RPC method metrics-cardinality and unknown-method Prometheus-label
  searches returned #10551, where a comment already names the RPC `method`
  label as a sibling metric-label surface to check. That partially covers the
  Prometheus-label side, but not the tracing attribute / telemetry-export side
  of this note.

## Local Proof Status

Prometheus metric-label side: test-backed. The local recorder test
`rpc_metrics_method_label_uses_raw_unknown_method_today` confirms distinct
unknown method names become distinct `method` label values today.

Tracing attribute side: test-backed. The local subscriber-layer test
`rpc_tracing_method_attribute_uses_raw_unknown_method_today` confirms distinct
unknown method names become distinct `rpc.method` span attribute values today.

Verification refreshed on 2026-05-09:

```sh
cargo test -p zebra-rpc rpc_metrics_method_label_uses_raw_unknown_method_today --lib
cargo test -p zebra-rpc rpc_tracing_method_attribute_uses_raw_unknown_method_today --lib
```

Result: passed.

## Suggested fix direction

- Normalize `rpc.method` before metrics and tracing, using the known method name
  for registered methods and `unknown` or `invalid` for everything else.
- Optionally record a bounded method-name length bucket for unknown requests
  rather than the raw string.
- Keep raw unknown method names out of Prometheus labels and exported tracing
  attributes unless explicitly enabled for short-lived local debugging.
- Add a small regression test that sends two different unknown method names and
  asserts both metrics/tracing classification paths use the same bounded
  fallback value.

## Disclosure triage

Public hardening issue.

Confidence: high that raw RPC method names reach both Prometheus metric labels
and tracing attributes. Practical impact is medium-low because it requires
reachable RPC plus configured metrics, telemetry export, or local trace
collection.
