# State Queued Block Timeout Retention Note

Date: 2026-05-03

Last updated: 2026-05-09

Disposition: local-only hardening. Do not post publicly without explicit
re-authorization.

## Summary

The non-finalized state keeps semantically verified blocks with missing parents
in `QueuedBlocks` until their parents arrive or until finalized-height pruning
removes them. Sync and inbound callers wrap block verification in
`BLOCK_VERIFY_TIMEOUT`, but timing out the caller future does not remove the
already queued block from state.

Follow-up audit found a sharper cleanup issue in the same queue:
`QueuedBlocks::dequeue_children()` can desynchronize the `by_height` secondary
index for same-height queued blocks under different parents, making some
surviving queued blocks invisible to finalized-height pruning. See
`docs/analysis/queued-block-height-index-desync-note.md`.

This is not a default remote crash or consensus bug. It is a local
availability-hardening lead around retained memory after caller timeout or
cancellation. Practical exploitability on public networks is limited because a
block must pass semantic verification, including proof-of-work, before it can
reach this queue.

## Evidence

- `zebra-state/src/service/queued_blocks.rs:54-81` stores each queued block,
  indexes it by parent and height, and also stores its `new_outputs` in
  `known_utxos`. There is no local queue length cap.
- `zebra-state/src/service.rs:659-714` queues a semantically verified block
  before checking whether its parent is currently forkable.
- `zebra-state/src/service.rs:746-748` returns early when the parent is not
  forkable, leaving the block queued.
- `zebra-state/src/service.rs:1028-1039` then awaits the queued block's
  `oneshot` result; if the caller future is dropped by a timeout, the queued
  sender remains stored with the block.
- `zebra-state/src/service/queued_blocks.rs:131-184` prunes only by finalized
  tip height and explicitly ignores errors when sending to a dropped receiver.
- `zebra-state/src/service.rs:787-815` dequeues queued blocks only when their
  parent has arrived and can be sent to the non-finalized write task.
- `zebrad/src/components/sync.rs:141-173` defines `BLOCK_VERIFY_TIMEOUT`
  specifically for missing previous blocks and other stuck verification cases.
- `zebrad/src/components/sync.rs:495-496` wraps the sync verifier in that
  timeout, and `zebrad/src/components/inbound.rs:275-280` wraps the inbound
  block verifier in the same timeout.
- `zebrad/src/components/sync/downloads.rs:589-613` cancels downloader tasks
  during sync reset but does not clear the state service's queued blocks.

The inbound downloader comments say malicious blocks eventually timeout or fail
contextual validation and then have their memory deallocated:

- `zebrad/src/components/inbound/downloads.rs:31-49`

That is true for blocks that fail before state queueing, but it is not true for
a block that has already become a `SemanticallyVerifiedBlock` and is waiting in
state for a missing parent.

## Existing Bounds

- `zebra-consensus/src/block.rs:218-228` checks block difficulty and Equihash
  before contextual state commit on networks where proof-of-work is enabled.
- `zebra-consensus/src/block/check.rs:105-140` enforces the context-free
  difficulty filter against the block hash and difficulty threshold.
- `zebrad/src/components/inbound/downloads.rs:31-49` caps inbound gossiped
  block download/verify concurrency at `MAX_INBOUND_CONCURRENCY = 200` and
  enforces one in-flight inbound download per advertiser IP.
- `zebrad/src/components/inbound/downloads.rs:320-390` drops gossiped blocks
  that are too far above the tip or behind the finalized/reorg window.
- `zebrad/src/components/sync.rs:276-295` sets default full verification
  concurrency to 20.
- `zebrad/src/components/sync.rs:620-640` pauses new sync downloads when
  in-flight downloads are at or above lookahead limits.
- `zebrad/src/components/sync/downloads.rs:39-55` reserves extra capacity for
  verifier/state/write pipelines and makes the syncer time out/reset if those
  queues remain full.
- `zebrad/src/components/sync/downloads.rs:405-460` drops blocks above the
  hard lookahead drop height.
- `zebra-state/src/service/queued_blocks.rs:131-184` eventually prunes queued
  blocks at or below the finalized tip height when the state service reaches a
  path that calls pruning.

## Caller And Network Reachability

RepoPrompt reachability pass on 2026-05-09 did not find a cheap no-PoW route on
Mainnet or the default public Testnet. The normal untrusted caller paths that
can reach this state queue all converge through
`Request::CommitSemanticallyVerifiedBlock`:

| Path | Reaches shared queued-block state? | PoW required on Mainnet/default Testnet? | Notes |
| --- | --- | --- | --- |
| P2P inbound block gossip | Yes | Yes | `AdvertiseBlock` downloads the block body, calls consensus `Request::Commit`, then state `CommitSemanticallyVerifiedBlock`. |
| P2P sync | Yes | Yes | Same consensus/state path as inbound, with downloader reset/timeout machinery outside the state queue. |
| RPC `submitblock` | Yes | Yes | `submitblock` also calls consensus `Request::Commit`; if its future is cancelled after queueing, state still owns the queued entry. |
| RPC `getblocktemplate` proposal mode | No | N/A | Proposal validation routes to `ReadRequest::CheckBlockProposalValidity` and validates against a cloned non-finalized state without mutating the shared queue. |
| RPC `generate` | Indirectly through `submitblock` | No when allowed | `generate` is gated on `network.disable_pow()` and normally builds tip children, so it is not itself a missing-parent queue route. |
| Trusted indexer sync | No | Trusted path bypasses this queue | `TrustedChainSync` constructs `SemanticallyVerifiedBlock::with_hash()` but commits directly to a local `NonFinalizedState`; missing parents fail with `NotReadyToBeCommitted` rather than being queued. |

