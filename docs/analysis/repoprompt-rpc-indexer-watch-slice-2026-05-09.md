# RepoPrompt RPC, Indexer, And Watch Slice

Date: 2026-05-09

Status: local audit note only. Do not post publicly without explicit user
direction.

Scope: a RepoPrompt-assisted follow-up over JSON-RPC compatibility and method
dispatch, the optional indexer gRPC server, trusted indexer sync, watch/listener
plumbing, and optional health/metrics/tracing endpoints.

## Summary

RepoPrompt returned five candidates:

1. JSON-RPC browser-origin request forgery when cookie auth is disabled and the
   compatibility layer rewrites `text/plain` to `application/json`;
2. health endpoint probe starvation through the global accept counter;
3. unauthenticated and unbounded tracing filter reload endpoint;
4. `TrustedChainSync` accepting malformed indexer messages / losing finalized
   tip forwarding;
5. metrics endpoint idle-connection retention.

After cross-checking, none are new private-disclosure candidates from this
slice. All five map onto existing local notes. The only ledger update from this
pass was to make the health endpoint note explicitly call out the practical
"one noisy client can starve real probes for the interval" variant.

## Candidate Triage

| Candidate | Verdict | Existing coverage |
| --- | --- | --- |
| JSON-RPC CSRF with disabled cookie auth | Duplicate | `docs/analysis/rpc-text-plain-csrf-hardening-note.md` already documents the `text/plain` rewrite, auth-disabled browser-origin shape, local proof, and fix direction. |
| Health endpoint probe starvation | Covered with small update | `docs/analysis/health-endpoint-connection-hardening-note.md` already covers the global accept counter, missing open-connection cap, and missing request timeout. This pass added explicit probe-starvation wording. |
| Tracing filter endpoint unauthenticated / unbounded | Duplicate | `docs/analysis/tracing-filter-endpoint-security-note.md` covers unauthenticated `POST /filter`, unbounded body collection, local proof, and fix direction. `docs/analysis/tracing-filter-reload-telemetry-amplification-note.md` covers the Sentry/OpenTelemetry composition. |
| `TrustedChainSync` malformed indexer messages / tip freeze | Duplicate | `docs/analysis/trusted-chain-sync-indexer-validation-boundary-note.md` covers hash/body mismatch and skipped recent-chain validation with end-to-end proofs. `docs/analysis/trusted-chain-sync-best-tip-forwarder-note.md` covers the permanent finalized-tip helper exit. |
| Metrics endpoint idle connections | Duplicate | `docs/analysis/metrics-endpoint-connection-hardening-note.md` covers dependency-owned Prometheus listener behavior, idle accepted connections, and missing Zebra-side allowlist/timeout/cap. |

## Health Probe-Starvation Addendum

The health endpoint nuance was worth recording because it describes the likely
operational symptom more directly than "accept-rate counter":

- `zebrad/src/components/health.rs:306-329` keeps a single
  `num_recent_requests` counter for the whole listener.
- When `MAX_RECENT_REQUESTS` is exhausted and `RECENT_REQUEST_INTERVAL` has not
  elapsed, the accept loop `continue`s, dropping the newly accepted socket
  without a `429` response.
- The counter is not keyed by remote address, connection identity, endpoint
  path, or caller purpose.

So one noisy client on an exposed health port can spend the shared five-second
budget and make legitimate `/healthy` or `/ready` probes fail until the next
counter reset. This is still public operational hardening: the endpoint is
opt-in and intended for internal probes, and the existing note already covers
stronger idle-connection accumulation.

## Eliminated Near-Misses

- `BlockAndHash::decode()` mismatch acceptance by itself is not new. The
  relevant end-to-end impact is already in the trusted-sync validation-boundary
  note.
- `NonFinalizedBlocksListener::unwrap()` remains an internal service-contract
  footgun. Current production state service creates a fresh listener per request
  and returns it by value, so a remote indexer client cannot clone the listener
  before the indexer method consumes it.
- JSON-RPC response compatibility buffering was already analyzed. The remaining
  hardening idea is a local response-size guard around the compatibility rewrite,
  but no response-cap bypass was promoted here.
- Optional endpoint bind panics remain startup/config fail-fast behavior, not a
  request-controlled vulnerability.

## Verification

Commands and checks used during this pass:

```sh
rp-cli -w 1 -e 'builder "Deep Zebra security audit slice: RPC/indexer/watch surfaces..." --response-type plan'
rg -n "indexer|watch|stream|mempool|grpc|rpc|z_get|z_list|getrawtransaction|getaddress|getblock|longpoll|snapshot|cookie|batch|MempoolChange" docs/analysis zebra-rpc/src zebra-state/src zebra-node-services/src zebrad/src --glob '!target/**' -S
rg -n "unwrap\\(|expect\\(|panic!|assert!|unreachable!" zebra-rpc/src zebrad/src/components/health.rs zebrad/src/components/metrics.rs zebrad/src/components/tracing --glob '!**/tests/**' --glob '!**/tests.rs' --glob '!**/snapshots/**' -S
rg -n "response_from_json_rpc_2|response body|max_response_body_size|compatibility.*response|JSON-RPC 1.0|into_version" docs/analysis zebra-rpc/src/server -S
```

No Rust tests were run for this pass because it only updated local analysis
notes and duplicate triage. The existing per-finding notes list the focused
current-behavior tests that were previously run for each candidate.
