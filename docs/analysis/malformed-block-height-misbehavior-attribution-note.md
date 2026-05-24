# Malformed Block Height Misbehavior Attribution Note

Date: 2026-05-03

Last updated: 2026-05-09

Disposition: local-only. Do not post publicly without explicit
re-authorization.

Scope: peer-misbehavior attribution for downloaded or gossiped blocks that do
not contain a coinbase height.

## Finding

`BlockError::MissingHeight` is a score-bearing consensus error, but the sync and
inbound download paths both preflight missing coinbase heights before the block
verifier can return that typed error. In those preflight paths, the serving peer
address is not preserved for misbehavior scoring.

Relevant paths:

- `zebra-consensus/src/block.rs:208-210` would return
  `BlockError::MissingHeight(hash)` during block verification.
- `zebra-consensus/src/error.rs:400-407` assigns nonzero block misbehavior score
  to `MissingHeight`.
- `zebrad/src/components/sync/downloads.rs:119-120` defines
  `BlockDownloadVerifyError::InvalidHeight { hash }` without
  `advertiser_addr`.
- `zebrad/src/components/sync/downloads.rs:442-451` returns that variant before
  verification when a downloaded block has no coinbase height.
- `zebrad/src/components/sync.rs:1143-1153` only reports misbehavior for the
  `Invalid { error, advertiser_addr: Some(..) }` variant.
- `zebrad/src/components/inbound/downloads.rs:354-365` converts a gossiped
  no-height block into a generic boxed error and explicitly maps it to
  `(e, None)`.
- `zebrad/src/components/inbound.rs:333-343` only attempts scoring when the
  completed download result has `Some(advertiser_addr)`.

So a peer serving a malformed no-height block can cause Zebra to drop/reject the
block, but the peer is not scored through the address-book misbehavior pipeline.

## Impact

This is public P2P robustness hardening, not a private consensus issue.

The malformed block is not accepted. The issue is inconsistent peer punishment:
the same semantic shape is score-bearing if it reaches the verifier, but loses
attribution in download preflight.

Bounds and mitigations:

- inbound gossiped block downloads are bounded and capped to one in-flight block
  per peer IP,
- sync download concurrency and lookahead limits bound concurrent work,
- the block is dropped before full contextual verification,
- impact is ban/scoring evasion rather than invalid acceptance.

## Suggested Public Fix

- Extend `BlockDownloadVerifyError::InvalidHeight` with
  `advertiser_addr: Option<PeerSocketAddr>`.
- Add a helper such as `BlockDownloadVerifyError::misbehavior_report()` so sync
  handles all score-bearing invalid-download variants consistently.
- In inbound gossiped block downloads, return a typed score-bearing error for
  missing height and preserve the known `advertiser_addr` instead of mapping it
  to `None`.
- Keep lookahead/behind-tip drops unscored, because those can reflect local
  progress or fork conditions rather than peer malice.

## Duplicate Check

Read-only duplicate search refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra MissingHeight malformed block misbehavior attribution'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "InvalidHeight" "misbehavior"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "gossiped block" "no height"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "synced block with no height"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "MissingHeight" "InvalidHeight"'
```

No issue hits were returned.

## Proof Status

Local current-behavior proof added and rerun on 2026-05-09:

```sh
cargo test -p zebrad invalid_height_download_error_does_not_send_misbehavior_today --lib
cargo test -p zebrad inbound_missing_height_error_drops_advertiser_addr_today --lib
```

Result: both commands passed. The sync test sends a
`BlockDownloadVerifyError::InvalidHeight` through
`ChainSync::handle_block_response()` and confirms no address-book misbehavior
update is emitted. The inbound test queues a gossiped block whose peer response
includes an advertiser address, but whose block has no coinbase height, and
confirms the completed inbound download error returns `None` for the advertiser
address.
