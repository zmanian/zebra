# RPC batch request hardening note

Date: 2026-05-02

Last updated: 2026-05-09

## Summary

Zebra starts the jsonrpsee HTTP server without setting a batch request policy,
so it inherits jsonrpsee's default `BatchRequestConfig::Unlimited` behavior.
Zebra does bound the total HTTP request body before allocation, and RPC is
disabled by default with cookie authentication enabled by default. This makes
the issue a public hardening lead rather than a private disclosure item.

The remaining risk is authenticated or intentionally exposed RPC work
amplification: one HTTP request can contain a very large number of JSON-RPC
calls, bounded by Zebra's roughly 4 MB request body limit but not by call count.
Each call is then parsed, dispatched, traced, and metered individually.

## Evidence

- `zebra-rpc/src/server.rs` builds the server with `.http_only()`,
  `.set_http_middleware(...)`, `.set_rpc_middleware(...)`, and
  `.max_response_body_size(...)`, but it does not call
  `.set_batch_request_config(...)`.
- Zebra's compatibility middleware bounds request bodies using
  `Limited::new(body, max_request_body_size)` before jsonrpsee sees the body.
  The configured limit is `(MAX_BLOCK_BYTES * 2) + 1024`; with
  `MAX_BLOCK_BYTES = 2_000_000`, this is 4,001,024 bytes.
- In jsonrpsee-server 0.24.10, the default server config sets
  `batch_requests_config: BatchRequestConfig::Unlimited` and
  `max_connections: 100`.
- jsonrpsee has a default 100-permit `ConnectionGuard`, but in Zebra's
  middleware composition that guard is acquired after Zebra's HTTP compatibility
  middleware has already authenticated, collected, and rewritten the request
  body. See `docs/analysis/rpc-pre-guard-http-connection-retention-note.md`.
  Zebra also calls `.http_only()`, so jsonrpsee subscription limits are not a
  meaningful part of this exposure.
- In jsonrpsee-server 0.24.10, batch handling parses the request body as
  `Vec<&JsonRawValue>`, checks only `batch.len() > max_len`, and for
  `Unlimited` uses `usize::MAX` as `max_len`.
- Batch calls are awaited sequentially, not spawned concurrently. This limits
  direct fan-out, but it still allows a single body to drive many method
  dispatches and many metrics/tracing events.
- Zebra's RPC metrics middleware labels requests by `request.method_name()`.
  A batch of many unknown method names therefore also amplifies the
  high-cardinality `method` label issue.

## Local Confidence Check

Source and dispatch evidence was rechecked on 2026-05-09:

- `zebra-rpc/src/server.rs:144-153` constructs the jsonrpsee server without
  calling `.set_batch_request_config(...)`.
- `zebra-rpc/src/server.rs:125-133` sets Zebra's compatibility-layer request
  body cap to `(MAX_BLOCK_BYTES * 2) + 1024`, currently 4,001,024 bytes.
- `Cargo.lock` pins `jsonrpsee-server` to `0.24.10`.
- Local dependency source for `jsonrpsee-server-0.24.10` has
  `ServerConfig::default().batch_requests_config = BatchRequestConfig::Unlimited`.
- The same dependency source maps `BatchRequestConfig::Unlimited` to
  `usize::MAX`, parses a batch as `Vec<&JsonRawValue>`, and rejects only when
  `batch.len() > max_len`.
- Added
  `zebra-rpc/src/server/tests/batch.rs::unlimited_batch_request_dispatches_every_call_today`,
  which sends a
  three-entry JSON-RPC batch through jsonrpsee's HTTP dispatcher with
  `BatchRequestConfig::Unlimited` and verifies all three entries reach the RPC
  service.

Focused verification:

```sh
cargo test -p zebra-rpc unlimited_batch_request_dispatches_every_call_today --lib
```

Result on 2026-05-09: passed.

I did not add a full Zebra-node HTTP server test for this item in this pass.
The new test proves the inherited jsonrpsee dispatch behavior directly; the
Zebra-specific source evidence proves Zebra does not override that default.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "RPC batch request limit"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "jsonrpsee" "BatchRequestConfig"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "RPC batch count cap"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "set_batch_request_config"'
```

No hits were returned.

## Impact

This is not a consensus divergence and is not enabled by default as an open
network service. The plausible impact is RPC availability and observability
pressure in deployments that expose RPC, disable cookie authentication, leak RPC
credentials, or intentionally share RPC access with semi-trusted clients.

Examples:

- a large batch of unknown method names causes many error responses and many
  `rpc.requests.total{method=...}` / `rpc.errors.total{method=...}` series;
- a large batch of cheap known methods repeatedly enters state/mempool read
  paths inside one HTTP request;
- a large batch containing expensive methods serializes work behind one
  connection, tying up one jsonrpsee connection slot and service capacity until
  the response limit or method behavior stops it.

The request body and response body limits are important mitigations. The
jsonrpsee connection guard is still a mitigation after the compatibility layer
has produced an inner JSON-RPC request, but it does not cover pre-guard body
collection or compatibility rewriting. The missing batch control is a direct cap
on the number of JSON-RPC calls per batch.

## Suggested fix direction

- Decide whether Zebra needs JSON-RPC batch request compatibility at all.
- If not required, configure jsonrpsee with `BatchRequestConfig::Disabled`.
- If required, configure a conservative `BatchRequestConfig::Limit(N)` and add
  a regression test that an oversized batch is rejected before method dispatch.
- Combine this with RPC metrics label normalization: known RPC method names can
  be labeled directly, while unknown methods should be recorded as `unknown`.

## Disclosure triage

Public hardening issue. It is useful to mention alongside the Prometheus
cardinality finding, but by itself it does not appear to require private
security disclosure.

Confidence: medium-high on the missing batch count cap and its interaction with
RPC method metrics; medium-low on practical exploitability because default RPC
configuration is conservative.
