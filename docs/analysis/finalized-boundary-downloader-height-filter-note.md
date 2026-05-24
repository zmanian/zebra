# Finalized Boundary Downloader Height Filter Note

Date: 2026-05-03

Last updated: 2026-05-09

Disposition: local-only. Do not post publicly without explicit
re-authorization and a fresh duplicate check.

Scope: follow-up finalization-boundary pass after the Zebra 4.4.0 security
fixes, focused on sync and inbound downloader early height filters.

## Summary

The non-finalized state finalizes blocks until the best non-finalized chain
length is at most `MAX_BLOCK_REORG_HEIGHT`. In steady state, a best tip at
height `T` implies the finalized tip is around `T - MAX_BLOCK_REORG_HEIGHT`.

Both the sync downloader and inbound downloader compute:

```text
min_accepted_height = tip_height - MAX_BLOCK_REORG_HEIGHT
```

They then drop downloaded blocks only when:

```text
block_height < min_accepted_height
```

That admits a block at the exact boundary height. Relative to the steady-state
finalization invariant, the exact boundary is the finalized tip height, so an
alternate block at that height should no longer be forkable. Later consensus or
state checks should reject the block, but the sync path does not classify that
later rejection the same way as its early "behind tip" filter. The result is
bounded extra download, decode, and verification work plus avoidable sync
restart amplification.

This is low-severity hardening, not a consensus split.

## Evidence

The write service finalizes while the best non-finalized chain length is greater
than `MAX_BLOCK_REORG_HEIGHT`:

- `zebra-state/src/service/write.rs:439-445`

`MAX_BLOCK_REORG_HEIGHT` is derived from coinbase maturity:

- `zebra-state/src/constants.rs:14-31`

The sync downloader computes `min_accepted_height` as
`tip_height.saturating_sub(MAX_BLOCK_REORG_HEIGHT)`:

- `zebrad/src/components/sync/downloads.rs:427-440`

It rejects downloaded blocks strictly below that minimum, but not at equality:

- `zebrad/src/components/sync/downloads.rs:501-512`

The local test `sync_downloader_verifies_exact_min_accepted_height_today`
sets the mock best tip to `block_height + MAX_BLOCK_REORG_HEIGHT`, downloads a
block at exactly `tip - MAX_BLOCK_REORG_HEIGHT`, and confirms the sync
downloader sends that block to the verifier rather than returning
`BehindTipHeightLimit`.

If the exact-boundary block reaches state, contextual validation rejects it as
an orphan because finalized-height candidates are not forkable:

- `zebra-state/src/service/check.rs:224-237`

The sync downloader converts verifier failures into
`BlockDownloadVerifyError::Invalid`:

- `zebrad/src/components/sync/downloads.rs:560-568`

`ChainSync::should_restart_sync()` treats `BehindTipHeightLimit` as
non-restart-worthy, but generic invalid-block errors fall through to the
restart path:

- `zebrad/src/components/sync.rs:1219-1285`

The inbound downloader uses the same calculation:

- `zebrad/src/components/inbound/downloads.rs:339-352`

It also rejects only when `block_height < min_accepted_height`:

- `zebrad/src/components/inbound/downloads.rs:379-391`

The local test `inbound_downloader_verifies_exact_min_accepted_height_today`
sets the latest chain tip to `block_height + MAX_BLOCK_REORG_HEIGHT`, downloads
a gossiped block at exactly `tip - MAX_BLOCK_REORG_HEIGHT`, and confirms the
inbound downloader sends that block to the verifier rather than rejecting it as
behind the finalized-tip approximation.

## Local Proof Status

Sync downloader: test-backed. The proof confirms exact-boundary synced blocks
reach the verifier today.

Inbound downloader: test-backed. The proof confirms exact-boundary gossiped
blocks reach the verifier today.

## Impact

Likely severity: low.

An attacker who can advertise or serve same-height stale blocks at the exact
finalization boundary may get those blocks past the downloader's early height
filter. They still need the block body to deserialize, and the normal verifier
and state paths should reject a non-forkable alternate to finalized history.

The practical impact is bounded availability and operational noise:

- extra block download and decode,
- possible semantic/contextual verification attempt,
- possible sync restart when the stale exact-boundary block is rejected later as
  a generic invalid block rather than by the downloader's `BehindTipHeightLimit`
  path,
- noisy logs/metrics around stale block rejection.

It does not appear to allow invalid block acceptance, valid block rejection, or
state corruption.

## Suggested Fix Direction

- Prefer using the actual finalized tip height for downloader lower-bound
  filtering, then reject `block_height <= finalized_tip_height`.
- If the approximate `tip - MAX_BLOCK_REORG_HEIGHT` bound remains, decide
  explicitly whether the boundary block should be admitted. If not, change the
  comparison to reject `block_height <= min_accepted_height`, noting that this
  remains approximate when the active non-finalized chain is shorter than
  `MAX_BLOCK_REORG_HEIGHT`.
- Add a regression test or documented contract for exact-boundary blocks in both
  sync and inbound downloader paths.

## Disclosure Triage

Public hardening.

This should not be private-disclosed unless a follow-up finds that exact-boundary
blocks can bypass later finalized-state rejection, cause unbounded retention, or
be weaponized into a sustained node availability failure beyond avoidable sync
restarts.

## Confidence

Confidence: high for the strict-boundary mismatch in the downloader filters.
Confidence: medium-high that sync can classify the later finalized-height
rejection as restart-worthy, because the early `BehindTipHeightLimit` branch is
missed and generic invalid-block failures restart sync.

Confidence: low for serious security impact because the later verifier/state
layers reject finalized-height alternates and the admitted work appears bounded.

## Duplicate Check

Read-only GitHub searches refreshed on 2026-05-09:

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
