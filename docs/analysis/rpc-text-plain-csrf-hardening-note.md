# RPC `text/plain` CSRF Hardening Note

Date: 2026-05-03

Last updated: 2026-05-09

## Summary

Zebra's JSON-RPC HTTP compatibility middleware accepts requests with no
`Content-Type`, or with `Content-Type` starting with `text/plain`, and rewrites
the header to `application/json` before jsonrpsee handles the request.

This preserves compatibility with older RPC clients, but it also weakens a
useful browser boundary for deployments that disable cookie authentication. A
browser can send cross-origin "simple" POST requests with `text/plain` request
bodies. If Zebra is running on localhost or an otherwise reachable address with
cookie auth disabled, a malicious page may be able to trigger JSON-RPC side
effects even though it cannot read the response.

This is not a consensus bug. It is public RPC hardening, and it composes with
the existing notes about unauthenticated Docker examples, batch amplification,
and long-running mining RPCs.

## Evidence

The middleware explicitly rewrites missing and `text/plain` content types:

- `zebra-rpc/src/server/http_request_compatibility.rs:86-124`

The rewrite happens before the inner jsonrpsee service is called:

- `zebra-rpc/src/server/http_request_compatibility.rs:234-253`

The same comment says `application/x-www-form-urlencoded` should be rejected so
browser forms cannot attack a local RPC port, but `text/plain` is also a
browser-simple request content type:

- `zebra-rpc/src/server/http_request_compatibility.rs:98-104`

Cookie auth is checked before request-body collection and prevents unauthenticated
browser requests when enabled:

- `zebra-rpc/src/server/http_request_compatibility.rs:72-83`
- `zebra-rpc/src/server/http_request_compatibility.rs:234-240`

But some documented compatibility setups disable cookie auth on localhost:

- `book/src/user/lightwalletd.md:49-64`
- `book/src/user/mining-testnet-s-nomp.md:52-57`

And some Docker examples disable cookie auth while binding or publishing RPC
more broadly:

- `docker/docker-compose.lwd.yml:16-20`
- `docker/docker-compose.observability.yml:28-37`
- `docker/mining/docker-compose.yml:9-10`
- `book/src/user/docker.md:91-116`

The RPC service includes methods with side effects or expensive behavior:

- `zebra-rpc/src/methods.rs:254` exposes `sendrawtransaction`.
- `zebra-rpc/src/methods.rs:526-548` exposes `getblocktemplate` and
  `submitblock`.
- `zebra-rpc/src/methods.rs:475-476` and
  `zebra-rpc/src/methods.rs:2163-2168` expose `stop` on supported regtest
  nodes.
- The existing submitblock/getblocktemplate timeout note documents long-running
  mining RPC paths.
- The existing batch and metric-label notes document request amplification once
  unauthenticated RPC is reachable.

## Attack Shape

For an auth-disabled local RPC endpoint, a malicious web page can attempt a
simple cross-origin request such as:

```js
fetch("http://127.0.0.1:8232/", {
  method: "POST",
  mode: "no-cors",
  headers: { "Content-Type": "text/plain" },
  body: JSON.stringify({
    jsonrpc: "2.0",
    id: 1,
    method: "getblocktemplate",
    params: []
  })
});
```

The page does not need to read the response for state-changing or expensive
methods to matter. Practical browser behavior can depend on modern private
network access enforcement and the request's source origin, so this should be
treated as a hardening issue rather than a default remote exploit.

## Local Confidence Check

Reran the direct middleware current-behavior test on 2026-05-09:

```sh
cargo test -p zebra-rpc text_plain_request_is_rewritten_to_json_without_auth_today --lib
```

Result: passed. The test builds an auth-disabled
`HttpRequestMiddleware`, sends a POST with
`Content-Type: text/plain; charset=utf-8`, and uses a mock inner service to
assert that the request reaches the inner RPC service with
`Content-Type: application/json`.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "RPC text/plain CSRF"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "Content-Type" "text/plain" "RPC"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "browser origin" "RPC" "cookie auth disabled"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "text_plain_request_is_rewritten_to_json_without_auth_today"'
```

Closest hits:

- #6363, "Accept text/plain RPC encodings automatically", is the closed
  compatibility issue that requested this behavior. It is related provenance,
  not a duplicate of this browser-origin hardening concern.
- #6885, the closed compatibility PR, implemented the content-type rewrite.

The CSRF/browser-origin and cookie-auth-disabled searches returned no hits.

## Preconditions

- Zebra JSON-RPC is enabled.
- Cookie auth is disabled, or an attacker otherwise has valid RPC credentials.
- The endpoint is reachable from the attacker's context:
  - localhost from a browser running on the same machine,
  - a shared/container network,
  - or a public bind/published port.

With cookie auth enabled, a normal browser page should not be able to add the
required `Authorization` header as a simple no-CORS request. With RPC disabled,
there is no surface.

## Impact

Severity: low/medium public hardening.

The strongest impact is not data theft, because browser SOP/CORS blocks reading
responses. The impact is request forgery against an auth-disabled RPC endpoint:

- force expensive RPC calls such as block-template work,
- submit arbitrary transactions through `sendrawtransaction`,
- trigger batch and metrics cardinality amplification,
- and, on supported regtest deployments, invoke `stop`.

For publicly bound unauthenticated Docker examples, this is subsumed by the
larger public RPC exposure issue. The more distinct case is an operator who
believes `127.0.0.1` plus disabled cookie auth is safe from web-origin traffic.

## Suggested Fix

- Do not rewrite `text/plain` to `application/json` when cookie auth is disabled,
  unless an explicit compatibility flag is enabled.
- Alternatively, require `Authorization` whenever accepting `text/plain` or
  missing `Content-Type`.
- Consider rejecting requests with `Origin` or `Referer` headers unless the
  origin is explicitly allowed.
- Keep the existing `application/x-www-form-urlencoded` rejection posture, and
  document that accepting `text/plain` is also browser-relevant.
- In docs that disable cookie auth for localhost compatibility, add a warning
  about browser-origin request forgery, not only network exposure.

## Confidence

Confidence: medium.

The code path is direct: `text/plain` is accepted and rewritten before jsonrpsee
parsing. The remaining uncertainty is browser-specific exploitability in current
public-to-localhost/private-network enforcement. The issue is still worth fixing
because the compatibility behavior is broader than the security comment implies,
and the fix can be narrow.
