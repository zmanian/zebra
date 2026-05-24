# Address Book Misbehavior Ban Panic With Multiple Connections Per IP

Date: 2026-05-03

## Summary

`AddressBook::update()` can panic when applying a peer-misbehavior update that
reaches the ban threshold if `max_connections_per_ip` is configured above the
default value of 1.

This is a configuration-dependent remote DoS lead. It does not affect consensus
validation and does not require accepting invalid blocks or transactions, but a
remote peer that can trigger a nonzero misbehavior score can drive the ban path.

## Preconditions

- The node is configured with `network.max_connections_per_ip > 1`.
- A remote peer causes a misbehavior update at or above
  `MAX_PEER_MISBEHAVIOR_SCORE`.
- The address-book updater applies that `UpdateMisbehavior` event.

The default is 1, so default Zebra nodes do not hit this specific branch with
`most_recent_by_ip` disabled.

## Evidence

- `zebra-network/src/address_book.rs:76-82` documents that
  `most_recent_by_ip` currently only supports `max_connections_per_ip == 1` and
  must be `None` for larger configured values.
- `zebra-network/src/address_book.rs:158-169` constructs
  `most_recent_by_ip` only when `max_connections_per_ip == 1`.
- `zebra-network/src/constants.rs:71-81` sets the default
  `DEFAULT_MAX_CONNS_PER_IP` to 1.
- `zebra-network/src/config.rs:177-195` documents the
  `max_connections_per_ip` setting and warns that increasing it above 1 reduces
  network security.
- `zebra-network/src/config.rs:931-957` accepts positive configured values
  rather than rejecting values above 1.
- `zebra-network/src/constants.rs:389-391` sets
  `MAX_PEER_MISBEHAVIOR_SCORE` to 100.
- `zebra-network/src/meta_addr.rs:315-321` defines
  `MetaAddrChange::UpdateMisbehavior`.
- `zebra-network/src/meta_addr.rs:1008-1019` creates a new `MetaAddr` from a
  change when the address book has no previous entry for that peer.
- `zebra-network/src/meta_addr.rs:958-966` returns the misbehavior increment
  for `UpdateMisbehavior`, and `zebra-network/src/meta_addr.rs:1154` /
  `zebra-network/src/meta_addr.rs:1180` add the increment to the stored score.
- `zebra-network/src/address_book.rs:443-458` checks whether the updated score
  reaches the ban threshold and then unconditionally calls
  `self.most_recent_by_ip.as_mut().expect(...).remove(&banned_ip)`.
- `zebra-network/src/address_book_updater.rs:101-114` applies address-book
  changes while holding the shared address-book mutex and expects it to remain
  unpoisoned.
- `zebra-network/src/address_book.rs:832-843` exposes the shared address book
  through `Arc<Mutex<AddressBook>>` helpers that panic if a previous holder
  poisoned the mutex.

Attacker-reachable misbehavior sources exist:

- `zebra-consensus/src/error.rs:260-300` assigns mempool misbehavior score 100
  to many transaction verification errors.
- `zebrad/src/components/mempool.rs:641-652` forwards nonzero mempool
  misbehavior scores with the advertising peer address.
- `zebra-consensus/src/error.rs:400-410` assigns block misbehavior score 100 to
  several block errors.
- `zebra-consensus/src/block.rs:109-117` also assigns score 100 to Equihash
  verification errors.
- `zebrad/src/components/inbound.rs:330-344` forwards nonzero block
  misbehavior scores from the inbound block-download path.
- `zebrad/src/components/sync.rs:1143-1151` forwards nonzero block
  misbehavior scores from the sync block-download path.
- `zebra-network/src/peer_set/initialize.rs:122-157` batches and forwards
  misbehavior updates to the address-book updater every 30 seconds.

## Local Confidence Check

Added a direct current-behavior `AddressBook` unit test:

- `zebra-network/src/address_book/tests/vectors.rs:40-50`

The test constructs an address book with `max_connections_per_ip = 2` and
applies:

```rust
MetaAddrChange::UpdateMisbehavior {
    addr: "127.0.0.1:8233".parse().unwrap(),
    score_increment: MAX_PEER_MISBEHAVIOR_SCORE,
}
```

The focused command:

```sh
cargo test -p zebra-network misbehavior_ban_panics_with_max_connections_per_ip_above_one_today --lib
```

passed with `#[should_panic]`, confirming the panic at the documented `expect`
site. I repeated this focused verification after formatting on 2026-05-04, and
the command passed again.

Follow-up live-updater proof added on 2026-05-09:

- `misbehavior_ban_panics_updater_and_poisons_address_book_today` in
  `zebra-network/src/address_book/tests/vectors.rs`.

The test spawns the real `AddressBookUpdater` with
`max_connections_per_ip = 2`, sends the same ban-threshold
`MetaAddrChange::UpdateMisbehavior` through the updater channel, awaits the
task, and asserts:

- the updater task exits by panic;
- the shared `Arc<Mutex<AddressBook>>` is poisoned after the panic.

The focused command:

```sh
cargo test -p zebra-network misbehavior_ban_panics_updater_and_poisons_address_book_today --lib
```

passed. This converts the previous source-level follow-on impact assessment into
a live-path proof: the panic is not confined to direct `AddressBook::update()`
calls; it reaches the normal updater task and poisons the shared address-book
mutex.

## Impact

The immediate effect is a panic in the address-book updater thread while it is
holding the shared `AddressBook` mutex. Follow-on impact is likely worse than a
single dropped update because other users of the shared address book call
`.lock().expect("mutex should be unpoisoned")`.

In the live updater path, `AddressBookUpdater::spawn()` locks
`worker_address_book`, calls `.update(event)`, and then uses the same shared
mutex again to publish ban updates. There is no `catch_unwind` or poisoned-lock
recovery around the update call. If the ban branch panics, the worker exits by
panic while holding the mutex, which poisons the shared address book. Later
calls through `AddressBookPeers for Arc<Mutex<AddressBook>>` explicitly expect
"panic in a previous thread that was holding the mutex" not to have happened, so
peer discovery or peer insertion can escalate the original updater panic into
additional panics in peer-management code.

This can degrade or crash peer-management code for nodes running the supported
but non-default multi-connection-per-IP configuration. It is not a default-node
remote crash and does not create a chain validation bypass.

## Suggested Fix Direction

- In the ban branch, only remove from `most_recent_by_ip` if the optional cache
  exists:

```rust
if let Some(most_recent_by_ip) = self.most_recent_by_ip.as_mut() {
    most_recent_by_ip.remove(&banned_ip);
}
```

- Keep the ban insertion and removal of all matching `by_addr` entries exactly
  as they are.
- Add a regression test for `max_connections_per_ip = 2` that applies an
  `UpdateMisbehavior` score at the ban threshold and asserts no panic, the IP is
  banned, and matching address-book entries are removed.

## Disclosure Triage

Recommended private disclosure. The affected configuration is non-default, but
the bug is a remotely triggerable panic/availability issue in network-facing
peer management once that supported configuration is enabled.

Confidence: high for the panic condition, direct reproducer, and live updater
panic/poisoning behavior; medium for practical exploitability because it depends
on a non-default configuration and normal peer-misbehavior scoring paths.
