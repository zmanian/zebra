# P2P stale gossiped address dial churn note

Date: 2026-05-03

Last updated: 2026-05-09

Disposition: local-only residual of
[#1865](https://github.com/ZcashFoundation/zebra/issues/1865). Do not post
publicly without explicit re-authorization.

Scope: follow-up on pass-5 B1 address-book poisoning and peer-crawler
availability checks.

## Finding

Zebra already bounds how many peer addresses it accepts from each peer crawl and
how many entries the address book retains. It also avoids gossiping addresses
whose last-seen time is too old. But old gossiped addresses are still accepted
into the address book and are still eligible for one initial outbound connection
attempt before Zebra marks them failed.

That creates a bounded peer-crawler churn surface. A connected peer can feed
Zebra old, unreachable, but syntactically valid Zcash listener addresses. Zebra
will not gossip those entries if they are outside the active-gossip window, but
it can still spend outbound connection attempts on them because
`NeverAttemptedGossiped` entries are treated as probably reachable until their
first failure.

This is not consensus-relevant, not unbounded memory growth, and not a direct
peer-set takeover. It is public P2P availability hardening: stale address input
can consume address-book slots and outbound crawler attempts until the entries
fail once or are evicted by the address-book cap.

## Evidence

- `zebra-network/src/peer_set/candidate_set.rs:315-323` accepts
  `Response::Peers(addrs)`, runs `validate_addrs()`, then sends the surviving
  entries into the address book.
- `zebra-network/src/peer_set/candidate_set.rs:446-479` documents a TODO to
  ignore peers older than 3 weeks, but the current validator only clamps future
  timestamps and rejects underflow cases.
- `zebra-network/src/meta_addr.rs:630-638` makes outbound readiness depend on
  valid services/address data, recent local update state, and
  `is_probably_reachable()`.
- `zebra-network/src/meta_addr.rs:666-684` explicitly treats a peer as probably
  reachable if it has never been attempted; the last-seen recency check only
  suppresses reconnecting after a failed attempt.
- `zebra-network/src/constants.rs:197-204` says peers older than three days
  after a failed attempt are considered offline, not that old never-attempted
  peers are skipped before the first attempt.
- `zebra-network/src/peer_set/candidate_set.rs:400-422` selects the next
  reconnect peer from `AddressBook::reconnection_peers()` and marks it
  `AttemptPending`.
- `zebra-network/src/peer_set/initialize.rs:1093-1159` attempts the outbound
  handshake and re-adds demand after failure, so stale unreachable candidates
  can spend dialer work before being marked failed.
- `zebra-network/src/constants.rs:88-90` limits each peer-address response used
  by the crawler to `DEFAULT_PEERSET_INITIAL_TARGET_SIZE *
  OUTBOUND_PEER_LIMIT_MULTIPLIER / 2`, which is 37 with current constants.
- `zebra-network/src/constants.rs:273-287` sets `GET_ADDR_FANOUT = 1`, and
  `zebra-network/src/constants.rs:255-271` rate-limits peer-address crawls to at
  least 31 seconds apart with an 8 second crawl timeout.
- `zebra-network/src/peer/connection.rs:1252-1277` also caches unsolicited
  `addr` messages per connection. When Zebra later asks that peer for peers,
  `zebra-network/src/peer/connection.rs:1028-1048` can satisfy the internal
  `Request::Peers` from the cached addresses instead of sending a wire
  `getaddr`.

## Duplicate Check

Read-only duplicate searches refreshed on 2026-05-09:

```sh
gh api repos/ZcashFoundation/zebra/issues/1865
gh api repos/ZcashFoundation/zebra/pulls/2178
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "stale gossiped address" "dial"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "NeverAttemptedGossiped" "last_seen"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "ignore peers" "older than 3 weeks"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "validate_addrs" "3 weeks"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "last seen" "older than 3 days" "gossiped"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "old gossiped" "connection attempt"'
```

Results:

- #1865 is the closest public history and explicitly includes "ignore peers
  that are older than 3 days" under accepting old peers from other nodes.
  It is closed, but the current checkout still preserves old never-attempted
  gossiped peers.
- PR #2178 fixed the related future-time trust issue from #1871. It does not
  appear to implement an old-time cutoff in `validate_addrs()`.
- The exact stale-dial and old-gossiped-connection-attempt searches returned no
  fresher dedicated issue.

## Local Confidence Check

Added focused current-behavior test:

```sh
cargo test -p zebra-network old_gossiped_peer_is_still_initially_connectable_today --lib
```

Result on 2026-05-09: passed. The test confirms a gossiped address last seen 30
days ago survives `validate_addrs()`, is not active for gossip, is not recently
seen, and is still initially dialable while in `NeverAttemptedGossiped`.

## Impact

Expected impact is bounded P2P availability pressure:

- a peer can supply old unreachable addresses that are not gossipable but are
  still worth one outbound attempt to Zebra;
- repeated crawls against malicious or polluted peers can add more stale
  never-attempted entries over time;
- those entries can consume address-book capacity until evicted by the
  `MAX_ADDRS_IN_ADDRESS_BOOK` ordering/limit;
- the crawler can spend connection attempts and handshake timeouts on stale
  entries before marking them failed;
- if the node is low on honest candidates, this can delay discovery of useful
  peers or keep the crawler busy with junk.

The main mitigations are meaningful:

- per-peer crawler responses are capped;
- crawler fanout is currently one and crawl attempts are rate-limited;
- the address book is capped;
- once a stale candidate fails, the three-day recency rule prevents repeated
  retries;
- outbound connection attempts are globally rate-limited and handshakes have
  timeouts;
- this only affects peer discovery and availability, not block or transaction
  validation.

## Suggested Fix

- Implement the existing `validate_addrs()` TODO: drop gossiped addresses older
  than the accepted reachability window, after applying any clock-skew offset.
- Keep the future-time clamping, but also enforce an old-time cutoff before
  `send_addrs()` writes entries into the address book.
- Consider treating very old unsolicited cached addresses as empty for
  `Request::Peers` responses.
- Add a regression asserting that a peer older than the cutoff is filtered by
  `validate_addrs()` and never reaches `AddressBook::reconnection_peers()`.
- Add a second regression for the unsolicited-cache path: old cached `addr`
  entries should not be returned to the candidate crawler.

## Disclosure Triage

Public hardening. This is attacker-influenced peer discovery work, but the path
is bounded, self-healing after first failure, and does not affect consensus or
persistent validated state.

Confidence: high that current code accepts and initially dials old gossiped
addresses; medium-low on practical severity because real impact depends on peer
mix, operator network exposure, address-book health, and outbound failure costs.
