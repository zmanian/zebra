# Feature-gated release paths B7 revisit

Date: 2026-05-03

Status: public release-engineering hardening / maintainer confirmation item.

Confidence: high on the source-controlled workflow shape; medium on production
artifact impact because the repository variable values are not readable with the
current GitHub token.

## Summary

I did not find a new private consensus or remote-availability vulnerability in
the release-enabled feature set.

The default Zebra binary feature set is narrow: `default-release-binaries`
enables compile-time log filtering, the progress bar, Prometheus, Sentry, and
OpenTelemetry. The more sensitive or experimental Cargo features I checked are
not in that default set: `indexer`, `internal-miner`, `elasticsearch`,
`filter-reload`, `tokio-console`, `tx_v6`, and `comparison-interpreter`.

One important nuance: the `indexer` Cargo feature gates extra state indexing
data, but the gRPC indexer server module itself is compiled by `zebra-rpc` and
is started by `zebrad start` whenever `rpc.indexer_listen_addr` is configured.
So the indexer server exposure is runtime-config gated, not fully
feature-gated. That surface is already covered separately in the indexer gRPC
hardening notes.

The main hardening finding is release-engineering hygiene: the continuous
deployment Docker workflow builds `runtime` images with both `RUST_PROD_FEATURES`
and `RUST_TEST_FEATURES`, while the official release-binaries workflow uses only
`RUST_PROD_FEATURES`. If `RUST_TEST_FEATURES` includes the same testing features
used by CI, deployment runtime images can include public test helpers and extra
test dependencies. I did not find a runtime path from `zebrad start` into those
helpers, so this is not a standalone security vulnerability on current evidence.

## Evidence

Default release features:

- `zebrad/Cargo.toml:52-58` defines `default-release-binaries` and makes it the
  default feature set.
- `zebrad/Cargo.toml:62-148` keeps `indexer`, `internal-miner`,
  `elasticsearch`, `filter-reload`, `tokio-console`, `tx_v6`, and
  `comparison-interpreter` outside `default-release-binaries`.
- `docker/Dockerfile:18` defaults the Docker build argument to
  `FEATURES="default-release-binaries"` when the caller does not override it.

Indexer server nuance:

- `zebra-rpc/src/lib.rs:9` exports `pub mod indexer` unconditionally.
- `zebrad/src/commands/start.rs:277-289` starts
  `zebra_rpc::indexer::server::init(...)` whenever
  `config.rpc.indexer_listen_addr` is configured.
- `zebra-rpc/src/config/rpc.rs:33-47` documents the indexer RPC listen address
  as disabled by default and warns against public binding.
- `zebra-rpc/src/indexer/server.rs:57-61` starts the tonic server with
  reflection and the indexer service.

Source-controlled release/deploy workflow shape:

- `.github/workflows/release-binaries.yml:28-38` builds the Docker `runtime`
  target for official Docker Hub publishing using `features:
  ${{ vars.RUST_PROD_FEATURES }}`.
- `.github/workflows/zfnd-deploy-nodes-gcp.yml:250-264` builds the same
  Docker `runtime` target for deployment using `features:
  ${{ format('{0} {1}', vars.RUST_PROD_FEATURES, vars.RUST_TEST_FEATURES) }}`.
- `.github/workflows/zfnd-build-docker-image.yml:70-72` maps the workflow input
  into `FEATURES`.
- `.github/workflows/zfnd-build-docker-image.yml:236-240` and
  `.github/workflows/zfnd-build-docker-image.yml:262-266` pass `FEATURES` as a
  Docker build argument.

Testing feature exposure if included in runtime builds:

- `zebrad/Cargo.toml:111-129` defines `proptest-impl`,
  `zebra-checkpoints`, and `lightwalletd-grpc-tests` as testing features.
- `zebra-state/src/lib.rs:27-28`, `zebra-state/src/lib.rs:70-85`, and
  `zebra-state/src/lib.rs:87-97` publicly export arbitrary/test helpers and
  hidden database-version writers under `feature = "proptest-impl"`.
- `zebra-consensus/src/router.rs:419-435` exposes `init_test()` under
  `feature = "proptest-impl"`.
- `zebra-network/src/address_book.rs:176-236` exposes an address-book
  constructor that explicitly says it can break address-book invariants, under
  `feature = "proptest-impl"`.

