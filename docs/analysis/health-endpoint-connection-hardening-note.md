# Health endpoint connection hardening note

Date: 2026-05-02

Last updated: 2026-05-09

Disposition: local-only. Do not post publicly without explicit
re-authorization and a fresh duplicate check.

## Summary

Zebra's optional health HTTP endpoint has a burst-style accept counter, but the
counter is applied at TCP accept time, not HTTP request time. It also does not
have a global open-connection cap or a per-connection request/header timeout. A
slow client can therefore hold accepted HTTP/1 connection tasks open without
sending a complete request, and a keep-alive client can send multiple HTTP
requests over one accepted socket while consuming only one unit of the burst
counter. Because the counter resets every interval, the number of live idle
connection tasks can grow over time if the endpoint is bound to a shared or
hostile network.

There is also a small log-amplification edge on `/ready`: when Zebra is close
enough to tip but the chain-tip metrics receiver still has
`remaining_sync_blocks = None`, every `/ready` request emits a WARN-level log.
With Sentry enabled, WARN events are exported as Sentry logs and breadcrumbs.
That makes this a reachable observability-amplification path for exposed health
endpoints during that startup/sync state.

This is a public availability hardening issue, not a private consensus issue.
The health endpoint is disabled by default and documented as an unauthenticated
internal probe surface.

## Duplicate Check

Read-only GitHub searches refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "health endpoint" "idle connection timeout"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "health server" "connection limit"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "ready endpoint" "warn log amplification"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "health HTTP" "keep-alive" "rate limit"'
```

Result: no hits returned.

## Evidence

- `zebrad/src/components/health/config.rs` defaults `listen_addr` to `None`, so
  the health endpoint is opt-in.
- `zebrad/src/components/health.rs` documents the endpoint as unauthenticated by
  design and advises binding it to internal interfaces.
- `run_health_server()` accepts TCP streams in a loop and uses
  `MAX_RECENT_REQUESTS = 10_000` over `RECENT_REQUEST_INTERVAL = 5s` as a
  burst counter.
- Each accepted stream is handed to `http1::Builder::new().serve_connection(...)`
  in a spawned task.
- The rate limiter counts accepted streams in the current interval, but it does
  not track currently open streams. When the interval elapses, the counter is
  reset even if previous connection tasks are still alive.
- The rate limiter is global, not per client or per source. One noisy client can
  consume the five-second accept budget and make legitimate orchestrator,
  load-balancer, or monitoring probes receive dropped sockets until the interval
  resets.
- The rate limiter is not called from `handle_request()`, so HTTP/1 keep-alive
  requests multiplexed over a single accepted connection are not individually
  counted.
- I did not find a semaphore, idle timeout, request-header timeout, or body/read
  timeout around the health connection task.
- `zebrad/src/components/health/tests.rs` now has a local current-behavior
  proof, `idle_health_connection_waits_without_request_timeout_today`, showing
  an idle connection remains open without sending a request and can later send a
  normal `/healthy` request on the same socket.
- `ready()` logs `tracing::warn!("syncer is getting block hashes from peers, but state is empty")`
  when `remaining_sync_blocks` is `None`.
- With the `sentry` feature enabled and `SENTRY_DSN` configured, Zebra's Sentry
  tracing layer maps WARN events to `EventFilter::Log | EventFilter::Breadcrumb`.
- Zebra's `LastWarnErrorLayer` also stores WARN/ERROR event messages for the
  `getinfo.errors` diagnostic field. In this case the message is static, so it
  is not a data leak, but repeated `/ready` probes can keep refreshing that
  operator-facing warning.

Focused proof rerun on 2026-05-09:

```sh
cargo test -p zebrad idle_health_connection_waits_without_request_timeout_today --lib
```

Result: passed.

## Impact

If the health endpoint is exposed outside a trusted local network, an attacker
can open many connections slowly enough to stay under the per-interval burst
counter, keep them idle, and accumulate connection tasks and sockets. The
endpoint responses are small and the handlers are cheap, so this is not about
expensive `/ready` or `/healthy` work. The primary risk is idle connection
retention. A same-window burst variant can also spend the global accept budget
and starve legitimate health probes for the rest of the interval, which can look
like readiness or liveness failure to orchestration systems. The secondary risk
is repeated warning/log export while the node is in the specific "close to tip
but no chain-tip estimate" state.

This does not affect consensus, wallet funds, chain state, or peer-to-peer
validation. It is also not enabled by default.

## Suggested fix direction

- Add a global concurrent-connection semaphore for the health server.
- Add a short request/header timeout before serving each connection.
- Disable HTTP/1 keep-alive for this tiny probe endpoint, or move the burst
  counter into `handle_request()` so it actually limits handled HTTP requests.
- Rate-limit or downgrade the repeated `/ready` "state is empty" warning, for
  example by logging it once per interval while returning the same 503 body.
- Optionally lower the burst counter or rename it to make clear that it is an
  accept-rate guard, not an open-connection guard.
- Keep documenting the endpoint as internal-only unless authentication and
  stronger server limits are added.

## Reproducer sketch

Run Zebra with `health.listen_addr` bound on localhost. Open many TCP
connections to that port without sending a complete HTTP request, staying below
`MAX_RECENT_REQUESTS` during each five-second interval. Observe that each
accepted stream gets a spawned `serve_connection()` task and remains open until
the client closes the socket. Repeat after the interval resets and confirm that
new streams are accepted even though the prior idle streams are still live.

For the keep-alive variant, send repeated `GET /healthy` or `GET /ready`
requests on one HTTP/1 connection. The accept loop increments
`num_recent_requests` once for that socket, not once for every handled request.

For the `/ready` warning variant, probe during the state where peer/sync checks
pass but `remaining_sync_blocks` is still `None`; every request reaches the WARN
log branch. This should be tested with Sentry disabled locally, then reasoned
about through the Sentry event filter rather than sending data off-host.

Regression-test shape: configure a small test-only connection cap and a short
request-header timeout, open more than the cap worth of idle TCP streams, and
assert that extra streams are rejected or timed out promptly while ordinary
`GET /healthy` still succeeds.

## Disclosure triage

Public hardening issue.

Confidence: high that the current counter is an accept/socket counter rather
than a handled-request counter. Confidence: medium-high that the endpoint lacks
an open-connection cap and request timeout. Confidence: medium on the `/ready`
log-amplification state because it depends on timing during startup/sync.
Practical impact remains medium-low because the endpoint is disabled by default
and intended for internal probes.
