# Config Debug Log Secret Disclosure Note

Date: 2026-05-09

Status: local-only. Do not post publicly without explicit user direction.

## Summary

`zebrad` logs the full processed `ZebradConfig` with derived `Debug` during
normal server startup. In builds compiled with the experimental
`elasticsearch` feature, that config includes `zebra_state::Config`
`elasticsearch_username` and `elasticsearch_password` fields as plain `String`s.

This means an opt-in Elasticsearch build with a non-empty configured password
can emit that password to normal startup logs, including terminal output,
systemd/journal logs, container logs, tracing sinks, support bundles, or any
centralized log collector that receives Zebra INFO logs.

## Evidence

The server startup path logs the entire config:

- `zebrad/src/application.rs:460-470` logs metadata and the config path, then
  calls `info!("{config:?}")` for server commands.

The root config derives `Debug` and contains the state config:

- `zebrad/src/config.rs:49-64` derives `Debug` for `ZebradConfig` and includes
  `pub state: zebra_state::config::Config`.

The state config also derives `Debug` and, under the `elasticsearch` feature,
contains credential fields:

- `zebra-state/src/config.rs:24-26` derives `Debug` for
  `zebra_state::config::Config`.
- `zebra-state/src/config.rs:130-140` defines `elasticsearch_url`,
  `elasticsearch_username`, and `elasticsearch_password`.
- `zebra-state/src/service/finalized_state.rs:174-190` uses those values as
  Elasticsearch Basic auth credentials.

The feature is not part of the default release feature set:

- `zebrad/Cargo.toml:54-58` makes `default-release-binaries` the default.
- `zebrad/Cargo.toml:73-75` keeps `elasticsearch` behind an explicit feature
  that enables `zebra-state/elasticsearch`.

One current-tree mitigation is present but does not remove this issue:

- `zebrad/src/config.rs:101-167` rejects sensitive environment-variable config
  overrides, including leaf keys ending in `password`.
- This blocks setting the password through `ZEBRA_STATE__ELASTICSEARCH_PASSWORD`,
  but it still allows a password in the config file and the server startup log
  still uses the derived debug representation.

## Current-Behavior Proof

Added a focused feature-gated proof:

- `zebrad/src/config.rs` now has
  `debug_config_includes_elasticsearch_password_today` under
  `#[cfg(all(test, feature = "elasticsearch"))]`.
- The test constructs a default `ZebradConfig`, sets a sentinel
  `state.elasticsearch_password`, formats the config with `Debug`, and confirms
  that both the `elasticsearch_password` field name and sentinel password are
  present in the formatted output.

Verification:

```sh
cargo test -p zebrad --features elasticsearch debug_config_includes_elasticsearch_password_today --lib
```

Result on 2026-05-09: passed.

## Duplicate Check

This is adjacent to
`docs/analysis/elasticsearch-feature-transport-and-panic-note.md`, but it is
not the same finding. That note covers disabled TLS certificate validation and
panic/assert behavior in the optional Elasticsearch path. This note covers
plain credential disclosure through `zebrad` startup logging.

Searches in the local analysis directory found prior RPC cookie credential
lifecycle notes, Sentry/OpenTelemetry redaction notes, and Elasticsearch
transport notes, but no existing note for full-config debug logging of
Elasticsearch credentials.

## Impact

Severity: low to medium confidentiality, deployment dependent.

This is not a remote unauthenticated exploit. The trigger is normal startup of a
Zebra binary compiled with `--features elasticsearch` and configured with a
non-empty Elasticsearch password. Exposure depends on log access, log retention,
and log forwarding.

The risk increases when:

- Zebra INFO logs are forwarded to centralized logging systems.
- Startup logs are included in support bundles or CI artifacts.
- Elasticsearch credentials are shared across systems or have write privileges.
- Multiple local users or containers can read the process logs.

The risk is lower for default release artifacts because the Elasticsearch
feature is opt-in and experimental.

## Suggested Hardening

- Stop logging the full `ZebradConfig` with derived `Debug` in
  `zebrad/src/application.rs`.
- Replace it with an explicit non-secret startup summary.
- Add type-level redaction for secret-bearing config fields, for example a
  redacting wrapper or a custom `Debug` implementation for
  `zebra_state::config::Config`.
- Add a regression test that constructs an Elasticsearch-enabled config with a
  sentinel password and asserts that the logged startup representation does not
  contain the sentinel.

## Confidence

Confidence is high that the source currently logs the full config and that the
derived `Debug` chain includes `elasticsearch_password` in Elasticsearch-enabled
builds. The focused feature-gated test confirms this current behavior.
Confidence is medium on real-world exposure because it depends on an opt-in
feature and deployment log access.
