# Sync FindBlocks Junk Hash Steering Note

Date: 2026-05-03

Last updated: 2026-05-09

Disposition: local-only. Do not post publicly without explicit
re-authorization.

## Summary

Zebra has explicit limits for malicious `FindBlocks` / `FindHeaders` responses,
so this is not unbounded memory growth and not a consensus issue. But a peer
can still steer sync work with non-empty junk `FindBlocks` responses.

During `obtain_tips` and `extend_tips`, Zebra accepts unknown hashes from
non-empty peer responses, appends them to the block download set in response
arrival order, and treats any non-empty `BlockHashes` response as a useful
stall-tracker clear. A fast malicious peer can therefore place attacker-chosen
unknown hashes ahead of honest hashes, causing bounded but repeated
`BlocksByHash` requests, retry work, `NotFound` handling, and delayed honest
download progress.

A local proof test now confirms the `obtain_tips` ordering part of this
behavior: when a first response contributes two unknown synthetic hashes and a
later response contributes the honest continuation, Zebra requests
`BlocksByHash` for the two synthetic hashes before the honest block hashes.

This is public P2P sync availability hardening. It does not require private
disclosure.

## Evidence

`zebrad/src/components/sync.rs:70-87` explicitly accounts for malicious
`FindBlocks` / `FindHeaders` responses:

- one malicious response can provide up to `MAX_TIPS_RESPONSE_HASH_COUNT = 500`
  hashes;
- default checkpoint lookahead is `MAX_TIPS_RESPONSE_HASH_COUNT * 2`; and
- the comments expect malicious block downloads to fail validation and trigger
  sync restart.

`zebrad/src/components/sync.rs:712-862` handles `obtain_tips` responses:

- `FANOUT = 3`, so Zebra asks up to three peers for block hashes;
- every non-empty response is scanned for the first hash not already in state;
- all unknown hashes after that first unknown are appended to `download_set`;
- the last two unknown hashes are used to create a prospective tip; and
- the code notes that "the first response determines our download order".

`zebrad/src/components/sync.rs:876-1005` does similar work in `extend_tips`.
It rejects responses whose first or second hash does not match the expected next
hash, but once the expected hash is present it appends the remaining unknown
hashes in response order and again records a prospective tip from the tail.

`zebrad/src/components/sync.rs:1072-1097` queues downloads for up to the
current lookahead limit and returns extra hashes for later loops. That bounds the
immediate queue, but preserves response-order steering into later iterations.

`zebrad/src/components/sync/downloads.rs:350-390` turns each queued hash into a
single `BlocksByHash` request. Unknown junk hashes are therefore sent through
the peer set and retried before they can be discarded as missing or invalid.

`zebrad/src/components/sync.rs:1239-1253` treats `NotFoundResponse` and
`NotFoundRegistry` download failures as temporary and continues syncing rather
than restarting. This is reasonable for genuine propagation lag, but it means a
junk-hash response can consume retry and routing work without tripping the
"malicious block validation failed, restart" path described in the lookahead
comments.

`zebra-network/src/peer_set/set.rs:175-183` classifies any non-empty
`Response::BlockHashes(_)` or `Response::BlockHeaders(_)` as `StallOutcome::Clear`.
The peer-set stall tracker therefore penalizes empty or failed `FindBlocks`
responses, but a peer that returns non-empty junk avoids the stall threshold.

`zebrad/src/components/sync/tests/vectors.rs` contains local proof test
`obtain_tips_queues_fast_junk_hashes_before_later_honest_hashes_today`. The
test drives a single `obtain_tips` round with a synthetic first `FindBlocks`
response `[junk1, junk2, junk3]` and a later honest response
`[block1, block2, block3]`. Because mainnet response handling drops each
response tail, the resulting `BlocksByHash` requests are observed in this order:
`junk1`, `junk2`, `block1`, `block2`.

## Impact

Practical impact is bounded but attacker-reachable:

- a fast peer can make Zebra queue attacker-chosen hashes before honest hashes;
- each queued junk hash can trigger `BlocksByHash` routing through the peer set;
- the retry layer can retry missing downloads across peers;
- missing inventory is then recorded, and local `NotFoundRegistry` failures are
  treated as temporary;
- fake prospective tips can keep `extend_tips` asking for continuations of a
  bogus chain for the current sync attempt.

This can delay sync or waste peer bandwidth/CPU, especially when a node has a
small honest peer set or several low-latency malicious peers. It does not let a
peer make Zebra accept invalid blocks, and it is bounded by response-size,
lookahead, retry, timeout, and sync restart limits.

Key mitigations already in place:

- `MAX_TIPS_RESPONSE_HASH_COUNT = 500`;
- `FANOUT = 3`;
- tip response timeout is 6 seconds;
- block download timeout is 20 seconds;
- block download retry limit is 3;
- request queuing is capped by checkpoint/full verification lookahead; and
- blocks that are too far ahead, too far behind, invalid, or missing are dropped
  or treated as temporary failures.

## Suggested Fix

- Treat non-empty `FindBlocks` / `FindHeaders` responses as useful for stall
  tracking only when they are plausibly connected to the requested locator or
  expected next hash.
- In `obtain_tips`, mix or prioritize hashes from multiple peer responses so a
  fast first responder cannot dominate the initial download order.
- Bound the number of hashes accepted from any single peer per sync round below
  the global lookahead when other peers also returned plausible hashes.
- Consider assigning a weaker stall outcome to non-empty responses that produce
  only hashes that immediately become `NotFoundRegistry` / `NotFoundResponse`.
- Keep the local ordering proof as a regression backstop, and add a fixed
  behavior test where one fast peer returns many unknown junk hashes and another
  returns the real continuation. Assert the real continuation is queued within
  the first lookahead window, or that junk hashes are deprioritized after early
  missing responses.

## Verification

Local proof and nearby sync limiter coverage rerun on 2026-05-09:

- `cargo test -p zebrad obtain_tips_queues_fast_junk_hashes_before_later_honest_hashes_today --lib`
- `cargo test -p zebrad sync_block_too_high_obtain_tips --lib`

Result: both commands passed. The first test directly confirms the
response-order steering behavior. The second test exercises the existing
"downloaded block height too high" limiter, which remains one of the bounds on
practical impact.

## Duplicate Check

Read-only duplicate searches performed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "FindBlocks" "junk hash"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "obtain_tips" "BlocksByHash"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "FindBlocks" "NotFoundRegistry"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "non-empty" "FindBlocks" "stall"'
```

No issue hits were returned.

## Disclosure Triage

Public hardening.

Confidence: high that non-empty junk responses can steer `obtain_tips` download
order; medium on practical exploitability because the damage is bounded and
depends on peer mix, response timing, and whether honest peers also return
useful chain continuations.
