# Peer Set Ban Watch Lazy Disconnect Note

Date: 2026-05-04

Last updated: 2026-05-09

Disposition: local-only residual of #9201/#10258. Do not post publicly without
explicit re-authorization.

Scope: propagation of address-book misbehavior bans into already-connected
peer-set services.

## Summary

Zebra publishes address-book bans through a `watch::Receiver`, and the peer set
consults the current ban map before keeping ready or newly-ready services. But
the peer set does not appear to register a `bans_receiver.changed()` future, so a
new ban does not by itself wake the peer set to drop an already-ready service.

The result is a lazy disconnect: active services for banned IPs are dropped on a
later `PeerSet::poll_ready()` pass, not immediately when the address-book updater
publishes the ban.

This is public P2P hardening. It is not a consensus issue and not a strong
private-disclosure candidate on current evidence.

## Evidence

- `zebra-network/src/address_book_updater.rs:101-116` applies address-book
  changes and sends the updated ban map over a `watch` channel when an
  `UpdateMisbehavior` event causes the event IP to be present in `bans_by_ip`.
- `zebra-network/src/peer_set/initialize.rs:221-233` passes the
  `bans_receiver` into `PeerSet`.
- `zebra-network/src/peer_set/set.rs:560-573` drops an unready service when it
  becomes ready and its IP is in the current ban map.
- `zebra-network/src/peer_set/set.rs:634-651` also drops a ready service during
  ready-peer error polling if its IP is in the current ban map.
- `zebra-network/src/peer_set/set.rs:1176-1178` filters banned IPs out of queued
  `AdvertiseBlockToAll` broadcasts.
- `zebra-network/src/peer_set/set.rs` has no `bans_receiver.changed()` or
  `borrow_and_update()` path; the peer set only samples the current map with
  `borrow()` while it is already being polled.
- `zebra-network/src/peer/connection.rs:710-779` shows a ready peer connection
  continues to process inbound peer messages while awaiting local client
  requests. The individual connection service does not consult the address-book
  ban map.

## Impact

Once a ban has been published, future inbound connection attempts are blocked in
`accept_inbound_connections()`, and newly-ready or ready-polled services are
dropped by the peer set. The gap is timing: if a peer becomes banned while its
service is already ready, that connection can remain alive until some later
network request, discovery event, inventory event, stall event, readiness check,
or other peer-set poll occurs.

During that interval, the connection task can still process inbound messages
from the banned peer. This could allow a recently banned peer to continue
creating inbound work for longer than intended on an otherwise quiet node.

Bounds and mitigations:

- misbehavior updates are already batched and flushed every 30 seconds, so this
  is an additional lazy-disconnect delay rather than the first delay in the ban
  path;
- active nodes make frequent peer-set requests, which should trigger the existing
  ban checks promptly;
- invalid blocks or transactions are still rejected;
- overload and timeout handling can still drop abusive inbound request streams.

## Suggested Fix Direction

- Add an explicit ban-change polling path in `PeerSet::poll_ready()`, using
  `bans_receiver.has_changed()` / `borrow_and_update()` or a `changed()` future
  pattern that registers a wakeup when the ban map changes.
- When a ban change is observed, scan `ready_services` and `cancel_handles` for
  matching IPs, drop ready services, and cancel unready work.
- Add a regression test that starts with a ready peer, publishes a ban through the
  watch sender without any discovery change, and asserts the peer is removed on
  the next ban-change-driven poll.

## Duplicate Check

Read-only duplicate search refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra peer set ban watch lazy disconnect'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "ban watch" "peer set"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "bans_receiver"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "misbehavior" "ban" "disconnect" "peer"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "banned" "disconnect" "peer set"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "banned IP" "ready" "peer"'
gh api repos/ZcashFoundation/zebra/pulls/9201
gh api repos/ZcashFoundation/zebra/pulls/10258
```

Closest hits:

- PR #9201 introduced the address-book misbehavior ban channel and states that
  `PeerSet::poll_ready()` drops connections to banned IPs. It does not document
  a ban-change wakeup/eager-disconnect mechanism.
- PR #10258 fixed stale cancel handles for banned unready peers and queued
  broadcasts. It is related peer-set ban cleanup work, but not a duplicate of
  the ready-peer lazy-disconnect residual.
- Search for exact `bans_receiver` / ban-watch phrasing did not find a public
  duplicate issue.

## Proof Status

Local current-behavior proof added and rerun on 2026-05-09:

```sh
cargo test -p zebra-network ban_watch_update_does_not_drop_ready_peer_until_peer_set_polled_today --lib
```

Result: passed. The test publishes a ban for an already-ready peer and confirms
the watch update alone leaves the peer in `ready_services`; a subsequent
`PeerSet::poll_ready()` samples the updated ban map and drops the peer.

## Triage

Severity: low.

This refines the earlier ban-propagation notes: active services are eventually
dropped through peer-set polling, but the watch update does not appear to be a
direct wakeup/disconnect mechanism. Treat as public availability hardening unless
follow-up testing shows a quiet-node workload can keep a banned peer doing
material verification work for an extended period.
