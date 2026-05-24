# RepoPrompt Async Backpressure Slice

Date: 2026-05-09

Status: local-only. No public issue or private disclosure candidate promoted.

## Summary

I ran a focused RepoPrompt slice over async backpressure, cancellation,
timeouts, and task lifetimes across `tower-batch-control`, `tower-fallback`,
primitive verifiers, `PeerSet`, RPC, and mempool background components.

The pass produced two new local-only availability hardening notes:

- `docs/analysis/halo2-batch-queue-weight-admission-note.md`
- `docs/analysis/peer-set-advertiseblocktoall-stale-unready-note.md`

It also repeated two already-known RPC themes:

- verifier-bound `submitblock` / proposal-mode `getblocktemplate` waits are
  covered by `docs/analysis/submitblock-timeout-security-note.md` and public
  #9301;
- the live `RpcServer::start()` path does not invoke cookie cleanup on task
  abort, covered by `docs/analysis/rpc-cookie-lifecycle-cleanup-note.md`.

One RepoPrompt hypothesis was demoted: aborting the returned RPC waiter task
does not appear to leave the jsonrpsee listener alive, because jsonrpsee
documents and implements `Server::start()` such that the server runs until the
`ServerHandle` is stopped or dropped. Zebra's spawned waiter future owns that
handle, so aborting the task drops it. The remaining live-path problem is
cleanup ownership, especially cookie cleanup, not an independently leaked RPC
listener.

## RepoPrompt Candidates

| Candidate | Disposition | Notes |
| --- | --- | --- |
| Halo2 request weight is used for batch flushing but not queue admission | New local-only note | The queue semaphore is per request, while `halo2::Item::request_weight()` is per Orchard action. This can admit more Halo2 work than the nominal batch budget implies. |
| `AdvertiseBlockToAll` queued-unready peers can keep futures pending | New local-only note | #10258 fixed banned-peer stale cancel handles, but current source still does not prune disconnected/removed unready peers or closed receivers from queued broadcast state. |
| RPC server ownership loses the listener handle | Eliminated as stated | jsonrpsee stops when the `ServerHandle` is dropped. The Zebra task owns the handle through `.stopped().await`, so aborting the task drops the handle. Cookie cleanup remains a duplicate of the existing lifecycle note. |
| RPC verifier waits lack local timeout | Duplicate | Covered by `submitblock-timeout-security-note.md` and public #9301. |
| Batch worker/fallback panic lifecycle | Folded into existing primitive note | Existing `primitive-verifier-failure-taxonomy-note.md` covers panic-on-drop branches, fallback CPU/metrics drift, and error taxonomy. No new production source-to-sink found. |
| Mempool crawler / queue checker waits | Eliminated | Crawler has peer and outer crawl timeouts. Queue checker is lower risk because the request is lightweight and tied to normal mempool `poll_ready()` progress. |

## Source Checks

### Halo2 Weighted Admission

`RequestWeight` is dynamic, but `Batch::poll_ready()` admits one request per
permit:

- `tower-batch-control/src/service.rs:234-293`
- `tower-batch-control/src/worker.rs:145-153`
- `zebra-consensus/src/primitives/halo2.rs:60-67`

See `docs/analysis/halo2-batch-queue-weight-admission-note.md`.

### Queued Broadcast Lifetime

Queued `AdvertiseBlockToAll` state keeps a `remaining_peers` set and only
shrinks it when peers become ready or are banned:

- `zebra-network/src/peer_set/set.rs:1129-1195`

The remove/cancel paths do not prune that set:

- `zebra-network/src/peer_set/set.rs:804-813`
- `zebra-network/src/peer_set/set.rs:586-603`

See `docs/analysis/peer-set-advertiseblocktoall-stale-unready-note.md`.

### RPC Listener Hypothesis

Zebra starts jsonrpsee as:

- `zebra-rpc/src/server.rs:144-161`

The jsonrpsee dependency says `Server::start()` runs until the `ServerHandle` is
stopped or dropped:

- `jsonrpsee-server-0.24.10/src/server.rs:116-123`

`ServerHandle::stopped(self)` consumes the handle and waits for the server to
stop:

- `jsonrpsee-server-0.24.10/src/future.rs:74-91`

Since the spawned waiter future owns the consumed handle, aborting that future
drops the handle. That invalidates the stronger "listener survives abort"
version of the RepoPrompt finding.

## Duplicate Checks

GitHub searches run:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "RequestWeight" "Halo2"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "batch" "Halo2" "queue"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "queued_broadcast_all" OR "AdvertiseBlockToAll"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "broadcast_all_queued" "disconnect"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "AdvertiseBlockToAll" "disconnect"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "queued broadcast" "disconnect"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "RpcServer::start" "ServerHandle"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "submitblock" "timeout" "getblocktemplate"'
```

Material results:

- #10258 is a partial closed duplicate for banned queued broadcasts, but not for
  ordinary disconnect/removal or closed receivers.
- #9301 is the existing public issue for `getblocktemplate` DoS/validation
  parity and is adjacent to the submitblock/proposal timeout note.

No public GitHub issue, comment, or advisory was posted from this slice.