Unknown repository variables:

- `gh api repos/ZcashFoundation/zebra/actions/variables/RUST_PROD_FEATURES
  --jq .value` returned HTTP 403: missing repository variables permission.
- `gh api repos/ZcashFoundation/zebra/actions/variables/RUST_TEST_FEATURES
  --jq .value` returned the same HTTP 403.

Compile smoke checks:

```sh
cargo test -p zebrad --no-default-features --features default-release-binaries \
  --bin zebrad config::tests::generate_with_no_args -- --exact
```

Result: compiled and ran the selected binary test harness successfully; the
filter selected 0 tests.

```sh
cargo test -p zebrad --no-default-features \
  --features "default-release-binaries proptest-impl lightwalletd-grpc-tests zebra-checkpoints" \
  --bin zebrad config::tests::generate_with_no_args -- --exact
```

Result: compiled and ran the selected binary test harness successfully; the
filter selected 0 tests.

## Triage

This is not private-disclosure material by itself. The concerning shape is not
"release builds accept invalid blocks"; it is "a deployment runtime image might
compile test-only helpers if the hidden GitHub variable contains testing
features." The static review did not find those helpers wired into normal
`zebrad start` execution.

It is still worth asking maintainers to confirm the private repository variable
values, because those variables decide whether risky future-gated combinations
are present in release or deployment artifacts. The most important values to
exclude from `RUST_PROD_FEATURES` and deployment runtime `RUST_TEST_FEATURES`
are:

- `tx_v6` unless intentionally testing unreleased consensus changes;
- `comparison-interpreter`;
- `internal-miner`;
- `elasticsearch`;
- `filter-reload`;
- `tokio-console` plus `RUSTFLAGS="--cfg tokio_unstable"`;
- `proptest-impl` for runtime deployment images.
- `indexer` if production does not intentionally need state indexing data,
  while remembering that the gRPC server itself is still controlled by
  `rpc.indexer_listen_addr`.

The `tx_v6` and `zcash_unstable` concerns remain covered by the separate NU7 /
ZIP-235 notes. This B7 revisit adds release-variable reachability context, not a
new independent exploit path.

## Eliminated or bounded hypotheses

`debug_skip_format_upgrades` is not a general production DB-upgrade bypass.
`ZebraDb::new()` documents the argument as test-only and only honors it when the
database is opened read-only or when `cfg!(test)` is true:

- `zebra-state/src/service/finalized_state/zebra_db.rs:90-99`
- `zebra-state/src/service/finalized_state/zebra_db.rs:118-122`

`debug_force_finished_sync` is runtime-reachable in release builds, but I only
found it influencing `getblockchaininfo` progress reporting. It is default-off,
passed into `RpcImpl`, and used to clamp estimated height/progress reporting:

- `zebra-rpc/src/config/rpc.rs:59-61`
- `zebra-rpc/src/config/rpc.rs:87-88`
- `zebrad/src/commands/start.rs:252-264`
- `zebra-rpc/src/methods.rs:999-1056`

`internal-miner` is both compile-gated and runtime-config gated. If compiled and
enabled it is extra mining/RPC surface, but I did not find a validation bypass:

- `zebrad/src/components.rs:20-21`
- `zebrad/src/commands/start.rs:397-411`

`CheckBlockProposalValidity` is production-reachable via proposal validation,
but it validates against a cloned non-finalized state and does not commit to the
live chain state:

- `zebra-consensus/src/block.rs:356-368`
- `zebra-state/src/service.rs:1655-1685`

## Suggested fix direction

- Split runtime deployment features from test-image features. Runtime Docker
  images should use only `RUST_PROD_FEATURES`, matching the release-binaries
  workflow, unless a temporary deployment explicitly opts into a test feature.
- Add a CI guard that fails if production/runtime feature variables contain
  `proptest-impl`, `comparison-interpreter`, `tx_v6`, `internal-miner`,
  `filter-reload`, or `tokio-console`.
- Add an explicit workflow comment documenting why deployment runtime images do
  or do not include `RUST_TEST_FEATURES`.
- Ask maintainers to confirm current `RUST_PROD_FEATURES`,
  `RUST_TEST_FEATURES`, and any release `RUSTFLAGS` values privately, since they
  are not readable by this token.
