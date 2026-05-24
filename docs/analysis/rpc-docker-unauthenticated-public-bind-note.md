# RPC Docker Unauthenticated Public Bind Note

Date: 2026-05-02

Last updated: 2026-05-09

Disposition: local-only. Do not post publicly without explicit
re-authorization and a fresh duplicate check.

Scope: shipped Docker examples and user docs that configure Zebra JSON-RPC
binding and cookie authentication.

## Finding

Several Docker examples bind Zebra JSON-RPC to `0.0.0.0`, disable cookie
authentication, and publish the RPC port on the Docker host. This removes the
main runtime mitigations used throughout the RPC security analysis: RPC is no
longer disabled, no longer authenticated, and no longer localhost-only.

This is not a consensus bug in Zebra's validator logic. It is an operational
exposure footgun that can turn public RPC hardening issues into remote attack
surface for users who copy the provided Docker recipes.

## Evidence

- `docker/docker-compose.lwd.yml:16-20` sets
  `ZEBRA_RPC__LISTEN_ADDR=0.0.0.0:8232`,
  `ZEBRA_RPC__ENABLE_COOKIE_AUTH=false`, and publishes `"8232:8232"`.
- `docker/docker-compose.observability.yml:27-37` sets
  `ZEBRA_RPC__LISTEN_ADDR=0.0.0.0:8232`,
  `ZEBRA_RPC__ENABLE_COOKIE_AUTH=false`, and publishes `"8232:8232"`.
- `docker/mining/docker-compose.yml:9-10` disables cookie auth and binds RPC to
  all interfaces inside the Compose network. This file does not publish the RPC
  port to the host by default, so it is a lower-risk internal-network variant.
- `book/src/user/docker.md:89-92` presents `0.0.0.0:8232` as the example RPC
  bind address and mentions disabling cookie auth in the same setup paragraph.
- `book/src/user/mining-docker.md:12-18` and `48-54` run Docker with
  `ZEBRA_RPC__LISTEN_ADDR=0.0.0.0:8232` / `0.0.0.0:18232`; the later config-file
  example disables cookie auth for mining RPC access.
- `book/src/user/mining-testnet-s-nomp.md:55-56` tells operators to bind RPC to
  localhost and disable cookie auth, which is safer than a public bind but still
  needs the same "trusted local network only" warning.
- `docker/default-zebra-config.toml:28-36` also documents `0.0.0.0` RPC bind
  examples, but keeps cookie authentication enabled by default. This is a lower
  risk supporting example rather than an unauthenticated host-publish by itself.
- `zebra-rpc/src/methods.rs:254`, `475-476`, `526-547`, and `713-741` expose
  state-changing or expensive methods such as `sendrawtransaction`, `stop`,
  `getblocktemplate`, `submitblock`, `generate`, and `addnode` on the same RPC
  service.

The lightwalletd user guide gives a safer localhost example:
`book/src/user/lightwalletd.md:55-63` uses `listen_addr = '127.0.0.1:8232'`
when disabling cookie auth. The Docker compose file does not preserve that
localhost-only boundary once it publishes the host port.

## Duplicate Check

Read-only GitHub searches on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "ZEBRA_RPC__ENABLE_COOKIE_AUTH=false" "8232:8232"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "Docker RPC" "cookie auth" "0.0.0.0"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "docker-compose.lwd.yml" "ENABLE_COOKIE_AUTH"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "RPC listen_addr" "0.0.0.0" "Docker" "cookie"'
```

Results:

- Exact `ZEBRA_RPC__ENABLE_COOKIE_AUTH=false` plus `8232:8232` search returned
  no hits.
- The closest overlap is merged PR #10464, which touched the same Docker and
  documentation files to expose the P2P port for inbound peer connections. Its
  motivation and solution do not describe unauthenticated host-published RPC or
  add the missing trusted-network warnings.
- Other broad searches returned closed #9904, #9768, and #9344. They are
  configuration/docs changes or unrelated RPC behavior, not duplicate reports
  of this exposure pattern.

## Local Proof Status

Source-evidence-only. This is a shipped configuration/documentation exposure
pattern rather than a single code path. The runtime RPC hardening implications
are covered by the separate RPC notes in the local ledger.

## Impact

Severity: medium operational exposure in copied Docker deployments.

If the Docker host is reachable from an untrusted network, unauthenticated
callers can access every enabled Zebra JSON-RPC method. Practical effects
include:

- submitting arbitrary transactions to the node's mempool path,
- forcing expensive block-template and block-submission validation work,
- stopping the node through the `stop` RPC,
- reading node/network/peer information, and
- amplifying the already documented RPC batch, method-label cardinality, and
  long-running verifier-timeout hardening issues.

The risk is configuration-dependent. It does not affect default `zebrad`
without these Docker settings, and the regular RPC default still has cookie auth
enabled.

## Suggested Fix

Prefer internal-only RPC for examples that need unauthenticated lightwalletd or
observability access:

- bind Zebra RPC to `127.0.0.1` when publishing a host port,
- or do not publish the Zebra RPC port at all and let peer containers reach it
  over a private Compose network,
- keep cookie auth enabled whenever the RPC port is published to the host,
- add comments beside any `enable_cookie_auth=false` example saying it must not
  be exposed outside a trusted local/container network, and
- in the general Docker docs, use `127.0.0.1:8232` as the first RPC example and
  make `0.0.0.0` a warned advanced choice.

For `docker/docker-compose.lwd.yml`, the narrowest change is likely to remove
the `"8232:8232"` host mapping and keep RPC reachable only from the
`lightwalletd` container. If host access is required for debugging, publish it
as `"127.0.0.1:8232:8232"`.

## Confidence

Confidence: high that the examples disable auth and bind/publish RPC broadly;
medium on real-world exposure because Docker host firewalling and local network
placement vary by operator.
