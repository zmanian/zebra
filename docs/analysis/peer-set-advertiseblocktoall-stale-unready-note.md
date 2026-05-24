# PeerSet AdvertiseBlockToAll Stale Unready Note

Date: 2026-05-09

Status: local-only. Do not post publicly without explicit re-authorization.

## Summary

`PeerSet::broadcast_all()` queues an `AdvertiseBlockToAll` retry for peers that
were unready at the moment of the broadcast. The queued state only removes a
peer when that peer later becomes ready and receives the replayed request, or
when the peer's IP is currently banned. It does not retire peers that disconnect
or are removed while still unready, and it does not drop queued state when the
original caller is no longer interested.

That can keep an `AdvertiseBlockToAll` future pending indefinitely after peer
churn. The issue is externally influenced through P2P timing/disconnects, but
the impact is bounded to best-effort block advertisement completion and peer-set
resource/lifetime cleanup. It is an availability hardening issue, not consensus
or validation correctness.

## Evidence

The peer set stores queued broadcast state as:

- `zebra-network/src/peer_set/set.rs:241-245`

`broadcast_all()` sends to ready peers immediately, then waits until the queued
broadcast channel closes:

- `zebra-network/src/peer_set/set.rs:1129-1143`

If any peers are unready, `queue_broadcast_all_unready()` stores all keys from
`cancel_handles` in `remaining_peers`:

- `zebra-network/src/peer_set/set.rs:1147-1167`

`broadcast_all_queued()` later removes banned IPs and removes peers only when
they are present in `ready_services`:

- `zebra-network/src/peer_set/set.rs:1171-1195`

If the channel has no send slot, the queued state is restored wholesale:

- `zebra-network/src/peer_set/set.rs:1178-1181`

The normal removal path cancels unready work, but it does not remove the key
from `queued_broadcast_all.remaining_peers`:

- `zebra-network/src/peer_set/set.rs:804-813`

When the canceled unready service resolves, `poll_unready()` logs and drops the
service, but it also does not prune queued broadcast state:

- `zebra-network/src/peer_set/set.rs:586-603`

So a peer that was unready when `AdvertiseBlockToAll` was queued and then
disconnects can remain in `remaining_peers` forever. Since no sender is dropped
while `remaining_peers` stays non-empty, the caller's receiver can wait forever.

Current-behavior proof:

- `zebra-network/src/peer_set/set/tests/vectors.rs` now includes
  `queued_broadcast_all_keeps_removed_unready_peer_today`.
- The test inserts a queued broadcast entry for a peer with an unready cancel
  handle, calls the normal `remove()` path for that peer, and confirms:
  - the cancel handle is removed;
  - the queued broadcast sender remains open;
  - the removed peer remains in `remaining_peers`;
  - a later `broadcast_all_queued()` retry sends an empty follow-up future but
    still keeps the stale peer in queued state.

Verification:

```sh
cargo test -p zebra-network queued_broadcast_all_keeps_removed_unready_peer_today --lib
```

Result on 2026-05-09: passed.

RepoPrompt's async/backpressure slice independently selected this as a fresh
candidate.

## Duplicate Check

Local docs only had an adjacent mention in
`docs/analysis/peer-set-ban-watch-lazy-disconnect-note.md`, focused on ban-watch
laziness and delayed disconnect behavior.

GitHub search on 2026-05-09 found a partial closed duplicate:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "queued_broadcast_all" OR "AdvertiseBlockToAll"'
```

Relevant result:

- #10258, `Fix/peerset banned cancel handles`

#10258 fixed the banned-peer version of this hang by removing stale cancel
handles for banned unready peers and filtering banned peers from
`remaining_peers`. Current source still only prunes banned IPs; it does not
prune ordinary disconnected/removed peers or closed receivers.

Additional exact searches returned no issue titles:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "broadcast_all_queued" "disconnect"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "AdvertiseBlockToAll" "disconnect"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "queued broadcast" "disconnect"'
```

## Impact

Suggested severity: low availability hardening.

A remote peer can influence this by being unready during a block advertisement
and then disconnecting or otherwise being removed before it becomes ready. The
stale key can keep the returned broadcast future pending. This does not prevent
other ready peers from receiving the immediate advertisement, and newer queued
broadcasts replace older queued state, so the blast radius is limited.

The finding matters most for callers that await `AdvertiseBlockToAll` completion
as part of a larger workflow, and for long-running peer-set hygiene where stale
queued state can survive after the relevant peer is gone.

## Suggested Fix Direction

- Replace the queued-broadcast tuple with a small named state struct.
- On every `poll_ready()`, prune queued peers that are no longer in
  `ready_services` or `cancel_handles`.
- Drop queued state if the result sender is closed.
- Keep the existing banned-IP pruning.
- Add a timeout because queued block advertisements are best-effort follow-up
  work.
- Add regression tests for:
  - unready peer disconnect/removal before becoming ready;
  - caller drops the returned receiver before replay completes;
  - banned peer pruning stays covered by #10258's intended behavior.

## Confidence

Confidence is high that current source can retain disconnected unready peer keys
in queued broadcast state. Confidence is medium on operational severity because
the affected request is best-effort block advertisement and immediate ready-peer
broadcast still happens.
