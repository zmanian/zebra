# Panic and Assertion Sweep - 2026-05-09

Disposition: local-only. Do not post publicly without explicit direction.

Scope: follow-up sweep of production `assert!`, `assert_eq!`, `panic!`,
`unreachable!`, `unwrap()`, and `expect()` call sites in `zebra-state`,
`zebra-network`, `zebra-rpc`, `zebra-consensus`, and selected `zebrad`
components after the v4.4.1 security-fix review.

## Live Finding

The highest-signal item from this sweep is the same-root side-chain
invalidation/finalization panic documented in
`docs/analysis/finalization-invalidated-record-retention-note.md`.

Summary: `NonFinalizedState::finalize()` reinserts side chains after popping the
shared root without checking whether the side chain is now empty. If a
side-chain tip was invalidated first, the shared-root finalization can leave an
empty `Chain` in `chain_set`, and a later finalization panics with `only called
while blocks is populated`.

Local proof:

```sh
cargo test -p zebra-state finalize_after_invalidating_same_root_side_chain_tip_panics_today --lib
```

Result: passed as a current-behavior `#[should_panic]` test.

Release reachability: `v4.4.1` and current `main`
(`589d64b9b7ea6ab4c32ecab41ba6b74f26907940`) have the same vulnerable
finalization logic in `zebra-state/src/service/non_finalized_state.rs`.

## Eliminated Leads

### Sync downloader single-block response assertions

Candidate:

- `zebrad/src/components/sync/downloads.rs` asserts that a response to a
  singleton `BlocksByHash` request contains exactly one block status.
- `zebrad/src/components/inbound/downloads.rs` has the same assertion for
  gossiped block downloads.

Conclusion: eliminated as a peer-triggered panic on current source review. The
connection handler tracks requested block hashes in `pending_hashes`; for a
singleton request, a matching block empties the set and returns exactly one
`Available` item. A missing singleton block returns `PeerError::NotFoundResponse`
rather than `Response::Blocks(vec![Missing])`, and unrelated blocks are treated
as unhandled inbound messages while the request keeps waiting.

### Sync `downloads is nonempty` expectations

Candidate:

- `zebrad/src/components/sync.rs` waits on `self.downloads.next().await.expect("downloads is nonempty")`.

Conclusion: eliminated for the obvious zero-limit shape. `ChainSync::new()`
raises too-low user config values to `MIN_CONCURRENCY_LIMIT` and
`MIN_CHECKPOINT_CONCURRENCY_LIMIT` before constructing the downloader, so
`lookahead_limit()` is not zero under supported configuration.

### P2P address-cache no-response assertion

Candidate:

- `zebra-network/src/peer/connection.rs` asserts that unsolicited `addr`
  messages do not take peers from the address cache.

Conclusion: eliminated. The unsolicited path calls `Handler::update_addr_cache`
with `response_size = None`, which resolves to zero. `partial_shuffle(..., 0)`
returns an empty response and leaves cached entries bounded by
`MAX_ADDRS_IN_MESSAGE`.

### Peer connection error-slot assertion

Candidate:

- `zebra-network/src/peer/connection.rs` asserts that closed connections have
  populated the shared error slot.

Conclusion: no peer-triggered bypass found in this pass. The visible loop exits
route through `fail_with()` or `shutdown_async()`, including peer EOF, peer read
errors, client-channel closure, service shutdown, and ping timeout. Request
timeouts that do not close the connection return to `AwaitingRequest` instead
of exiting the loop.

### Peer-set missing cancel handle assertion

Candidate:

- `zebra-network/src/peer_set/set.rs` asserts that an unready service that
  errors has a cancel handle.

Conclusion: eliminated for duplicate, banned, outdated-version, and explicit
remove paths. Those paths either keep the newer duplicate's handle, send the
cancel signal and then return `UnreadyError::Canceled`, or use debug-only
assertions when dropping banned unready peers. No remote sequence was found that
removes the cancel handle and then returns `UnreadyError::Inner`.

## Notes

This sweep was intentionally limited to source reachability and focused tests.
It does not claim every panic-shaped invariant in the workspace is externally
unreachable. It records the high-signal result and the nearest eliminated leads
to avoid repeating the same triage.
