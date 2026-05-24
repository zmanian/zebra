# Sentry and OpenTelemetry Privacy Note

Date: 2026-05-03

Last updated: 2026-05-09

Disposition: local-only. Do not post publicly without explicit
re-authorization and a fresh duplicate check.

Scope: follow-up audit of Zebra's default-release telemetry features, Sentry
export, OpenTelemetry export, and the `getinfo.errors` RPC diagnostic field.

## Finding

I did not find a default-on remote vulnerability in the Sentry or
OpenTelemetry paths. Both exporters require explicit runtime configuration
before Zebra sends telemetry off-host:

- Sentry requires `SENTRY_DSN`.
- OpenTelemetry requires `tracing.opentelemetry_endpoint` or
  `OTEL_EXPORTER_OTLP_ENDPOINT`.

However, official release builds compile both features by default, and the user
docs understate the amount of data that can be exported once an operator opts
in. This is a public privacy and operational-hardening item, not a private
vulnerability by itself.

## Evidence

`zebrad/Cargo.toml` includes both features in `default-release-binaries`:

```toml
default-release-binaries = ["release_max_level_info", "progress-bar", "prometheus", "sentry", "opentelemetry"]
default = ["default-release-binaries"]
```

Sentry initialization is gated on the runtime environment variable:

```rust
if env::var_os("SENTRY_DSN").is_some() {
    #[cfg(feature = "sentry")]
    let guard = crate::sentry::init();
    ...
}
```

The Sentry client options enable logs, and the tracing layer maps:

- `ERROR` to Sentry event plus log,
- `WARN` to Sentry log plus breadcrumb,
- `INFO` to Sentry breadcrumb,
- lower levels to ignored.

Panic handling sends the formatted panic report as a fatal Sentry event. This
means an opted-in operator can export panic messages, warning/error events,
info breadcrumbs, and structured tracing fields attached to those events.

OpenTelemetry is also compiled into default release builds, but the layer
returns `(None, None)` without constructing exporter state when no endpoint is
configured. If an endpoint is configured, Zebra appends `/v1/traces` as needed
and exports sampled spans through the OTLP HTTP exporter. The default sample
percentage is 100 if the endpoint is configured and no sample percentage is
provided.

Follow-up on 2026-05-09: Zebra also overloads
`OTEL_TRACES_SAMPLER_ARG` as a percentage value. This matches the Docker
observability docs, but differs from the standard OpenTelemetry ratio form. If
an operator uses conventional OTEL syntax such as
`OTEL_TRACES_SAMPLER=traceidratio` and `OTEL_TRACES_SAMPLER_ARG=0.1`, Zebra
ignores `OTEL_TRACES_SAMPLER`, fails to parse `0.1` as a `u8`, and falls back
to 100% sampling once an OTLP endpoint is configured. This is an opt-in
operational hardening issue, not a private disclosure issue.

Local proof added on 2026-05-09:

- `otel_traceidratio_point_one_falls_back_to_full_sampling_today` sets
  `OTEL_EXPORTER_OTLP_ENDPOINT`, `OTEL_TRACES_SAMPLER=traceidratio`, and
  `OTEL_TRACES_SAMPLER_ARG=0.1`, then confirms Zebra resolves no integer
  sample percentage and defaults the effective sample percentage to 100.
- `otel_sampler_selector_env_is_ignored_today` sets
  `OTEL_TRACES_SAMPLER=always_off` without a sampler argument and confirms the
  selector alone does not change Zebra's sample percentage today.
- `otel_integer_sampler_arg_is_treated_as_percentage_today` confirms
  `OTEL_TRACES_SAMPLER_ARG=10` is treated as 10%.
- `zebra_config_sample_percent_overrides_otel_env_today` confirms Zebra's
  config field takes precedence over the OTEL environment fallback.
- `none_sample_percent_defaults_to_full_sampling_today`,
  `ten_percent_maps_to_point_one_today`, and
  `oversized_percent_clamps_to_full_sampling_today` prove the pure sampler-rate
  conversion used by the OpenTelemetry layer.

The `getinfo.errors` RPC diagnostic path is separate from off-host telemetry.
Zebra stores the last WARN or ERROR event in a global watch channel and returns
the last message through `getinfo.errors`. The stored value is only the tracing
event's `message` field, not the structured fields. A targeted search for
dynamic WARN/ERROR message strings found a small set of local operator/disk
paths rather than a broad remote-input channel:

- `zebrad/src/commands/tip_height.rs`: state read failure text,
- `zebra-state/src/service/finalized_state/disk_db.rs`: cache path
  canonicalization or move failure text.

