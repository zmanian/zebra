# Address Book Ban Cleanup Assumes Same-IP Entries Are Contiguous

Date: 2026-05-03

Last updated: 2026-05-09

Disposition: local-only residual of ban implementation work in
[#9201](https://github.com/ZcashFoundation/zebra/pull/9201). Do not post
publicly without explicit re-authorization. This is separate from the privately
reported `max_connections_per_ip > 1` ban-path panic.

## Summary

When a peer reaches the misbehavior ban threshold, `AddressBook::update()` tries
to remove all address-book entries with the banned IP. The removal code assumes
that all entries with the same IP are contiguous in `by_addr.descending_keys()`.
That assumption is not valid because `by_addr` is ordered by `MetaAddr`
reconnection priority, and `MetaAddr::Ord` compares connection state and times
before it compares IP and port.

The result is stale banned-IP entries can remain in the address book after a
ban. This does not appear to bypass the IP ban for actual peer-set use, because
future `AddressBook::update()` calls reject banned IPs and the peer set receives
ban updates. But stale entries can still consume address-book space, be selected
as reconnection candidates before being rejected, and in some cases remain
eligible for cache/gossip sanitation if the stale entry itself has zero
misbehavior score.

## Evidence

- `zebra-network/src/address_book.rs:443-480` inserts the banned IP, removes
  one entry from `most_recent_by_ip`, then builds `banned_addrs` using:

```rust
self.by_addr
    .descending_keys()
    .skip_while(|addr| addr.ip() != banned_ip)
    .take_while(|addr| addr.ip() == banned_ip)
```

- `zebra-network/src/address_book.rs:73-74` shows `by_addr` is an `OrderedMap`
  keyed by socket address but ordered by `Reverse<MetaAddr>`.
- `zebra-network/src/meta_addr.rs:1187-1285` orders `MetaAddr`s first by
  connection state, peer preference, local attempt/failure/response times,
  untrusted last-seen time, and services; IP and port are only final
  tie-breakers.
- Therefore same-IP entries are not guaranteed to form one contiguous run in
  `descending_keys()`.
- `zebra-network/src/address_book.rs:417-423` rejects future address-book
  updates for banned IPs, so the leftover entries are blocked at update time
  rather than successfully re-added or reconnected.
- `zebra-network/src/peer_set/candidate_set.rs:400-423` asks for one
  reconnection peer and immediately calls `guard.update(new_reconnect)`. If a
  stale banned entry is selected, `update()` returns `None` and `next()` stops
  instead of looping to another candidate.
- `zebra-network/src/address_book.rs:285-311` builds gossipable addresses from
  address-book entries using `sanitize()` and active-age filtering.
- `zebra-network/src/meta_addr.rs:707-745` suppresses entries whose own
  `misbehavior_score` is nonzero, but a stale previous entry left behind by the
  ban path can still have score zero because the ban branch computes the updated
  misbehavior score without inserting it before cleanup.

## Local Confidence Check

Added a direct current-behavior unit test with three gossiped address-book
entries:

- `127.0.0.1:8233`, old timestamp,
- `127.0.0.2:8233`, middle timestamp,
- `127.0.0.1:8234`, newest timestamp.

The test then applies an `UpdateMisbehavior` at
`MAX_PEER_MISBEHAVIOR_SCORE` to `127.0.0.1:8233` and confirms that the shared
IP is banned while at least one same-IP entry remains in the address book. The
focused command:

```sh
cargo test -p zebra-network ban_cleanup_leaves_non_contiguous_same_ip_entries_today --lib
```

passed on 2026-05-09.

This proves the current cleanup does not reliably remove even the misbehaving
address when another same-IP entry appears earlier in `MetaAddr` ordering and a
different IP separates it from the target entry.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "address book" "ban" "same IP" "contiguous"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "misbehavior" "ban" "address book" "cleanup"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "banned IP" "address book" "same-IP"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "bans_by_ip" "by_addr"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "max_connections_per_ip" "ban" "panic"'
gh api repos/ZcashFoundation/zebra/pulls/9201
```

Results:

- PR #9201 introduced misbehavior tracking and the current ban cleanup logic.
  It is implementation history, not a duplicate report of the non-contiguous
  same-IP cleanup bug.
- The targeted same-IP, cleanup, and `bans_by_ip` searches returned no exact
  issue hits.
- The `max_connections_per_ip` panic search returned no public duplicate; that
  variant remains on the private-disclosure track and should not be conflated
  with this lower-severity cleanup residual.

## Impact

The direct impact is ban cleanup incompleteness, not ban bypass:

- active peer-set services for banned IPs are still dropped through the ban
  watch path;
- future address-book updates for the banned IP return `None`;
- actual outbound connection attempts are blocked when candidate selection tries
  to mark a stale banned entry as `AttemptPending`.

Residual effects:

- stale entries for banned IPs can keep consuming address-book capacity;
- candidate selection can waste a connection slot/tick on a stale banned entry
  before returning `None`;
- stale score-zero entries can remain eligible for address cache/gossip
  sanitation until they age out, depending on their last-seen data.

This is public P2P hardening. I would not include it in private disclosure
unless further testing shows a strong peer-isolation or outbound-connectivity
impact.

## Suggested Fix Direction

- Replace the contiguous-run removal with a scan over all keys, collecting every
  address whose `addr.ip() == banned_ip`.
- Keep the removal vector to avoid mutating `by_addr` during iteration.
- Add a regression with non-contiguous same-IP entries, asserting that all
  banned-IP addresses are removed and unrelated addresses remain.
- Consider filtering `bans_by_ip` in `sanitized()`, `cacheable()`, and
  `reconnection_peers()` as defense in depth, so stale banned entries cannot
  affect gossip or candidate selection if cleanup misses anything.

Confidence: high on the cleanup bug from source review and direct repro; medium
on security impact because the ban map still blocks the most important reuse
paths.
