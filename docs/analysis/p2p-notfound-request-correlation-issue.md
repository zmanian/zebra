# P2P notfound handling should be request-correlated before updating inventory routing

## Summary

Zebra currently lets remote peers influence inventory-routing state with
`notfound` messages before confirming that the message corresponds to an active
request.

There are two related paths:

- unsolicited inbound `notfound` messages are registered as missing inventory for
  the sending peer before the connection state machine determines that the
  message is unsolicited;
- while Zebra is awaiting a `BlocksByHash` or `TransactionsById` response, an
  unrelated `notfound` message is logged as unexpected but still completes the
  active request as if the originally requested inventory was missing.

This is not a consensus issue and does not create unbounded memory growth. The
registry is capped and entries expire quickly. The practical risk is bounded P2P
availability/routing degradation: attacker-controlled peers can cheaply mark
themselves as missing specific hashes, abort active single-hash downloads with
unrelated `notfound`, and push Zebra toward local `NotFoundRegistry` failures for
that inventory if all ready peers are marked missing.

## Duplicate Check

Before filing this, I searched all open and closed issues for:

- `notfound`
- `NotFoundResponse`
- `missing inventory`
- `inventory registry`
- `request correlation`
- `unrelated response`
- `canceled request notfound`
- `notfound inv collector`

The closest related closed issues are:

- #2156, which intentionally added `notfound` inventory tracking;
- #2726, which tracks sending/using `notfound` to finish requests;
- #3271, broader `inv` / address-book message DoS hardening;
- #3235, peer-set retry placement for block downloads.

Those are related background, but I do not think they duplicate this finding:
the local proof here is specifically that `notfound` updates and request
completion are not sufficiently correlated with the active request.

## Code Path

- `register_inventory_status()` registers every inbound `Message::NotFound` as
  missing inventory for `connected_addr.get_transient_addr()`:
  `zebra-network/src/peer/handshake.rs:1192`
- The connection handler later treats `notfound` that reaches
  `handle_message_as_request()` as unsolicited or from a canceled request:
  `zebra-network/src/peer/connection.rs:1221`
- For `TransactionsById`, mismatched `notfound` contents are logged, but if no
  requested transactions were received, the request is completed with
  `PeerError::NotFoundResponse` for the pending transaction IDs:
  `zebra-network/src/peer/connection.rs:249`
- For `BlocksByHash`, mismatched `notfound` contents are logged, but if no
  requested blocks were received, the request is completed with
  `PeerError::NotFoundResponse` for the pending block hashes:
  `zebra-network/src/peer/connection.rs:358`
- Inventory-aware routing then excludes peers in
  `inventory_registry.missing_peers(hash)` and returns `NotFoundRegistry` if all
  ready peers are marked missing:
  `zebra-network/src/peer_set/set.rs:984`
- `MissingInventoryCollector` is the safer request-correlated path: it is created
  only for Zebra-originated inventory downloads and suppresses locally generated
  `NotFoundRegistry` feedback loops:
  `zebra-network/src/peer/client.rs:337`

## Local Proof Tests

I added focused current-behavior tests and reran them on 2026-05-07:

```sh
cargo test -p zebra-network unrelated_notfound_completes_active --lib
cargo test -p zebra-network unsolicited_notfound_registers_missing_inventory_today --lib
cargo test -p zebra-network missing_inv_collector_ignores_local_registry_errors --lib
cargo test -p zebra-network inv_registry_prefer_missing_ok --lib
```

Results:

- `unrelated_notfound_completes_active_block_request_today`: an unrelated
  `notfound` completes an active block request as `NotFoundResponse`.
- `unrelated_notfound_completes_active_transaction_request_today`: an unrelated
  `notfound` completes an active transaction request as `NotFoundResponse`.
- `unsolicited_notfound_registers_missing_inventory_today`: an unsolicited
  inbound `notfound` is registered as missing inventory for the peer.
- The adjacent collector/registry tests confirm the intended local
  `NotFoundRegistry` feedback guard and missing-over-available routing behavior.

## Expected Behavior

`notfound` should update missing-inventory routing state only when it is
correlated with an active Zebra-originated inventory request.

For `BlocksByHash` and `TransactionsById`, a `notfound` with no overlap with the
pending request should be ignored for request completion, or treated as peer
misbehavior/noise, rather than completing the active request as missing.

## Suggested Fix Direction

- Only register inbound `notfound` as missing inventory when the connection is
  awaiting a matching inventory response.
- In `BlocksByHash` and `TransactionsById` handlers, ignore `notfound` messages
  that have no overlap with the pending request.
- Continue registering missing inventory through `MissingInventoryCollector`,
  which is already tied to Zebra-originated requests.
- Add post-fix regressions where unrelated `notfound` leaves the active handler
  waiting, while overlapping `notfound` still completes the relevant request.
