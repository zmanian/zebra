# P2P Inventory Routing Poisoning Note

Date: 2026-05-03

Status: reported publicly as
[#10560](https://github.com/ZcashFoundation/zebra/issues/10560).

## Finding

Zebra's peer inventory registry is bounded and expires quickly, so the earlier
"unbounded inventory growth" hypothesis remains eliminated. But the routing
state can still be attacker-influenced in a more targeted way: inbound
`notfound` messages are registered as missing inventory before the peer
connection state machine determines whether the message was solicited.

This means a peer can mark its own transient address as missing for
attacker-chosen block or transaction inventory without Zebra having requested
that inventory from the peer. The effect is time-bounded and self-scoped, but it
can steer single-hash inventory routing away from peers or make Zebra fail fast
with a synthetic `NotFoundRegistry` when all ready peers are marked missing.

There is a second related request-correlation issue: while Zebra is waiting for
a `BlocksByHash` or `TransactionsById` response, an unrelated `notfound` message
is still accepted as the response. Zebra logs that the `notfound` did not match
the pending request, but then completes the request using the pending hashes as
missing inventory.

That second path has two separate inventory-state effects. The handshake wrapper
registers the hashes named in the inbound `notfound` payload before response
correlation. Then, if the connection handler completes the active request as
`NotFoundResponse` or as a partial response with missing entries, the client-side
`MissingInventoryCollector` can register the originally requested hashes as
missing for that same peer.

## Evidence

- `zebra-network/src/peer/handshake.rs:1192-1260` runs
  `register_inventory_status()` on every inbound `Message::Inv` and
  `Message::NotFound`, converts `notfound` entries into
  `InventoryChange::new_missing_multi()`, and keys the update by
  `connected_addr.get_transient_addr()`.
- `zebra-network/src/peer/connection.rs:1213-1224` later treats a `notfound`
  that reaches `handle_message_as_request()` as unsolicited or from a canceled
  request and logs it as unused. By that point, the wrapper has already had the
  chance to update inventory state.
- `zebra-network/src/peer_set/set.rs:984-1040` uses missing inventory in
  `route_inv()`: it first tries recent advertisers, then filters ready peers by
  excluding any peers returned by `inventory_registry.missing_peers(hash)`.
- `zebra-network/src/peer_set/set.rs:1042-1060` returns a synthetic
  `PeerError::NotFoundRegistry` if every ready peer is marked missing for that
  hash.
- `zebra-network/src/peer/connection.rs:242-287` handles
  `TransactionsById + Message::NotFound`. It detects a mismatch between the
  incoming `notfound` transaction IDs and the pending IDs, but still finishes
  the request with either `PeerError::NotFoundResponse` or a partial
  `Response::Transactions`.
- `zebra-network/src/peer/connection.rs:351-391` does the same for
  `BlocksByHash + Message::NotFound`: mismatched or unrelated block hashes are
  logged, then the request is completed using the pending hashes as missing.
- `zebra-network/src/peer/client.rs:337-400` has the safer request-correlated
  negative-inventory path: `MissingInventoryCollector` is created only for
  actual inventory downloads and suppresses local `NotFoundRegistry` feedback
  loops.
- `zebra-network/src/peer/client.rs:369-408` forwards missing inventory from
  `NotFoundResponse` errors and partial `Response::Blocks` /
  `Response::Transactions`, so the active request's pending hashes can be
  registered as missing after the mismatched response completes.
- `zebra-network/src/peer_set/inventory_registry.rs:53-68` caps retained
  inventory state at 1000 hashes per rotating map and 70 peers per hash.
- `zebra-network/src/constants.rs:151-154` rotates inventory every 53 seconds,
  so entries expire after roughly one to two rotation intervals.

Local proof:

- Durable current-behavior test in
  `zebra-network/src/peer_set/inventory_registry/tests/vectors.rs`
  `unsolicited_notfound_registers_missing_inventory_today` passed with
  `cargo test -p zebra-network unsolicited_notfound_registers_missing_inventory_today --lib`.
  The test sent a `Message::NotFound(vec![block_hash])` through
  `register_inventory_status()` and observed the peer in
  `InventoryRegistry::missing_peers(block_hash)`.
- The surrounding inventory-registry vector tests passed with
  `cargo test -p zebra-network peer_set::inventory_registry::tests::vectors --lib`.
- Local current-behavior tests
  `unrelated_notfound_completes_active_block_request_today` and
  `unrelated_notfound_completes_active_transaction_request_today` now pass with
  `cargo test -p zebra-network unrelated_notfound_completes_active --lib`.
  These tests send `BlocksByHash` and `TransactionsById` requests to a test
  connection, then send `Message::NotFound` for unrelated inventory. In both
  cases the request completes with `PeerError::NotFoundResponse` for the
  originally requested inventory, and the connection remains open.

Independent cross-check:

- RepoPrompt builder chat `notfound-correlation-aud-6941EA` independently
  validated the zero-overlap in-flight `notfound` completion path, the
  handshake-vs-client missing-registration distinction, and the bounded severity
  classification.

## Impact

This is not a consensus issue and does not create unbounded memory growth.

The practical risk is bounded P2P availability degradation:

- a peer can cheaply poison its own missing-state for up to 1000 hashes per
  message;
- multiple ready attacker peers can mark themselves missing for the same hash,
  reducing the candidate set for single-hash `BlocksByHash` and
  `TransactionsById` requests;
- if all ready peers are marked missing, Zebra can fail the request locally with
  `NotFoundRegistry` without trying a peer;
- a peer selected for an active single-hash download can abort that request with
  an unrelated `notfound`, causing an immediate retry/failure path and
  request-correlated missing-state update for the originally requested hash;
- repeated across enough attacker-controlled ready peers, this can drive Zebra
  toward a local `NotFoundRegistry` result for that hash before an honest peer is
  tried;
- false `inv` advertisements can also dominate the preferred advertiser set
  until each lying advertiser fails and is marked missing.

The main limits are meaningful:

- entries are keyed to the actual transient peer address, not to claimed wire
  addresses, so one peer cannot directly mark another peer missing;
- state is in-memory, capped, and expires quickly;
- only single-hash block and transaction requests use inventory-aware routing;
- disconnected peers are filtered from ready-peer routing, although stale
  entries remain until rotation.
- sync treats `NotFoundResponse` and `NotFoundRegistry` as temporary download
  failures rather than chain-state or consensus failures.

## Suggested Fix

Keep the bounded registry, but make missing-state updates request-correlated:

- Only register inbound `notfound` as missing inventory when the connection is
  actually awaiting a matching inventory response.
- In `BlocksByHash` and `TransactionsById` handlers, ignore `notfound` messages
  that have no overlap with the pending request instead of completing the
  request.
- Continue registering missing inventory from `MissingInventoryCollector`, which
  already runs on Zebra-originated requests and suppresses local
  `NotFoundRegistry` feedback loops.
- Consider pruning inventory entries for a peer on disconnect, or tagging entries
  with a connection generation so reconnects do not inherit same-interval stale
  `Missing` state.
- Consider making `route_inv()` mix advertisers with generic non-missing ready
  peers when several advertised attempts have failed, so a colluding advertiser
  set cannot consume all retry budget before fallback.
- Add direct request-correlation regressions for block and transaction handlers:
  unrelated `notfound` should leave the handler waiting, while overlapping
  `notfound` should still complete the request.

## Verification

Targeted mitigation/backstop tests run locally:

- `cargo test -p zebra-network missing_inv_collector_ignores_local_registry_errors --lib`
- `cargo test -p zebra-network inv_registry_prefer_missing_ok --lib`
- `cargo test -p zebra-network unrelated_notfound_completes_active --lib`
- `cargo test -p zebra-network unsolicited_notfound_registers_missing_inventory_today --lib`
- `cargo test -p zebra-network peer_set::inventory_registry::tests::vectors --lib`

These tests verify the existing local `NotFoundRegistry` self-refresh guard, the
registry's missing-over-available preference, and the current in-flight
request-correlation gap. The remaining missing regression is the desired fixed
behavior: unrelated `notfound` messages should leave the handler waiting instead
of completing the request.

Rerun on 2026-05-07: all three focused commands above passed. The full
inventory-registry vector group took about 53 seconds because it includes the
real rotation/expiry timing test.

## Disclosure Triage

Public hardening. The behavior is attacker-influenced and worth fixing, but it
is bounded, expires within about two minutes, and affects routing/availability
rather than validation correctness or persistent state.

Public issue filed on 2026-05-07 after checking open and closed issues for
`notfound`, `NotFoundResponse`, `missing inventory`, `inventory registry`,
`request correlation`, `unrelated response`, `canceled request notfound`, and
`notfound inv collector`. Related closed issues #2156, #2726, #3271, and #3235
cover intended `notfound` tracking or broader inventory-rate hardening, but not
this request-correlation gap.

Confidence: medium-high that unsolicited `notfound` currently mutates registry
state; high that unrelated in-flight `notfound` currently completes the active
block/transaction download request; medium on practical impact because real
exploitability depends on peer mix, retry behavior, and whether the target hash
would otherwise be available from honest ready peers.
