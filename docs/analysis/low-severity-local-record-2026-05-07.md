# Low-Severity Local Record

Date: 2026-05-07

Scope: local record for low-severity or public-hardening security findings from
the post-v4.4.0 Zebra audit. Per user direction on 2026-05-07, stop posting new
public GitHub issues for these items unless explicitly re-authorized.

This file is a routing ledger, not a replacement for the detailed finding notes.

## Posting Policy

- Do not create more public GitHub issues for low-severity findings without a new
  explicit instruction.
- Keep duplicate checks and evidence locally.
- Private-disclosure-worthy items remain separate in
  `docs/analysis/private-advisory-drafts-2026-05-07.md`.
- If an item is later escalated, re-run duplicate checks against open and closed
  GitHub issues before posting or commenting.

## Public Or Explicitly Re-authorized

These items have been covered by public issues or comments. Most were posted
before the stop instruction; any later entries note the explicit re-authorization.

| Issue | Local item | Triage |
| --- | --- | --- |
| [#9301](https://github.com/ZcashFoundation/zebra/issues/9301#issuecomment-4393550468) | Mining RPC timeout and error-taxonomy hardening context | Public comment posted before stop; detailed local note is `docs/analysis/submitblock-timeout-security-note.md`; keep further details local unless re-authorized |
| [#10534](https://github.com/ZcashFoundation/zebra/issues/10534#issuecomment-4393606461) | V6 `auth_digest()` panic evidence on the same librustzcash conversion boundary | Public comment posted before stop; do not re-report as fresh |
| [#10545](https://github.com/ZcashFoundation/zebra/issues/10545#issuecomment-4393785968) | P2P codec header-only body reservation proof | Public comment posted before stop; local ledger keeps the fuller low-severity note |
| [#10564](https://github.com/ZcashFoundation/zebra/issues/10564) | P2P codec reserves full declared message body before receiving body bytes | Filed on 2026-05-08 after explicit re-authorization; standalone sibling of #10545 |
| [#10549](https://github.com/ZcashFoundation/zebra/issues/10549#issuecomment-4393548171) | P2P `getblocks` / `getheaders` locator length cap and pre-cap scan evidence | Public comment posted before stop; covered publicly |
| [#10550](https://github.com/ZcashFoundation/zebra/issues/10550) | Multi-query RPC snapshot consistency for `getblock`, `getblockheader`, and `gettxout` | RPC response-coherence hardening; covers `docs/analysis/rpc-gettxout-snapshot-consistency-note.md` and the public consistency portion of `docs/analysis/rpc-getblock-height-snapshot-consistency-note.md` |
| [#10551](https://github.com/ZcashFoundation/zebra/issues/10551#issuecomment-4393547108) | Peer Prometheus high-cardinality labels plus sibling mempool/RPC metric-label context | Public comment posted before stop; peer user-agent metric label is now locally test-backed; mempool/RPC sibling proofs stay local unless re-authorized |
| [#10552](https://github.com/ZcashFoundation/zebra/issues/10552#issuecomment-4393552792) | GBT template byte-budget mismatch / final serialized block-size guidance | Public comment posted before stop; covered publicly |
| [#10553](https://github.com/ZcashFoundation/zebra/issues/10553) | P2P unknown-command decoder `Ok(None)` after consuming frame | Low-severity P2P availability/correctness hardening |
| [#10556](https://github.com/ZcashFoundation/zebra/issues/10556) | Non-finalized transparent `received` overflow | Low-severity address-index RPC correctness hardening |
| [#10557](https://github.com/ZcashFoundation/zebra/issues/10557) | Regtest funding-stream validation bypass panic | Low-severity custom-network availability hardening; detailed local note is `docs/analysis/regtest-funding-stream-validation-bypass-panic-note.md` |
| [#10558](https://github.com/ZcashFoundation/zebra/issues/10558) | Custom lockbox disbursement config panic | Low-severity custom-network availability hardening; detailed local note is `docs/analysis/custom-lockbox-disbursement-config-panic-issue.md` |
| [#10559](https://github.com/ZcashFoundation/zebra/issues/10559) | Mempool infrastructure failures cached as exact-tip rejections | Low-severity mempool availability hardening |
| [#10560](https://github.com/ZcashFoundation/zebra/issues/10560) | P2P `notfound` request-correlation gap | Bounded P2P availability hardening |
| [#10565](https://github.com/ZcashFoundation/zebra/issues/10565) | V5 mempool witnessed-ID exactness and same-effects pending limits | Filed on 2026-05-08 after explicit re-authorization; low-severity mempool correctness and availability hardening |
| [#10566](https://github.com/ZcashFoundation/zebra/issues/10566) | Transaction `getdata` IDs should be capped before mempool lookup | Filed on 2026-05-08 after explicit re-authorization; low-severity P2P availability hardening |
| [#10568](https://github.com/ZcashFoundation/zebra/issues/10568) | Unsupported BIP37 and unexpected empty-command P2P bodies | Filed on 2026-05-09 after explicit re-authorization; low-severity P2P parser/conformance hardening |
| [#10569](https://github.com/ZcashFoundation/zebra/issues/10569) | Nonzero counted-header counts and trailing `block` / `tx` bytes | Filed on 2026-05-09 after explicit re-authorization; low-severity P2P parser/conformance hardening |

## Local-Only Queue

### P2P mempool request full-enumeration work

Status: local-only, not posted.

Detailed note: `docs/analysis/p2p-mempool-request-enumeration-note.md`.

Summary: a tiny unauthenticated P2P `mempool` message can ask Zebra to enumerate
the local mempool transaction ID set before response truncation. Existing
timeouts, load shedding, and mempool-size bounds limit severity.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "mempool" "TransactionIds" "enumeration"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "MempoolTransactionIds" "MAX_TX_INV_IN_SENT_MESSAGE"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "mempool" "inv" "25,000"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "mempool message" "transaction ids" "cap"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra MempoolTransactionIds'
```

Closest overlaps:

- Targeted cap/enumeration searches returned no hits.
- The broad `MempoolTransactionIds` search returned historical test and
  implementation work such as #5384, #5706, #2214, #2726, and related PRs.
  These cover flaky tests, general fanout limits, and `notfound` behavior, not
  this bounded-enumeration/cap-hardening concern.

Local functional evidence rerun:

```sh
cargo test -p zebrad mempool_requests_for_transactions --lib
cargo test -p zebrad mempool_transaction_ids_request_forwards_full_set_before_connection_cap_today --lib
```

Result on 2026-05-09: passed. The existing real-mempool test confirms the
inbound `MempoolTransactionIds` path returns the mempool's stored transaction ID
set. The added current-behavior test
`mempool_transaction_ids_request_forwards_full_set_before_connection_cap_today`
uses a mock mempool response with `MAX_TX_INV_IN_SENT_MESSAGE + 3` IDs and
confirms inbound forwards the entire set before the connection-layer `inv` cap.

### P2P transaction inv queue amplification

Status: local-only, not posted.

Detailed note: `docs/analysis/p2p-transaction-inv-queue-amplification-note.md`.

Summary: transaction `inv` advertisements can enqueue many mempool download
candidates before tighter per-peer or per-message policy checks. This is bounded
but attacker-influenced P2P work.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "AdvertiseTransactionIds" "FullQueue"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "transaction inv" "mempool" "queue" "amplification"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "mempool" "MAX_INBOUND_CONCURRENCY" "inv"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra AdvertiseTransactionIds'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "transaction downloader" "FullQueue"'
```

Closest overlaps:

- Targeted `AdvertiseTransactionIds` / `FullQueue`, transaction-inv queue
  amplification, and transaction-downloader `FullQueue` searches returned no
  hits.
- #10565 is adjacent V5 pending-limit/exactness work, not this inbound
  advertisement queue path.
- #6911 is broad slow inbound-service hardening.
- PR #6625 is the closest overlap: it added sent-message inventory caps and
  gossip rate limiting, but does not cap inbound peer advertisements before
  `mempool::Request::Queue` bookkeeping.

Local proof rerun:

```sh
cargo test -p zebrad advertise_transaction_ids_forwards_full_set_to_mempool_before_download_cap_today --lib
cargo test -p zebrad mempool_queue_reports_every_gossiped_id_before_download_cap --lib
```

Result on 2026-05-09: passed. The inbound test confirms a full advertised ID
set reaches `mempool::Request::Queue` before the downloader cap. The mempool
test confirms an enabled mempool returns a full-length `Response::Queued` vector
for `MAX_INBOUND_CONCURRENCY + 3` unique gossiped IDs, while only 25 downloads
remain in flight and overflow entries return `FullQueue`.

### Mempool empty spent-outputs standardness boundary

Status: local-only, not posted.

Detailed note:
`docs/analysis/mempool-empty-spent-outputs-standardness-boundary-note.md`.

Summary: `Storage::reject_if_non_standard_tx()` skips input standardness and
P2SH sigop accounting when `VerifiedUnminedTx.spent_outputs` is empty. A
synthetic storage test proves a transparent-input transaction rejects when
non-standard previous outputs are supplied, but inserts when `spent_outputs =
[]`. Current production reachability still appears blocked by verifier-side
previous-output resolution, so this is local defense-in-depth rather than a
live remote report.

Local proof:

```sh
cargo test -p zebrad transparent_input_empty_spent_outputs_bypasses_input_standardness_today --lib
```

Result on 2026-05-09: passed.

### P2P BIP37 and empty-body message strictness

Status: publicly covered by #10568; keep the detailed local note as evidence.

Detailed note: `docs/analysis/p2p-bip37-filter-message-hardening-note.md`.

Summary: ignored BIP37 filter messages and request-like empty-body messages such
as `mempool`, `getaddr`, `filterclear`, and `verack` should be rejected or
tightly size-checked when they carry unexpected body bytes. Severity is bounded
parser/request hardening.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "BIP37" "filterload" "filteradd" "filterclear"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "filteradd" "520"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "mempool" "getaddr" "verack" "extra bytes"'
```

Closest overlaps:

- #10315 is a broad ZIP-204 network conformance tracker, not a duplicate of the
  concrete unsupported-BIP37 and extra-body acceptance paths.
- #520 is an unrelated dependency bump.
- The `mempool` / `getaddr` / `verack` extra-bytes search returned no hits.

Local proofs rerun:

```sh
cargo test -p zebra-network filteradd_message_too_large_is_truncated_and_accepted_today --lib
cargo test -p zebra-network filterload_too_many_hash_functions_is_accepted_today --lib
cargo test -p zebra-network filterclear_message_with_body_is_accepted_today --lib
cargo test -p zebra-network mempool_message_with_body_is_accepted_today --lib
cargo test -p zebra-network getaddr_message_with_body_is_accepted_today --lib
cargo test -p zebra-network verack_message_with_body_is_accepted_today --lib
cargo test -p zebra-network bip37_filter_messages_are_consumed_without_inbound_request_today --lib
```

Result on 2026-05-09: all focused tests passed.

### P2P counted-header and trailing-junk parse strictness

Status: publicly covered by #10569; keep the detailed local note as evidence.

Detailed note: `docs/analysis/p2p-block-header-parse-strictness-note.md`.

Summary: wire parsing should reject nonzero counted-header transaction counts
and reject trailing bytes for fixed-format `block` / `tx` messages. This is
public protocol-strictness hardening unless a stronger impact is proven.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "counted header" "transaction count"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "headers" "nonzero transaction count"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "trailing bytes" "block" "tx" "message"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "extra data after decoding message"'
```

Closest overlaps:

- #1920 implemented trusted vector preallocation and is parser-adjacent history,
  not a duplicate of nonzero counted-header transaction-count acceptance.
- #2446 added ZIP-239 `MSG_WTX` inventory parsing and is unrelated to accepting
  trailing bytes after parsed `block` / `tx` prefixes.
- The nonzero-headers and exact extra-data-log searches returned no hits.

Local proofs rerun:

```sh
cargo test -p zebra-chain counted_header_nonzero_transaction_count_is_accepted_today --lib
cargo test -p zebra-network accepted_today --lib
```

Result on 2026-05-09: the counted-header proof passed, and the
`accepted_today` network test run covered all focused codec proofs.

### P2P block locator length hardening

Status: publicly covered by #10549; not a fresh local-only candidate.

Detailed note: `docs/analysis/p2p-block-locator-length-hardening-note.md`.

Summary: `getblocks` and `getheaders` response sizes are capped, but the inbound
request locator length is bounded only by the overall P2P message size before
state searches for a chain intersection. A peer can send a large locator full of
unknown hashes and make Zebra scan many entries before returning a capped
response or `Nil`.

Live issue check on 2026-05-09:

- #10549, open, `Cap getblocks/getheaders locator vector length at
  deserialization time`, covers the same core issue: the P2P codec deserializes
  locator hash vectors up to the generic `block::Hash` preallocation bound,
  while honest locators are much smaller. Do not re-report.

Duplicate check performed on 2026-05-07:

```sh
gh issue list --repo ZcashFoundation/zebra --search 'block locator length getblocks getheaders in:title,body' --state all --limit 100
gh issue list --repo ZcashFoundation/zebra --search 'FindBlockHashes large locator scan in:title,body' --state all --limit 100
gh issue list --repo ZcashFoundation/zebra --search 'getblocks locator response cap in:title,body' --state all --limit 100
gh issue list --repo ZcashFoundation/zebra --search 'getheaders locator maximum in:title,body' --state all --limit 100
```

Closest hit:

- #8907, closed, covers P2P `headers` response semantics and zcashd
  compatibility. It does not cover oversized request locator scan cost.

The other searches returned no hits.

Local proofs rerun:

```sh
cargo test -p zebra-network locator_longer_than_response_cap --lib
cargo test -p zebra-state find_blocks_scans_large_locator_before_response_cap_today --lib
```

Result on 2026-05-07: both commands passed. The tests confirm locator vectors
longer than response caps decode and round-trip at the P2P layer, and state
scans past the response caps to find a late locator intersection today.

### Sync FindBlocks junk-hash steering

Status: local-only, not posted.

Detailed note: `docs/analysis/sync-findblocks-junk-hash-steering-note.md`.

Summary: malicious-but-nonempty `FindBlocks` responses are bounded, but they can
still steer sync ordering. A fast peer can put attacker-chosen unknown hashes
ahead of honest hashes, causing bounded `BlocksByHash`, retry, and `notfound`
work before honest continuation hashes are downloaded.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "FindBlocks" "junk hash"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "obtain_tips" "BlocksByHash"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "FindBlocks" "NotFoundRegistry"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "non-empty" "FindBlocks" "stall"'
```

No issue hits were returned.

Local proof added:

```sh
cargo test -p zebrad obtain_tips_queues_fast_junk_hashes_before_later_honest_hashes_today --lib
cargo test -p zebrad sync_block_too_high_obtain_tips --lib
```

Result on 2026-05-09: both commands passed. The first test directly confirms
response-order steering; the second exercises a nearby sync height limiter that
bounds impact.

### P2P unsolicited full-block eager decode

Status: local-only, not posted.

Detailed note: `docs/analysis/p2p-unsolicited-block-decode-hardening-note.md`.

Summary: full `block` messages are deserialized by the network codec before the
connection state machine decides whether the block was requested, mismatched,
unsolicited, or useful. Unsolicited/canceled full blocks are later marked
`Unused`, and handshake loops decode and ignore non-handshake messages until the
handshake timeout. This is bounded P2P CPU/memory hardening rather than
consensus acceptance.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "unsolicited block" "inbound"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "full block" "decoded" "unsolicited"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "handshake" "full block" "message"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "mismatched block" "decoded" "ignored"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "Message::Block" "Unused"'
```

Closest overlaps:

- Direct unsolicited/full-block/mismatched-block searches returned no duplicate
  hits.
- #10545 is adjacent generic untrusted-vector preallocation hardening, not this
  unsolicited full-block routing path.
- #6662, #5257, #3295, and #2660 are unrelated or historical network/RPC PRs.

Local proof status: partial current-behavior coverage. The new
`unsolicited_block_message_is_unused_without_inbound_request_today` test proves
that an already-decoded unsolicited `Message::Block` is treated as unused
without inbound-service routing. The full eager-decode cost remains direct
source evidence in the codec and handshake paths.

### P2P peer-set unready age hardening

Status: already publicly tracked by #7822; do not post as a fresh issue.

Detailed note: `docs/analysis/p2p-peer-set-unready-availability-note.md`.

Summary: Zebra's peer set expects connected peers to become ready within a few
minutes or timeout, but there is no explicit peer-set-level age limit for peers
that remain unready because they keep their connection busy with inbound
messages. Existing connection limits, per-IP limits, timeout behavior, and
overload handling bound impact, so this is availability hardening rather than
private disclosure material.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra peer set unready inbound messages never ready'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "peer set" "unready" "inbound"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "unready" "never ready" "peer"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "drop peers" "overload" "never become ready"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "Discover::Insert" "peer set" "limit"'
```

Closest hits:

- #7822, open, exactly tracks synthetic nodes taking up connection slots. Its
  body includes the overload scenario where peers block readiness by constantly
  sending inbound requests and a TODO to drop peers that overload Zebra and
  never become ready.
- #1435 and PR #7859 are historical network-hang context.
- #6936, #1965, #10248, #10024, #1405, and related hits are broad or adjacent.

Local proof rerun:

```sh
cargo test -p zebra-network unready_peer_with_full_request_channel_is_not_age_evicted_today --lib
```

Result on 2026-05-09: passed. The test confirms a mock peer with a saturated
request channel remains in the unready set with its cancel handle after ten
minutes of paused Tokio time.

### P2P peer-path `try_send()` backpressure audit

Status: eliminated as a fresh vulnerability candidate; local-only note.

Detailed note: `docs/analysis/p2p-try-send-backpressure-audit-note.md`.

Summary: a focused pass over `zebra-network` peer-path `try_send()` sites did
not find a distinct score-loss, disconnect-loss, or unique-demand-loss issue.
The audited paths fail explicitly, time out and report peer failure, preserve an
already-queued `MorePeers` demand token, requeue demand after failed dials when
there is room, or defer stall disconnects until the next peer-set poll. The
remaining availability concern is still the already tracked peer-set unready
capacity class in #7822.

Local proof added:

```sh
cargo test -p zebra-network full_more_peers_channel_preserves_existing_demand_today --lib
cargo test -p zebra-network failed_dial_requeues_consumed_demand_token_today --lib
cargo test -p zebra-network stall_events_are_deferred_until_next_poll_then_disconnect_today --lib
cargo test -p zebra-network client_call_on_disconnected_server_tx_returns_error_today --lib
cargo test -p zebra-network heartbeat_full_server_tx_times_out_and_reports_error_today --lib
```

Result on 2026-05-09: all five focused tests passed.

### P2P getaddr empty-cache rescan amplification

Status: local-only residual under existing public `getaddr` rate-limit history;
not posted.

Detailed note: `docs/analysis/p2p-getaddr-response-amplification-note.md`.

Summary: a normal non-empty `getaddr` cache avoids repeated address-book scans,
but empty refreshes do not advance the refresh deadline. A peer can repeatedly
send small `getaddr` requests in an empty-cache state and force repeated
clone/filter/shuffle work over the address book.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "getaddr" "empty cache" "address book"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "getaddr" "amplification"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "fresh_get_addr_response" "refresh_time"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "CACHED_ADDRS_REFRESH_INTERVAL" "getaddr"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "empty" "getaddr" "refresh"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "getaddr" "Nil" "refresh_time"'
```

Closest hits:

- #7823 and PR #7955 cover the broad repeated `GetAddr` response rate-limit
  issue and introduced the cached response.
- The exact `refresh_time` / `Nil` empty-result searches returned no exact hit.

Local proof rerun:

```sh
cargo test -p zebrad empty_getaddr_refresh_leaves_refresh_time_stale_today --lib
cargo test -p zebrad caches_getaddr_response --lib
```

Result on 2026-05-09: both passed. The first test confirms empty refreshes leave
the deadline stale; the second confirms normal non-empty responses are cached.

### P2P stale gossiped address dial churn

Status: local-only residual of #1865, not posted.

Detailed note: `docs/analysis/p2p-stale-gossiped-address-dial-churn-note.md`.

Summary: old gossiped peer addresses are filtered out of gossip responses once
they are no longer active, but they are still accepted into the address book and
eligible for one initial outbound connection attempt while in
`NeverAttemptedGossiped`. This creates bounded peer-crawler churn if a peer feeds
old, unreachable, syntactically valid listener addresses.

Duplicate check refreshed on 2026-05-09:

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

Closest hits:

- #1865 explicitly includes the older-peer acceptance hardening idea under
  accepting old peers from other nodes.
- PR #2178 fixed the related future `last_seen` trust problem from #1871, but
  the current checkout still accepts old never-attempted gossiped peers.
- Exact stale-dial and old-gossiped-connection-attempt searches returned no
  fresher dedicated issue.

Local proof added:

```sh
cargo test -p zebra-network old_gossiped_peer_is_still_initially_connectable_today --lib
```

Result on 2026-05-09: passed. The test confirms a gossiped address last seen 30
days ago survives `validate_addrs()`, is not active for gossip, is not recently
seen, and is still initially dialable.

### Isolated outbound handshake fingerprint

Status: local-only residual of #3300 and isolated connection design history,
not posted.

Detailed note: `docs/analysis/isolated-handshake-fingerprint-note.md`.

Summary: Zebra isolated outbound connections emit a stable `version` message
profile that differs from normal Zebra peer-set handshakes: empty top-level
services, unspecified default-port address fields, `start_height = 0`,
`relay = false`, and often an empty user agent. A destination peer can classify
traffic as using Zebra's isolated API, which is privacy hardening for wallet-like
or transaction-submission use cases rather than consensus/security emergency
material.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra isolated handshake fingerprint version message'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "isolated" "handshake" "fingerprint"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "connect_isolated" "version message"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "address_from" "relay=false" "isolated"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "isolated" "user_agent" "start_height"'
gh api repos/ZcashFoundation/zebra/issues/3300
gh api repos/ZcashFoundation/zebra/pulls/1014
gh api repos/ZcashFoundation/zebra/pulls/4870
```

Closest hits:

- #3300 covers isolated `version` timestamp anonymity and remote peer services
  as transaction-broadcast privacy hardening.
- PR #1014 and PR #4870 are isolated-connection design/API history.
- Exact full-tuple fingerprint searches returned no fresher dedicated issue.

Local proof rerun:

```sh
cargo test -p zebra-network connect_isolated_sends_anonymised_version_message_mem --lib
```

Result on 2026-05-09: passed. The test confirms the current isolated
`VersionMessage` wire profile over the in-memory transport.

### P2P inbound ephemeral address reconnect candidates

Status: local-only residual/regression of #2120 and #7951/#7977, not posted.

Detailed note:
`docs/analysis/p2p-inbound-ephemeral-address-reconnect-note.md`.

Summary: successful inbound handshakes can store the remote TCP source socket as
an inbound address-book entry. The inbound flag prevents gossip, but does not
currently prevent later outbound reconnect selection after the normal recent-peer
delay. This is bounded peer-discovery churn, not consensus risk.

Duplicate check refreshed on 2026-05-09:

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

Closest hits:

- #2120 is earlier security work to stop putting temporary inbound remote
  addresses in the address book.
- #7951 and PR #7977 are later closed security work to send handshake peer
  addresses to the connection cache rather than directly to the address book.
- #7824 is broader synthetic-node spread tracking.
- Exact current-behavior searches for `InboundDirect` plus address-book or
  reconnection behavior returned no fresher dedicated issue.

Local proof added:

```sh
cargo test -p zebra-network inbound_ephemeral_address_becomes_reconnection_candidate_today --lib
```

Result on 2026-05-09: passed. The test confirms an inbound remote socket address
is accepted as an inbound `MetaAddr` today and later appears as a reconnect
candidate after `MIN_PEER_RECONNECTION_DELAY`.

### Address-book ban cleanup assumes same-IP entries are contiguous

Status: local-only residual of #9201, not posted; separate from the privately
reported `max_connections_per_ip > 1` ban-path panic.

Detailed note:
`docs/analysis/address-book-ban-noncontiguous-ip-cleanup-note.md`.

Summary: cleanup code for banned same-IP entries appears to assume same-IP
entries are contiguous in address-book iteration order. This is distinct from
the privately reported `max_connections_per_ip > 1` ban-path panic and currently
looks like low-severity peer-management hardening.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "address book" "ban" "same IP" "contiguous"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "misbehavior" "ban" "address book" "cleanup"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "banned IP" "address book" "same-IP"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "bans_by_ip" "by_addr"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "max_connections_per_ip" "ban" "panic"'
gh api repos/ZcashFoundation/zebra/pulls/9201
```

Closest hit:

- PR #9201 introduced the current misbehavior-ban implementation. It is
  implementation history, not a duplicate report of the non-contiguous same-IP
  cleanup residual.
- Targeted same-IP cleanup searches returned no exact issue hit.

Local proof added:

```sh
cargo test -p zebra-network ban_cleanup_leaves_non_contiguous_same_ip_entries_today --lib
```

Result on 2026-05-09: passed. The test builds non-contiguous same-IP entries,
applies a threshold misbehavior update, confirms the IP is banned, and confirms
at least one banned-IP entry remains in the address book today.

### Peer-set ban watch lazy disconnect

Status: local-only residual of #9201/#10258, not posted.

Detailed note: `docs/analysis/peer-set-ban-watch-lazy-disconnect-note.md`.

Summary: address-book bans are published through a watch channel, and the peer
set samples the current ban map in existing polling paths, but it does not appear
to register a ban-change wakeup. Already-ready services for newly banned IPs are
therefore dropped lazily on a later peer-set poll, rather than immediately on the
ban update.

Duplicate check refreshed on 2026-05-09:

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

Closest hits: PR #9201 introduced the address-book misbehavior ban channel and
`PeerSet::poll_ready()` ban checks; PR #10258 fixed related stale cancel-handle
cleanup for banned unready peers. Neither appears to cover the ready-peer
ban-watch lazy-disconnect residual exactly.

Local proof added:

```sh
cargo test -p zebra-network ban_watch_update_does_not_drop_ready_peer_until_peer_set_polled_today --lib
```

Result on 2026-05-09: passed. The test publishes a ban for an already-ready
peer and confirms the watch update alone leaves the peer in `ready_services`;
the next `PeerSet::poll_ready()` samples the ban map and drops it.

### Invalid block peer-misbehavior attribution gaps

Status: local-only, not posted.

Detailed notes:

- `docs/analysis/inbound-gossiped-block-router-error-misbehavior-note.md`
- `docs/analysis/malformed-block-height-misbehavior-attribution-note.md`

Summary: invalid blocks are rejected, but two score-bearing invalid-block paths
can lose address-book misbehavior attribution. Inbound gossiped block verifier
errors are boxed as `RouterError`, while cleanup downcasts to `VerifyBlockError`.
Malformed no-height blocks can be rejected in pre-verifier download paths before
the score-bearing `BlockError::MissingHeight` variant preserves the serving peer
address.

Duplicate checks refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra RouterError VerifyBlockError misbehavior score inbound gossiped block'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra MissingHeight malformed block misbehavior attribution'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "RouterError" "VerifyBlockError" "misbehavior"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "InvalidHeight" "misbehavior"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "invalid gossiped block" "misbehavior"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "gossiped block" "no height"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "synced block with no height"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "MissingHeight" "InvalidHeight"'
```

No issue hits were returned.

Local proofs rerun:

```sh
cargo test -p zebrad score_bearing_router_error_does_not_downcast_to_verify_block_error --lib
cargo test -p zebrad invalid_height_download_error_does_not_send_misbehavior_today --lib
cargo test -p zebrad inbound_missing_height_error_drops_advertiser_addr_today --lib
```

Result on 2026-05-09: all three commands passed. The first test confirms a score-bearing
`VerifyBlockError` wrapped as `RouterError` does not downcast back to
`VerifyBlockError`, while the boxed `RouterError` still carries the nonzero
misbehavior score. The second test sends a sync `InvalidHeight` response
through `ChainSync::handle_block_response()` and confirms no address-book
misbehavior update is emitted. The third test queues a gossiped no-height block
whose peer response includes an advertiser address and confirms the inbound
download error returns `None` for the advertiser address.

### Primitive verifier failure taxonomy and fallback hardening

Status: local-only residual of #1186/#10559, not posted.

Detailed note: `docs/analysis/primitive-verifier-failure-taxonomy-note.md`.

Summary: no invalid-acceptance path was found in the primitive verifier stack:
batch failures, dropped worker responses, and fallback verification fail closed.
The remaining hardening is taxonomy, panic containment, observability, and
latency: some consensus-invalid primitive failures surface as
`InternalDowncastError`, some verifier dropped-channel paths still panic with
"verifier was dropped without flushing", primary batch metrics can count valid
neighbors as invalid before fallback localizes a mixed batch, and invalid
mempool/RPC traffic can make unrelated contemporaneous valid items pay
single-item fallback cost.

Duplicate checks refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra primitive verifier InternalDowncastError fallback batch metrics'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra verifier was dropped without flushing primitive'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra transaction verifier error taxonomy InternalDowncastError'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "InternalDowncastError" "TransactionError"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "batch" "fallback" "invalid" "metrics" "verifier"'
gh api repos/ZcashFoundation/zebra/issues/1186
gh api repos/ZcashFoundation/zebra/issues/10559
```

Closest hits: #1186 is an older closed broad cleanup issue for verification
error type deduplication and removing `InternalDowncastError`; #10559 publicly
covers the sharper mempool consequence where infrastructure errors can be
cached as exact-tip rejections. No exact public tracker was found for primitive
batch fallback metrics drift, watch-channel panic containment, or the full
async primitive error taxonomy.

Local proof rerun:

```sh
cargo test -p zebra-consensus v4_with_modified_joinsplit_is_rejected --lib
```

Result on 2026-05-09: passed. This confirms the focused modified-JoinSplit
rejection path still fails closed; broader synthetic worker-failure and fallback
metrics proofs remain future hardening work.

### Sapling TransmissionKey public API panic

Status: local-only residual/adjacent to #5476, not posted.

Detailed note:
`docs/analysis/sapling-transmission-key-public-api-panic-note.md`.

Summary: `zebra_chain::sapling::keys::TransmissionKey::try_from([u8; 32])`
documents a fallible malformed-byte parser, but currently unwraps the Jubjub
point decode before checking the `CtOption`. Malformed bytes can panic through
this public library API. No current Zebra node, consensus, mempool, P2P, or RPC
call site was found that feeds attacker-controlled input into this constructor,
so this is local library API hardening rather than private node disclosure.

Duplicate checks refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra TransmissionKey malformed bytes panic Sapling'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra Sapling TransmissionKey TryFrom unwrap panic'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "TransmissionKey::try_from"'
gh api repos/ZcashFoundation/zebra/issues/5476
```

Closest hit:

- #5476, broad cleanup for unused shielded key/address code and known key
  parsing/conversion panics. It is adjacent historical context, but not an exact
  tracker for this remaining malformed-byte `TransmissionKey::try_from()`
  panic.

No exact issue hit was returned.

Local proof rerun:

```sh
cargo test -p zebra-chain transmission_key_panics_on_malformed_bytes_today --lib
```

Result on 2026-05-09: passed. The test confirms
`TransmissionKey::try_from([0xff; 32])` panics today.

### Elasticsearch feature transport and panic hardening

Status: local-only, not posted; partial overlap with closed #8329/#7270.

Detailed note:
`docs/analysis/elasticsearch-feature-transport-and-panic-note.md`.

Summary: the experimental `elasticsearch` feature is outside default release
binaries, but when compiled in, the read-write state service enables indexing
and builds the client with TLS certificate validation disabled. After an
endpoint passes the initial `ping`, bulk send failures, unparsable responses, or
responses with `"errors": true` can panic the indexing path. This matters for
operators who compile the feature and point Zebra at a shared or remote
Elasticsearch endpoint.

Overlap/duplicate check refreshed on 2026-05-09:

- Closed #8329 covers Elasticsearch-unavailable panics.
- Closed #7270 covers Elasticsearch bulk-size panic risk.

```sh
gh api repos/ZcashFoundation/zebra/issues/8329
gh api repos/ZcashFoundation/zebra/issues/7270
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra Elasticsearch CertificateValidation None panic bulk errors true'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "CertificateValidation::None" elasticsearch'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "ES error" elasticsearch'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "errors" "true" "elasticsearch" "bulk"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "ES Request should never fail" elasticsearch'
```

Those do not cleanly cover endpoint-controlled HTTP 200 bulk responses with
`"errors": true`, or disabled TLS certificate validation. Keep this as local
feature-gated hardening context unless explicitly re-authorized.

Local proof rerun:

```sh
cargo test -p zebra-state elasticsearch_bulk_error_response_panics_today --features elasticsearch --lib
```

Result on 2026-05-09: passed. The test uses a local fake endpoint that accepts
`ping`, returns HTTP 200 with a JSON bulk response containing `"errors": true`,
and confirms the Elasticsearch indexing path panics today under the optional
feature.

### Custom-network funding stream and NU6.1 activation config hardening

Status: local-only residual/adjacent to #10557/#10558 and implementation
history in #9526/#9710; not posted.

Detailed notes:

- `docs/analysis/configured-funding-streams-config-panic-note.md`
- `docs/analysis/custom-network-parameter-panic-sweep-note.md`
- `docs/analysis/custom-network-implicit-nu6-1-lockbox-boundary-note.md`

Summary: several custom-network configuration shapes can still turn malformed or
surprising local config into panics or unintended activation semantics rather
than typed configuration errors. Configured Testnet funding streams can panic on
too few recipient addresses, excessive numerators, or wrong-network addresses.
Configured Testnet `slow_start_interval` values can also panic during default
funding-stream address-period validation, and programmatic custom networks that
clear funding streams can later panic in `founders_reward()` if the floored
slow-start subsidy is not exactly divisible by five.
Separately, omitted `nu6_1` activation can inherit a later configured upgrade
height such as NU7, making NU6.1 one-time lockbox logic unexpectedly active at
that later height on custom Regtest/Testnet parameters.

Duplicate/overlap checks refreshed on 2026-05-09:

- #10557 covers the Regtest-specific funding-stream validation bypass and later
  runtime panic path.
- #10558 covers configured lockbox disbursement invalid-address and
  invalid-total panic paths.

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra NU6.1 lockbox omitted nu7 activation fallback'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra custom network nu6_1 omitted lockbox disbursements'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "NU6.1" "NU7" "lockbox" "Regtest"'
gh api repos/ZcashFoundation/zebra/issues/10557
gh api repos/ZcashFoundation/zebra/issues/10558
gh api repos/ZcashFoundation/zebra/pulls/9526
gh api repos/ZcashFoundation/zebra/pulls/9710
```

No exact issue hits were returned for omitted `nu6_1` inheriting a later
configured NU7 height for NU6.1 lockbox logic.

Local proofs rerun:

```sh
cargo test -p zebra-chain check_configured_funding_stream_constraints --lib
cargo test -p zebra-chain activates_network_upgrades_correctly --lib
cargo test -p zebra-chain omitted_nu6_1_inherits_nu7_lockbox_boundary_today --lib
cargo test -p zebra-chain configured_slow_start_interval_can_make --lib
```

Result on 2026-05-09: all passed. The first confirms current configured
funding-stream assertion panics; the second confirms the activation-height
fallback primitive; the third ties that fallback to the Regtest lockbox boundary
by showing omitted `nu6_1` inherits a later configured NU7 height while default
Regtest lockbox disbursements are empty. The slow-start tests confirm both the
default funding-stream validation panic and the deferred founders reward
`div_exact(5)` panic.

### Sync concurrency config overflow

Status: local-only, not posted.

Detailed note: `docs/analysis/sync-concurrency-config-overflow-note.md`.

Summary: sync concurrency config values are only lower-bounded. Extremely large
local values can flow into unchecked `usize` sizing arithmetic. The concrete
proof uses `full_verify_concurrency_limit = usize::MAX` and confirms
`ChainSync::lookahead_limit()` panics in debug/test builds when crossing from
checkpoint verification into full verification. In release builds this is likely
to wrap rather than panic, creating an unexpectedly small effective lookahead
limit. A follow-up lower-level proof also confirms that an effective downloader
`lookahead_limit = usize::MAX` can panic the sync download task's height filter
before the node has a best tip.

Local proof:

```sh
cargo test -p zebrad huge_full_verify_concurrency_limit_can_overflow_lookahead_limit_today --lib
cargo test -p zebrad huge_lookahead_limit_can_panic_downloader_height_filter_today --lib
```

Result on 2026-05-09: both passed.

### Network peerset config overflow

Status: local-only, not posted.

Detailed note: `docs/analysis/network-peerset-config-overflow-note.md`.

Summary: `peerset_initial_target_size` is accepted as `usize` and only
zero-checked. A very large TOML-representable value can overflow derived
inbound/outbound connection-limit arithmetic in debug/test builds; in release
builds it is likely to wrap into surprising effective limits.

Local proof:

```sh
cargo test -p zebra-network oversized_peerset_initial_target_size_overflows_connection_limits_today --lib
```

Result on 2026-05-09: passed.

### Peer cache startup ingest hardening

Status: local-only, not posted.

Detailed note: `docs/analysis/peer-cache-startup-ingest-hardening-note.md`.

Summary: `Config::load_peer_cache()` reads the whole peer-cache file, parses
every line, logs every invalid entry, and collects all valid entries before later
startup dial limits apply. Zebra's own cache writer caps persisted peer entries,
so current evidence supports local/persisted-input startup hardening rather than
remote private disclosure.

Duplicate check refreshed on 2026-05-09:

```sh
gh issue list --repo ZcashFoundation/zebra --search 'peer cache startup ingest in:title,body' --state all --limit 100
gh issue list --repo ZcashFoundation/zebra --search 'load_peer_cache oversized file in:title,body' --state all --limit 100
gh issue list --repo ZcashFoundation/zebra --search 'cached peer list invalid line log in:title,body' --state all --limit 100
gh issue list --repo ZcashFoundation/zebra --search 'MAX_PEER_DISK_CACHE_SIZE load peer cache in:title,body' --state all --limit 100
gh issue list --repo ZcashFoundation/zebra --search 'peer cache invalid lines in:title,body' --state all --limit 100
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "oversized_peer_cache_file_loads_more_than_writer_limit_today"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "load_peer_cache" "MAX_PEER_DISK_CACHE_SIZE"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "peer cache" "oversized"'
```

No hits were returned.

Local proof added:

```sh
cargo test -p zebra-network oversized_peer_cache_file_loads_more_than_writer_limit_today --lib
```

Result on 2026-05-09: passed. The test writes a temporary peer-cache file with
`MAX_PEER_DISK_CACHE_SIZE + 25` valid entries and confirms
`Config::load_peer_cache()` loads every entry today, rather than applying the
writer-side peer-cache limit to the read path. Invalid-line logging and startup
address-book update work remain source-evidence-only.

### Direct pushed transaction source-attribution loss

Status: local-only, not posted.

Detailed note:
`docs/analysis/mempool-direct-push-source-attribution-note.md`.

Summary: direct pushed P2P transactions lose source-peer metadata before mempool
verification, limiting attribution to the address-book misbehavior pipeline.

Duplicate check refreshed on 2026-05-09:

```sh
gh issue list --repo ZcashFoundation/zebra --search 'direct pushed transaction advertiser addr misbehavior in:title,body' --state all --limit 100
gh issue list --repo ZcashFoundation/zebra --search 'PushTransaction source attribution mempool in:title,body' --state all --limit 100
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "PushTransaction" "advertiser_addr"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "invalid_direct_pushed_transaction_has_no_advertiser_addr_today"'
```

No hits were returned.

Local proof added:

```sh
cargo test -p zebrad invalid_direct_pushed_transaction_has_no_advertiser_addr_today --lib
```

Result on 2026-05-09: passed. The test confirms a direct pushed transaction can
fail with score-bearing `TransactionError::BadBalance`, but the resulting
`TransactionDownloadVerifyError::Invalid` has `advertiser_addr: None`.

### Mempool PendingOutputs retention

Status: local-only, not posted.

Detailed note: `docs/analysis/mempool-pending-outputs-retention-note.md`.

Summary: abandoned `AwaitOutput` waiters leave a `PendingOutputs` sender entry
behind until an explicit `prune()`, matching output insertion, or mempool clear.
Active verifier pressure is bounded, but repeated unique missing outpoints can
leave small stale entries between cleanup events.

Duplicate check refreshed on 2026-05-09:

```sh
gh issue list --repo ZcashFoundation/zebra --search 'PendingOutputs retention AwaitOutput in:title,body' --state all --limit 100
gh issue list --repo ZcashFoundation/zebra --search 'mempool pending outputs waiter prune in:title,body' --state all --limit 100
gh issue list --repo ZcashFoundation/zebra --search 'AwaitOutput timeout pending output in:title,body' --state all --limit 100
gh issue list --repo ZcashFoundation/zebra --search 'TransparentInputNotFound pending_outputs in:title,body' --state all --limit 100
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "PendingOutputs" "AwaitOutput"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "mempool_dropped_await_output_waiter_survives_poll_until_pruned_today"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "pending_outputs.prune"'
```

No hits were returned.

Local proof rerun:

```sh
cargo test -p zebrad pending_outputs --lib
cargo test -p zebrad mempool_dropped_await_output_waiter_survives_poll_until_pruned_today --lib
```

Result on 2026-05-09: both commands passed. The tests confirm dropped waiters
remain until `prune()`, duplicate outpoints share a pending sender, and an
abandoned `Request::AwaitOutput` queued through the normal mempool service
survives ordinary `CheckForVerifiedTransactions` polling until explicit prune.

### Mempool cascading removal notification consistency

Status: local-only, not posted.

Detailed note:
`docs/analysis/mempool-cascading-removal-notification-note.md`.

Summary: dependency removal can remove more transactions than callers report.
The sharpest path is insertion-time ZIP-401 eviction selecting an ancestor of a
newly inserted dependent: `VerifiedSet::remove()` removes the full dependency
set, but `evict_one()` returns only the selected victim, so `Storage::insert()`
can report the new transaction as added even if it was removed as a dependent.
Expiry cleanup has a similar "returned IDs are narrower than actually removed
dependents" shape.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "mempool cascading removal notification"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "VerifiedSet" "evict_one" "dependents"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "MempoolChange" "added" "evicted dependent"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "ZIP-401 eviction" "dependent transaction" "notification"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "remove_expired_transactions" "dependents"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "expired_parent_removes_unreported_non_expired_dependent_today"'
```

No hits were returned.

Local proof rerun:

```sh
cargo test -p zebrad expired_parent_removes_unreported_non_expired_dependent_today --lib
```

Result on 2026-05-09: passed. The test confirms that expiry cleanup can remove
both an expired parent and a non-expired dependent, while returning only the
expired parent ID to the caller.

Insertion-time proof added on 2026-05-09:

```sh
cargo test -p zebrad evicted_parent_reports_dependent_inserted_today --lib
```

Result: passed. The test uses a test-only eviction-key hook to force ZIP-401
eviction to select a parent after inserting its dependent. `Storage::insert()`
returns `Ok(child_id)`, but the child has already been cascade-removed from
storage and is not cached as rejected.

### Mempool tip-local rejection cache thrash

Status: local-only, not posted.

Detailed note: `docs/analysis/mempool-tip-rejection-cache-thrash-note.md`.

Summary: Zebra bounds `tip_rejected_exact` and `tip_rejected_same_effects` by
clearing the whole map when it grows past `MAX_EVICTION_MEMORY_ENTRIES`. This is
memory-safe, but enough unique tip-local rejections before the next block can
flush the cache and make older bad transaction IDs eligible for repeated
download or verification work.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "mempool tip rejection cache clear"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "tip_rejected_exact" "MAX_EVICTION_MEMORY_ENTRIES"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "rejection cache thrash" mempool'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "exact_tip_rejection_cache_clear_makes_old_reject_retryable_today"'
```

Closest hit:

- #10559, open, is adjacent but not duplicate. It covers infrastructure
  failures being cached as exact-tip rejections; this item covers all-or-nothing
  cache clearing after many ordinary tip-local rejections.

The other searches returned no hits.

Local proof rerun:

```sh
cargo test -p zebrad exact_tip_rejection_cache_clear_makes_old_reject_retryable_today --lib
cargo test -p zebrad reject_lists_are_limited --lib
```

Result on 2026-05-09: passed. The focused test confirms that an exact-tip
rejection suppresses retry before the cache exceeds the cap, then becomes
retry-eligible after enough distinct exact-tip rejections clear the entire map.
The broader property test confirms current over-limit behavior for the
tip-local rejection maps and the chain-wide eviction list.

### Lossy misbehavior report transport

Status: local-only, not posted.

Detailed note: `docs/analysis/misbehavior-reporting-lossy-channel-note.md`.

Summary: some misbehavior reports can be dropped by lossy transport before
address-book scoring sees them. Current evidence supports reliability hardening,
not a standalone high-severity vulnerability.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "misbehavior" "try_send" "dropped"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "misbehavior channel" "full"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "peer misbehavior report" "lost"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "full_misbehavior_channel_drops_score_bearing_sync_report_today"'
```

No hits were returned.

Local proof rerun:

```sh
cargo test -p zebrad full_misbehavior_channel_drops_score_bearing_sync_report_today --lib
cargo test -p zebrad full_misbehavior_channel_drops_score_bearing_mempool_report_today --lib
cargo test -p zebrad full_misbehavior_channel_drops_score_bearing_inbound_branch_today --lib
```

Result on 2026-05-09: passed. The sync and mempool tests fill the bounded
misbehavior channel with a sentinel, then trigger score-bearing invalid
sync-block and downloaded-mempool-transaction reports with advertiser
addresses. The inbound branch-level proof fills the same kind of channel and
injects a score-bearing `VerifyBlockError` into the extracted inbound reporting
helper. In all three cases the sentinel remains the only queued message, showing
the score report is dropped when `try_send()` hits a full channel. The ordinary
live inbound path still has the separate `RouterError` reachability caveat
documented in `docs/analysis/inbound-gossiped-block-router-error-misbehavior-note.md`.

### RPC batch request count cap

Status: local-only, not posted.

Detailed note: `docs/analysis/rpc-batch-request-hardening-note.md`.

Summary: Zebra configures request/response body limits and cookie auth, but
jsonrpsee batch count remains effectively body-size bounded rather than
explicitly request-count bounded. Relevant for authenticated or exposed RPC
deployments.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "RPC batch request limit"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "jsonrpsee" "BatchRequestConfig"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "RPC batch count cap"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "set_batch_request_config"'
```

No hits were returned.

Local evidence refreshed on 2026-05-09: source plus dispatch proof. Zebra's RPC
server builder does not set `BatchRequestConfig`, `Cargo.lock` pins
`jsonrpsee-server-0.24.10`, and the local dependency source defaults batch
handling to `Unlimited`, mapping it to `usize::MAX` during batch validation.
Added
`zebra-rpc/src/server/tests/batch.rs::unlimited_batch_request_dispatches_every_call_today`,
which shows a
three-entry HTTP batch reaches the RPC service three times under the inherited
`Unlimited` policy.

Focused verification:

```sh
cargo test -p zebra-rpc unlimited_batch_request_dispatches_every_call_today --lib
```

Result on 2026-05-09: passed.

### RPC text/plain compatibility and browser-origin hardening

Status: local-only, not posted.

Detailed note: `docs/analysis/rpc-text-plain-csrf-hardening-note.md`.

Summary: compatibility rewriting of `text/plain` to JSON can remove a useful
browser-origin barrier for auth-disabled reachable RPC deployments.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "RPC text/plain CSRF"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "Content-Type" "text/plain" "RPC"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "browser origin" "RPC" "cookie auth disabled"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "text_plain_request_is_rewritten_to_json_without_auth_today"'
```

Closest hits:

- #6363, "Accept text/plain RPC encodings automatically", is the closed
  compatibility issue that requested this behavior. It is related provenance,
  not a duplicate of this browser-origin hardening concern.
- #6885, the closed compatibility PR, implemented the content-type rewrite.

The CSRF/browser-origin and cookie-auth-disabled searches returned no hits.

Local proof added:

```sh
cargo test -p zebra-rpc text_plain_request_is_rewritten_to_json_without_auth_today --lib
```

Result on 2026-05-09: passed. The test confirms an auth-disabled middleware
rewrites `Content-Type: text/plain; charset=utf-8` to `application/json` before
calling the inner RPC service.

### RPC pre-guard HTTP connection retention

Status: local-only, not posted.

Detailed note: `docs/analysis/rpc-pre-guard-http-connection-retention-note.md`.

Summary: wrong-auth requests are rejected before body collection, but valid-auth
or auth-disabled clients can still hold body-collection futures before the inner
jsonrpsee guard. This is exposure-dependent availability hardening.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "RPC pre guard body collection"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "jsonrpsee connection guard" "RPC body"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "RPC body collection timeout" "connection"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "pending_body_waits_before_inner_service_today"'
```

No hits were returned.

Local proof added:

```sh
cargo test -p zebra-rpc pending_body_waits_before_inner_service_today --lib
cargo test -p zebra-rpc rpc_server_incomplete_body_waits_before_dispatch_today --lib
```

Result on 2026-05-09: passed. The middleware test confirms an auth-disabled
request with an incomplete body remains pending in Zebra's compatibility
middleware and does not call the inner RPC service. The real-server test starts
the actual auth-disabled `RpcServer`, sends complete valid JSON-RPC body bytes
with a larger declared `Content-Length` on three connections, and confirms those
connections remain open without a response while the mocked services receive no
requests.

### RPC HTTP compatibility parse amplification

Status: local-only, not posted.

Detailed note:
`docs/analysis/rpc-http-compatibility-parse-amplification-note.md`.

Summary: Zebra's HTTP compatibility middleware parses, reserializes, and
rebuilds strict JSON-RPC 2.0 request and response bodies even when no legacy
compatibility rewrite is needed. This is bounded, post-auth availability
hardening rather than a private-disclosure item.

Local proof added:

```sh
cargo test -p zebra-rpc strict_json_rpc_2 --lib
cargo test -p zebra-rpc rpc_server_text_plain_strict_json_rpc_reaches_dispatch_today --lib
```

Result on 2026-05-09: passed. The middleware tests prove strict 2.0 request and
response bodies are canonicalized before/after inner service handling. The
real-server test proves the production auth-disabled `RpcServer` stack accepts a
strict 2.0 request sent as `Content-Type: text/plain; charset=utf-8` via Zebra's
compatibility middleware and reaches jsonrpsee dispatch without backend service
requests.

### RPC address-index query bounds

Status: local-only, not posted.

Detailed note: `docs/analysis/rpc-address-index-query-bounds-note.md`.

Summary: address-index RPCs should cap address counts, chain ranges, and returned
items. RPC is disabled/authenticated by default and response-size caps mitigate
severity.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "address index RPC bounds"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "getaddresstxids" "address count limit"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "getaddressutxos" "response limit"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "rpc_getaddresstxids_forwards_large_default_range_today"'
```

No hits were returned.

Local proofs added:

```sh
cargo test -p zebra-rpc rpc_getaddresstxids_forwards_large_default_range_today --lib
cargo test -p zebra-rpc rpc_getaddress --lib
```

Result on 2026-05-09: passed. The tests confirm:

- `getaddresstxids` accepts 256 unique transparent addresses with omitted
  `start`/`end` and forwards the full set plus `Height(0)..=tip` to state;
- `getaddressbalance` accepts and forwards the full 256-address set to state;
- `getaddressutxos` accepts and forwards the full 256-address set to state.

### RPC getblockchaininfo snapshot and fallback consistency

Status: local-only, not posted.

Detailed note:
`docs/analysis/rpc-getblockchaininfo-snapshot-fallback-note.md`.

Summary: `getblockchaininfo` combines `UsageInfo`, `TipPoolValues`,
`ChainInfo`, and chain-tip watcher values that are not tied to one atomic state
snapshot. It also intentionally returns a successful genesis-like response when
`TipPoolValues` fails, and can use default/minimum difficulty if `ChainInfo`
fails in that path. This is RPC client-safety and monitoring hardening rather
than consensus risk.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "getblockchaininfo" "genesis fallback" "TipPoolValues"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "getblockchaininfo" "snapshot consistency" "ChainInfo" "TipPoolValues"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "getblockchaininfo" "returns genesis" "state fails"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "get_blockchain_info_returns_genesis_when_tip_pool_fails"'
```

No hits were returned.

Local proof rerun:

```sh
cargo test -p zebra-rpc get_blockchain_info_returns_genesis_when_tip_pool_fails --lib
```

Result on 2026-05-09: passed. The test confirms current fallback behavior:
`TipPoolValues` and `ChainInfo` failures still produce a successful genesis-like
`getblockchaininfo` response.

### RPC getrawtransaction residual snapshot/exactness hardening

Status: local-only, not posted.

Detailed notes:

- `docs/analysis/rpc-getrawtransaction-snapshot-consistency-note.md`
- `docs/analysis/rpc-getrawtransaction-v5-blockhash-exactness-note.md`

Summary: PR #10523 fixed the sharper caller-`blockhash` TOCTOU issue by reusing
the caller-provided block hash and initial best-chain flag. Two residual
low-severity RPC hardening concerns remain locally: the no-`blockhash` verbose
path still combines `AnyChainTransaction(txid)` with a later
`BestChainBlockHash(height)` lookup, and the caller-`blockhash` path validates
V5 block membership by mined `txid` before fetching the body through a global
mined-`txid` lookup rather than from the named block.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "getrawtransaction snapshot consistency"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "getrawtransaction" "AnyChainTransaction" "BestChainBlockHash"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "AnyChainTransactionIdsForBlock" "AnyChainTransaction" "blockhash"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "getrawtransaction" "V5" "blockhash" "authdigest"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "getrawtransaction" "blockhash" "V5" "same txid"'
```

No exact issue hits were returned.

Closest related artifacts:

- PR #10523, merged, fixed the adjacent RPC advisory for the caller-`blockhash`
  TOCTOU class. It does not add a block-specific transaction-body lookup keyed
  by `(blockhash, txid)`.
- PR #9884, closed/merged, added side-chain support in `getrawtransaction`. It
  is provenance for the side-chain behavior, not a tracker for the residual
  snapshot-consistency or V5 exactness concerns.

Local confidence check:

```sh
cargo test -p zebra-rpc rpc_getrawtransaction --lib
cargo test -p zebra-rpc getrawtransaction_no_blockhash_can_mix_mined_tx_and_best_chain_blockhash_today --lib
cargo test -p zebra-rpc getrawtransaction_blockhash_can_return_different_v5_auth_variant_today --lib
```

Result on 2026-05-09: all commands passed. The broader
`rpc_getrawtransaction` test confirms the ordinary current paths still work.
The no-`blockhash` snapshot proof makes the mempool miss, returns a mined
transaction from `AnyChainTransaction(txid)`, then returns a different same-
height block hash from `BestChainBlockHash(height)`. Zebra currently returns a
single verbose object with the mined transaction's height, confirmations, and
block time, but the later mismatched block hash and `in_active_chain = true`.
The V5 exactness proof builds two V5 transactions with the same mined ID but
different authorizing data, makes the caller-supplied block membership check
succeed for one variant, and makes the later global `AnyChainTransaction(txid)`
lookup return the other variant.

### Mining RPC generation and template-work amplification

Status: local-only, not posted.

Detailed notes:

- `docs/analysis/rpc-generate-uncapped-disabled-pow-note.md`
- `docs/analysis/gbt-longpoll-full-mempool-poll-amplification-note.md`
- `docs/analysis/gbt-longpoll-max-time-already-reached-note.md`
- `docs/analysis/gbt-dependency-dag-selection-amplification-note.md`

Summary: several mining-RPC paths have bounded but avoidable resource or
availability hardening issues. `generate(num_blocks)` is uncapped on disabled-PoW
networks such as Regtest and custom disabled-PoW Testnets. GBT long-poll waiters
can independently repeat full mempool/template work, including an immediate
retry subcase when state and mempool tips are temporarily mismatched. A matching
long-poll request can also stay pending when template time is already clamped to
`maxtime`. ZIP-317 template selection can do repeated dependency-DAG checks over
attacker-shaped but valid mempool transaction graphs.

Duplicate/overlap checks refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "generate" "disabled_pow" "num_blocks" "cap"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "getblocktemplate" "longpoll" "full mempool" "amplification"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "getblocktemplate" "longpoll" "max_time" "already reached"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "getblocktemplate" "dependency DAG" "selection amplification"'
gh api repos/ZcashFoundation/zebra/issues/9301
gh api repos/ZcashFoundation/zebra/issues/9727
```

No exact issue hits were returned.

Closest broad overlap:

- #9301, open, "DoS vulnerability in `getblocktemplate` RPC"
- #9727, open, "Respond quickly to long-polled `getblocktemplate` RPC on new
  chain tips"

These cover general GBT DoS / long-poll responsiveness themes, but not these
exact disabled-PoW `generate`, repeated full-mempool polling, zero-duration
max-time, or dependency-DAG selection shapes.

Local proof status updated on 2026-05-09:

```sh
cargo test -p zebra-rpc getblocktemplate_matching_longpollid_waits_when_maxtime_already_reached_today --lib
cargo test -p zebra-rpc getblocktemplate_matching_longpollid_repeats_full_mempool_poll_today --lib
cargo test -p zebra-rpc multi_parent_dependency_check_repeatedly_scans_selected_transactions_today --lib
cargo test -p zebra-rpc rpc_generate_u32_max_reaches_template_work_on_disabled_pow_today --lib
cargo test -p zebra-rpc rpc_generate --lib
cargo test -p zebra-rpc getblocktemplate --lib
```

Result on 2026-05-09: passed. This confirms ordinary `getblocktemplate` paths
still work, including the existing proposal-hang and non-ASCII longpollid
current-behavior tests. The new focused proof constructs a matching current
`longpollid` with `cur_time == max_time` and confirms the RPC remains pending
past a short timeout instead of returning `submitold=false`.
The repeated-poll proof constructs a matching current `longpollid`, answers one
state/full-mempool polling cycle, advances the virtual clock by
`MEMPOOL_LONG_POLL_INTERVAL`, and confirms a second `FullTransactions` poll is
performed while the RPC remains pending.
The disabled-PoW `generate` proof constructs a Regtest RPC instance, calls
`generate(u32::MAX)`, and observes the first template `ChainInfo` request,
showing the maximum caller-supplied count reaches generation work instead of a
parameter-boundary cap.

The dependency-DAG proof constructs 12 unrelated selected transactions and a
four-parent dependent shape, then confirms each parent encounter scans the
entire selected transaction vector after the length gate is satisfied. This
proves the local repeated-scan behavior, but it is not a full random-selector
stress benchmark.

### RPC verbose response quadratic assembly

Status: local-only, not posted.

Detailed notes:

- `docs/analysis/rpc-getrawmempool-verbose-quadratic-note.md`
- `docs/analysis/rpc-verbose-orchard-action-quadratic-note.md`

Summary: verbose RPC response construction contains two bounded quadratic work
patterns. `getrawmempool(true)` rebuilds a full transaction-ID lookup map once
per returned mempool transaction, and verbose transaction output searches the
same Orchard authorized-action list once per action to recover signatures. RPC
is disabled/authenticated by default and response sizes are bounded, so this is
local resource-hardening rather than private disclosure.

Local proof status updated on 2026-05-09:

```sh
cargo test -p zebra-rpc verbose_mempool_object_rebuilds_lookup_for_each_transaction_today --lib
cargo test -p zebra-rpc verbose_transaction_searches_authorized_orchard_actions_repeatedly_today --lib
```

Result: both commands passed. The mempool proof constructs eight non-coinbase
mempool transactions, builds one verbose object per transaction, and confirms
eight lookup-map builds over eight inputs each. The Orchard proof deserializes
the first Testnet two-action Orchard transaction, builds
`TransactionObject::from_transaction()`, and confirms three authorized-action
comparisons for two actions. Both verbose quadratic assembly shapes are now
proof-backed.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "getrawmempool" "verbose" "quadratic"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "MempoolObject" "transactions_by_id" "full mempool"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "verbose Orchard action" "quadratic"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "TransactionObject" "Orchard actions" "find signature"'
```

No hits were returned.

Earlier functional coverage for the `getrawmempool` verbose path was also
rerun:

```sh
cargo test -p zebra-rpc mempool_transactions_are_sent_to_caller --lib
```

Result on 2026-05-09: passed.

Related verbose mempool shape proof added on 2026-05-09:

```sh
cargo test -p zebra-rpc mempool_object_counts_only_direct_dependents_today --lib
```

Result: passed. The test confirms a parent -> child -> grandchild mempool
dependency chain reports only the parent plus direct child in the parent's
verbose `descendantcount`, `descendantsize`, and `descendantfees` fields. This
is RPC correctness hardening rather than a private disclosure candidate.

### RPC solution-rate window bounds

Status: local-only, not posted; known duplicate/follow-up of closed issue #6688.

Detailed note: `docs/analysis/rpc-solution-rate-window-bounds-note.md`.

Summary: caller-controlled `num_blocks` can force broad ancestor scans. This is
bounded by chain height and RPC exposure defaults.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "getnetworksolps" "num_blocks" "limit"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "SolutionRate" "num_blocks" "RPC"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "getnetworkhashps" "window bounds"'
gh api repos/ZcashFoundation/zebra/issues/6688
```

Closest hit:

- #6688, closed, directly covered large `num_blocks` hangs in
  `getnetworksolps` / `getnetworkhashps`.
- PR #7647 fixed the higher-cost full-block-read behavior and intentionally did
  not add a height/range limit.
- #7403 tracked a longer-term optimization and is also closed.

Local proof added:

```sh
cargo test -p zebra-rpc rpc_getnetworksolps_forwards_i32_max_window_today --lib
```

Result on 2026-05-09: passed. The test confirms `Some(i32::MAX)` is forwarded
to state as `ReadRequest::SolutionRate { num_blocks: i32::MAX as usize, ... }`.

### RPC subtree limit overflow distinction

Status: local-only, not posted.

Detailed note: `docs/analysis/rpc-subtree-limit-overflow-hardening-note.md`.

Summary: subtree RPCs should distinguish omitted `limit` from explicit range
overflow and return clearer bounded errors.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "z_getsubtreesbyindex" "limit overflow"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "Subtrees" "limit overflow" "ReadRequest"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "NoteCommitmentSubtreeIndex" "checked_add" "limit"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "subtree_overflow_limit_matches_omitted_limit_today"'
```

Closest hit:

- #7436, closed/merged, implemented `z_getsubtreesbyindex`. It is provenance,
  not a duplicate of the explicit-overflow distinction.

No exact issue hits were returned.

Local proof added:

```sh
cargo test -p zebra-state subtree_overflow_limit_matches_omitted_limit_today --lib
```

Result on 2026-05-09: passed. The test confirms explicit overflowing Sapling
and Orchard subtree limits currently return the same suffix as omitted limits.

### Indexer gRPC exposure and stream limits

Status: local-only, not posted.

Detailed note: `docs/analysis/indexer-grpc-exposure-hardening-note.md`.

Summary: the opt-in indexer gRPC server lacks auth/TLS/concurrency/stream limits
if bound to a shared network. This is deployment-dependent endpoint hardening.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "indexer gRPC" "auth" "stream limit"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "indexer_listen_addr" "public" "auth" "TLS"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "NonFinalizedStateChange" "stream subscriber limit"'
gh api repos/ZcashFoundation/zebra/issues/10405
```

Closest hit:

- #10405, closed, mentions the gRPC indexer's auth gap as part of a broader RPC
  access-groups/auth redesign. It was closed as too complex / not needed rather
  than fixed.

No dedicated open issue was found for indexer auth plus stream/subscriber
limits.

Local proof added:

```sh
cargo test -p zebra-rpc indexer_accepts_many_unauthenticated_streams_today --lib
```

Result on 2026-05-09: passed. The test confirms one unauthenticated client can
open 32 indexer streaming subscribers today.

### Indexer stream lifecycle and non-finalized-state subscriber amplification

Status: local-only, not posted.

Detailed notes:

- `docs/analysis/indexer-idle-stream-disconnect-retention-note.md`
- `docs/analysis/indexer-non-finalized-state-stream-amplification-note.md`

Summary: the optional indexer gRPC streaming methods spawn per-subscriber tasks
and only observe client disconnects after the next source event reaches a
`try_send()`. `NonFinalizedStateChange` additionally creates an independent
state-side listener per subscriber, multiplying non-finalized-state clone/diff
and full-block serialization work across subscribers. This composes with the
broader missing indexer auth/stream-limit finding.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "indexer idle stream disconnect retention"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "NonFinalizedStateChange" "subscriber amplification"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "indexer gRPC" "stream limit" "subscriber"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "mempool change" "broadcast lag" "indexer"'
```

No hits were returned.

Local proof rerun on 2026-05-09:

```sh
cargo test -p zebra-rpc indexer_accepts_many_unauthenticated_streams_today --lib
cargo test -p zebra-rpc non_finalized_state_streams_request_one_listener_each_today --lib
cargo test -p zebra-rpc dropped_chain_tip_change_stream_retains_task_until_tip_event_today --lib
cargo test -p zebra-rpc dropped_mempool_change_stream_retains_subscription_today --lib
cargo test -p zebra-rpc dropped_non_finalized_state_stream_retains_state_listener_today --lib
cargo test -p zebra-rpc lagged_mempool_change_stream_ends_as_unavailable_today --lib
```

Result: passed. These prove missing admission limits for many unauthenticated
streams, one independent state-side listener request per
`NonFinalizedStateChange` subscriber, idle-disconnect retention for
`ChainTipChange`, idle-disconnect retention for `MempoolChange`, and
idle-disconnect retention for the `NonFinalizedStateChange` state listener.
They also prove lagged `MempoolChange` subscribers terminate with
`Code::Unavailable` and the same status text used for upstream channel closure.

### TrustedChainSync indexer validation boundary

Status: local-only, not posted.

Detailed notes:

- `docs/analysis/trusted-chain-sync-indexer-validation-boundary-note.md`
- `docs/analysis/trusted-chain-sync-best-tip-forwarder-note.md`

Summary: `TrustedChainSync` is an opt-in trusted read-state/indexer mirror path.
It accepts streamed `BlockAndHash` messages from a caller-supplied indexer
endpoint, decodes the transmitted hash and block body separately, and commits
through lower-level non-finalized state methods rather than the normal
state-service contextual-validity gate. The adjacent best-tip forwarding helper
can also exit permanently on ordinary non-finalized best-tip hashes that are not
in finalized storage.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "TrustedChainSync" "BlockAndHash" "hash mismatch"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "TrustedChainSync" "initial_contextual_validity"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "trusted indexer" "validation boundary"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "TrustedChainSync" "best tip forwarding"'
```

No hits were returned.

Local proof added and rerun:

```sh
cargo test -p zebra-rpc block_and_hash_decode_accepts_mismatched_hash_today --lib
cargo test -p zebra-state lower_level_commit_skips_recent_chain_height_check_today --lib
cargo test -p zebra-rpc trusted_chain_sync_stream_propagates_transmitted_hash_today --lib
cargo test -p zebra-rpc trusted_chain_sync_stream_accepts_nonsequential_child_of_genesis_today --lib
cargo test -p zebra-rpc trusted_chain_sync_tip_forwarder_exits_on_unfinalized_tip_today --lib
```

Result on 2026-05-09: passed. The decoder test confirms
`BlockAndHash::decode()` accepts a valid serialized block body paired with a
different transmitted hash. The state-layer contrast test confirms direct
lower-level non-finalized commits skip the normal recent-chain height check.
The end-to-end syncer tests now prove the live gRPC `TrustedChainSync` stream
path publishes a non-finalized mirror tip using the transmitted hash and accepts
the non-sequential height-2 child-of-genesis fixture. The best-tip-forwarder
runtime test confirms the adjacent forwarding helper exits on a non-finalized
best-tip hash not present in finalized storage.

### Indexer MempoolChange privacy

Status: local-only, not posted.

Detailed note: `docs/analysis/indexer-mempool-change-privacy-note.md`.

Summary: `MempoolChange` exposes local mempool timing plus V5 authorization
digests to subscribers. This should be documented as privileged/local-only data
unless auth/TLS/capability controls are added.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "MempoolChange" "auth_digest" "privacy"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "indexer" "mempool timing" "privacy"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "UnminedTxId" "auth digest" "indexer"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "mempool change" "gRPC" "privacy"'
```

Closest hit:

- #10405, closed, is related broad prior art for RPC access groups, indexer
  gRPC auth, and mempool timing feeds. It does not specifically document V5
  authorization digest exposure through `MempoolChange`.

Local proof added:

```sh
cargo test -p zebra-rpc mempool_change_stream_exposes_v5_auth_digest_today --lib
```

Result on 2026-05-09: passed. The test confirms `MempoolChange` streams V5
authorization digest bytes today.

### Health endpoint connection retention

Status: local-only, not posted.

Detailed note: `docs/analysis/health-endpoint-connection-hardening-note.md`.

Summary: the optional unauthenticated health endpoint should have open
connection caps and request/header timeouts if exposed beyond internal probes.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "health endpoint" "idle connection timeout"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "health server" "connection limit"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "ready endpoint" "warn log amplification"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "health HTTP" "keep-alive" "rate limit"'
```

No hits were returned.

Local proof added:

```sh
cargo test -p zebrad idle_health_connection_waits_without_request_timeout_today --lib
```

Result on 2026-05-09: passed. The test confirms an idle health connection can
remain open without sending a request and later send `/healthy` on the same
socket.

### Metrics endpoint connection retention

Status: local-only, not posted.

Detailed note: `docs/analysis/metrics-endpoint-connection-hardening-note.md`.

Summary: the optional Prometheus metrics endpoint delegates to
`metrics-exporter-prometheus`'s HTTP listener without a Zebra-side allowlist,
connection cap, or request/header timeout. The endpoint is disabled by default
and normally documented for localhost, so this is deployment-dependent
observability hardening.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "metrics endpoint" "connection timeout"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "Prometheus endpoint" "connection limit"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "metrics exporter" "slow client"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "with_http_listener" "add_allowed_address"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra Prometheus allowlist metrics endpoint'
```

Closest hits:

- Exact phrase searches returned no hits.
- The broader `Prometheus allowlist metrics endpoint` search returned no hits.
- A broad unquoted `metrics endpoint connection timeout` search returned only
  unrelated release, sync, lightwalletd, and connectivity issues; none described
  the exporter listener connection-retention path.

Local proof status: partial current-behavior coverage plus dependency-source
evidence. `zebrad/src/components/metrics.rs` calls
`PrometheusBuilder::new().with_http_listener(addr).install()`, while
`metrics-exporter-prometheus` 0.16.2 defaults to allow-all addresses and spawns
one Hyper connection task per accepted TCP stream. The dependency's
`idle_timeout()` builder option is metric-recency cleanup, not a request/header
or connection timeout around `serve_connection()`.

Local proof added on 2026-05-09:

```sh
cargo test -p zebrad prometheus_metrics_connection_waits_without_request_timeout_today --lib
```

Result: passed. The test uses `PrometheusBuilder::build()` rather than
`install()` to avoid mutating the process-global metrics recorder. It opens an
idle TCP connection to the exporter, sends no request bytes for 250 ms, then
sends a normal scrape over the same connection and receives a successful
metrics response. This is a bounded current-behavior proof, not a mathematical
proof that no larger timeout exists.

### Tracing filter endpoint body limit and auth

Status: local-only, not posted.

Detailed notes:

- `docs/analysis/tracing-filter-endpoint-security-note.md`
- `docs/analysis/tracing-filter-reload-telemetry-amplification-note.md`

Summary: the optional tracing filter-reload endpoint is an unauthenticated
runtime control endpoint and should have a body limit and authentication or
localhost-only deployment guard if exposed.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "tracing filter endpoint" "body limit"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "filter-reload endpoint" unauthenticated'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "POST /filter" "body limit"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "tracing endpoint" "telemetry amplification"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra tracing endpoint filter reload'
```

Exact phrase searches for body-limit/auth and telemetry-amplification returned
no hits. The broader filter-reload search returned old adjacent history: #1001
covered a broken tracing endpoint, #995 requested listener acceptance-test
coverage, and #4539 made diagnostics optional by default. None cover endpoint
authentication, request-body bounds, public binding hardening, or telemetry
export amplification.

Local proof rerun:

```sh
cargo test -p zebrad tracing_filter_endpoint_read_filter_accepts_large_body_today --features filter-reload --lib
```

Result on 2026-05-09: passed. The test confirms `read_filter()` collects and
accepts a multi-megabyte UTF-8 `POST /filter` body without a local size cap.

### RPC tracing and metrics method-cardinality hardening

Status: local-only, not posted.

Detailed note: `docs/analysis/rpc-tracing-method-attribute-amplification-note.md`.

Summary: RPC tracing and metrics middleware copy raw JSON-RPC method names into
OpenTelemetry span attributes and Prometheus metric labels before unknown
methods are normalized. Request body caps bound individual names, but a reachable
RPC client can still generate many distinct unknown method names in deployments
that expose RPC plus telemetry or metrics.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "RPC tracing" "method attribute"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "rpc.method" OpenTelemetry cardinality'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "RPC method" "metrics cardinality"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "unknown RPC method" "Prometheus label"'
```

Closest hits:

- #10174 is provenance for adding OpenTelemetry RPC spans and `rpc.method`, but
  not a hardening issue.
- #10551 already has a public comment that names the RPC `method` label as a
  sibling high-cardinality metric-label surface. That partially covers the
  Prometheus-label side, but not the tracing attribute / telemetry-export side
  of this local note.

Local proof status: metric-label side and tracing-attribute side are now both
test-backed. `rpc_metrics_method_label_uses_raw_unknown_method_today` installs a
local metrics recorder, sends two different unknown JSON-RPC method names through
`RpcMetricsMiddleware`, and confirms both are used directly as Prometheus
`method` label values on request and duration metrics.
`rpc_tracing_method_attribute_uses_raw_unknown_method_today` installs a local
tracing subscriber layer, sends two different unknown JSON-RPC method names
through `RpcTracingMiddleware`, and confirms both are used directly as
`rpc.method` span attributes. The sibling active-request gauge drift is also
test-backed locally by
`rpc_active_requests_gauge_not_decremented_when_response_future_dropped_today`,
which confirms dropping a pending RPC response future after `call()` increments
`rpc.active_requests` does not run the matching decrement.

Local proof rerun:

```sh
cargo test -p zebra-rpc rpc_metrics_method_label_uses_raw_unknown_method_today --lib
cargo test -p zebra-rpc rpc_tracing_method_attribute_uses_raw_unknown_method_today --lib
cargo test -p zebra-rpc rpc_active_requests_gauge_not_decremented_when_response_future_dropped_today --lib
```

Result on 2026-05-09: passed.

### Sentry and OpenTelemetry privacy/documentation hardening

Status: local-only, not posted.

Detailed note: `docs/analysis/sentry-opentelemetry-privacy-note.md`.

Summary: official release builds compile Sentry and OpenTelemetry support, but
both exporters require explicit runtime configuration before telemetry leaves
the node. The remaining issue is opt-in privacy/documentation hardening: once an
operator sets `SENTRY_DSN` or an OpenTelemetry endpoint, Zebra can export panic
reports, logs/breadcrumbs, span names, span fields, and runtime identifiers that
operators may not expect from the current docs.

2026-05-09 sampler follow-up: Zebra reads `OTEL_TRACES_SAMPLER_ARG` as a
percentage `u8`, then defaults to 100% sampling on parse failure. That matches
the Zebra Docker observability examples such as `OTEL_TRACES_SAMPLER_ARG=10`,
but a conventional OpenTelemetry deployment using ratio syntax like
`OTEL_TRACES_SAMPLER=traceidratio` plus `OTEL_TRACES_SAMPLER_ARG=0.1` will be
silently interpreted as no configured sample percentage. With an OTLP endpoint
configured, Zebra then exports at 100% instead of the likely intended 10%.
This remains local-only operational hardening, not private disclosure, because
telemetry export is opt-in and the input is local deployment configuration.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "Sentry" "OpenTelemetry" privacy'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra SENTRY_DSN telemetry privacy'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra OTEL_EXPORTER_OTLP_ENDPOINT privacy'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "OpenTelemetry sample" "Sentry logs"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra sentry logs breadcrumbs privacy'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra OpenTelemetry export sample percent'
```

Closest overlaps:

- #10490 upgraded Sentry, enabled Sentry Logs, kept OpenTelemetry in default
  releases, and updated observability docs. It is implementation/provenance, not
  a privacy-hardening duplicate.
- #10174 added OpenTelemetry tracing and documents 100% sampling in the feature
  PR context. It is provenance, not a privacy/redaction follow-up.
- The other privacy/redaction-focused searches returned no hits.

Local proof status: proof-backed for the OpenTelemetry sampler env
parsing/defaulting subcase; source-evidence-only for the broader exported
event/log/breadcrumb/span-field documentation scope. Activation gates are
visible in `zebrad/src/application.rs`,
`zebrad/src/components/tracing/component.rs`, and
`zebrad/src/components/tracing.rs`; export behavior is visible in
`zebrad/src/sentry.rs` and `zebrad/src/components/tracing/otel.rs`. The focused
tests confirm `OTEL_TRACES_SAMPLER_ARG=0.1` with
`OTEL_TRACES_SAMPLER=traceidratio` resolves to no integer sample percentage,
then defaults to 100% sampling once an OTLP endpoint is configured. The current
user tracing docs mention the activation gates, but not the full exported
event/log/breadcrumb/span-field scope.

Local proof commands:

```sh
cargo test -p zebrad components::tracing::component::tests --lib
cargo test -p zebrad components::tracing::otel::tests --lib
```

Result on 2026-05-09: passed.

### Docker observability broad-bind examples

Status: local-only, not posted.

Detailed note: `docs/analysis/docker-default-observability-public-bind-note.md`.

Summary: shipped Docker/default observability examples include broad
`0.0.0.0` binds for metrics and health, and the observability compose stack
publishes metrics plus auth-disabled RPC. This is not a new code bug by itself,
but it makes the local metrics endpoint, health endpoint, RPC Docker
unauthenticated bind, and telemetry cardinality hardening findings easier to
copy into exposed deployments.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "Docker observability" "0.0.0.0" metrics health'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "ZEBRA_METRICS__ENDPOINT_ADDR" "0.0.0.0"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "docker compose" metrics RPC "cookie auth" false'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "health" "listen_addr" "0.0.0.0" Docker'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra observability compose 9999 8232'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra docker health metrics public bind'
```

Closest overlaps:

- #9768 introduced the layered config/environment-variable model, but is not a
  public-bind warning.
- #9895 added the opt-in unauthenticated health endpoint and docs, but is not a
  deployment-hardening duplicate.
- The `observability compose 9999 8232` search matched unrelated issues whose
  numbers look like ports (#8232 and #9999).
- No direct duplicate was found.

Local proof status: source-evidence-only. Evidence is in
`docker/default-zebra-config.toml`, `docker/docker-compose.observability.yml`,
`zebrad/src/commands/generate.rs`, and the user docs. The runtime pieces are
covered by the separate endpoint-specific local notes.

### RPC Docker unauthenticated public-bind examples

Status: local-only, not posted.

Detailed note: `docs/analysis/rpc-docker-unauthenticated-public-bind-note.md`.

Summary: shipped Docker and user-doc examples bind JSON-RPC to `0.0.0.0`,
disable cookie auth, and in some compose files publish the RPC port on the host.
This is a configuration footgun rather than a consensus bug, but it removes the
main deployment mitigations assumed by the RPC hardening findings.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "ZEBRA_RPC__ENABLE_COOKIE_AUTH=false" "8232:8232"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "Docker RPC" "cookie auth" "0.0.0.0"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "docker-compose.lwd.yml" "ENABLE_COOKIE_AUTH"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "RPC listen_addr" "0.0.0.0" "Docker" "cookie"'
```

Closest overlaps:

- #10464, merged P2P reachability docs/compose PR that touches the same Docker
  files but does not report or fix unauthenticated host-published RPC.
- #9904, unrelated regtest `getblockchaininfo` progress behavior.
- #9768 and #9344, historical Docker/configuration PRs rather than duplicate
  reports.

The exact `ZEBRA_RPC__ENABLE_COOKIE_AUTH=false` plus `8232:8232` search returned
no hits.

Local proof status: source-evidence-only. Evidence is in
`docker/docker-compose.lwd.yml`, `docker/docker-compose.observability.yml`,
`docker/mining/docker-compose.yml`, and the user docs. Runtime impact composes
with the separate RPC batch, text/plain, pre-guard connection, method
cardinality, and expensive-method notes.

### Mempool metrics full-scan amplification

Status: local-only, not posted.

Detailed note: `docs/analysis/mempool-metrics-full-scan-amplification-note.md`.

Summary: `VerifiedSet::update_metrics()` recomputes several aggregate mempool
metrics by scanning every stored verified transaction after hot-path insert and
remove operations. Accepted transaction growth is bounded by mempool cost limits
and verification cost, but the metric accounting adds avoidable
`1 + 2 + ... + n` work while a mempool fills.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "mempool metrics" "full scan"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "zcash.mempool.actions.unpaid" update_metrics'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "verified mempool" update_metrics scan'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "mempool metrics" amplification'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra mempool update_metrics'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "zcash.mempool.size.weighted"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "VerifiedSet::update_metrics"'
```

Closest overlaps:

- #2860 is historical metrics provenance and explicitly asks whether tracking
  serialized size incrementally is worthwhile to avoid iterating the whole
  mempool.
- #6972 added the current bucketed mempool action/weighted-size metrics.
- Neither is a current hardening duplicate.

Local proof status: proof-backed for the successful-insert growth and
independent-removal shrink shapes.

```sh
cargo test -p zebrad mempool_insert_recomputes_metrics_over_growing_set_today --lib
cargo test -p zebrad mempool_remove_recomputes_metrics_over_shrinking_set_today --lib
```

Result on 2026-05-09: both commands passed. The insert proof inserts five
accepted transactions into eviction-free storage, confirms five
`update_metrics()` calls, and confirms `1 + 2 + 3 + 4 + 5` transaction visits
across the growing verified set. The removal proof removes those five
independent transactions through `Storage::remove_exact()`, confirms five more
`update_metrics()` calls, and confirms `4 + 3 + 2 + 1 + 0` visits across the
shrinking verified set.

### Queued block height-index desync

Status: local-only, not posted.

Detailed note: `docs/analysis/queued-block-height-index-desync-note.md`.

Summary: `QueuedBlocks::dequeue_children()` removes the entire `by_height`
bucket for each dequeued child height. If another queued same-height block is
waiting on a different missing parent, it remains in the primary and parent
indexes but becomes invisible to finalized-height pruning.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra QueuedBlocks dequeue_children by_height prune_by_height'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "queued block" "height index" "prune"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "dequeue_children" "by_height"'
```

Direct searches returned no issue hits. The broader quoted `queued block` /
`height index` / `prune` search returned merged PR #902, a 2020 state-updates
RFC rather than a duplicate bug report or implementation fix.

Local proof rerun:

```sh
cargo test -p zebra-state dequeue_drops_height_index_for_other_parents_today --lib
```

Result on 2026-05-09: passed. The test queues two same-height children under
different missing parents, dequeues one parent, and confirms the surviving child
is retained while the height index no longer lets `prune_by_height()` remove it.

Release reachability check: local `v4.4.1` and current `main` both retain the
same whole-height-bucket removal in `QueuedBlocks::dequeue_children()` and the
same `by_height`-driven expiry in `prune_by_height()`.

### Pre-Heartwood Sapling root checkpoint coverage on custom Regtest

Status: local-only, not posted.

Detailed note:
`docs/analysis/pre-heartwood-sapling-root-checkpoint-coverage-note.md`.

Summary: Zebra intentionally treats pre-Heartwood `FinalSaplingRoot` header
commitments as structurally valid and relies on checkpoint coverage for the
pre-Canopy production range. Default Mainnet/Testnet and configured Testnet are
covered, but custom Regtest can create weaker checkpoint coverage and route some
proposal/helper paths toward structural-only validation.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "pre-Heartwood" "Sapling root" "Regtest"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "FinalSaplingRoot" "checkpoint"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "hashLightClientRoot" checkpoint Regtest'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "mandatory checkpoint" "Regtest" "Canopy"'
```

Closest overlaps:

- #2092 is historical provenance: it proposed implementing
  `FinalSaplingRoot`, but explicitly notes the rule was not needed for
  production validation because Zebra checkpoints on Canopy.
- #8428, #8475, #8485, and #8629 cover Regtest/custom-network and mandatory
  checkpoint design history.
- No direct custom-Regtest `FinalSaplingRoot` checkpoint-coverage hardening
  issue was found.

Local evidence rerun:

```sh
cargo test -p zebra-state all_upgrades_and_wrong_commitments_with_fake_activation_heights --lib
cargo test -p zebra-chain checkpoint_list_hard_coded_mandatory --lib
```

Result on 2026-05-09: both commands passed. These are coverage/backstop tests,
not a full retained reproducer; the detailed note records that the concrete
custom-Regtest structural-only proof was temporary and removed.

### Finalized-boundary downloader height filter

Status: local-only, not posted.

Detailed note:
`docs/analysis/finalized-boundary-downloader-height-filter-note.md`.

Summary: sync and inbound downloader early height filters reject downloaded
blocks strictly below `tip - MAX_BLOCK_REORG_HEIGHT`, but admit the exact
boundary height. In steady state, that exact boundary corresponds to the
finalized tip, so stale alternate blocks can miss the cheap early
`BehindTipHeightLimit` classification and be rejected later through noisier
verification/state paths.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "finalized boundary" downloader height filter MAX_BLOCK_REORG_HEIGHT'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "BehindTipHeightLimit" "MAX_BLOCK_REORG_HEIGHT"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "tip - MAX_BLOCK_REORG_HEIGHT"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "behind the finalized tip" "downloaded block"'
```

The first three searches returned no issue hits. The broad finalized-tip
download search returned #3167, the historical security PR that added ahead-tip
and behind-finalized-tip dropping. That is filter provenance, not a duplicate of
the exact-boundary hardening note.

Local proof status: both downloader paths are now test-backed. The
`sync_downloader_verifies_exact_min_accepted_height_today` test sets the mock
best tip to `block_height + MAX_BLOCK_REORG_HEIGHT`, downloads a synced block at
exactly `tip - MAX_BLOCK_REORG_HEIGHT`, and confirms the sync downloader sends
that block to the verifier rather than returning `BehindTipHeightLimit`. The
`inbound_downloader_verifies_exact_min_accepted_height_today` test sets the
latest chain tip to the same boundary and confirms a gossiped exact-boundary
block reaches inbound verification too.

Local proof rerun:

```sh
cargo test -p zebrad sync_downloader_verifies_exact_min_accepted_height_today --lib
cargo test -p zebrad inbound_downloader_verifies_exact_min_accepted_height_today --lib
```

Result on 2026-05-09: both commands passed.

### GBT Testnet sync-gate bypass

Status: local-only, not posted.

Detailed note: `docs/analysis/testnet-gbt-sync-gate-bypass-note.md`.

Summary: `check_synced_to_tip()` documents a PoW-disabled exception, but returns
early for every test network. Default Testnet has PoW enabled, so mining RPC
template/proposal calls can bypass the near-tip guard that Mainnet uses.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra check_synced_to_tip getblocktemplate testnet disable_pow'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "check_synced_to_tip" "is_a_test_network"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "getblocktemplate" "disable_pow" "Testnet"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "GBT" "sync" "Testnet"'
```

The first three searches returned no hits. The broad `GBT sync Testnet` search
returned #6025, a manually triggered Testnet mining workflow issue that waits
for sync before mining; it is adjacent mining workflow history, not a duplicate
of the `check_synced_to_tip()` predicate mismatch.

Local proof added and run:

```sh
cargo test -p zebra-rpc default_testnet_gbt_sync_check_ignores_unsynced_status_today --lib
```

Result on 2026-05-09: passed. The test confirms an unsynced mock Mainnet call is
rejected while the same unsynced mock state is accepted for default Testnet.

### GBT custom pre-Canopy panic

Status: local-only, not posted.

Detailed note: `docs/analysis/gbt-custom-pre-canopy-panic-note.md`.

Summary: on a custom Testnet or Regtest-like configuration where the next block
height is before Canopy, lower-level coinbase generation returns an unsupported
pre-Canopy error, but `BlockTemplateResponse::new_internal()` unwraps it. With
Zebra's aborting panic profile, a mining RPC call can terminate the process on
that custom-network shape.

Duplicate/overlap check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra getblocktemplate pre-Canopy coinbase panic'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "pre-Canopy" "block templates" "panic"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "Zebra does not support generating pre-Canopy"'
gh api repos/ZcashFoundation/zebra/issues/8434
```

The searches did not find a panic/abort issue. Open issue #8434 already tracks
support for constructing pre-Canopy block templates on Regtest/custom Testnets.
That is the natural upstream home for this hardening detail, but #8434 does not
currently call out that the existing unsupported path can abort the process via
`BlockTemplateResponse::new_internal()` unwrapping
`generate_coinbase_and_roots()`.

Local proof added and run:

```sh
cargo test -p zebra-rpc pre_canopy_custom_network --lib
```

Result on 2026-05-09: passed, 2 tests. The tests confirm
`generate_coinbase_and_roots()` returns the unsupported pre-Canopy error for a
custom Testnet with Canopy at height 10 and a height-1 candidate block, and that
`BlockTemplateResponse::new_internal()` currently panics on that error via the
`coinbase should be valid under the given parameters` unwrap.

### GBT time-envelope mismatch

Status: local-only, not posted.

Detailed note: `docs/analysis/gbt-time-envelope-mismatch-note.md`.

Summary: `getblocktemplate` exposes `mintime` / `maxtime` values that can
disagree with later proposal or `submitblock` validation. The visible cases are
not intersecting `maxtime` with the node-local future-time rule, applying the
MTP+90-minute bound unconditionally, and using the previous block height for the
Testnet minimum-difficulty time split at activation boundaries.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra getblocktemplate maxtime mintime median time past 90 minutes'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra getblocktemplate testnet Blossom minimum difficulty time spacing'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "getblocktemplate" "maxtime" "mintime"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "minimum difficulty" "mintime"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "median time past" "90 minutes" "getblocktemplate"'
```

Closest hits:

- closed #5871 and #5925 covered older Testnet min/max time fixes;
- closed #5659 populated some GBT block-header fields from state.

Those remain adjacent rather than duplicate coverage. They do not cleanly cover
the current node-local future-time intersection, the height-gated
MTP+90-minute rule mismatch, or the candidate-height spacing boundary.

Local proof status: all three concrete subcases tested. On 2026-05-09, these
commands passed:

```sh
cargo test -p zebra-state gbt_maxtime --lib
cargo test -p zebra-state testnet_gbt_time_adjustment_uses_previous_upgrade_boundary_today --lib
cargo test -p zebra-state service::read::difficulty::tests --lib
```

The tests prove:

- a Mainnet template can advertise `maxtime` later than the same node's
  `now + 2 hours` semantic future-time limit;
- a custom Testnet below `TESTNET_MAX_TIME_START_HEIGHT` still advertises
  `maxtime = median_time_past + 90 minutes` even though contextual validation
  would not enforce that upper bound;
- a custom Testnet with a Blossom candidate at height 299,189 keeps the
  previous-height 900-second standard-difficulty `maxtime`, even though the
  candidate-height Blossom minimum-difficulty rule starts at 451 seconds.

### NU7 custom activation V5 serialization panic

Status: local-only, not posted.

Detailed note:
`docs/analysis/nu7-custom-activation-v5-serialization-panic-note.md`.

Summary: in a normal non-test build without `tx_v6`, custom Regtest/Testnet NU7
activation can make the GBT coinbase path construct a V5 transaction tagged
with `NetworkUpgrade::Nu7`. Serializing that transaction panics because the NU7
branch ID is compiled only for tests or the `zebra-test` feature.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra NU7 custom activation V5 serialization branch_id panic'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "NU7" "branch_id" panic'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "valid transactions must have a network upgrade with a branch id"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "new_v5_coinbase" "NU7"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "ConfiguredActivationHeights" "NU7" "getblocktemplate"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "ConsensusBranchId" "Nu7" "zebra-test"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "NU7" "tx_v6" "coinbase"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "NU7" "V5" "panic"'
```

Relevant hits:

- open #10210 explicitly covers the confusing `zcash_unstable=nu7` /
  `feature=tx_v6` conditions and calls out the GBT state where `Nu7` can use a
  V5 coinbase transaction;
- closed #2075 is historical V5 consensus-branch-ID serialization provenance;
- open #10534 is a separate future V6 hash panic when `tx_v6` is enabled.

Local triage: keep this local as concrete panic evidence for #10210 rather than
filing a separate public issue without explicit direction.

Local proof status: temporary normal-build probe recorded in the detailed note;
not retained as a test because `cfg(test)` includes the placeholder NU7 branch
ID and masks this exact normal-build panic.

### GBT ZIP-317 selection quadratic work

Status: local-only, not posted.

Detailed note: `docs/analysis/gbt-zip317-selection-quadratic-note.md`.

Summary: when mining RPC is enabled, ZIP-317 transaction selection rebuilds a
fresh `WeightedIndex` over the remaining candidate vector after each selected
transaction. With many independent candidate transactions, this creates
avoidable quadratic CPU work during `getblocktemplate` construction.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra getblocktemplate ZIP-317 quadratic WeightedIndex'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "ZIP-317" "WeightedIndex"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "select_mempool_transactions" "weighted"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "getblocktemplate" "quadratic" mempool'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "ZIP-317" "getblocktemplate"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "fee_weight_ratio"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "block template" "ZIP-317"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "transaction selection" "getblocktemplate"'
```

Relevant adjacent hits were closed #5473/#5724 for ZIP-317 selection, closed
#6006 for duplicate selection, and closed #8857 for dependent transaction
support. None cover the repeated `WeightedIndex` rebuild / quadratic CPU work
angle.

Local proof status: proof-backed as of 2026-05-09. A focused test-only counter
around `setup_fee_weighted_index()` confirms `select_mempool_transactions()`
rebuilds the weighted index over candidate counts `n, n - 1, ..., 1`.

Focused proof run on 2026-05-09:

```sh
cargo test -p zebra-rpc independent_transaction_selection_rebuilds_weighted_index_after_each_candidate_today --lib
```

Result: passed. A durable performance benchmark over a large synthetic mempool
would still be useful, but the current repeated-rebuild behavior is now captured
by a small regression-style proof.

### GBT dependency metadata mismatch

Status: local-only, not posted.

Detailed note: `docs/analysis/gbt-mempool-dependency-template-note.md`.

Summary: Zebra's mempool and ZIP-317 selector can include dependent
transactions after their selected parents, but serialized GBT transaction
templates still set `depends` to an empty list and carry a stale comment saying
Zebra's mempool does not support dependencies.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra getblocktemplate transaction depends mempool dependencies'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "depends" "getblocktemplate" "mempool"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "TransactionTemplate" "depends"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "mempool dependencies" "block template"'
```

Closest adjacent hits:

- closed #9645 tracks block-proposal validation of mempool transactions and
  dependency-aware template construction internals.
- closed #8857 adds mempool verification for unmined inputs and updates GBT
  selection to include dependencies when their parents are selected.
- closed #5496/#5554 added the `TransactionTemplate` fields and conversion
  plumbing, including the current empty `depends` behavior.
- closed #6195 fixed duplicate transparent spends in GBT responses, and its
  historical logs show `depends: []` in templates, but it does not track
  dependency metadata correctness.

Those issues do not cleanly cover the serialized GBT `depends` metadata
mismatch.

Local proof reruns:

```sh
cargo test -p zebra-rpc includes_tx_with_selected_dependencies --lib
cargo test -p zebra-rpc selected_dependent_transaction_has_empty_template_depends_today --lib
```

Results on 2026-05-09: both passed. The first verifies selected dependent
transactions are possible. The second constructs a `BlockTemplateResponse` from
a selected child transaction with dependency depth 1 and confirms its serialized
template still has `depends: []`.

### Finalization invalidated-record retention

Status: local-only, not posted.

Detailed note:
`docs/analysis/finalization-invalidated-record-retention-note.md`.

Summary: `NonFinalizedState::finalize()` keeps invalidated-block records at the
height that has just been finalized, even though comments and cleanup intent say
records at or below the finalized height should be removed. Follow-up testing
found a sharper same-root side-chain finalization panic: after invalidating a
side-chain tip, finalizing the shared root can leave an empty side chain in
`chain_set`, and the next finalization panics with `only called while blocks is
populated`. Current evidence points to trusted-RPC/control-state availability,
not consensus-invalid acceptance.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra invalidateblock finalization invalidated records retain finalized height'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "invalidated_blocks" "finalized"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "reconsiderblock" "ParentChainNotFound"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "invalidateblock" "reconsiderblock" "finalized"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra finalization invalidating side chain tip empty chain panic'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "only called while blocks is populated"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "invalidateblock" "finalize" "panic"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "side chain" "finalize" "invalidated"'
```

Relevant adjacent hits are closed #9167 for `invalidate_block()`, closed #9260
for `reconsider_block()`, closed #9551 for the trusted RPC methods, and closed
#9921 for `InvalidateBlock` error propagation. Closed #6498/#6552 cover an older
test-only empty-chain panic with the same panic string, and closed #9884 touched
side-chain RPC support. None cover the finalized-height retention predicate or
the same-root side-chain finalization panic reproduced here.

Related local test run on 2026-05-09:

```sh
cargo test -p zebra-state reconsider_block_twice_replays_stale_invalidated_entry_today --lib
cargo test -p zebra-state finalize_after_invalidating_same_root_side_chain_tip_panics_today --lib
cargo test -p zebra-state state_service_invalidate_side_chain_then_finalization_aborts_today --lib
cargo test -p zebra-state finalize_retains_invalidated_record_at_finalized_height_today --lib
```

Result: all four commands passed. The first covers stale invalidated entries
being visible to repeated `reconsider_block()` calls. The second invalidates a
same-root side-chain tip, finalizes the shared root, and then reproduces the
empty-chain finalization panic. The third proves the trusted state-service route
in a subprocess by driving `Request::InvalidateBlock` and observing `SIGABRT`
after automatic finalization reaches the panic. The fourth isolates the
finalized-height retention predicate by inserting a test invalidated record at
the exact height of the next finalized root, finalizing a normal two-block
chain, and confirming the record is still retained.

Local proof status: proof-backed for the stale-record retention predicate, the
direct empty-chain panic, and the trusted state-service route into the
process-fatal panic.

Release reachability check: `v4.4.1` and current `main`
(`589d64b9b7ea6ab4c32ecab41ba6b74f26907940`) both have the same finalization
side-chain reinsertion logic and inclusive invalidated-record retention
predicate in `zebra-state/src/service/non_finalized_state.rs`.

### RPC boundary arithmetic hardening

Status: local-only, not posted.

Detailed note: `docs/analysis/rpc-boundary-arithmetic-hardening-note.md`.

Summary: several RPC response paths use boundary-sensitive arithmetic for next
heights, `verificationprogress`, and displayed difficulty. The current
mainnet/testnet storage and network bounds make the sharper cases non-ordinary,
but custom/synthetic/future states can hit panics or non-finite response values.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra RPC boundary arithmetic verificationprogress Height::MAX difficulty NaN'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "verificationprogress" "NaN"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "Height::MAX" "getblockchaininfo"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "getdifficulty" "NaN"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "getdifficulty" "Infinity"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "verificationprogress" "debug_force_finished_sync"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "valid chain tips are a lot less than Height::MAX"'
```

Relevant adjacent hits are closed #3143/#3891 for `getblockchaininfo`, closed
#6081/#6099/#6105 for `getdifficulty`, and closed #6330 for height-difference
refactoring. None cover the boundary behaviors tracked here.

Local proof status: source-evidence plus ordinary-value and boundary tests. The
detailed note maps each arithmetic site and the storage/network bounds that keep
the strongest failures out of ordinary current deployments.

Sanity test run on 2026-05-09:

```sh
cargo test -p zebra-rpc rpc_getdifficulty --lib
```

Result: passed, 1 test. This covers ordinary `getdifficulty` behavior, not the
custom/boundary non-finite cases.

Boundary test added and run:

```sh
cargo test -p zebra-rpc rpc_getdifficulty_custom_tiny_target_returns_nan_today --lib
```

Result on 2026-05-09: passed, 1 test. The test builds a custom Testnet with a
representable target difficulty limit of `1`, returns that same compact
difficulty from mocked `ChainInfo`, and confirms `chain_tip_difficulty()`
currently returns `NaN` after the high-128-bit display calculation shifts both
operands to zero.

### Sapling validating-key panic eliminated lead

Status: local-only eliminated lead, not posted.

Detailed note: `docs/analysis/sapling-validating-key-panic-sweep-note.md`.

Summary: `ValidatingKey::try_from(redjubjub::VerificationKey)` contains a real
`unwrap()`, but malformed remote P2P transaction bytes first pass through the
upstream `redjubjub::VerificationKey` decoder. Current evidence shows accepted
upstream keys imply the follow-up affine decode succeeds.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra Sapling ValidatingKey malformed rk panic redjubjub'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "Sapling" "ValidatingKey" "panic"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "redjubjub" "ValidatingKey" "unwrap"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "rk" "redjubjub" "panic"'
```

Relevant adjacent hits are closed #3154 for Zcash-specific key checks and
closed #10321 for a broader lazy-deserialization refactor. Neither is a
duplicate live panic finding.

Verification rerun on 2026-05-09:

```sh
cargo test -p zebra-chain validating_key_rejects_malformed_and_small_order_bytes_without_panicking --lib
cargo test -p zebra-chain redjubjub_validating_key_success_implies_affine_decode_success --lib
```

Result: both targeted tests passed.

### Checkpoint auth-data binding eliminated lead

Status: local-only eliminated lead, not posted.

Detailed note: `docs/analysis/checkpoint-auth-data-binding-note.md`.

Summary: the checkpoint verifier defers NU5+ authorizing-data binding, but
finalized-state commit recomputes `hashBlockCommitments` from the previous
history-tree root and `block.auth_data_root()` before writing the checkpoint
block to disk. Current evidence eliminates bad-state persistence through the
normal checkpoint path.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra checkpoint verifier auth_data_root authorizing data commitment'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "checkpoint" "auth data" "merkle root"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "authorizing data hash" "checkpoint"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "hashBlockCommitments" "checkpoint"'
```

Relevant historical hits are closed #2633 for the finalized-state checkpoint
auth-data commitment design, closed #2697 for replacing same-hash queued
checkpoint blocks with newer full block data, and closed #2336 for earlier
auth-data-root validation hardening. These confirm this is an already-designed
deferred-validation boundary, not a fresh consensus acceptance finding.

Targeted verification rerun on 2026-05-09:

```sh
cargo test -p zebra-state all_upgrades_and_wrong_commitments_with_fake_activation_heights --lib
```

Result: passed, 1 test. This corrupts commitments around Heartwood and NU5
custom activation heights and expects finalized-state checkpoint-style commits
to reject the corrupted blocks.

### Time-consensus parity eliminated lead

Status: local-only eliminated lead, not posted.

Detailed note: `docs/analysis/time-consensus-parity-note.md`.

Summary: reviewed locktime, expiry, median-time-past, node-local future time,
and mempool MTP read paths line up in the important places. No time-related
consensus or mempool-policy vulnerability was confirmed.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra time consensus locktime median time past future time mempool'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "BestChainNextMedianTimePast" locktime mempool'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "median time past" "mempool"'
```

Closest historical hits:

- closed #3060 validates transaction lock times;
- closed #5984 reports the original missed mempool `nLockTime`/MTP check; and
- closed #6027 implements `BestChainNextMedianTimePast` and mempool locktime
  validation.

Local proof rerun on 2026-05-09:

```sh
cargo test -p zebra-consensus mempool_request_with_invalid_lock_time_is_rejected --lib
cargo test -p zebra-consensus transaction_is_rejected_based_on_lock_time --lib
cargo test -p zebra-consensus time_is_valid_for_historical_blocks --lib
cargo test -p zebra-consensus mempool_cached_result_bypasses_expiry_check_for_block_at_next_height --lib
```

Result: all four commands passed. The added cache-expiry regression is the
stronger backstop for the stale-mempool-cache bypass shape.

### Shielded reorg finalization read-order eliminated lead

Status: local-only eliminated lead, not posted.

Detailed note:
`docs/analysis/shielded-reorg-finalization-read-order-note.md`.

Summary: the suspected post-finalize/pre-DB publication race is not exposed to
the reviewed read requests. Read-only state uses the published watch snapshot,
so concurrent readers see either the pre-finalization snapshot or a later
published state, not the writer-private intermediate state.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra shielded reorg finalization read order anchors nullifiers'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "CheckBestChainTipNullifiersAndAnchors"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "CheckBlockProposalValidity" "anchors"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "non-finalized" "watch" "finalization" "nullifiers"'
```

No direct hits were returned for the read-order/finalization race shape. The
closest historical hit is closed #5716 for best-chain mempool contextual
validation of anchors and nullifiers.

Local proof rerun on 2026-05-09:

```sh
cargo test -p zebra-state service::check::tests::anchors --lib
cargo test -p zebra-state service::check::tests::nullifier --lib
cargo test -p zebra-state service::non_finalized_state::tests::prop::forked_equals_pushed --lib
```

Result: all three commands passed, covering 2 anchor tests, 13 nullifier tests,
and 2 non-finalized fork/finalization property tests.

### Indexer block serialization panic eliminated lead

Status: local-only eliminated lead, not posted.

Detailed note:
`docs/analysis/indexer-block-serialization-panic-reachability-note.md`.

Summary: `BlockAndHash::new()` has an internal `expect()` while serializing
state-held blocks for the optional indexer stream, but current evidence
eliminates remote reachability because subscribers cannot provide the block
being serialized and normal state ingestion uses the same version checks.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra BlockAndHash indexer block serialization panic'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "block serialization should not fail"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "NonFinalizedStateChange" "BlockAndHash"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "indexer" "serialization" "panic" "block"'
```

No direct duplicate hits were returned. The closest historical hit is closed
#9654, which introduced `NonFinalizedBlocksListener` and the
`NonFinalizedStateChange` gRPC method.

Local proof rerun on 2026-05-09:

```sh
cargo test -p zebra-chain round_trip_blocks --lib
cargo test -p zebra-chain blockheader_serialization --lib
cargo test -p zebra-chain block_commitment --lib
```

Result: all three commands passed. These are serialization/commitment backstops,
not a direct malformed-state indexer-panic reproducer.

### Serde helper unwrap reachability

Status: local-only internal-format hardening, not posted.

Detailed note:
`docs/analysis/serde-helper-unwrap-reachability-note.md`.

Summary: Serde helper conversions for Jubjub/Pallas/Sapling/Orchard types have
real `unwrap()` sites, but current source tracing did not find an attacker-fed
remote/RPC Serde deserialization path. The credible reachability is malformed
trusted local disk/internal formats.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra serde_helpers unwrap bincode malformed disk panic'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "serde_helpers" "unwrap"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "from_bytes" "unwrap" "bincode"'
```

No direct duplicate hits were returned. The closest broad hit was closed #2185
for Orchard nullifier storage, which is historical state-format work rather than
malformed-Serde panic handling.

Local proof rerun on 2026-05-09:

```sh
cargo test -p zebra-state roundtrip_sapling_tree_root --lib
cargo test -p zebra-state roundtrip_sapling_subtree_data --lib
cargo test -p zebra-state roundtrip_orchard_tree_root --lib
cargo test -p zebra-state roundtrip_orchard_subtree_data --lib
```

Result: all four commands passed. These are valid-data disk-format backstops;
they do not eliminate malformed local-state panics.

### State startup validation admission barrier

Status: local-only startup/data-integrity hardening, not posted.

Detailed note:
`docs/analysis/state-startup-validation-admission-barrier-note.md`.

Summary: `CheckOpenCurrent` and `Downgrade` state databases are opened,
non-upgrade format work is marked finished, and usable state handles are
returned before detailed format validation completes in the background. A
malformed current-version database can therefore briefly be treated as
operational before the checker panic is observed.

Local proof added:

```sh
cargo test -p zebra-state check_open_current_marks_upgrades_finished_before_validation_panics_today --lib
```

Result on 2026-05-09: passed. The test creates a current-version database with
raw block/header/transaction data but missing detailed-format data. Standalone
detailed validation fails while `finished_format_upgrades()` remains false, but
the `CheckOpenCurrent` path marks `finished_format_upgrades()` true before the
validation failure is observed.

Triage: local/state-integrity hardening rather than a private-disclosure
candidate on current evidence. Requires malformed local disk state or unusual
cross-version reuse; no peer-triggered path was proven.

### RPC response-construction panic sweep

Status: local-only eliminated lead, not posted.

Detailed note:
`docs/analysis/rpc-response-construction-panic-sweep-note.md`.

Summary: no fresh private-disclosure RPC panic was confirmed after excluding the
already-known `longpollid`, `z_listunifiedreceivers`, and height-based
confirmation cases. The most worthwhile remaining hardening is replacing
address-index sentinel assertions with request-scoped errors.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra RPC response construction panic address index sentinel genesis'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "getaddresstxids" "genesis" "panic"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "response json should have an id"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "getrawmempool" "response json should have an id"'
```

No direct hits were returned for the address-index sentinel shape. The
JSON-RPC ID middleware subcase is historically covered by closed #9314, closed
#9421, and duplicate/autogenerated #9386.

Local proof rerun on 2026-05-09:

```sh
cargo test -p zebra-rpc rpc_getaddresstxids_response --lib
cargo test -p zebra-rpc rpc_getaddressutxos_response --lib
cargo test -p zebra-rpc rpc_getaddresstxids_forwards_large_default_range_today --lib
```

Result: all three commands passed. Current source evidence still excludes the
genesis transparent coinbase sentinel collision in normal on-chain address
queries.

### RPC z_gettreestate response-bound eliminated lead

Status: local-only eliminated lead, not posted.

Detailed note: `docs/analysis/rpc-z-gettreestate-response-bound-note.md`.

Summary: `z_gettreestate` serializes Sapling/Orchard tree frontiers, not all
historical note commitments. The response is bounded by Merkle depth, so the
large whole-tree response lead is eliminated.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra z_gettreestate finalState response bound whole tree'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "z_gettreestate" "finalState"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "z_gettreestate" "response" "large"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "CommitmentTree" "finalState"'
```

No duplicate hits were returned for the large-response/whole-tree bound shape.
Relevant adjacent historical hits are closed #3990 for the original RPC
implementation and closed #9445/#9451 for optional `finalState` compatibility.

Local proof rerun on 2026-05-09:

```sh
cargo test -p zebra-rpc test_z_get_treestate --lib
```

Result: passed, 1 test. The detailed note calculates the frontier serialization
bound from the relevant tree and legacy commitment-tree types.

### P2P compact-block non-support eliminated lead

Status: local-only eliminated lead, not posted.

Detailed note: `docs/analysis/p2p-compact-block-non-support-note.md`.

Summary: Zebra does not implement Bitcoin-style compact-block relay in its P2P
surface. There are no `sendcmpct`, `cmpctblock`, `getblocktxn`, or `blocktxn`
message variants or internal reconstruction requests, so the compact-block
parsing/reconstruction lead is eliminated for the current checkout.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra compact block sendcmpct cmpctblock getblocktxn blocktxn Zebra'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "sendcmpct" OR "cmpctblock" OR "getblocktxn" OR "blocktxn"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "compact block" "P2P"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "compact blocks" "network"'
```

No direct issue hits were returned. Broad `"compact blocks" "network"` search
only returned unrelated epics/refactors.

Local proof status: source-evidence-only. Unsupported command frames still cost
bounded frame buffering/checksum work, tracked separately by the already-public
unknown-command issue #10553.

### P2P gossiped-address services unwrap eliminated lead

Status: local-only eliminated lead, not posted.

Detailed note:
`docs/analysis/p2p-gossiped-address-services-unwrap-elimination-note.md`.

Summary: the gossiped-address `expect()` sites look attacker-adjacent, but
accepted `addr` / `addrv2` entries carry both service bits and last-seen time,
and outbound `getaddr` responses are sanitized before serialization. Current
evidence eliminates a remote P2P panic path.

Duplicate checks performed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra gossiped address services unwrap MetaAddr AddrV1 panic'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "Received gossiped peers always have services set"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "MetaAddrs should be sanitized before serialization"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "addrv2" "services" "panic"'
```

The exact panic-path searches returned no issue hits. The broader
`addrv2`/`services`/`panic` search returned only old closed PR #2976 ("Make
`services` field in `MetaAddr` optional"), which is historical context rather
than a duplicate report of this reachability question.

Local proof rerun:

```sh
cargo test -p zebra-network addr_v1_sanitized_roundtrip --lib
cargo test -p zebra-network addr_v2_sanitized_roundtrip --lib
cargo test -p zebra-network parses_msg_addr_v1_ip --lib
cargo test -p zebra-network parses_msg_addr_v2_ip --lib
```

Result on 2026-05-09: all four commands passed.

### P2P header command trace unwrap eliminated lead

Status: local-only eliminated lead, not posted.

Detailed note: `docs/analysis/p2p-header-command-trace-unwrap-note.md`.

Summary: `Codec::decode()` unwraps `String::from_utf8()` for peer-controlled
command bytes only after passing every byte through `std::ascii::escape_default`.
The resulting bytes are ASCII and therefore valid UTF-8, eliminating the remote
panic concern.

Duplicate checks performed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra P2P command String::from_utf8 escape_default unwrap'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "String::from_utf8" "escape_default" "command"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "command_bytes" "from_utf8"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "unknown command" "from_utf8_lossy"'
```

No issue hits were returned.

Local proof rerun:

```sh
cargo test -p zebra-network version_timestamp_out_of_range --lib
cargo test -p zebra-network unknown_command_before_valid_frame_strands_buffered_frame_today --lib
```

Result on 2026-05-09: both commands passed. Suggested hardening is readability
only: hide the invariant in a helper or avoid the `unwrap()`.

### P2P unknown-command log hygiene

Status: local-only hygiene hardening, not posted.

Detailed note: `docs/analysis/p2p-log-injection-b6-recheck-note.md`.

Summary: the B6 log-injection sweep did not find a high-impact path. The
remaining issue is that unknown 12-byte command names are logged as a lossy
UTF-8 display string at debug level, allowing bounded control-character log
confusion if debug logs are enabled.

Duplicate checks performed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra P2P unknown command lossy UTF-8 log control character'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "unknown message command from peer"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "String::from_utf8_lossy" "unknown" "command"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "log injection" "P2P"'
```

The direct log-hygiene searches returned no issue hits. The exact log-message
search returned old closed PR #3120 ("Stop closing connections on unexpected
messages"), which is historical context rather than a duplicate of the
lossy-display logging question.

Local proof rerun:

```sh
cargo test -p zebra-network unknown_command_before_valid_frame_strands_buffered_frame_today --lib
```

Result on 2026-05-09: passed. The field is fixed-size and debug-level, so this
stays hygiene hardening.

### Queued block AwaitUtxo scope

Status: local-only hardening, not posted.

Detailed note: `docs/analysis/queued-block-awaitutxo-scope-note.md`.

Summary: block/proposal semantic verification can receive UTXO hints from the
global queued-block `known_utxos` map, even when the queued missing-parent block
is unrelated to the candidate's eventual parent chain. Later contextual state
validation rebuilds UTXOs from the selected parent chain and rejects invalid
spends, so current evidence points to extra work rather than invalid acceptance.

Duplicate checks performed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra AwaitUtxo queued block known_utxos missing parent'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "AwaitUtxo" "known_utxos"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "queued block" "UTXO" "missing parent"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "known_utxos" "best-effort"'
```

No direct duplicate issue hits were returned. The broad
`AwaitUtxo`/`known_utxos` search returned old closed PR #5257 ("change(state):
Write non-finalized blocks to the state in a separate thread, to avoid network
and RPC hangs"), which mentions `AwaitUtxo` follow-up cleanup but does not report
this queued-only hint scope question.

Local proof rerun:

```sh
cargo test -p zebra-consensus dont_skip_verification_of_block_transactions_in_mempool --lib
cargo test -p zebra-state service::queued_blocks::tests::vectors --lib
cargo test -p zebra-state queued_utxo_lookup_is_global_across_parent_hashes_today --lib
```

Result on 2026-05-09: all commands passed. This supports the
mempool-elimination side and current queued-block bookkeeping. The focused
global-lookup test queues a block under one parent, verifies an unrelated parent
has no queued children, and still reads the queued output through
`QueuedBlocks::utxo()`, confirming that the queued UTXO lookup itself is not
parent-scoped.

### State queued-block timeout retention

Status: local-only hardening, not posted.

Detailed note: `docs/analysis/state-queued-block-timeout-retention-note.md`.

Summary: sync/inbound caller futures wrap block verification in
`BLOCK_VERIFY_TIMEOUT`, but once a semantically verified missing-parent block is
queued in state, dropping the caller future does not remove the queued block.
Retention lasts until the parent arrives or finalized-height pruning catches up.

Duplicate checks performed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra queued blocks timeout retention missing parent semantically verified'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "QueuedBlocks" "timeout"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "BLOCK_VERIFY_TIMEOUT" "queued"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "missing parent" "timeout" "state"'
```

No direct duplicate issue hits were returned. The broad
`QueuedBlocks`/`timeout` search returned adjacent history: open #5709 ("Fix
repeated block timeouts during initial sync"), closed #6763 ("Intermittent long
delays to inbound peer connection handshakes, Credit: Ziggurat Team"), and
closed #5257 ("change(state): Write non-finalized blocks to the state in a
separate thread, to avoid network and RPC hangs"). These do not duplicate this
specific missing-parent queued-block retention after caller cancellation.

Local proof rerun:

```sh
cargo test -p zebra-state service::queued_blocks::tests::vectors --lib
```

Result on 2026-05-09: passed. This covers queued-block pruning/dequeue
invariants. It also includes
`queued_block_remains_after_result_receiver_is_dropped_today`, which queues a
semantically verified block with its result receiver already dropped and confirms
the block and its queued UTXOs remain until height pruning removes them. This is
not a full sync/inbound timeout harness, but it directly covers the queue-level
retention primitive that caller cancellation relies on.

### PeerSet `AdvertiseBlockToAll` stale unready queued state

Status: local-only hardening, not posted.

Detailed note:
`docs/analysis/peer-set-advertiseblocktoall-stale-unready-note.md`.

Summary: `PeerSet::broadcast_all()` queues a follow-up broadcast for peers that
were unready at the moment of an `AdvertiseBlockToAll` request. Current source
prunes banned peers and peers that later become ready, but not peers removed or
disconnected while still unready. A stale key in `remaining_peers` can keep the
queued broadcast state and returned broadcast future open after the peer is gone.

Duplicate checks recorded in the detailed note found closed #10258 as a partial
duplicate for the banned-peer variant, but not the ordinary removed/disconnected
unready-peer variant.

Local proof added:

```sh
cargo test -p zebra-network queued_broadcast_all_keeps_removed_unready_peer_today --lib
```

Result on 2026-05-09: passed. The test inserts a queued broadcast entry for a
peer with an unready cancel handle, calls the normal `remove()` path, and
confirms that the cancel handle is removed while the queued broadcast sender
stays open and `remaining_peers` still contains the removed peer. A later
`broadcast_all_queued()` retry sends an empty follow-up future but still does not
prune the stale peer.

Triage: low availability hardening. The immediate broadcast to ready peers still
happens, newer queued broadcasts replace older queued state, and the issue
affects best-effort mined-block advertisement completion rather than validation
or consensus.

### Halo2 batch queue weight admission

Status: local-only hardening, not posted.

Detailed note: `docs/analysis/halo2-batch-queue-weight-admission-note.md`.

Summary: `tower-batch-control` uses `RequestWeight` to decide when the worker
should flush a batch, but queue admission reserves one semaphore permit per
request. That matches the unit-weight primitive verifiers but undercounts
Halo2, where one queued item can contain many Orchard actions and
`halo2::Item::request_weight()` returns the action count.

Duplicate checks recorded in the detailed note found older batch/performance
history, including #4750, #4752, #4789, #9308, and #10179, but no exact open
duplicate for weight-insensitive queue admission.

Local proof added:

```sh
cargo test -p tower-batch-control weighted_requests_only_consume_one_queue_permit_today --test worker
```

Result on 2026-05-09: passed. The test creates a synthetic full-weight request
type, configures `max_items_weight_in_batch = 2` and `max_batches = 1`, and
shows that two full-weight requests are admitted before the third request is
backpressured. This confirms admission is request-count based even when the
worker-side flush budget is weight based.

Triage: low-to-medium availability hardening. This can amplify verifier work
queued behind attacker-influenced Orchard action counts, but it does not bypass
semantic verification and remains bounded by transaction/block size, primitive
verifier concurrency, and outer verification timeouts.

### Misbehavior score overflow before ban enforcement

Status: local-only hardening, not posted.

Detailed note: `docs/analysis/misbehavior-score-overflow-hardening-note.md`.

Summary: the peer-set misbehavior batcher and
`MetaAddrChange::UpdateMisbehavior` both use raw `u32` addition before
address-book ban enforcement sees the combined score. Normal score producers and
the low ban threshold make practical exploitability low, but saturated arithmetic
would be safer and clearer.

Duplicate checks recorded in the detailed note found no exact open issue for
misbehavior score overflow or saturating addition. This is separate from the
lossy-channel note and the `max_connections_per_ip > 1` ban panic.

Local proof added:

```sh
cargo test -p zebra-network misbehavior_update_addition_overflows_before_ban_today --lib
cargo test -p zebra-network misbehavior_batch_accumulator_overflows_before_flush_today --lib
```

Result on 2026-05-09: both passed. The `MetaAddrChange` test creates an
existing `MetaAddr` with `misbehavior_score = u32::MAX` and applies an
`UpdateMisbehavior` increment of `1`. Debug builds panic before ban enforcement;
release builds wrap the score to zero before the ban check. The peer-set batch
test exercises the same helper used by the batcher task and confirms the
pending per-peer score has the same debug-panic / release-wrap behavior before
flush.

Triage: low hardening. Current remote practicality is low because normal
score-bearing errors already meet the ban threshold, but this is a clean
defense-in-depth fix if the peer misbehavior path is touched.

### Config debug log Elasticsearch secret disclosure

Status: local-only confidentiality hardening, not posted.

Detailed note: `docs/analysis/config-debug-log-secret-disclosure-note.md`.

Summary: `zebrad` logs the full processed `ZebradConfig` with derived `Debug`
during normal server startup. In opt-in `elasticsearch` builds, the embedded
state config includes `elasticsearch_username` and `elasticsearch_password`
plain `String` fields, so a config-file password can be emitted to INFO startup
logs.

This is separate from the Elasticsearch transport/panic note: that note covers
TLS verification and panic behavior in the optional Elasticsearch path; this
entry covers credential disclosure through full-config debug logging.

Local proof added:

```sh
cargo test -p zebrad --features elasticsearch debug_config_includes_elasticsearch_password_today --lib
```

Result on 2026-05-09: passed. The test sets a sentinel
`state.elasticsearch_password`, formats `ZebradConfig` with the current derived
`Debug`, and confirms the sentinel password appears in the formatted output.

Triage: low-to-medium confidentiality, deployment dependent. This is not a
default-release remote issue, but it is worth fixing if the experimental
Elasticsearch feature is maintained or if support bundles/log forwarding are in
scope.

### Checkpoint subsidy output validation gap

Status: local-only trusted-checkpoint/custom-network hardening, not posted.

Detailed note: `docs/analysis/checkpoint-subsidy-output-validation-gap-note.md`.

Summary: full semantic validation checks actual Canopy+ funding-stream outputs
and NU6.1 lockbox disbursement outputs in the coinbase. The checkpoint verifier
does not; it derives only a synthetic deferred-pool delta from the configured
schedule before committing checkpoint-verified blocks. A trusted checkpoint hash
for a malformed, Merkle-consistent block can therefore pass checkpoint pre-checks
even though full semantic subsidy validation rejects the coinbase payout
structure.

Local proof added:

```sh
cargo test -p zebra-consensus checkpoint_check_block_accepts_missing_funding_stream_outputs_today --lib
```

Result on 2026-05-09: passed. The test mutates a funding-stream-era block so its
coinbase omits required funding-stream outputs, updates the Merkle root,
confirms full semantic subsidy validation rejects it with
`FundingStreamNotFound`, then confirms checkpoint pre-check accepts the same
block when the mutated hash is in the trusted checkpoint list on a
proof-of-work-disabled custom Testnet.

Triage: low-to-medium correctness hardening within the checkpoint trust
boundary. This is not ordinary peer-triggered Mainnet/default-Testnet exposure,
but it is a real full-validation versus checkpoint-validation discrepancy for
custom checkpoint sets and trusted checkpoint generation mistakes.

### Address read height boundary arithmetic

Status: local-only boundary hardening, not posted.

Detailed note:
`docs/analysis/address-read-height-boundary-arithmetic-note.md`.

Summary: address-index read helpers compute the non-finalized overlap start as
one block after the finalized tip. The balance helper unwraps `Height + 1` and
panics at the valid terminal `Height::MAX`. The txid and UTXO helpers use raw
`u32 + 1`; they do not panic at valid `Height::MAX` because Zebra's valid
maximum height is `u32::MAX / 2`, but debug builds panic if an invalid public
tuple height such as `Height(u32::MAX)` reaches those helpers.

Local proof added:

```sh
cargo test -p zebra-state terminal_finalized_tip_panics_balance_overlay_in_debug_today --lib
cargo test -p zebra-state invalid_max_finalized_tip_panics_tx_id_overlay_in_debug_today --lib
cargo test -p zebra-state invalid_max_finalized_tip_panics_utxo_overlay_in_debug_today --lib
```

Result on 2026-05-09: all passed. The first test proves the valid
`Height::MAX` balance panic. The second and third prove the invalid-height raw
addition debug panic shape and eliminate the original hypothesis that txid/UTXO
panic at valid `Height::MAX`.

Triage: low availability/correctness hardening. Current finalized storage and
normal height parsing keep these values out of ordinary deployments, so this is
not a remote vulnerability on current evidence.

### Invalidateblock restart replay boundary

Status: local-only trusted-RPC operational hardening, not posted.

Detailed note: `docs/analysis/invalidateblock-restart-replay-boundary-note.md`.

Summary: non-finalized block invalidations are stored in the in-memory
`NonFinalizedState::invalidated_blocks` map. A fresh non-finalized state starts
with an empty invalidation map, so a block invalidated before restart can be
accepted again if the node later receives it and it still passes normal
validation. This is an operational quarantine boundary for trusted
`invalidateblock`, not an ordinary peer-only exploit.

Local proof added:

```sh
cargo test -p zebra-state fresh_non_finalized_state_forgets_invalidated_block_today --lib
cargo test -p zebra-state backup_restore_replays_invalidated_block_today --lib
```

Result on 2026-05-09: both passed. The first test commits and invalidates a
child block, confirms the live state rejects that child with
`BlockPreviouslyInvalidated`, then constructs a fresh non-finalized state,
commits the same parent, and confirms the previously invalidated child is
accepted. The second test writes the stale chain to the backup cache,
invalidates the child in memory without refreshing the backup, then restores
from backup and confirms the invalidated child is present again.

Triage: low-to-medium operational correctness depending on expected
`invalidateblock` semantics. Both the backup restore route and the
re-receive-after-restart route are now proof-backed.

## Next Local-Only Work

When continuing, prefer converting one local-only queue item at a time into:

1. a duplicate-search transcript,
2. source-line evidence,
3. one focused current-behavior test when practical,
4. a short local issue-style note,
5. an updated row in this ledger.
