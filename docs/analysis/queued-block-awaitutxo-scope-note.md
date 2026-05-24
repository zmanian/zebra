# Queued Block AwaitUtxo Scope Note

Date: 2026-05-03

Last updated: 2026-05-09

Disposition: local-only hardening. Do not post publicly without explicit
re-authorization.

## Summary

`Request::AwaitUtxo` can use UTXOs from semantically verified blocks that are
queued while waiting for missing parents. That queued UTXO cache is global across
the non-finalized block queue, not scoped to a particular parent chain.

This can influence block and proposal semantic verification: an unrelated block
can get a transparent previous output from a queued missing-parent block and use
it during transaction script and value checks. I did not find an acceptance
bypass. The later state commit/proposal path rebuilds spent UTXOs from the actual
parent chain plus finalized state and rejects spends that are not valid in that
context.

The mempool path is a strong elimination: mempool transaction verification uses
`UnspentBestChainUtxo` and optional mempool `AwaitOutput`, not `AwaitUtxo`, so
queued block UTXOs do not help mempool admission.

## Evidence

Queued missing-parent blocks expose their new transparent outputs through a
queue-global map:

- `zebra-state/src/service/queued_blocks.rs:54-80` inserts queued blocks by hash,
  height, parent, and stores all `new_outputs` in `known_utxos`.
- `zebra-state/src/service/queued_blocks.rs:210-214` returns UTXOs from that
  queue-global `known_utxos` map.

`AwaitUtxo` checks that queued cache before consulting committed state:

- `zebra-state/src/service.rs:1096-1114` queues the pending UTXO waiter, then
  returns immediately if `non_finalized_state_queued_blocks.utxo()` has the
  outpoint.
- `zebra-state/src/service.rs:1116-1124` next checks sent non-finalized blocks.
- `zebra-state/src/service.rs:1132-1162` only then asks the read service for
  `AnyChainUtxo`.

The block transaction-verification path uses `AwaitUtxo`:

- `zebra-consensus/src/transaction.rs:681-727` branches by request type and, for
  non-mempool block verification, calls `zebra_state::Request::AwaitUtxo` for
  unknown transparent prevouts.
- `zebra-consensus/src/transaction.rs:469-483` uses the resulting spent outputs
  to build the cached FFI transaction used for script verification.
- `zebra-consensus/src/transaction.rs:545-583` uses the same semantic-phase UTXO
  data for value balance, miner fee, and sigop accounting.
- `zebra-consensus/src/block.rs:343-354` builds a `SemanticallyVerifiedBlock`
  after transaction checks complete.

That semantic result does not carry the UTXOs returned by `AwaitUtxo`:

- `zebra-state/src/request.rs:238-263` defines `SemanticallyVerifiedBlock`; it
  includes the block, hash, height, new outputs, transaction hashes, and deferred
  pool balance, but no spent-output map.
- `zebra-state/src/request.rs:288-315` shows spent outputs only appear in
  `ContextuallyVerifiedBlock`.

The authoritative state path reruns transparent-spend lookup from the selected
parent chain:

- `zebra-state/src/service/write.rs:55-69` routes canonical non-finalized commit
  through `validate_and_commit_non_finalized()`.
- `zebra-state/src/service.rs:1655-1689` uses the same write validation path on a
  cloned non-finalized state for `CheckBlockProposalValidity`.
- `zebra-state/src/service/non_finalized_state.rs:551-607` calls
  `check::utxo::transparent_spend()` before constructing a
  `ContextuallyVerifiedBlock`.
- `zebra-state/src/service/check/utxo.rs:38-96` walks every transparent input,
  rejects duplicate/missing/early/invalid spends, and rechecks remaining
  transaction value.
- `zebra-state/src/service/check/utxo.rs:126-173` only accepts an outpoint if it
  is from an earlier transaction in the same block, an unspent UTXO in the
  selected non-finalized parent chain, or the finalized DB.
- `zebra-state/src/service/check/utxo.rs:66-74` explicitly documents the pending
  UTXO risk and says contextual validation checks against known-valid UTXOs.

## Mempool Elimination

The mempool path does not use `AwaitUtxo`:

- `zebra-consensus/src/transaction.rs:681-717` uses
  `zebra_state::Request::UnspentBestChainUtxo` for mempool transactions.
- `zebra-consensus/src/transaction.rs:736-767` then optionally waits on
  `mempool::Request::AwaitOutput` for unmined transaction dependencies.

Therefore queued missing-parent block outputs do not affect mempool transaction
acceptance, mempool script verification, or mempool dependency tracking.

## Impact

This is an availability/resource-hardening issue, not an invalid-chain
acceptance issue.

An attacker who can supply semantically valid, proof-of-work-valid
missing-parent blocks can seed `QueuedBlocks::known_utxos`. Those queued UTXOs
can help other blocks pass semantic transaction checks even if the spend is not
valid on that later block's actual parent chain. The later contextual state path
rejects the invalid block, but Zebra may do extra script/value/block verification
work and may queue additional semantically verified blocks before the rejection
is known.

