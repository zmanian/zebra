# Tracing Filter Reload Telemetry Amplification Note

Date: 2026-05-03

Last updated: 2026-05-09

Disposition: local-only. Do not post publicly without explicit
re-authorization and a fresh duplicate check.

Scope: composition follow-up on the optional tracing filter reload endpoint and
runtime telemetry exporters.

## Finding

The existing filter-reload finding covers the unauthenticated endpoint and
unbounded `POST /filter` body. A separate composition issue is that, when
filter reload and telemetry export are both enabled, the endpoint becomes a
remote control plane for the volume and shape of telemetry Zebra exports.

This is public hardening, not private disclosure by itself. It requires the
non-default `filter-reload` feature, a configured `tracing.endpoint_addr`, and a
configured telemetry exporter such as Sentry or OpenTelemetry.

## Evidence

- `zebrad/Cargo.toml:54` includes `sentry` and `opentelemetry` in
  `default-release-binaries`, while `zebrad/Cargo.toml:81` keeps
  `filter-reload` as a separate feature.
- `zebrad/src/components/tracing/endpoint.rs:36-49` reads the entire
  `POST /filter` body into a UTF-8 string.
- `zebrad/src/components/tracing/endpoint.rs:157-165` applies that request body
  through `Tracing::reload_filter(filter)`.
- `zebrad/src/components/tracing/component.rs:445-461` replaces the live
  subscriber filter through the reload handle.
- `zebrad/src/application.rs:402-406` initializes Sentry only when
  `SENTRY_DSN` is set.
- `zebrad/src/components/tracing/component.rs:271-312` installs Sentry and
  OpenTelemetry layers when their feature/runtime gates are active.
- `zebrad/src/components/tracing.rs:195-226` documents OpenTelemetry endpoint
  and sample-percent configuration, including environment-variable fallback.
- `zebrad/src/sentry.rs:133-142` maps `ERROR` events to Sentry event plus log,
  `WARN` to log plus breadcrumb, and `INFO` to breadcrumb.

## Duplicate Check

Read-only GitHub searches refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "tracing endpoint" "telemetry amplification"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra tracing endpoint telemetry amplification'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra tracing endpoint filter reload'
```

The telemetry-amplification searches returned no hits. The broader filter
reload search returned old adjacent tracing endpoint history, including #1001,
#995, and #4539, but none of those track remote filter reload as a telemetry
export amplification control plane.

## Attack Shape

If an operator exposes the filter endpoint and also exports telemetry, a remote
caller can:

- raise logging/span verbosity by posting a broader filter such as a crate-wide
  `trace` or `debug` directive,
- keep the new filter active until another reload or restart,
- increase CPU, allocation, network, and backend-ingest cost for the configured
  telemetry exporter, and
- potentially broaden which structured fields leave the process through Sentry
  logs/breadcrumbs or OpenTelemetry spans.

The remote caller does not get direct access to the telemetry backend through
this path. The risk is that they can influence what Zebra emits to a backend the
operator already configured.

## Triage

Classification: public telemetry and availability hardening.

This does not create coins, bypass consensus checks, change chain state, or
expose telemetry unless export is already configured. It is still worth fixing
because it composes two opt-in administrative surfaces into a remotely
reachable export-amplification control plane.

## Suggested Fix

- Treat `tracing.endpoint_addr` as an administrative endpoint in docs and config
  examples.
- Require localhost-only binding, authentication, or a strong warning when the
  endpoint is non-loopback.
- Add a small request body cap for `POST /filter`.
- Warn at startup when filter reload and Sentry/OpenTelemetry export are enabled
  together.
- Consider exporter-safe filter policy, for example rejecting reloads that
  broaden below a configured maximum level.

## Confidence

Confidence: medium-high.

The source-level composition is direct. Practical impact depends on an
operator-enabled combination of feature, endpoint binding, and telemetry export,
so this should remain public hardening rather than a private vulnerability.
