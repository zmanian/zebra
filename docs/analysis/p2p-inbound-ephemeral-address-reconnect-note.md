# P2P Inbound Ephemeral Address Reconnect Note

Date: 2026-05-03

Last updated: 2026-05-09

Disposition: local-only residual/regression of closed security work
[#2120](https://github.com/ZcashFoundation/zebra/pull/2120),
[#7951](https://github.com/ZcashFoundation/zebra/issues/7951), and
[#7977](https://github.com/ZcashFoundation/zebra/pull/7977). Do not post
publicly without explicit re-authorization.

## Finding

Zebra's address-book invariants say remote addresses from inbound connections
must not be stored, because they contain ephemeral outbound ports rather than
Zcash listener ports. The current handshake path can still store the
`InboundDirect` remote socket address after a successful handshake and mark it
as a responded peer.

After the normal recent-update delay expires, that inbound ephemeral
`IP:port` can become an outbound reconnection candidate. This is bounded peer
discovery churn, not consensus impact, but it gives any inbound peer that
completes a handshake a way to add at least one likely-useless reconnect target
derived from its transient TCP source port.

## Evidence

The address-book contract says it should contain listener addresses, and
explicitly excludes remote addresses from inbound connections:

- `zebra-network/src/address_book.rs:45-63`

`ConnectedAddr::InboundDirect` is documented as holding an inbound remote
address whose port is ephemeral, not a listener port:

- `zebra-network/src/peer/handshake.rs:159-169`

But `ConnectedAddr::get_address_book_addr()` returns that inbound direct
address as an address-book key:

- `zebra-network/src/peer/handshake.rs:249-267`

After a successful handshake, the common handshake path sends
`MetaAddr::new_connected(...)` with that `book_addr` and the inbound flag:

- `zebra-network/src/peer/handshake.rs:974-988`
- `zebra-network/src/meta_addr.rs:384-392`

`AddressBook::update()` rejects only addresses that are invalid for outbound
connection syntax and then inserts the updated address:

- `zebra-network/src/address_book.rs:482-499`

The inbound flag prevents gossiping the address to other peers, but it does not
exclude the address from outbound reconnection selection:

- `zebra-network/src/meta_addr.rs:707-715`
- `zebra-network/src/meta_addr.rs:630-663`
- `zebra-network/src/address_book.rs:637-654`

The peer-cache writer also strips `MetaAddr` provenance down to socket
addresses. `AddressBook::cacheable()` keeps active peers without checking
`is_inbound`, and `update_peer_cache_once()` writes only `meta_addr.addr`:

- `zebra-network/src/address_book.rs:318-338`
- `zebra-network/src/peer_cache_updater.rs:39-49`
- `zebra-network/src/config.rs:486-518`

## Local Confidence Check

A focused unit test was added in
`zebra-network/src/address_book/tests/vectors.rs`:

Targeted command:

```sh
cargo test -p zebra-network inbound_ephemeral_address_becomes_reconnection_candidate_today --lib
```

Result on 2026-05-09: passed. The test confirms a successful inbound connection
remote socket address is accepted as an inbound `MetaAddr` today and later
appears as a reconnect candidate after `MIN_PEER_RECONNECTION_DELAY`.

## Duplicate Check

Read-only duplicate search refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra inbound ephemeral address reconnect candidate'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "inbound" "ephemeral" "address book"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "InboundDirect" "AddressBook"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "remote addresses of inbound connections"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "get_address_book_addr" "InboundDirect"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "is_inbound" "reconnection_peers"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "peer cache" "inbound" "address"'
gh api repos/ZcashFoundation/zebra/pulls/2120
gh api repos/ZcashFoundation/zebra/issues/7951
gh api repos/ZcashFoundation/zebra/pulls/7977
gh api repos/ZcashFoundation/zebra/issues/7824
```

Results:

- #2120 is the earlier security fix whose motivation says Zebra was putting
  inbound remote addresses in its address book even though their ports are
  ephemeral. The PR says it stops putting inbound addresses in the address book.
- #7951 and PR #7977 are later closed security work to put peer `version` and
  remote IP addresses in the connection cache instead of sending handshake peer
  addresses directly to the address book.
- #7824 is the broader synthetic-node spreading tracker and lists #7951 as a
  required change.
- Exact searches for `InboundDirect` plus address-book/reconnection behavior did
  not find a fresher dedicated issue.

This should therefore be treated as a current-behavior residual or regression
against already-closed security intent, not as a new public issue without a
maintainer-directed route.

## Impact

Expected impact is public P2P availability hardening:

- inbound peers can make Zebra retain their transient source socket as a
  responded address-book entry;
- those entries are not gossiped, but they can later consume outbound crawler
  attempts after the recent-response window expires;
- if still active when peer-cache update runs, their socket addresses can be
  persisted without the inbound provenance bit;
- many successful inbound handshakes from different source ports or IPs can add
  bounded address-book churn;
- failed reconnects should eventually mark those entries failed, so the issue
  is self-limiting.

Existing mitigations keep this below private-disclosure severity:

- inbound handshakes are rate-limited and connection-limited;
- the address book has a maximum size;
- outbound connection attempts are globally rate-limited;
- failed reconnects stop repeated attempts once the address is no longer
  recently reachable;
- no block, transaction, or consensus validation behavior depends on these
  entries.

## Suggested Fix

- Make `ConnectedAddr::get_address_book_addr()` return `None` for
  `InboundDirect`, matching its documentation and the address-book invariant.
- Add a defense-in-depth guard so `MetaAddr::is_ready_for_connection_attempt()`
  returns false for `is_inbound` entries, covering any stale in-memory entries.
- Exclude `is_inbound` entries from `AddressBook::cacheable()` before the peer
  cache writer strips provenance down to bare socket addresses.
- If Zebra wants to learn listener addresses from inbound peers, promote only
  validated `Version.address_from` candidates through the existing alternate
  address cache/crawler path, not through direct address-state updates.
- Add a regression asserting that an inbound direct remote address is not added
  to the address book and never appears in `reconnection_peers()`.
- Consider adding a debug metric for discarded inbound remote addresses so
  operators can still observe inbound connection churn without turning it into
  crawler input.

## Disclosure Triage

Public hardening.

This is an invariant violation in peer discovery and reconnect scheduling, but
it is bounded, does not crash the node, and does not affect consensus or
validated state. It is useful to fix because the code comments already identify
the exact trust boundary Zebra intends to enforce.

Confidence: high on the source-to-sink behavior and the local proof test.
Practical severity is medium-low because the impact depends on inbound
reachability, peer churn, address-book pressure, and outbound dialer load.