Most attacker-influenced data at WARN/ERROR level appears in structured fields,
such as `?e`, `?updated`, `?change`, or peer-set metrics. Those fields can be
included in Sentry logs or OpenTelemetry spans when export is enabled, but they
are not copied into `getinfo.errors`.

The current user tracing docs mention that Sentry activates when `SENTRY_DSN`
is set and OpenTelemetry activates when an endpoint is configured. They do not
spell out the exported event/log/breadcrumb/span-field scope, and the
OpenTelemetry sample-percentage details are more visible in generated config
docs than in the user guide.

## Duplicate Check

Read-only GitHub searches refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "Sentry" "OpenTelemetry" privacy'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra SENTRY_DSN telemetry privacy'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra OTEL_EXPORTER_OTLP_ENDPOINT privacy'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "OpenTelemetry sample" "Sentry logs"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra sentry logs breadcrumbs privacy'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra OpenTelemetry export sample percent'
```

Results:

- `Sentry` / `OpenTelemetry` privacy returned #10490, which upgraded Sentry,
  enabled Sentry Logs, kept OpenTelemetry in default releases, and updated
  observability docs. It is implementation/provenance, not a privacy-hardening
  follow-up.
- `OpenTelemetry export sample percent` returned #10174, which added the
  OpenTelemetry tracing feature and documents 100% sampling in its test plan /
  configuration context.
- The other privacy/redaction-focused searches returned no hits.

## Attack / Exposure Shape

The relevant attacker model is indirect:

1. An operator enables Sentry or OpenTelemetry export.
2. Zebra emits WARN/ERROR/INFO tracing events while processing peer, sync,
   mempool, RPC, state, or deployment activity.
3. Remote network peers or RPC callers may influence some structured fields in
   those events, such as peer addresses, block hashes, transaction identifiers,
   RPC method names, and error details.
4. The configured third-party or internal telemetry backend receives those
   events.

This does not let a remote attacker read local telemetry unless they control or
can access the configured telemetry backend. It also does not bypass RPC auth,
change consensus behavior, or expose secrets by default.

The risk is mostly that an operator may think Sentry only sends crashes, while
the compiled default feature can also send logs and breadcrumbs after
`SENTRY_DSN` is set. Similarly, an operator may not expect OpenTelemetry export
to default to 100% sampling once an endpoint is configured.

## Severity

Suggested severity: public privacy hardening / documentation hardening.

Do not treat this as a private disclosure finding unless a deployment profile
shows that sensitive secrets are actually logged at WARN/ERROR/INFO or that a
telemetry endpoint is configured by default for users without consent.

## Suggested Fix

- Document that Sentry export includes panic reports, ERROR events/logs, WARN
  logs/breadcrumbs, INFO breadcrumbs, and tracing fields.
- Document that OpenTelemetry export can send span names and span fields, and
  that the default sample percentage is 100 when an endpoint is configured.
- Clarify the `OTEL_TRACES_SAMPLER_ARG` semantics, or move Zebra's percentage
  form to a Zebra-namespaced environment variable. If the standard OTEL variable
  remains supported, warn or fail closed when ratio syntax like `0.1` does not
  parse as a percentage.
- Add a `before_send` / `before_send_log` redaction pass for known sensitive
  keys before data leaves the process.
- Consider reducing the default OpenTelemetry sample percentage when an endpoint
  is set only through the environment.
- Consider making `getinfo.errors` return a bounded static category or
  operator-facing warning string rather than arbitrary WARN/ERROR message text
  if RPC is expected to be exposed to shared services.
- Keep `SENTRY_DSN` and `OTEL_EXPORTER_OTLP_ENDPOINT` out of example user
  deployments unless the example explicitly explains the telemetry destination.

## Local Confidence Check

Focused tests added and run on 2026-05-09:

```sh
cargo test -p zebrad components::tracing::component::tests --lib
cargo test -p zebrad components::tracing::otel::tests --lib
```

Result: passed. These tests do not initialize the global tracing subscriber or
construct an OTLP exporter. They prove the current env/config resolution and
sample-rate conversion at pure helper boundaries.

## Confidence

Confidence: high on the sampler env parsing/defaulting behavior; medium on
overall practical privacy impact.

The activation gates and export filters are direct in the code. The remaining
uncertainty is practical data sensitivity in real deployments: the reviewed
default paths do not export without runtime configuration, and the highest-risk
dynamic values I found are operational identifiers rather than secrets. The
standard-OTEL ratio mismatch is now deterministic-test-backed, but still
requires an operator-configured OTLP endpoint before any telemetry leaves the
node.
