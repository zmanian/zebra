# Finalization Invalidated-Record Retention Note

Date: 2026-05-04

Last updated: 2026-05-09

Disposition: local-only. Do not post publicly without explicit
re-authorization.

Scope: Workstream A4 follow-up on deep reorgs, finalization boundaries, and
finalized-chain immutability assumptions.

## Finding

`NonFinalizedState::finalize()` keeps invalidated-block records at exactly the
height that has just been finalized, even though the local comment says records
at or below the finalized height should be removed.

The original stale-record shape is not consensus-critical and does not appear to
let finalized history be replaced. Follow-up testing found a sharper adjacent
panic: if a side-chain tip is invalidated, leaving a root-only side chain that
shares the best-chain root, finalizing that shared root inserts an empty side
chain back into the non-finalized state. The next finalization then panics with
`only called while blocks is populated`.

That panic is still in trusted-RPC/control-state territory, because the concrete
path uses invalidation. It is stronger than the stale-record cleanup issue
because Zebra release profiles use `panic = "abort"`.

## Evidence

Finalization pops the root block of the best non-finalized chain, commits that
block to finalized state, and removes side chains whose root no longer matches:

- `zebra-state/src/service/non_finalized_state.rs:284-329`

Immediately after that, the code says it removes all invalidated records at or
below the finalized height, but retains entries whose key is equal to the
finalized root height:

- `zebra-state/src/service/non_finalized_state.rs:331-333`

```rust
// Remove all invalidated_blocks at or below the finalized height
self.invalidated_blocks
    .retain(|height, _blocks| *height >= best_chain_root.height);
```

The predicate removes records below the finalized height, but it keeps records
at the finalized height. If the intent matches the comment and the
`reconsiderblock` logic's later comment, the predicate should retain only
records above the finalized height.

`reconsider_block()` has the same inclusive retention shape after replay:

- `zebra-state/src/service/non_finalized_state.rs:490-494`

It also searches invalidated records before testing whether the invalidated
root's parent can still be attached to finalized or non-finalized state:

- `zebra-state/src/service/non_finalized_state.rs:426-470`

Contextual validation itself rejects blocks at or below the finalized tip, so
this does not appear to let old blocks re-enter the active chain:

- `zebra-state/src/service/check.rs:224-237`

The process-fatal route is reachable through the normal state writer path, not
just direct unit-method calls:

- `zebra-rpc/src/methods.rs:2917-2928` exposes trusted RPC `invalidateblock`
  and sends `zebra_state::Request::InvalidateBlock`.
- `zebra-state/src/service.rs:1196-1215` routes that request to
  `send_invalidate_block()`.
- `zebra-state/src/service.rs:829-849` sends a
  `NonFinalizedWriteMessage::Invalidate` into the non-finalized block writer.
- `zebra-state/src/service/write.rs:354-360` handles that message by calling
  `non_finalized_state.invalidate_block(hash)`.
- `zebra-state/src/service/write.rs:439-445` later calls
  `non_finalized_state.finalize()` during ordinary non-finalized block commits
  once the best chain grows beyond `MAX_BLOCK_REORG_HEIGHT`.

Release reachability was checked locally on 2026-05-09: `git show
v4.4.1:zebra-state/src/service/non_finalized_state.rs` has the same
side-chain reinsertion logic and the same inclusive invalidated-record retention
predicate as current `main` (`589d64b9b7ea6ab4c32ecab41ba6b74f26907940`).

## Impact

Expected impact of the stale-record retention itself is low:

- stale invalidated records can survive one finalization step longer than the
  comments imply;
- `reconsiderblock` may return a later `ParentChainNotFound` style error for a
  now-finalized invalidated root instead of treating the invalidation record as
  gone;
- the existing `MAX_INVALIDATED_BLOCKS` bound still limits total retained
  records;
- this does not bypass finalized-chain immutability or make Zebra accept an
  alternate finalized-history block on current evidence.

The stale-record retention by itself should be handled as local hardening and
cleanup, not private disclosure.

The side-chain invalidation/finalization panic has higher availability impact:

- an operator or RPC caller with access to `invalidateblock` can invalidate a
  side-chain tip;
