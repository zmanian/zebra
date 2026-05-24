# P2P Peer-Path `try_send` Backpressure Audit

Date: 2026-05-09

## Summary

This pass checked Zebra's peer-path `try_send()` sites for silent loss of
score-bearing, disconnect, backpressure, or peer-demand signals.

Current conclusion: no new independent vulnerability was found in the
`zebra-network` peer-path sites. The audited paths either fail explicitly, time
out and report peer failure, preserve an already-queued demand token, or defer
disconnect enforcement until the next peer-set poll. The stronger confirmed
lossy signal issue remains the separate `zebrad` misbehavior-report transport
covered in `docs/analysis/misbehavior-reporting-lossy-channel-note.md`.

## Scope

Primary files:

- `zebra-network/src/peer/client.rs`
- `zebra-network/src/peer/handshake.rs`
- `zebra-network/src/peer_set/set.rs`
- `zebra-network/src/peer_set/initialize.rs`
- `zebra-network/src/peer_set/stall_tracker.rs`

Duplicate and boundary references:

- `docs/analysis/p2p-peer-set-unready-availability-note.md`
- `docs/analysis/misbehavior-reporting-lossy-channel-note.md`
- `docs/analysis/p2p-unknown-command-buffer-stranding-note.md`

## Site Classification

| Site | Signal kind | Current behavior | Triage |
| --- | --- | --- | --- |
| `Client::call()` request enqueue | outbound peer request | disconnected channel returns a peer error; full channel panics as Tower readiness contract violation | eliminated as silent-drop path |
| `send_one_heartbeat()` heartbeat enqueue | peer liveness and failure reporting | full channel falls back to awaited `send()`, then outer heartbeat timeout reports peer failure | eliminated as silent-drop path |
| `PeerSet::poll_ready()` `demand_signal.try_send(MorePeers)` | more-peer demand | full channel preserves existing queued demand; extra demand is duplicate elision | overlaps existing unready-peer availability hardening |
| `dial()` failed outbound attempt redemand | replacement demand after failed dial | failed dial requeues the consumed demand token when the channel has room | eliminated as unique-token loss path |
| `route_p2c()` stall events | disconnect after repeated empty/failed `FindBlocks` or `FindHeaders` responses | unbounded event channel; enforcement is deferred until next peer-set poll | eliminated as bounded-channel drop path |

## Proof Tests

```sh
cargo test -p zebra-network full_more_peers_channel_preserves_existing_demand_today --lib
cargo test -p zebra-network failed_dial_requeues_consumed_demand_token_today --lib
cargo test -p zebra-network stall_events_are_deferred_until_next_poll_then_disconnect_today --lib
cargo test -p zebra-network client_call_on_disconnected_server_tx_returns_error_today --lib
cargo test -p zebra-network heartbeat_full_server_tx_times_out_and_reports_error_today --lib
```

Result on 2026-05-09: all five focused tests passed.

## Findings

### Demand signal loss is duplicate-only in the direct full-channel case

`PeerSet::poll_ready()` sends `MorePeers` when there are no ready services. It
uses `try_send()` and intentionally ignores a full channel to avoid deadlocking
against the crawler, which is also the receiver.

The `full_more_peers_channel_preserves_existing_demand_today` test fills the
demand channel until `try_send()` reports `is_full()`, polls an otherwise empty
peer set, then drains the receiver. The queued demand count is unchanged. This
proves the direct full-channel branch does not destroy the already-queued
liveness token.

This does not eliminate the broader availability class where busy unready peers
can reduce effective peer capacity. That remains covered by
`docs/analysis/p2p-peer-set-unready-availability-note.md` and public issue
#7822.

### Failed dials requeue the consumed demand token

`dial()` consumes a demand signal to attempt an outbound connection. On
handshake failure, it reports the failed candidate to the address book updater
and tries to put one `MorePeers` token back into the demand channel.

The `failed_dial_requeues_consumed_demand_token_today` test calls `dial()`
directly with a connector that always errors and a demand channel with room. The
test confirms no peer is emitted, an address-book failure update is sent, and a
replacement demand token is queued.

### Stall disconnects are poll-deferred, not dropped

`FindBlocks` and `FindHeaders` responses are classified into stall or clear
events. The event transport is an unbounded Tokio channel, and
`PeerSet::poll_ready()` drains those events before checking peer readiness.

The `stall_events_are_deferred_until_next_poll_then_disconnect_today` test sends
the stall threshold of events for a ready peer. Before the next poll, the peer is
still present. After one peer-set poll, the peer is removed and its cancel
handle is absent. This is a poll-gated enforcement model, not a dropped-signal
model.

### Per-peer request and heartbeat channels fail closed

`Client::call()` uses `server_tx.try_send()`, but a disconnected request channel
returns a peer error to the caller. A full channel still panics because callers
are expected to obey the Tower `poll_ready()` contract.

`send_one_heartbeat()` also uses `try_send()`, but on `is_full()` it awaits
`send()` and relies on the outer heartbeat timeout to close/report the peer if
that wait takes too long.

The client and heartbeat tests confirm both paths surface explicit failure
rather than silently dropping the request or heartbeat.

## Triage Outcome

No new public issue recommended from this pass.

The only surviving availability concern is still the previously documented
unready-peer capacity issue. The peer-path `try_send()` sites did not produce a
distinct score-loss, disconnect-loss, or unique-demand-loss vulnerability.
