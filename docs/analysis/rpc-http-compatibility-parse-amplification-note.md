# RPC HTTP Compatibility Parse Amplification

Date: 2026-05-09

Scope: `zebra-rpc` HTTP compatibility middleware request/response body
normalization.

## Summary

Zebra's HTTP compatibility middleware parses and reserializes strict JSON-RPC
2.0 request and response bodies even when no legacy compatibility rewrite is
needed. The request is then passed to jsonrpsee as a rebuilt HTTP body, so the
normal RPC stack still has to parse it for dispatch.

This is a local-only RPC availability hardening issue. It is not a consensus
or private-disclosure candidate on current evidence because RPC is disabled or
authenticated by default and the work is bounded by configured request/response
body limits. It is still a useful cleanup target because the extra parse,
serialization, and body-copy work is deterministic for ordinary strict 2.0
traffic.

## Duplicate Boundary

This is distinct from:

- `rpc-pre-guard-http-connection-retention-note.md`, which covers request-body
  collection before jsonrpsee's inner connection guard;
- `rpc-batch-request-hardening-note.md`, which covers uncapped batch call count
  inside a bounded HTTP body.

This note covers post-auth, post-body-read compatibility normalization work for
single strict JSON-RPC 2.0 requests and responses.

## Current Behavior

`HttpRequestMiddleware::call()`:

1. checks cookie authentication from headers,
2. inserts or rewrites `content-type`,
3. calls `request_to_json_rpc_2()`,
4. calls the inner jsonrpsee service with the rebuilt request,
5. calls `response_from_json_rpc_2()` on the rebuilt response.

`request_to_json_rpc_2()`:

- fully buffers the bounded HTTP request body using `Limited::new(...).collect()`;
- attempts to parse it as `JsonRpcRequest`;
- for any recognized version, including strict `"jsonrpc": "2.0"`, serializes
  `request.into_2()`;
- builds a new `HttpBody` from `bytes.as_ref().to_vec()`.

`response_from_json_rpc_2()`:

- fully buffers the response body;
- attempts to parse it as `JsonRpcResponse`;
- serializes `response.into_version(version)`;
- builds a new `HttpBody` from `bytes.as_ref().to_vec()`.

For strict JSON-RPC 2.0, this is compatibility work without a compatibility
rewrite. It canonicalizes the body and pays an avoidable full-body parse and
copy before/after normal RPC handling.

## Reachability And Bounds

Reachability:

- authenticated RPC callers, or deployments that intentionally expose RPC
  without cookie authentication;
- most directly exercised by single large requests such as `submitblock`-style
  request bodies;
- response-side sibling is exercised by RPCs with larger JSON outputs.

Bounds:

- request body size is capped at roughly twice the maximum block byte size plus
  overhead in `RpcServer::start()`;
- response body size is capped by `max_response_body_size`;
- jsonrpsee still performs normal parsing/dispatch after middleware.

Impact: bounded CPU/memory-copy amplification for configured RPC access.

## Local Proofs

Added middleware-local current-behavior tests in
`zebra-rpc/src/server/tests/http_request_compatibility.rs`:

- `strict_json_rpc_2_request_is_reserialized_before_inner_service_today`
  captures the raw request body received by a mock inner service and shows that
  a strict 2.0 request with whitespace and non-struct field order is
  canonicalized before inner dispatch.
- `strict_json_rpc_2_response_is_reserialized_before_client_today` returns a
  strict 2.0 response with whitespace and non-struct field order from a mock
  inner service and shows the middleware canonicalizes it before returning to
  the caller.

Added a real-server composition proof in
`zebra-rpc/src/server/tests/vectors.rs`:

- `rpc_server_text_plain_strict_json_rpc_reaches_dispatch_today` starts the
  actual auth-disabled `RpcServer`, sends a strict JSON-RPC 2.0 request with
  `Content-Type: text/plain; charset=utf-8`, and receives a JSON-RPC
  method-not-found response for a nonexistent method while the mocked backend
  services receive no requests.
- This proves the production server stack traverses Zebra's HTTP compatibility
  middleware before jsonrpsee dispatch. The byte-for-byte reserialization claim
  remains owned by the middleware-local request and response tests above.

Verification:

```sh
cargo test -p zebra-rpc strict_json_rpc_2 --lib
cargo test -p zebra-rpc rpc_server_text_plain_strict_json_rpc_reaches_dispatch_today --lib
```

Result on 2026-05-09: both passed.

## Suggested Fix

Keep compatibility logic local to
`zebra-rpc/src/server/http_request_compatibility.rs`.

Request side:

- continue collecting the bounded body once;
- only reserialize when the request is positively identified as legacy
  `Bitcoind` or `Lightwalletd`;
- forward strict JSON-RPC 2.0 and `Unknown` payload bytes unchanged.

Response side:

- only collect/parse/reserialize the response for legacy request versions that
  need response-shape compatibility;
- return strict JSON-RPC 2.0 and `Unknown` responses untouched.

Optional further hardening:

- add a cheap first-non-whitespace-byte gate to avoid object parse attempts for
  batch arrays or obvious non-object payloads.

Post-fix tests should invert the two current-behavior proofs so strict 2.0
request and response bytes are preserved while legacy compatibility tests still
pass.
