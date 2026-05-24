# RPC Pre-Guard HTTP Connection Retention Note

Date: 2026-05-03

Last updated: 2026-05-09

Scope: follow-up on RPC admission/auth ordering, request-body handling, and the
jsonrpsee connection guard after the pass-5 RPC batch and `text/plain` CSRF
notes.

## Summary

Zebra's RPC cookie-auth check happens before request-body collection, which
protects the default auth-enabled deployment from wrong-auth large-body work.
However, Zebra's HTTP compatibility middleware is installed outside the inner
jsonrpsee RPC service. In that composition, Zebra collects and rewrites the
request body before calling the inner service that acquires jsonrpsee's
`ConnectionGuard` permit.

That means jsonrpsee's default 100-permit guard is not a complete mitigation for
slow or large HTTP request bodies when RPC is intentionally exposed with cookie
auth disabled, or when a caller has valid credentials. The guard is acquired
after Zebra has already collected up to the compatibility middleware's
`max_request_body_size` and attempted JSON-RPC 1.0/2.0 rewriting.

There is also no apparent accept-level connection semaphore or request/header
timeout before Hyper starts serving an accepted RPC TCP stream. Clients that open
connections and send no complete HTTP request can occupy spawned connection
tasks before Zebra's auth middleware or jsonrpsee's inner request guard can run.

This is public RPC availability hardening. It does not bypass cookie auth, and
RPC is disabled by default.

## Evidence

Zebra configures the jsonrpsee server with custom HTTP middleware:

- `zebra-rpc/src/server.rs:123-147`

The compatibility middleware checks cookie credentials first:

- `zebra-rpc/src/server/http_request_compatibility.rs:234-240`

Then it rewrites missing or `text/plain` content types:

- `zebra-rpc/src/server/http_request_compatibility.rs:242-243`

For authenticated or auth-disabled requests, it collects the full request body
through `Limited::new(body, max_request_body_size)` before calling the inner
jsonrpsee service:

- `zebra-rpc/src/server/http_request_compatibility.rs:127-154`
- `zebra-rpc/src/server/http_request_compatibility.rs:245-252`

The configured Zebra compatibility limit is based on a hex-encoded full block:

- `zebra-rpc/src/server.rs:123-125`

jsonrpsee's own server default has a 100-connection guard:

- `jsonrpsee-server-0.24.10/src/server.rs:338-345`
- `jsonrpsee-server-0.24.10/src/future.rs:98-128`

But in the HTTP path, that guard is acquired inside `TowerServiceNoHttp::call()`:

- `jsonrpsee-server-0.24.10/src/server.rs:1030-1044`

Zebra's HTTP middleware wraps `TowerServiceNoHttp` before Hyper serves the
connection:

- `jsonrpsee-server-0.24.10/src/server.rs:1210-1223`

So for Zebra's compatibility middleware, body collection happens before the
inner `TowerServiceNoHttp::call()` reaches the guard.

At the accept layer, jsonrpsee accepts TCP connections and spawns a Hyper
connection task without first acquiring that `ConnectionGuard`:

- `jsonrpsee-server-0.24.10/src/server.rs:130-154`
- `jsonrpsee-server-0.24.10/src/server.rs:1224-1231`

## Local Confidence Check

Reran the direct middleware current-behavior test on 2026-05-09:

```sh
cargo test -p zebra-rpc pending_body_waits_before_inner_service_today --lib
```

Result: passed. The test builds an auth-disabled
`HttpRequestMiddleware`, sends a POST whose body never yields a frame, and
asserts that the middleware future remains pending while the mock inner RPC
service is not called. This proves the body-collection wait happens outside the
inner RPC service in Zebra's middleware layer.

This test does not model the jsonrpsee `ConnectionGuard` directly. The guard
ordering remains source-backed by the local `jsonrpsee-server-0.24.10` code:
`TowerServiceNoHttp::call()` acquires the guard, and Zebra's HTTP middleware
wraps that inner service before Hyper serves the connection.

