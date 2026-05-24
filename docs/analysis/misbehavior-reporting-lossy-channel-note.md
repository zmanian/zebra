# Misbehavior Reporting Lossy Channel Note

Date: 2026-05-03

Last updated: 2026-05-09

Scope: peer-misbehavior reporting transport from `zebrad` verification
components into the `zebra-network` address-book ban pipeline.

## Finding

Zebra's address-book ban logic is additive and IP-wide once a misbehavior update
arrives, but the report transport into that pipeline is lossy. The current sync
and mempool score producers call `try_send()` on a bounded `mpsc` channel and
ignore the result. Inbound has the same lossy branch for `VerifyBlockError`
inputs, but current ordinary reachability is separately limited by
`RouterError` wrapping.

Relevant paths:

- `zebra-network/src/peer_set/initialize.rs:122-128` creates the
  `misbehavior_tx` channel with capacity
  `config.peerset_total_connection_limit().max(MIN_CHANNEL_SIZE)`.
- With default network config this is 200 entries:
  `DEFAULT_PEERSET_INITIAL_TARGET_SIZE` is 25, inbound multiplier is 5, outbound
  multiplier is 3.
- `zebra-network/src/peer_set/initialize.rs:141-145` drains the channel into a
  per-peer score map when the receiver task is scheduled.
- `zebra-network/src/peer_set/initialize.rs:149-157` flushes the coalesced map
  to the address-book updater every 30 seconds.
- `zebrad/src/components/sync.rs:1147-1150` reports invalid sync blocks using
  ignored `try_send()`.
- `zebrad/src/components/mempool.rs:648-652` reports invalid downloaded
  transactions using ignored `try_send()`.

`zebrad/src/components/inbound.rs` has the same ignored `try_send()` shape
after downcasting errors to `VerifyBlockError`, but the live semantic block
verifier type returns `RouterError`. That current type mismatch is tracked
separately in `inbound-gossiped-block-router-error-misbehavior-note.md`; it
prevents the inbound path from reaching the lossy transport branch for ordinary
score-bearing verifier errors.

The receiver task does not intentionally wait 30 seconds before draining; it
aggregates reports continuously and flushes later. But `try_send()` can still
return `Full` during a burst if many verification completions report scores
before the receiver drains capacity. Because the error is discarded, those score
updates are lost silently.

## Impact

This is public P2P robustness hardening, not a private consensus issue.

Invalid blocks or transactions are still rejected. The gap is punishment
reliability: an attacker who can cause many score-bearing invalid verification
results in a short burst may avoid some address-book score increments. Since
many score-bearing errors increment by 100 and the ban threshold is also 100,
losing even one report can matter for that peer.

Bounds and mitigations:

- the channel capacity is sized to the total connection limit, not tiny under
  default settings;
- the receiver drains into memory without taking the address-book mutex on every
  report;
- verification/download concurrency limits bound the number of simultaneous
  producers;
- this does not affect consensus acceptance.

## Local Confidence Check

Source evidence is direct:

- `zebra-network/src/peer_set/initialize.rs:122-128` creates a bounded
  `mpsc` channel for misbehavior updates.
- `zebra-network/src/peer_set/initialize.rs:141-157` drains and batches that
  channel before forwarding scores to the address-book updater.
- `zebrad/src/components/sync.rs:1147-1150` and
  `zebrad/src/components/mempool.rs:648-652` call `try_send()` and assign the
  result to `_`, so `Full` or `Closed` errors are intentionally discarded.
- `zebrad/src/components/inbound.rs` contains an adjacent ignored `try_send()`
  branch for `VerifyBlockError`, but current `RouterError` wrapping prevents
  the normal score-bearing verifier error from reaching that branch.

Focused current-behavior tests added and run on 2026-05-09:

```sh
cargo test -p zebrad full_misbehavior_channel_drops_score_bearing_sync_report_today --lib
cargo test -p zebrad full_misbehavior_channel_drops_score_bearing_mempool_report_today --lib
cargo test -p zebrad full_misbehavior_channel_drops_score_bearing_inbound_branch_today --lib
```

Result: all three passed. The sync test fills the bounded misbehavior channel
with a sentinel, then feeds `ChainSync::handle_block_response()` a
score-bearing invalid-block response with an advertiser address. The mempool
test fills the same kind of channel, drives the normal downloaded-transaction
path with an advertiser address, and makes the verifier return a 100-point
mempool error. The inbound branch-level test fills the same kind of channel and
injects a score-bearing `VerifyBlockError` into the extracted inbound reporting
helper. Because all three tested paths use ignored `try_send()`, the sentinel
remains the only message and the score-bearing report is absent after the
handler runs.

The existing `inbound_router_error_score_is_not_reported_today` proof still
documents the separate ordinary-reachability caveat: live inbound verifier
errors are boxed as `RouterError`, so normal score-bearing verifier errors do
not currently reach the extracted `VerifyBlockError` reporting helper.

Duplicate check refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "misbehavior" "try_send" "dropped"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "misbehavior channel" "full"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "peer misbehavior report" "lost"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "full_misbehavior_channel_drops_score_bearing_sync_report_today"'
```

No hits were returned.

## Suggested Public Fix

- Replace raw `mpsc::Sender<(PeerSocketAddr, u32)>` clones with a small
  `MisbehaviorReporter` that coalesces increments in a shared map and cannot
  fail due to channel capacity.
- Alternatively, use `send().await` from non-hot paths and log/count any failure
  explicitly, but avoid holding locks or address-book access on producer paths.
- Add a metric for dropped or queued misbehavior reports if a bounded transport
  remains.
- Preserve current policy boundaries: `notfound`, timeout, overload, stall, and
  protocol-oddity paths can remain disconnect/routing-only unless maintainers
  intentionally decide to score them.
