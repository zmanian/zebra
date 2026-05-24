# Elasticsearch Feature Transport and Panic Note

Date: 2026-05-03

Last updated: 2026-05-09

Disposition: local-only; partial overlap with closed #8329/#7270. Do not post
publicly without explicit re-authorization.

## Summary

The experimental `elasticsearch` feature is not part of default release
binaries, but when it is compiled into `zebrad`, the read-write state service
unconditionally enables Elasticsearch indexing. The client disables TLS
certificate validation and the indexing path can panic on bulk request or
response failures after the endpoint has answered an initial `ping`.

This is opt-in external-service hardening, not a consensus issue and not a
default-node remote vulnerability. It matters for deployments that enable the
experimental feature and point Zebra at an Elasticsearch endpoint outside a
fully trusted local boundary.

## Evidence

- `zebrad/Cargo.toml:73-77` keeps `elasticsearch` behind an explicit
  experimental feature.
- `zebra-state/src/config.rs:130-140` adds `elasticsearch_url`,
  `elasticsearch_username`, and `elasticsearch_password` config fields under
  that feature.
- `zebra-state/src/config.rs:222-227` defaults the URL to
  `https://localhost:9200`, username to `elastic`, and password to an empty
  string.
- `zebra-state/src/service.rs:305-316` calls `FinalizedState::new(..., true)`
  for the live read-write state service whenever the feature is compiled.
- `zebra-state/src/service.rs:1785-1794` calls
  `FinalizedState::new_with_debug(..., false, true)` for read-only state, so the
  exposure is only on the writer path.
- `zebra-state/src/service/finalized_state.rs:172-194` builds the client with
  Basic auth and `CertificateValidation::None`.
- `zebra-state/src/service/finalized_state.rs:477-557` batches finalized blocks
  into JSON bulk requests, pings the endpoint, then:
  - panics if `.send().await` returns an error,
  - panics if the response body cannot be parsed as JSON,
  - asserts that the Elasticsearch response `errors` field is false.

## Impact

An operator who compiles the experimental feature and configures a remote or
shared Elasticsearch service is trusting that service and the network path to
it. Because certificate validation is disabled, a network-positioned attacker
could impersonate the endpoint even when the configured URL is `https://...`.
That can expose Basic-auth credentials to the impersonating endpoint.

If the endpoint responds to `ping` but then returns a malformed, erroring, or
otherwise unexpected bulk response, Zebra can panic during finalized block
indexing. In the normal binary profiles used by Zebra, a panic can be
process-fatal.

## Triage

Classification: public hardening for an experimental, opt-in feature.

Confidence: high for the code behavior; deployment impact depends on an
operator compiling `--features elasticsearch` and configuring an Elasticsearch
endpoint outside a fully trusted local environment.

## Verification

```sh
cargo check -p zebra-state --features elasticsearch
```

Result: passed after Cargo downloaded the optional `elasticsearch` dependency.

Focused current-behavior repro:

```sh
cargo test -p zebra-state elasticsearch_bulk_error_response_panics_today --features elasticsearch --lib
```

Result on 2026-05-09: passed.

The test
`service::finalized_state::tests::vectors::elasticsearch_bulk_error_response_panics_today`
starts a local fake Elasticsearch endpoint. The fake endpoint accepts the
client's ping, then returns HTTP 200 with a JSON bulk response containing
`"errors": true`. Zebra panics with `ES error: ...`, confirming that a reachable
endpoint-controlled bulk response can abort the indexing path under the optional
feature.

## Duplicate / Overlap Check

Known public overlap:

- Closed #8329 covers Elasticsearch-unavailable panics.
- Closed #7270 covers Elasticsearch bulk-size panic risk.

Read-only duplicate search refreshed on 2026-05-09:

```sh
gh api repos/ZcashFoundation/zebra/issues/8329
gh api repos/ZcashFoundation/zebra/issues/7270
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra Elasticsearch CertificateValidation None panic bulk errors true'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "CertificateValidation::None" elasticsearch'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "ES error" elasticsearch'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "errors" "true" "elasticsearch" "bulk"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "ES Request should never fail" elasticsearch'
```

Those closed issues and search hits do not cleanly cover the local proof that
an endpoint which passes `ping` and then returns HTTP 200 with `"errors": true`
can still panic, or the disabled TLS certificate validation concern. Given the
feature is experimental and opt-in, keep this as local hardening context unless
explicitly re-authorized.

## Suggested Fix Direction

- Enable normal certificate validation by default. If insecure TLS is needed for
  local development, require an explicit unsafe config knob.
- Do not unconditionally enable Elasticsearch indexing just because the feature
  is compiled; add a runtime `enable_elasticsearch` boolean or treat an empty
  URL as disabled.
- Convert bulk request, response parse, and Elasticsearch item errors into
  logged indexing failures rather than panics.
- Avoid sending Basic-auth credentials over unauthenticated or certificateless
  transports.
