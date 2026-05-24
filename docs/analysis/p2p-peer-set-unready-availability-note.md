# P2P Peer-Set Unready Availability Note

Date: 2026-05-02

Last updated: 2026-05-09

Disposition: already publicly tracked by
[#7822](https://github.com/ZcashFoundation/zebra/issues/7822). Do not post a
new public issue.

Scope: follow-up on pass-5 peer-set availability leads in
`zebra-network/src/peer_set/initialize.rs`,
`zebra-network/src/peer_set/set.rs`,
`zebra-network/src/peer/connection.rs`, and
`zebra-network/src/peer_set/inventory_registry.rs`.

## Finding

The peer-set max-size panic, crawler demand channel, and inventory registry do
not appear remotely unbounded in the reviewed production paths. Inbound and
outbound connection admission happens before peer insertion, crawler demand is
bounded, and inventory tracking has explicit map limits.

The remaining availability lead is narrower: Zebra has no explicit peer-set
level age limit for peers that remain unready because they keep their connection
busy with inbound messages. Existing comments expect these peers to time out or
close after a few minutes, but the code also has a TODO to drop peers that
overload Zebra with inbound messages and never become ready.

This is not a consensus issue. It is a bounded P2P availability hardening item.
A distributed attacker may be able to occupy a high fraction of available peer
slots with peers that handshake successfully and then delay readiness, reducing
the ready peer pool until timeout, overload handling, or connection limits clear
the slots.

## Evidence

- `zebra-network/src/peer_set/set.rs:495-499` says connected peers should become
  ready within a few minutes or timeout, then leaves `TODO: drop peers that
  overload us with inbound messages and never become ready`.
- `zebra-network/src/peer/connection.rs:740-751` documents that a peer with
  inbound messages will delay Zebra-originated requests to that peer, then uses
  `future::select(peer_rx.next(), self.client_rx.next())`.
- `zebra-network/src/peer_set/initialize.rs:650-656` rejects inbound
  connections when the inbound connection limit or recent-per-IP limiter is
  exceeded.
- `zebra-network/src/peer_set/initialize.rs:213-216` bounds the `MorePeers`
  demand channel by the outbound connection limit.
- `zebra-network/src/peer_set/initialize.rs:897-904` handles potentially
  unlimited demand last in the biased select and drops demand once outbound
  connections reach the configured limit.
- `zebra-network/src/peer_set/initialize.rs:924-968` reserves an outbound
  connection slot before spawning each demand-triggered dial or crawl task.
- `zebra-network/src/peer_set/set.rs:1310-1325` keeps a hard panic assertion if
  ready plus unready peers exceed the total peer-set connection limit.
- `zebra-network/src/peer_set/inventory_registry.rs:53` caps each inventory map
  at 1,000 hashes, and `zebra-network/src/peer_set/inventory_registry.rs:68`
  caps retained peers per inventory hash at 70.
- Local current-behavior test added during this audit:
  `zebra-network/src/peer_set/set/tests/vectors.rs`
  `unready_peer_with_full_request_channel_is_not_age_evicted_today`.
  It keeps a mock peer's request channel saturated, advances Tokio time by ten
  minutes, and confirms the peer remains in the unready set with its cancel
  handle rather than being age-evicted.

## Eliminated Hypotheses

**Remote-triggered peer-count panic**

No reviewed remote-only path inserts peers past the total peer-set limit before
admission checks. The panic remains a useful internal invariant, but the
reviewed production inbound and outbound paths gate connections before
insertion. A buggy internal `Discover` source could still violate the invariant,
so a pre-insert drop guard would be good defensive hardening.

**Unbounded crawler demand**

No unbounded queue or task growth was found. Demand is sent with `try_send`,
the demand channel is sized from the outbound connection limit, and demand is
dropped while the outbound tracker is at limit.

**Unbounded inventory registry growth**

No unbounded inventory memory growth was found. The registry keeps current and
previous maps, each capped at 1,000 hashes, with at most 70 peers per hash.
There is no per-peer fair-share quota across all hashes, but the global caps
dominate memory growth.

**Peer `mempool` message full-ID response**

An inbound `mempool` message makes Zebra collect all mempool transaction IDs in
`zebrad/src/components/mempool.rs:770-777`. With default configuration, the
80,000,000 byte ZIP-401 cost limit and 10,000 minimum transaction cost imply
about 8,000 retained transaction IDs, below the 25,000 transaction-inventory
message cap in `zebra-network/src/protocol/external/inv.rs:201`. This is an
O(n) peer-triggered operation, but it is bounded by mempool policy in normal
configuration. If operators substantially raise `mempool.tx_cost_limit`, it
becomes a public rate-limit/per-peer-work hardening item.

## Suggested Fix

Add explicit unready-age eviction in `PeerSet`:

- track the instant a peer enters the unready set,
- disconnect peers that remain continuously unready beyond a conservative
  threshold,
- choose the threshold relative to existing request/keepalive timeouts so slow
  honest peers are not dropped too aggressively.

Optional hardening:

- reject or drop `Discover::Insert` updates when peer-set occupancy is already
  at the total connection limit, before the metrics panic can fire,
- add fairness in the connection loop so continuous inbound messages cannot
  indefinitely delay Zebra-originated requests to the same peer,
- add a cheap per-peer or per-interval cap for inbound `mempool` full-ID
  requests if operators raise mempool capacity.

## Suggested Tests

- Fill the peer set with mock services that enter unready and never become
  ready; assert stale unready peers are evicted after the threshold.
- Add an over-producing `Discover` stream test; assert excess inserts are
  dropped and `update_metrics()` does not panic.
- Saturate crawler demand; assert in-flight dial/crawl work never exceeds the
  outbound connection limit and demand does not accumulate unboundedly.
- Add a noisy inbound peer test; assert Zebra-originated requests either make
  progress or the peer is eventually disconnected.

## Confidence

Confidence: medium-high as public availability hardening.

The code evidence is direct for the missing unready-age eviction and for the
existing bounds around connection admission, demand, and inventory state. The
remaining uncertainty is practical exploitability: default per-IP and total
peer limits bound the blast radius, and overload/timeout behavior may already
disconnect many noisy peers. The missing explicit unready-age cap is still worth
fixing because it makes that availability invariant local and testable.

## Duplicate Check

Read-only duplicate search refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra peer set unready inbound messages never ready'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "peer set" "unready" "inbound"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "unready" "never ready" "peer"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "drop peers" "overload" "never become ready"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "Discover::Insert" "peer set" "limit"'
```

Results:

- #7822 is an exact public tracking issue for synthetic nodes occupying
  connection slots. Its body explicitly lists the overload scenario where peers
  block readiness by constantly sending inbound requests, and includes the
  unchecked fix item "Drop peers that overload us with inbound messages and
  never become ready".
- #1435 and PR #7859 are historical network-hang context.
- #6936, #1965, #10248, #10024, #1405, and related hits are broad or adjacent,
  not more specific than #7822 for this lead.

## Proof Status

Current-behavior proof added and run on 2026-05-09:

```sh
cargo test -p zebra-network unready_peer_with_full_request_channel_is_not_age_evicted_today --lib
```

Result: passed. The test proves the peer-set layer has no elapsed-time eviction
for a continuously unready peer in this mock shape. Practical exploitability is
still bounded by connection admission, per-IP limits, connection timeout
behavior, overload handling, and the broader #7822 synthetic-node tracking work.