The no-PoW route is operator-selected network configuration:

- Regtest sets `disable_pow` through
  `Parameters::new_regtest(...).with_disable_pow(true)`.
- Custom Testnet config deserializes
  `[network.testnet_parameters].disable_pow` and passes it to
  `ParametersBuilder::with_disable_pow(disable_pow)`.
- The user docs explicitly advertise `disable_pow = true` for custom Testnets.

So the practical cheap-insertion variant is: Regtest or a custom Testnet with
`disable_pow = true`, plus a P2P or RPC path that submits a semantically valid
missing-parent block far enough through consensus to reach state queueing. This
is release-enabled but not a default Mainnet/default-Testnet exposure.

One follow-up proof caveat is now test-backed: the documented
`Request::KnownBlock` interface says it checks block queues, but the current
implementation does not report blocks retained in
`non_finalized_state_queued_blocks`. That means a full cross-crate RPC
`submitblock` cancellation proof cannot currently observe queued membership
through the state service's public request API without changing production
behavior. It also means upstream duplicate prechecks can miss blocks that are
already retained in the non-finalized validation queue, leaving the later queue
replacement path to handle the duplicate after verification work has already
been repeated.

## Impact

An attacker who can supply many semantically valid, proof-of-work-valid blocks
with missing parents can retain deserialized block data and queued UTXO data in
state after the inbound/sync verifier timeout fires. Because the caller future
times out but the queued block is not removed, this retention can last until
the parent arrives or until finalized-height pruning catches up.

On public Mainnet/default Testnet, the proof-of-work requirement is a strong
practical mitigation: this is closer to valid-work resource pressure than cheap
malformed input. The concern is sharper on Regtest and custom Testnet
configurations where proof-of-work is disabled, especially if P2P or RPC is
reachable by semi-trusted clients.

## Suggested Fix Direction

- Add cancellation/timeout cleanup for queued semantically verified blocks when
  the caller receiver is dropped, or periodically prune queued blocks whose
  result receiver has been abandoned.
- Make `Request::KnownBlock` match its documented contract by reporting blocks
  retained in `non_finalized_state_queued_blocks` as `KnownBlock::Queue`.
- Add a local metric or debug assertion for queued-block count versus the
  expected sync/inbound lookahead-derived bound.
- Consider making the state queue's cap explicit rather than relying only on
  upstream download/verifier limits.
- Update the inbound downloader comment so it does not imply all timed-out
  semantically verified missing-parent blocks are deallocated immediately.

Disclosure triage: local hardening. Keep it out of a private report unless a
cheap no-PoW path to `SemanticallyVerifiedBlock` queueing is found for a
network-facing default configuration.

Confidence: medium-high on the retention behavior from source review; medium-low
on default-network severity because exploitability is gated by proof-of-work,
height lookahead, concurrency limits, and eventual finalized-height pruning.
Confidence is medium for operator-selected no-PoW custom-network availability
impact because the config/docs expose `disable_pow = true`, but a practical
attack still needs a reachable P2P/RPC surface and missing-parent blocks that
pass the remaining semantic checks.

## Local Verification

Focused queued-block vector group rerun on 2026-05-09:

```sh
cargo test -p zebra-state service::queued_blocks::tests::vectors --lib
cargo test -p zebra-state dropped_semantic_commit_future_retains_queued_missing_parent_today --lib
cargo test -p zebra-state known_block_misses_queued_missing_parent_today --lib
```

Result: all passed. The vector group covers queued-block pruning/dequeue
invariants, including the related height-index desync test. It also includes
`queued_block_remains_after_result_receiver_is_dropped_today`, which queues a
semantically verified block with its result receiver already dropped and confirms
the block and its queued UTXOs remain until height pruning removes them. This is
not a full sync/inbound timeout harness, but it directly covers the queue-level
retention primitive that caller cancellation relies on.

The service-level test
`dropped_semantic_commit_future_retains_queued_missing_parent_today` drives the
real `StateService::call(Request::CommitSemanticallyVerifiedBlock)` boundary
with a post-mandatory-checkpoint missing-parent block. It confirms the block is
queued before the returned future is awaited, then drops the future and confirms
the queued entry remains. This upgrades the evidence from direct queue API
behavior to state-service caller-cancellation behavior, while still stopping
short of a full RPC/P2P timeout harness.

The additional service-level test
`known_block_misses_queued_missing_parent_today` queues the same kind of
missing-parent block, confirms it is retained internally, and then shows
`Request::KnownBlock(block_hash)` returns `KnownBlock(None)` today. This proves
the documented queue-visibility gap and explains why a non-invasive RPC-level
retention proof cannot directly observe queued membership through existing
state requests.

Duplicate checks on 2026-05-09:

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
separate thread, to avoid network and RPC hangs"). These cover sync/inbound
timeout behavior and state-writer history, but not this specific
missing-parent queued-block retention after caller cancellation.
