# Inbound Gossiped Block Router Error Misbehavior Note

Date: 2026-05-03

Last updated: 2026-05-09

Disposition: local-only. Do not post publicly without explicit
re-authorization.

Scope: inbound gossiped block download and verification cleanup in
`zebrad/src/components/inbound.rs` and
`zebrad/src/components/inbound/downloads.rs`.

## Summary

Inbound gossiped block verification errors with nonzero misbehavior scores can
escape address-book misbehavior reporting. The invalid block is still rejected,
so this is not a consensus issue. The impact is weaker peer scoring and banning
for peers that provide invalid gossiped blocks.

Classification: public P2P hardening, not private disclosure.

## Finding

The inbound service's semantic block verifier is typed as a service whose error
is `RouterError`:

- `zebrad/src/components/inbound.rs:82-87`

`Inbound::poll_ready()` drains completed gossiped block download tasks and only
forwards peer misbehavior when the boxed error downcasts to `VerifyBlockError`:

- `zebrad/src/components/inbound.rs:332-343`

But the actual verifier failures are returned through the router error type.
`RouterError` wraps `VerifyBlockError` and exposes its own
`misbehavior_score()` method:

- `zebra-consensus/src/router.rs:100-153`

The download task preserves the responding peer address for verifier errors:

- `zebrad/src/components/inbound/downloads.rs:394-398`

So the address information is available, and the score is available, but the
cleanup code currently checks the wrong concrete boxed error type before
calling `misbehavior_score()`.

## Local Proofs

A local regression-style unit test documents the current mismatch:

- `score_bearing_router_error_does_not_downcast_to_verify_block_error`

It constructs a score-bearing `VerifyBlockError`, wraps it into `RouterError`,
boxes it, and confirms that downcasting to `VerifyBlockError` fails while the
boxed `RouterError` still has a nonzero misbehavior score.

A service-level current-behavior test now drives the real inbound boundary:

- `inbound_router_error_score_is_not_reported_today`

The test builds an `Inbound` service with a real state service, mocked block
download peer set, mocked semantic block verifier, and a real misbehavior
channel. It queues `AdvertiseBlock`, returns the block from the mocked
`BlocksByHash` response with `Some(advertiser_addr)`, returns a score-bearing
`RouterError` from `Request::Commit`, drives `Inbound::poll_ready()` until the
download queue is drained, and confirms the misbehavior channel remains empty.
This proves the advertiser and nonzero score reach the inbound cleanup boundary
but are skipped by the current downcast.

Verification run:

```text
cargo test -p zebrad score_bearing_router_error_does_not_downcast_to_verify_block_error --lib
cargo test -p zebrad inbound_router_error_score_is_not_reported_today --lib
```

Result on 2026-05-09: both passed.

## Duplicate Check

Read-only duplicate search refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra RouterError VerifyBlockError misbehavior score inbound gossiped block'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "RouterError" "VerifyBlockError" "misbehavior"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "invalid gossiped block" "misbehavior"'
```

No issue hits were returned.

## Eliminated Amplification Hypotheses

This pass did not find unbounded downloader growth:

- `Downloads::download_and_verify()` deduplicates by block hash before queuing,
  so the same hash from multiple advertisers creates one in-flight task.
- The pending task count is capped by `full_verify_concurrency_limit`, clamped
  to `MAX_INBOUND_CONCURRENCY = 200`.
- Advertisers with `Some(PeerSocketAddr)` are capped to one in-flight download
  per source IP via `in_flight_ips`.
- Normal download and verifier timeouts return through the task result and
  `Downloads::poll_next()` removes both the hash cancel handle and the
  in-flight IP entry.

The remaining state-read stall nuance is bounded: the initial `KnownBlock`
state query has no local timeout in the downloader task, so a stuck state
service could retain a bounded hash/IP slot. That is internal-service lifecycle
hardening, not a fresh remote growth issue.

## Impact

Likely severity: low.

Invalid gossiped blocks are not accepted. The issue is that peers serving
score-bearing invalid blocks can avoid the intended address-book scoring path
on the inbound gossip cleanup side. Sync downloader misbehavior reporting is
more type-aware: it matches `BlockDownloadVerifyError::Invalid { error:
RouterError, advertiser_addr: Some(..), .. }` and checks
`RouterError::misbehavior_score()`.

The practical effect is reduced automatic discouragement or banning for peers
that provide invalid gossiped blocks, increasing repeated invalid-block
download/verification work until other peer-management paths disconnect or
avoid them.

## Suggested Fix Direction

- In `Inbound::poll_ready()`, downcast boxed verifier errors to `RouterError`
  and use `RouterError::misbehavior_score()`.
- Keep support for direct `VerifyBlockError` only if tests or future service
  wiring still need it.
- Reuse the service-level proof test by changing the final assertion to expect
  `(advertiser_addr, 100)` on the misbehavior channel after the fix.

## Disclosure Triage

Public hardening.

This should not be private-disclosed unless follow-up work shows invalid
gossiped blocks can be used for materially unbounded remote work or can affect
block acceptance. Current evidence is limited to missed peer scoring after
rejection.

## Confidence

Confidence: high for the type mismatch, because the service alias, cleanup
downcast, router error wrapper, local downcast test, and service-level inbound
proof all line up.

Confidence: medium on operational severity. Peer scoring is a defense-in-depth
control, and other connection/error paths can still remove bad peers.