Added a real-server boundary proof on 2026-05-09:

```sh
cargo test -p zebra-rpc rpc_server_incomplete_body_waits_before_dispatch_today --lib
```

Result: passed. The test starts the actual auth-disabled `RpcServer`, opens
three HTTP connections, sends complete valid JSON-RPC body bytes but declares a
larger `Content-Length`, then confirms each connection remains open without a
response while the mocked mempool, state, read-state, and block-verifier
services receive no requests. This raises confidence that the incomplete-body
retention exists at the real server boundary, not only in the direct middleware
unit test.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "RPC pre guard body collection"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "jsonrpsee connection guard" "RPC body"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "RPC body collection timeout" "connection"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "pending_body_waits_before_inner_service_today"'
```

No hits were returned.

## Impact

Likely severity: low-to-medium public availability hardening.

Preconditions:

- JSON-RPC is enabled.
- For request-body collection pressure, cookie auth is disabled or the caller has
  valid RPC credentials.
- For idle pre-request connection pressure, the attacker can reach the RPC TCP
  listener; cookie auth does not help because no request headers have arrived.

Potential effects:

- Many slow auth-disabled or valid-auth HTTP requests can accumulate body
  collection futures before jsonrpsee's inner 100-permit request guard is
  acquired.
- Each body is bounded by the Zebra compatibility limit, but aggregate memory and
  task pressure are not obviously bounded by jsonrpsee's guard at this layer.
- Many accepted TCP streams that do not complete request headers can retain
  spawned Hyper connection tasks until the peer disconnects or lower-level
  socket limits intervene.

The issue composes with public examples that disable cookie auth and bind RPC
broadly, and with the existing batch/method-amplification notes. It is not a
consensus issue and does not make unauthenticated large-body work possible under
the default auth-enabled configuration once request headers arrive.

## Suggested Fix Direction

- Add an outer Zebra-side connection semaphore and request/header/body read
  timeout around the HTTP middleware, before body collection and compatibility
  rewriting.
- Acquire an RPC admission permit before `request_to_json_rpc_2()` for
  auth-disabled or valid-auth requests, not only inside the inner jsonrpsee
  service.
- Do not rely only on `Server::builder().max_request_body_size(...)` unless a
  source/test check proves that it runs outside Zebra's HTTP middleware. In
  jsonrpsee-server 0.24.10, the HTTP request-size setting is passed to the inner
  HTTP handler after the middleware layer has been entered.
- Keep the existing early auth check so wrong-auth requests are rejected without
  collecting bodies.
- Add tests or an integration probe showing that oversized, slow, and headerless
  HTTP connections are bounded by the intended RPC connection/request limits.
- Update the batch-request note to avoid treating jsonrpsee's inner
  `ConnectionGuard` as covering all pre-dispatch Zebra compatibility work.

## Eliminated Leads

- Wrong-auth requests do not reach Zebra body collection:
  `HttpRequestMiddleware::call()` returns before `request_to_json_rpc_2()`.
- Wrong-auth requests do not reach RPC metrics, tracing, batch splitting, or
  method dispatch; those layers run only after the inner jsonrpsee service sees
  an admitted request.
- Authenticated oversized bodies are bounded by the existing
  `Limited::new(body, max_request_body_size)` path, which is covered by the
  current `oversized_request_body_is_rejected` unit test.
- The post-response compatibility rewrite remains bounded by jsonrpsee's
  response construction limit plus small constant-size JSON-RPC compatibility
  field changes; this is already covered by the response-construction sweep.

## Disclosure Triage

Public hardening.

This should not be private-disclosed unless a follow-up demonstrates unbounded
default-auth resource retention or a path where wrong-auth clients can still
force body collection.

## Confidence

Confidence: medium-high for the middleware ordering and missing outer guard from
source review. Confidence: medium on practical impact because OS limits, Hyper
behavior, deployment binding, and whether cookie auth is disabled determine the
real blast radius.
