# Tracing Filter Endpoint Security Note

Date: 2026-05-02

Last updated: 2026-05-09

Disposition: local-only. Do not post publicly without explicit
re-authorization and a fresh duplicate check.

Scope: follow-up audit of Zebra's optional dynamic tracing filter endpoint.

## Finding

When Zebra is built with the optional `filter-reload` feature and configured
with `tracing.endpoint_addr`, it starts an HTTP endpoint that allows dynamic
runtime control of tracing filters. The endpoint has no authentication, no
authorization, and no request-body size limit on `POST /filter`.

This is not enabled in Zebra's default runtime config and `filter-reload` is not
part of `default-release-binaries`. It is still a real operational hardening
issue for operators or test deployments that compile the feature and bind the
endpoint to an attacker-reachable address.

## Evidence

`zebrad/src/components/tracing/endpoint.rs` starts a Hyper server whenever
`tracing.endpoint_addr` is configured under the `filter-reload` feature. The
request handler supports:

- `GET /filter` to read the current filter string,
- `POST /filter` to replace the current filter string.

The `POST /filter` path calls:

```rust
req.into_body()
    .collect()
    .await
```

and then converts the complete body to UTF-8. There is no equivalent to the RPC
server's `http_body_util::Limited` wrapper, and no local maximum filter length.

The new filter is then applied through `Tracing::reload_filter(filter)`. Invalid
filter directives appear to be parsed lossily by `tracing_subscriber::EnvFilter`
rather than causing a clean request error, but I did not find an invalid-filter
panic path.

## Duplicate Check

Read-only GitHub searches refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "tracing filter endpoint" "body limit"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "filter-reload endpoint" unauthenticated'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "POST /filter" "body limit"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra tracing filter endpoint auth body limit'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra tracing endpoint filter reload'
```

Result:

- The exact phrase/body-limit searches returned no hits.
- The broad `tracing filter endpoint auth body limit` search returned only
  unrelated release issue #10000.
- The broader `tracing endpoint filter reload` search returned old adjacent
  history, including #1001 (broken tracing endpoint), #995 (listener acceptance
  test coverage), and #4539 (making diagnostics optional by default). These do
  not cover endpoint authentication, request body bounds, or public binding
  hardening.

## Local Proof

Added a feature-gated current-behavior unit test:
`zebrad/src/components/tracing/endpoint.rs`.

The test builds a `POST /filter` request whose UTF-8 body is several megabytes
of repeated filter directives, calls `read_filter()` directly, and confirms the
whole body is accepted and returned unchanged.

Focused command:

```sh
cargo test -p zebrad tracing_filter_endpoint_read_filter_accepts_large_body_today --features filter-reload --lib
```

Result on 2026-05-09:

```text
test components::tracing::endpoint::tests::tracing_filter_endpoint_read_filter_accepts_large_body_today ... ok
```

## Attack Shape

If the endpoint is reachable by an attacker:

- A large `POST /filter` body can force Zebra to collect the full request body
  in memory before parsing.
- A request can set highly verbose filters such as broad `trace` directives,
  increasing logging volume and CPU/disk pressure.
- Invalid comma-separated directives can cause parser diagnostics to be emitted
  while the lossy parser ignores them.
- `GET /filter` exposes the current tracing filter, which can reveal operator
  debugging focus or instrumentation choices.

This is most dangerous if `tracing.endpoint_addr` is set to a public or
container-wide bind address such as `0.0.0.0:...`. The user docs demonstrate a
localhost endpoint, but the code does not enforce localhost-only binding.

## Severity

Suggested severity: public hardening / operational DoS risk.

This should not use private disclosure by itself because it requires an
optional non-default compile feature plus an enabled runtime endpoint. But the
fix is low-cost and aligns with the hardened RPC endpoint behavior.

## Suggested Fix

- Add a small maximum body size for `POST /filter`, for example 4 KiB or 16 KiB.
- Return `413 Payload Too Large` when the filter body exceeds the limit.
- Consider requiring localhost binding, or warn loudly when the endpoint is
  configured on a non-loopback address.
- Consider basic auth or cookie auth if remote use is expected.
- Parse filters with a fallible API and return `400 Bad Request` for invalid
  filters instead of silently applying a lossy subset.
- Document the endpoint as an unauthenticated administrative endpoint that
  should not be exposed beyond localhost.

## Confidence

Confidence: high that the endpoint is unauthenticated and has no body limit.
Confidence: medium on practical impact, because the feature and endpoint are
both opt-in.
