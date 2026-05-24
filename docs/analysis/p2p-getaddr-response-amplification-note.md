# P2P getaddr empty-cache rescan amplification note

Date: 2026-05-03

Last updated: 2026-05-09

Status: local-only residual under existing public `getaddr` rate-limit history.
Do not post publicly without explicit re-authorization. Broad repeated
`getaddr` response rate limiting was already tracked by
[#7823](https://github.com/ZcashFoundation/zebra/issues/7823) and implemented
in [#7955](https://github.com/ZcashFoundation/zebra/pull/7955); this note is
about the narrower empty-result refresh-deadline residual.

Scope: follow-up on P2P `getaddr` / internal `Request::Peers` handling after
the pass-5 parser and mempool request findings.

## Finding

The naive concern that every remote `getaddr` request clones, filters, shuffles,
and truncates the whole address book is eliminated for the normal non-empty
cache path: Zebra caches one partial address-book response and refreshes it only
periodically.

There is still a narrower public availability hardening lead in the empty-result
path. When `CachedPeerAddrResponse::try_refresh()` gets no gossipable peers, it
does not advance `refresh_time`. That means each later inbound `getaddr` can
immediately retry `AddressBook::fresh_get_addr_response()`. On a node with a
large retained address book but no currently gossipable peers, a tiny `getaddr`
can repeatedly force the full address-book clone/filter/collect/shuffle path.

Separately, when the cache is non-empty, every inbound `getaddr` still clones the
cached response and sends it back as an `addr` message. That work is bounded and
less concerning than the empty-cache rescan path, but it is still a useful
secondary reason to add per-peer `getaddr` response throttling.

This is not consensus relevant, not an address-book poisoning issue, and not an
unbounded allocation issue. It is a bounded CPU/allocation/egress hardening item
for publicly reachable P2P nodes, especially stale or isolated nodes whose
address books contain many retained but non-gossipable entries.

## Evidence

- `zebra-network/src/peer/connection.rs:1345` maps an inbound
  `Message::GetAddr` to internal `Request::Peers`.
- `zebrad/src/components/inbound.rs:397-409` handles `Request::Peers` by calling
  `cached_peer_addr_response.try_refresh()` and then returning
  `cached_peer_addr_response.value()`. The nearby comment explicitly says Zebra
  does not monitor repeated requests.
- `zebrad/src/components/inbound/cached_peer_addr_response.rs:40-42` clones the
  cached `Response` for each caller.
- `zebrad/src/components/inbound/cached_peer_addr_response.rs:44-60` refreshes
  the cached value only when it is stale, using
  `AddressBook::fresh_get_addr_response()`.
- `zebrad/src/components/inbound/cached_peer_addr_response.rs:16` sets the
  refresh interval to 10 minutes, so repeated requests inside the interval reuse
  the same cached peer list rather than reshuffling the address book.
- `zebrad/src/components/inbound/cached_peer_addr_response.rs:66-69` advances
  `refresh_time` only when the refreshed peer list is non-empty.
- `zebrad/src/components/inbound/cached_peer_addr_response.rs:71-90` handles
  empty refreshes and lock contention without advancing `refresh_time`, so the
  next request remains eligible to retry refresh immediately.
- `zebra-network/src/address_book.rs:274-280` truncates a fresh response to
  `min(MAX_ADDRS_IN_MESSAGE, address_book_len / ADDR_RESPONSE_LIMIT_DENOMINATOR)`.
- `zebra-network/src/address_book.rs:285-315` shows the expensive work in a
  refresh: clone `by_addr`, insert the local listener, sanitize/filter all
  entries, collect, and shuffle.
- `zebra-network/src/constants.rs:301-313` sets `MAX_ADDRS_IN_MESSAGE = 1000`
  and `ADDR_RESPONSE_LIMIT_DENOMINATOR = 4`.
- `zebra-network/src/constants.rs:321-322` caps the address book at
  `MAX_ADDRS_IN_MESSAGE * (ADDR_RESPONSE_LIMIT_DENOMINATOR + 1)`, so a full
  address book can make the cached response hit the 1,000-address protocol cap.
- `zebra-network/src/peer/connection.rs:1465-1469` sends
  `Response::Peers(addrs)` as `Message::Addr(addrs)`.
- `zebra-network/src/protocol/external/codec.rs:274-283` serializes every
  outbound `Message::Addr` as v1 `addr` entries.
- `zebra-network/src/protocol/external/addr/v1.rs:97-104` serializes each v1
  address as timestamp, services, 16-byte IP, and port, so a 1,000-address
  response is roughly 30 KB plus protocol framing.
- `zebra-network/src/peer/connection/peer_tx.rs:27-33` applies only the generic
  20-second send timeout to outbound messages.
- `zebrad/src/commands/start.rs:168-175` wraps the inbound service in
  load-shed, buffer, and timeout layers, which bounds overload but does not make
  successful `getaddr` responses rare.

Existing tests show the cache behavior directly:

- `zebrad/src/components/inbound/tests/fake_peer_set.rs:756-865` asserts that
  repeated `Request::Peers` calls return the same response until refresh time.

Local proof added on 2026-05-07:

- `zebrad/src/components/inbound/cached_peer_addr_response.rs:
  empty_getaddr_refresh_leaves_refresh_time_stale_today`

The test forces the cached getaddr refresh deadline into the past, uses an
address book with no gossipable peers, calls `try_refresh()` twice, and confirms
the response remains `Nil` while the refresh deadline remains stale. That means a
subsequent request can immediately attempt another empty refresh.

## Impact

Expected impact is bounded P2P availability pressure:

- in the normal non-empty-cache state, repeated `getaddr` requests reuse the same
  cached peer list and avoid full address-book rescans;
- in an empty-refresh state, a connected peer can send repeated small `getaddr`
  messages and force repeated attempts to rebuild a fresh response;
- each empty refresh can scan a retained address book of up to 5,000 entries even
  though the final response is `Nil`;
- if the cache is non-empty, the peer can still spend little inbound bandwidth to
  trigger response cloning, v1 address conversion, serialization, metrics, and
  network upload of up to roughly 30 KB per request.

The empty-refresh state needs realistic preconditions: the node must be
attacker-reachable, its address book must contain many retained entries, and
those entries must not be active for gossip. This can happen on stale, isolated,
outdated, or poorly connected nodes. Healthy nodes with gossipable peers mostly
take the bounded cached-response path.

The path is bounded by connection limits, request sequencing per peer, send
timeouts, inbound service load-shed/timeout layers, the 1,000-address protocol
cap, and the 5,000-address book cap. Treat this as public hardening unless
benchmarks show that default nodes can be materially degraded by repeated
`getaddr` traffic.

## Suggested Fix

- Split the next-refresh deadline from cache expiry, or otherwise advance
  `refresh_time` after empty refresh results.
- Cache `Nil` briefly after an empty refresh so repeated `getaddr` requests do
  not immediately rescan the address book.
- Add a per-connection `getaddr` response rate limit or a "respond once per
  connection / once per interval" rule.
- Consider returning `Nil` or a much smaller response for repeated non-empty
  `getaddr` requests inside the cache interval from the same peer.
- Keep the global cached response; it correctly avoids per-request address-book
  scanning and makes rapid full-book scraping harder.
- Add tests for repeated empty-refresh `Request::Peers` calls and repeated
  `Message::GetAddr` over one connection to prove the intended throttling.

## Confidence

Confidence: medium-high on current behavior, medium-low on security severity.

The code and existing test coverage clearly show cached response reuse and
per-request cloning/sending. The remaining uncertainty is practical exploitability
under real networking conditions, because TCP backpressure, connection limits,
load shedding, and operator firewalls all shape the outcome.

Verification rerun on 2026-05-09:

```sh
cargo test -p zebrad empty_getaddr_refresh_leaves_refresh_time_stale_today --lib
cargo test -p zebrad caches_getaddr_response --lib
```

Both focused tests passed.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "getaddr" "empty cache" "address book"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "getaddr" "amplification"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "fresh_get_addr_response" "refresh_time"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "CACHED_ADDRS_REFRESH_INTERVAL" "getaddr"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "empty" "getaddr" "refresh"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "getaddr" "Nil" "refresh_time"'
```

Results:

- #7823 and PR #7955 are the main public history. They cover the broad Ziggurat
  `GetAddr` repeated-response issue and introduced the global cached response.
- The exact `refresh_time` / `Nil` empty-result searches returned no exact hit.
- Treat this as local residual hardening unless explicitly re-authorized for a
  public follow-up.