The existing queue-retention findings amplify the operational angle:

- `docs/analysis/state-queued-block-timeout-retention-note.md` documents that a
  caller timeout or cancellation does not remove an already queued semantically
  verified block.
- `docs/analysis/queued-block-height-index-desync-note.md` documents a
  same-height, different-parent queue index desync that can make some queued
  blocks invisible to finalized-height pruning.

On default Mainnet/Testnet, the proof-of-work requirement is a strong practical
bound. This is sharper on custom/no-PoW networks or trusted/internal feeders that
can cheaply enqueue semantically verified missing-parent blocks.

## Suggested Hardening

- Treat queued-block UTXOs as hints only, and consider making the hint scope
  explicit in the `AwaitUtxo` response or metrics.
- For block verification, consider distinguishing UTXOs from committed chains
  from UTXOs found only in pending/queued blocks, then adding targeted tests that
  prove queued-only UTXOs cannot survive contextual commit.
- Keep the mempool path on `UnspentBestChainUtxo` and add a regression test that
  a mempool transaction spending a queued-block-only output is rejected or waits
  only on mempool dependencies.
- Fix the queued-block retention and height-index desync issues to reduce the
  life span of queued artifacts that can seed these hints.

## Confidence

Confidence: high that queued-block UTXOs can influence block/proposal semantic
verification, high that mempool admission is not affected, and high that the
normal state commit/proposal path reruns authoritative contextual UTXO checks
before state mutation.

Disclosure posture: local hardening. I do not see a private-disclosure
consensus or invalid-chain acceptance issue in this path.

## Finalized Checkpoint Queue Sibling

RepoPrompt recheck on 2026-05-09 found a sibling issue on the opposite side of
the same `AwaitUtxo` boundary: checkpoint-finalized queued outputs are not
visible to a waiter that arrives after the one-shot pending-UTXO check.

Current flow:

- `zebra-state/src/service.rs:1048-1070` handles
  `Request::CommitCheckpointVerifiedBlock`.
- `zebra-state/src/service.rs:1063-1064` calls
  `pending_utxos.check_against_ordered(&finalized.new_outputs)` before queueing
  the checkpoint block for finalized-state commit.
- `zebra-state/src/service.rs:1070` then queues the block in
  `finalized_state_queued_blocks`.
- `zebra-state/src/service.rs:1096-1124` handles a later
  `Request::AwaitUtxo` by checking pending waiters, the non-finalized queue, and
  sent non-finalized blocks.
- `zebra-state/src/service.rs:1126-1130` explicitly ignores
  `finalized_state_queued_blocks`, calling this a rare race and referencing
  issue #5126.
- `zebra-state/src/service.rs:1138-1164` falls back to `AnyChainUtxo` and then
  waits on the pending UTXO channel if the read service cannot see the output
  yet.

The narrow race is:

1. A checkpoint-verified block's outputs are checked against existing pending
   UTXO waiters.
2. The block is placed in the finalized commit queue.
3. A new `AwaitUtxo` request for one of that block's outputs arrives before the
   finalized read service can observe the commit.
4. The request ignores the finalized queue and can wait until caller timeout or
   retry, even though the output is already in-flight inside the state service.

This is distinct from the main queued-block scope note above. The earlier issue
is extra visibility from the non-finalized missing-parent queue; this sibling is
missing visibility from the finalized checkpoint queue.

Impact is best framed as checkpoint-boundary availability hardening. It is not
an invalid-chain acceptance issue, and a remote peer cannot forge trusted
checkpoint blocks. A remote peer may still influence sync timing and block
arrival order near the checkpoint/non-finalized boundary, so the bug can plausibly
show up as verifier waits, retries, or slow sync in that narrow window.

Suggested proof:

- Build a deterministic state-service harness that queues a
  `CheckpointVerifiedBlock` with a target transparent output.
- Issue `AwaitUtxo` after
  `pending_utxos.check_against_ordered(&finalized.new_outputs)` has run but
  before the finalized DB/read service can observe the output.
- Assert current behavior waits or times out, then fix by checking
  `finalized_state_queued_blocks` before falling back to `AnyChainUtxo`.

Confidence: high that the blind spot exists in current source, medium that it
can cause real verifier waits near checkpoints, and low-to-medium that it is a
standalone security report without a focused timing proof.

## Local Verification

Focused backstops rerun on 2026-05-09:

```sh
cargo test -p zebra-consensus dont_skip_verification_of_block_transactions_in_mempool --lib
cargo test -p zebra-state service::queued_blocks::tests::vectors --lib
cargo test -p zebra-state queued_utxo_lookup_is_global_across_parent_hashes_today --lib
```

Both commands passed. The consensus test supports the mempool-elimination side
of the note. The queued-block tests cover current queue map/pruning/dequeue
behavior, including the best-effort `known_utxos` bookkeeping. The focused
global-lookup test queues a block under one parent, verifies an unrelated parent
has no queued children, and still reads the queued output through
`QueuedBlocks::utxo()`, confirming that the lookup itself is not parent-scoped.

Duplicate checks on 2026-05-09:

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