- when the shared non-finalized root is finalized, an empty side chain can remain
  in `chain_set`;
- a later finalization panics while trying to read the empty chain's root hash.

This still does not indicate invalid block acceptance, but it is a process-fatal
trusted-RPC availability bug on current evidence.

Trust-boundary caveat:

- Zebra RPC is disabled by default, and cookie authentication is enabled by
  default when RPC is enabled.
- `invalidateblock` is therefore not a default unauthenticated peer/P2P path.
- The current evidence supports a trusted-RPC/control-plane availability issue:
  an authenticated caller, or an operator-exposed RPC endpoint, can drive the
  state writer into a process-fatal finalization path.

## Duplicate Check

Read-only duplicate search refreshed on 2026-05-09:

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

Relevant adjacent hits:

- closed #9167 added `invalidate_block()` and the `invalidated_blocks` field;
- closed #9260 added `reconsider_block()`;
- closed #9551 added the trusted RPC methods;
- closed #9921 improved `InvalidateBlock` error propagation.
- closed #6498 and #6552 covered an intermittent test-only empty-chain panic
  with the same panic message, but the fix avoided a generated empty
  `partial_chain`; it did not address this invalidation/finalization path.
- closed #9884 touched `invalidateblock`/`reconsiderblock` while adding side
  chain RPC support, but does not cover this finalization panic.

Those are implementation provenance, but they do not cover the finalized-height
retention predicate, stale records at the finalized boundary, or the same-root
side-chain finalization panic reproduced here.

Related local test run on 2026-05-09:

```sh
cargo test -p zebra-state reconsider_block_twice_replays_stale_invalidated_entry_today --lib
cargo test -p zebra-state finalize_after_invalidating_same_root_side_chain_tip_panics_today --lib
cargo test -p zebra-state state_service_invalidate_side_chain_then_finalization_aborts_today --lib
cargo test -p zebra-state finalize_retains_invalidated_record_at_finalized_height_today --lib
```

Result: all four commands passed. The first two are direct
`NonFinalizedState` `#[should_panic]` tests. The third is a service-level
subprocess proof: the parent test drives normal `StateService::call()` requests,
including `Request::InvalidateBlock`, and asserts that the child test process
exits via `SIGABRT` after automatic finalization reaches the empty-chain panic.
The fourth isolates the stale-retention predicate by inserting a test
invalidated record at the exact height of the next finalized root, finalizing a
normal two-block chain, and confirming the record is still retained.

Service-level caveat: the subprocess proof uses a synthetic private testnet
with Canopy active at height 1 and fake semantically verified blocks so that the
test can keep block construction short while still exercising the full
state-service queue, async writer, invalidation, and automatic finalization
path. This weakens any claim about unauthenticated P2P reachability, but it
raises confidence that the trusted RPC/control-plane path is real rather than a
lower-level unit-test artifact.

Local proof status: the empty-chain panic is directly covered by a
current-behavior unit test, the trusted RPC to state-writer route is covered by
a subprocess service-level test, and the narrower stale-record retention
predicate is now isolated by a direct current-behavior test.

## Suggested Fix

- Change the retention predicate to keep only records strictly above the
  finalized height:

```rust
self.invalidated_blocks
    .retain(|height, _blocks| *height > best_chain_root.height);
```

- Apply the same strict-height cleanup in `reconsider_block()` when it prunes
  finalized invalidation records.
- In `finalize()`, only reinsert side chains after popping the shared finalized
  root if the side chain is still non-empty, matching the best-chain reinsertion
  check.
- Add a regression test that invalidates a side-chain block at the next
  finalizable height, finalizes the competing best-chain block at that height,
  and asserts the invalidated record at that height is removed.

## Confidence

Confidence: medium-high on the off-by-one retention behavior.

Confidence: high on the empty-side-chain finalization panic reproduced by the
local direct and service-level tests.

Confidence: medium-high on trusted-RPC/control-plane availability severity. The
relevant RPC methods are trusted control methods, the service-level proof uses a
synthetic private testnet, and current contextual validation still blocks
reintroducing finalized-height blocks. But the demonstrated failure is
process-fatal in release profiles and now reaches the async state writer through
normal state-service requests.
