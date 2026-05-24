# Docker Default Observability Public Bind Note

Date: 2026-05-03

Last updated: 2026-05-09

Disposition: local-only. Do not post publicly without explicit
re-authorization and a fresh duplicate check.

Scope: shipped Docker/default configuration examples that make optional
observability endpoints reachable beyond localhost.

## Finding

Pass 5 already documented Docker examples that bind Zebra RPC to `0.0.0.0`,
disable cookie authentication, and publish the RPC port. The same
copy-paste exposure pattern also appears in the default Docker config for
metrics and health endpoints.

This is not a new implementation bug in metrics or health. It is a
documentation/configuration composition issue: the code-level endpoint hardening
gaps become more relevant when shipped examples normalize broad binds.

## Evidence

- `docker/default-zebra-config.toml:60-64` has a commented example:
  `endpoint_addr = "0.0.0.0:9999"` for Prometheus metrics.
- `docker/default-zebra-config.toml:66-71` has a commented example:
  `listen_addr = "0.0.0.0:8080"` for `/healthy` and `/ready`.
- `docker/docker-compose.observability.yml:24-37` sets
  `ZEBRA_METRICS__ENDPOINT_ADDR=0.0.0.0:9999` and publishes `9999:9999`;
  the same compose file also publishes unauthenticated RPC.
- `zebrad/src/components/metrics.rs:16-43` delegates the Prometheus listener to
  `metrics_exporter_prometheus` without Zebra-side allowlisting, connection
  limits, or request/header timeouts.
- `zebrad/src/components/health.rs:303-337` accepts health endpoint TCP streams
  and spawns a Hyper HTTP/1 task per accepted connection.
- `zebrad/src/components/health.rs:318-326` applies a recent-accept counter, but
  does not track currently open connections or per-request keep-alive work.
- `book/src/user/metrics.md` uses `127.0.0.1:9999`, which is the safer default,
  while `book/src/user/health.md` uses `0.0.0.0:8080` for probe examples.

## Duplicate Check

Read-only GitHub searches refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "Docker observability" "0.0.0.0" metrics health'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "ZEBRA_METRICS__ENDPOINT_ADDR" "0.0.0.0"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "docker compose" metrics RPC "cookie auth" false'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "health" "listen_addr" "0.0.0.0" Docker'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra observability compose 9999 8232'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra docker health metrics public bind'
```

Results:

- The exact Docker observability and compose/RPC/health hardening searches
  returned no direct duplicate.
- `ZEBRA_METRICS__ENDPOINT_ADDR` returned #9768, the layered configuration PR
  that introduced the current environment-variable model, not a public-bind
  warning.
- `health listen_addr 0.0.0.0 Docker` returned #9895, the PR that added the
  unauthenticated opt-in health endpoint and docs. It is endpoint provenance,
  not a deployment-hardening duplicate.
- `observability compose 9999 8232` matched issue numbers #8232 and #9999
  because those numbers look like ports; both hits are unrelated.

## Local Proof Status

Source-evidence-only. This is a configuration/documentation composition issue,
not a single runtime code path. The supporting runtime behavior is covered in
the local metrics endpoint, health endpoint, RPC Docker unauthenticated bind,
and RPC tracing/metrics cardinality notes.

## Impact

Severity: public hardening / operator footgun.

The endpoints are disabled by default in normal runtime configuration. But if an
operator starts from the Docker default config or observability compose file,
they can accidentally expose unauthenticated observability endpoints outside a
trusted local or container network.

That exposure compounds existing public hardening findings:

- Prometheus metrics can be expensive to scrape if the registry grows or
  high-cardinality labels are present.
- The metrics exporter listener lacks a Zebra-owned admission guard.
- The health endpoint is intentionally unauthenticated and lacks an open
  connection cap or request/header timeout.

This does not affect consensus validation or chain state. It can affect node
availability and monitoring reliability in copied Docker deployments.

## Suggested Fix

- Prefer localhost examples such as `127.0.0.1:9999` and `127.0.0.1:8080` when
  publishing host ports.
- If the endpoint is meant only for other Compose services, avoid host port
  publication and keep it on the private Compose network.
- Add comments next to any `0.0.0.0` metrics or health examples saying they must
  stay on a trusted network unless endpoint-level admission limits are added.
- Cross-link metrics and health docs to their unauthenticated/internal-probe
  deployment assumptions.

## Confidence

Confidence: high on the documented public-bind examples and on the existing
endpoint hardening gaps. Practical risk is configuration-dependent because
operators may still protect Docker host ports with firewalling or private
networks.
