# RepoPrompt wide-search triage

Date: 2026-05-09

Status: local-only. Do not post publicly without explicit user direction.

## Summary

I used RepoPrompt as a wider-search assistant after the previous local audit
passes had already filled much of the ledger. The first shortlist recycled two
items that were already covered. I sent RepoPrompt a corrected follow-up with
those duplicate eliminations, then rechecked the replacement candidates against
the local notes and source.

Current result: no new high-priority or private-disclosure finding was promoted
from this pass. The only plausibly fresh item is a low-practicality
misbehavior-score overflow family. The other stronger-looking candidates reduce
to already-documented address-index, retry-queue, transparent spent-output, or
RPC boundary-arithmetic hardening notes.

## Candidate triage

### Misbehavior-score overflow before ban enforcement

RepoPrompt candidate: fresh low-severity arithmetic hardening.

Relevant code:

- `zebra-network/src/peer_set/initialize.rs` batches peer misbehavior reports in
  a `HashMap<PeerSocketAddr, u32>` and adds increments with raw `+=`.
- `zebra-network/src/meta_addr.rs` applies
  `MetaAddrChange::UpdateMisbehavior` by computing
  `previous.misbehavior_score + self.misbehavior_score()`.
- `zebra-network/src/address_book.rs` checks
  `updated.misbehavior() >= MAX_PEER_MISBEHAVIOR_SCORE` after score
  construction.

Why it is not a duplicate: the existing
`docs/analysis/misbehavior-reporting-lossy-channel-note.md` covers dropped
score-bearing reports on a full channel. It does not cover arithmetic overflow
in the batch accumulator or in `MetaAddrChange` application.

Practicality check:

- Current score producers found in `zebra-consensus` return either `100` or `0`.
- The ban threshold is `MAX_PEER_MISBEHAVIOR_SCORE = 100`, so ordinary score
  values should ban long before an accumulator approaches `u32::MAX`.
- The batcher flushes every 30 seconds. Reaching wraparound through normal
  remote behavior would require an extreme number of score-bearing reports in
  one flush interval.
- In debug/test builds, overflowing raw `u32` addition panics. In release builds,
  the score can wrap and suppress a ban if an impossible or already-corrupted
  large score is present.

Triage: fresh but low severity and low practical exploitability. Worth fixing
with saturating addition if touching peer misbehavior code, but not worth
reporting as a standalone vulnerability on current evidence.

Local proof added:

```sh
cargo test -p zebra-network misbehavior_update_addition_overflows_before_ban_today --lib
cargo test -p zebra-network misbehavior_batch_accumulator_overflows_before_flush_today --lib
```

Result on 2026-05-09: both passed. The first test covers the
`MetaAddrChange::UpdateMisbehavior` addition path with an existing
`misbehavior_score = u32::MAX` and `score_increment = 1`. It confirms debug
builds panic before ban enforcement and release builds wrap before the ban
check. The second test covers the peer-set batch accumulator helper with the
same `u32::MAX + 1` shape, confirming debug builds panic before a batched score
can flush and release builds wrap the pending batch value.

Detailed note: `docs/analysis/misbehavior-score-overflow-hardening-note.md`.

### Mempool `spent_outputs.is_empty()` standardness bypass

RepoPrompt candidate: validation-boundary bug in
`Storage::reject_if_non_standard_tx()`.

Relevant code:

- `zebrad/src/components/mempool/storage.rs` checks scriptSig size and push-only
  rules for every transparent input.
- It only checks `spent_outputs.len() == transaction.inputs().len()` and calls
  `policy::are_inputs_standard()` when `spent_outputs` is non-empty.
- If `spent_outputs` is empty, it treats the transaction as shielded-only or
  otherwise lacking spent outputs, and checks only legacy sigops.

Duplicate and reachability check:

- This is adjacent to
  `docs/analysis/transparent-spent-output-alignment-note.md`.
- That note already records the downstream length guard and, more importantly,
  the production verifier invariant: successful mempool verification fills the
  spent-output slot for every `PrevOut` input or returns
  `TransparentInputNotFound` before constructing `VerifiedUnminedTx`.
- `zebra-consensus/src/transaction.rs::spent_utxos()` preallocates one slot per
  input, fills slots by original input index, resolves mempool-created outputs,
  and returns an error if missing mempool outputs cannot be resolved.
- `VerifiedUnminedTx::new()` itself accepts a caller-supplied
  `spent_outputs` vector, and a synthetic storage test now manufactures the
  bypass shape. I did not find a normal remote production path that can hand
  mempool storage a transparent transaction with empty spent outputs.

Triage: not promoted as a fresh live vulnerability. It is a useful
defense-in-depth boundary check, but the live reachability question is already
covered by the transparent spent-output alignment note.

Proof added:

- `transparent_input_empty_spent_outputs_bypasses_input_standardness_today`
  inserts the same transparent-input transaction twice at the storage boundary:
  it rejects with supplied non-standard previous outputs, then accepts with
  `spent_outputs = []`.
- `cargo test -p zebrad transparent_input_empty_spent_outputs_bypasses_input_standardness_today --lib`
  passed on 2026-05-09.
- Detailed note:
  `docs/analysis/mempool-empty-spent-outputs-standardness-boundary-note.md`.

### Terminal-height arithmetic in address read helpers

RepoPrompt candidate: terminal-height panic / wraparound family in address-index
read helpers.

Relevant code:

- `zebra-state/src/service/read/address/tx_id.rs` computes
  `finalized_tip_range.start().0 + 1`.
- `zebra-state/src/service/read/address/utxo.rs` computes
  `finalized_tip_range.start().0 + 1`.
- `zebra-state/src/service/read/address/balance.rs` computes
  `(tip + 1).unwrap()`.

Duplicate and reachability check:

- The earlier address-index continuation note closed the overlap/assertion hunt
  for normal finalized/non-finalized state races.
- The broader `docs/analysis/rpc-boundary-arithmetic-hardening-note.md` already
  records `Height::MAX` next-height assumptions in RPC response paths.
- Current finalized storage bounds are far below `Height::MAX`, so this is not
  remotely reachable on current mainnet/testnet deployments.

Triage: local hardening sibling to the existing RPC boundary-arithmetic note,
not a fresh vulnerability. If these helpers are touched, replace terminal-height
`+ 1` assumptions with checked arithmetic and explicit "no child height" logic.

Local proof added:

```sh
cargo test -p zebra-state terminal_finalized_tip_panics_balance_overlay_in_debug_today --lib
cargo test -p zebra-state invalid_max_finalized_tip_panics_tx_id_overlay_in_debug_today --lib
cargo test -p zebra-state invalid_max_finalized_tip_panics_utxo_overlay_in_debug_today --lib
```

Result on 2026-05-09: all passed. The balance path panics in debug builds at
valid `Height::MAX` because it unwraps `(tip + 1)`. The txid and UTXO raw
`u32 + 1` paths do not panic at valid `Height::MAX`, because `Height::MAX` is
`u32::MAX / 2`; they only panic in debug builds if an invalid public tuple
height such as `Height(u32::MAX)` reaches the helper. Detailed note:
`docs/analysis/address-read-height-boundary-arithmetic-note.md`.

## Dead-end lanes confirmed

- `getaddressutxos` empty-result full-history txid lookup is already documented
  in `docs/analysis/rpc-address-index-query-bounds-note.md` and the local audit
  continuation.
- `sendrawtransaction` retry-queue spacing arithmetic is already ruled out as
  RPC-controlled. `NetworkUpgrade::target_spacing_for_height()` uses fixed
  protocol target-spacing constants; custom testnets configure activation
  heights, not arbitrary spacing.

## Recommendation

Do not file or privately report anything from this pass yet.

If we want to keep building evidence, the best next step is a narrow source pass
for default-reachable production paths that can violate internal constructor
invariants, especially places where a public constructor accepts data that the
normal verifier path carefully maintains. The synthetic mempool
`spent_outputs` case is a good pattern to search from, but it is not itself a
live remote finding without a production source.

## Follow-up after second RepoPrompt pass

A second RepoPrompt oracle pass against the active `Zebra audit leads` context
returned no high-confidence new non-overlapping candidate. It rejected the
address-book ban panic, peerset config overflow, lossy misbehavior transport,
address-index race/assert ideas, and RPC/state query-cap ideas as already
covered by earlier local notes.

### Constructor-invariant trust boundary recheck

I traced public `SemanticallyVerifiedBlock` / `CheckpointVerifiedBlock`
constructors and commit request wrappers after the first pass recommended
searching for default-reachable invariant bypasses.

Result: no new default-reachable vulnerability found.

- The normal full verifier constructs `SemanticallyVerifiedBlock` only after
  semantic validation in `zebra-consensus/src/block.rs`.
- `copy_state` uses `CommitCheckpointVerifiedBlock` for local state copy/import,
  not a remote boundary.
- `TrustedChainSync` constructs `SemanticallyVerifiedBlock::with_hash()` from a
  caller-supplied trusted indexer stream; that validation boundary is already
  tracked in `trusted-chain-sync-indexer-validation-boundary-note.md` and
  overlap #8821.
- Direct `CheckpointVerifiedBlock` construction and downstream value-pool
  effects are already proof-backed in
  `value-pool-error-suppression-note.md`.
- Checkpoint same-hash V5 auth replacement and the finalized-state backstop are
  already tracked in `checkpoint-auth-data-binding-note.md`.

Triage: keep as internal API hardening/documentation. No private or public
report from this recheck.

### `getaddr` response generation recheck

RepoPrompt suggested inbound `getaddr` response generation as the best next
default-reachable area because `fresh_get_addr_response()` clones, sanitizes,
filters, and shuffles the address book.

Result: already covered by `p2p-getaddr-response-amplification-note.md`.

- The normal non-empty cache path is not a per-request full-book scan: inbound
  `Request::Peers` uses `CachedPeerAddrResponse`, and repeated requests inside
  the 10-minute refresh interval clone/send the cached response.
- The note already records the residual empty-cache behavior: empty refreshes do
  not advance `refresh_time`, so repeated `getaddr` can retry the full refresh
  path on stale/isolated nodes with no gossipable peers.
- The same note already records per-request cached response cloning/egress, the
  1,000-address cap, the 5,000-address-book cap, and duplicate history
  #7823/#7955.

Triage: no new issue. Keep the existing local residual as the source of truth.

### Mechanical panic/assertion sweep

I also did a targeted `assert!` / `expect` / `unwrap` sweep across RPC, network,
inbound/mempool, and state read surfaces. The promising hits were duplicates or
internal-invariant paths:

- `FindBlockHashes` / `FindBlockHeaders` read helper assertions are already
  rechecked in the local continuation; live callers pass nonzero protocol limits
  and response length is a postcondition.
- Peer connection state-machine panics are already covered by the
  peer-connection sweep; malformed peer messages are handled as protocol errors
  or ignored messages rather than state corruption.
- `ChainTipChange::action()` asserts unchanged tips are filtered by
  `ChainTipSender::update()` before notification.

Triage: no fresh panic finding from this mechanical pass.

## Follow-up after third RepoPrompt slice

After the user started RepoPrompt locally, I ran a fresh audit slice focused on
lifecycle/shutdown, chain-tip/watch, peer discovery/backoff, config defaults,
and less-audited state/chain trust boundaries. RepoPrompt returned five leads:

- startup full-config logging of secret-bearing fields;
- misbehavior score loss on shutdown channel close;
- `CandidateSet::next()` marking peers `AttemptPending` before the dial attempt
  is actually consumed;
- `AwaitUtxo` missing checkpoint-finalized queued outputs;
- public checkpoint-verified commit APIs.

### Promoted local finding: full-config startup logging

Result: confirmed as a local/log-sink confidentiality issue in opt-in
Elasticsearch builds.

Detailed note:
`docs/analysis/config-debug-log-secret-disclosure-note.md`.

Short version:

- `zebrad/src/application.rs:470` logs `info!("{config:?}")` in server mode.
- `zebrad/src/config.rs:49-64` derives `Debug` for `ZebradConfig` and embeds the
  state config.
- `zebra-state/src/config.rs:24-26` derives `Debug` for state config.
- `zebra-state/src/config.rs:130-140` defines `elasticsearch_username` and
  `elasticsearch_password` under the opt-in `elasticsearch` feature.

This is not default-release remote exposure. It is a credential disclosure risk
for Elasticsearch-enabled deployments whose logs are readable or forwarded.

Local proof added:

```sh
cargo test -p zebrad --features elasticsearch debug_config_includes_elasticsearch_password_today --lib
```

Result on 2026-05-09: passed. The test constructs an Elasticsearch-enabled
`ZebradConfig` with a sentinel `state.elasticsearch_password`, formats it with
the current derived `Debug`, and confirms the sentinel appears in the formatted
config.

### Promoted local sibling: finalized checkpoint queued outputs and `AwaitUtxo`

Result: confirmed source blind spot, but kept local until a timing proof shows
security-grade availability impact.

Detailed sibling section:
`docs/analysis/queued-block-awaitutxo-scope-note.md`.

Short version:

- `CommitCheckpointVerifiedBlock` does a one-shot
  `pending_utxos.check_against_ordered(&finalized.new_outputs)` before queueing
  the checkpoint block.
- A later `AwaitUtxo` checks non-finalized queued and sent blocks, then
  explicitly ignores `finalized_state_queued_blocks`.
- If the read service cannot see the finalized commit yet, the waiter can hang
  until timeout or retry even though the output is already queued in state.

This is the inverse of the existing queue-global `AwaitUtxo` note: missing
visibility from the checkpoint finalized queue, not extra visibility from the
non-finalized queue.

### Checkpoint fast-path API recheck

Result: already covered as internal trust-boundary debt.

The public `CheckpointVerifiedBlock` constructors and
`Request::CommitCheckpointVerifiedBlock` do expose a privileged state fast path,
but current evidence still points to an in-process API footgun rather than a new
default-reachable remote issue. Existing notes already cover the concrete
downstream effects:

- `docs/analysis/value-pool-error-suppression-note.md`
- `docs/analysis/checkpoint-auth-data-binding-note.md`
- `docs/analysis/trusted-chain-sync-indexer-validation-boundary-note.md`

### Eliminated third-slice near-misses

- Misbehavior batch loss on channel close: real shutdown hygiene issue, but it
  happens during teardown and does not currently show a durable/security-grade
  ban bypass beyond the already tracked lossy misbehavior channel note.
- `CandidateSet::next()` pre-marks `AttemptPending`: real TODO, but the normal
  crawler immediately consumes the returned candidate for a dial task. I did not
  find a remote-triggerable cancellation loop outside shutdown/abort paths.
- Broadcast-all queue replacement for unready peers: can drop older mined-block
  gossip intents for peers that were already unready, but impact still looks like
  best-effort propagation reliability rather than a security vulnerability.

## Consensus/serialization RepoPrompt slice

Detailed note:
`docs/analysis/repoprompt-consensus-serialization-slice-2026-05-09.md`.

Result: no new private disclosure candidate promoted.

RepoPrompt returned four plausible candidates in the consensus/serialization
slice:

- trailing bytes after top-level `block` / `tx` payloads;
- transparent script truncated-length allocation;
- Orchard coinbase transactions with `nActionsOrchard > 0` and `ENABLE_SPENDS`
  rejected after Orchard bundle parsing;
- panic-prone block helper preconditions if invalid parsed blocks are routed
  into Merkle / network-upgrade helpers in the wrong order.

After duplicate and reachability filtering:

- trailing bytes are already covered by public issue #10569;
- transparent script allocation is already covered by public issue #10554;
- the Orchard coinbase shape is only bounded malformed-block parse work under
  current verifier ordering, because the transaction verifier rejects it before
  state, script, or proof verification;
- the missing-height block helper panic remains an internal API footgun, but the
  normal full-block and checkpoint paths preflight coinbase height first.

## RPC/indexer/watch RepoPrompt slice

Detailed note:
`docs/analysis/repoprompt-rpc-indexer-watch-slice-2026-05-09.md`.

Result: no new private disclosure candidate promoted.

RepoPrompt returned five plausible candidates in the RPC/indexer/watch slice:

- JSON-RPC browser-origin request forgery when cookie auth is disabled and the
  compatibility layer accepts `text/plain`;
- health endpoint probe starvation through the global accept counter;
- unauthenticated and unbounded tracing filter reload endpoint;
- `TrustedChainSync` malformed indexer-message trust and finalized-tip helper
  freeze;
- metrics endpoint idle-connection retention.

After duplicate filtering, all five mapped to existing local notes. I updated
`docs/analysis/health-endpoint-connection-hardening-note.md` to explicitly name
the health probe-starvation symptom, but did not promote a new finding from this
slice.

## Script / FFI primitive RepoPrompt slice

Detailed note:
`docs/analysis/repoprompt-script-ffi-primitive-slice-2026-05-09.md`.

Result: no new private disclosure candidate promoted.

RepoPrompt focused on script verification FFI, P2SH sigop accounting, and
primitive-boundary code. The main question was whether
`zebra_script::p2sh_sigop_count()` can be reached in production with mismatched
`spent_outputs`, causing the release-mode `zip()` to truncate and undercount.

After source and RepoPrompt cross-checking:

- successful non-coinbase verifier paths fill previous outputs by original
  input index before constructing `CachedFfiTransaction`;
- missing chain or mempool outputs return `TransparentInputNotFound` before the
  cached script transaction is built;
- `CachedFfiTransaction::is_valid()` has its own length and input-index guard;
- the only normal cached construction with mismatched lengths is coinbase, and
  the P2SH path returns `0` before the `zip()`;
- the direct `p2sh_sigop_count()` mismatch remains library-boundary hardening,
  not a current production-reachable disclosure-grade issue.

Keep the existing `script-ffi-safety-a6-revisit-note.md` and
`transparent-spent-output-alignment-note.md` as the source of truth unless a
future source-to-sink shows peer/RPC-controlled bytes reaching a production
script FFI or sighash panic, silent acceptance, or materially unbounded work
before rejection.

## Custom-network / checkpoint economics RepoPrompt slice

Detailed note:
`docs/analysis/checkpoint-subsidy-output-validation-gap-note.md`.

Result: one new local-only trusted-checkpoint/custom-network finding recorded.

RepoPrompt focused on custom-network checkpoint economics, NU6.1 lockbox
disbursements, funding streams, state commit/replay, and mining RPC economics.
The retained finding is distinct from the earlier lockbox-underflow defaulting
note: checkpoint verification derives a synthetic deferred-pool delta from the
schedule, but it does not validate that the actual coinbase outputs contain the
required non-deferred funding-stream outputs or NU6.1 lockbox disbursements.

Full semantic validation would reject those missing or redirected outputs through
`subsidy_is_valid()` and `miner_fees_are_valid()`. Checkpoint verification can
still finalize the block if its hash matches the trusted checkpoint list.

Triage: keep local-only. This is not an ordinary peer-triggered Mainnet/default
Testnet exploit; it requires a malicious or incorrect checkpoint set, or custom
network checkpoint control. It is still worth tracking because it is a real
full-validation versus checkpoint-validation discrepancy at an economic
accounting boundary.

Local proof added:

```sh
cargo test -p zebra-consensus checkpoint_check_block_accepts_missing_funding_stream_outputs_today --lib
```

Result on 2026-05-09: passed. The test constructs a custom proof-of-work-disabled
Testnet checkpoint verifier, mutates a funding-stream-era block so its coinbase
omits required funding-stream outputs, updates the Merkle root, confirms full
semantic subsidy validation rejects the block with `FundingStreamNotFound`, and
then confirms the checkpoint pre-check accepts the same block when the mutated
hash is trusted by the checkpoint list.

The same RepoPrompt pass also raised public custom-network builder subsidy
panics, but those map back to existing local notes:
`custom-network-parameter-panic-sweep-note.md` and
`configured-funding-streams-config-panic-note.md`.

## State persistence RepoPrompt slice

Detailed notes:

- `docs/analysis/indexer-spend-index-feature-toggle-migration-gap-note.md`
- `docs/analysis/invalidateblock-restart-replay-boundary-note.md`

Result: one new local-only indexer migration finding recorded, plus one
lower-confidence trusted-RPC restart-boundary note.

RepoPrompt focused on state persistence, finalized/non-finalized replay,
disk-format migration, read API consistency, and shielded/transparent index
persistence. It returned three candidates:

- arbitrary `history_tree()` fallback returning the finalized tip tree;
- interrupted indexer/non-indexer spend-index migrations corrupting transparent
  spend lookup state;
- non-finalized invalidated blocks being replayable after restart.

After source and duplicate filtering:

- The `history_tree()` fallback looks suspicious but is eliminated as a current
  vulnerability. Current production call sites request the current tip tree or
  fetch Sapling/Orchard trees by specific hash for `z_gettreestate`; I did not
  find an arbitrary RPC/indexer source-to-sink that lets a caller request an
  unknown history-tree key and receive the finalized tip tree as authoritative
  data.
- The spend-index migration finding is distinct from the previous address-index
  duplicate routing. The earlier note covered normal consistency assumptions in
  `track_tx_locs_by_spends`; this pass found the mixed-state sequence where a
  cancelled non-indexer drop globally removes transparent spent-output-location
  mappings, leaves some shielded nullifier entries in their old indexer format,
  and a later indexer rebuild skips a whole height after one surviving shielded
  probe resolves.
- The invalidated-block restart boundary is real at the source level, but lower
  confidence as a security finding because `invalidateblock` is a trusted RPC
  control-plane operation and Zebra may not intend invalidations to persist
  across restart. Keep it local until product expectations are clarified.

Local proof added for the re-receive route:

```sh
cargo test -p zebra-state fresh_non_finalized_state_forgets_invalidated_block_today --lib
cargo test -p zebra-state backup_restore_replays_invalidated_block_today --lib
```

Result on 2026-05-09: both passed. The first test confirms the live
non-finalized state rejects a previously invalidated block, then a freshly
constructed `NonFinalizedState` accepts the same block after committing the same
parent. The second test writes the stale non-finalized chain to the backup
cache, invalidates the child in memory without refreshing the cache, then
restores from backup and confirms the invalidated child is present again.

No public GitHub issue, comment, or advisory was posted from this slice.

## Manual continuation after RepoPrompt socket block

Timestamp: 2026-05-09 20:35:01 CEST.

After starting RepoPrompt, `rp-cli -e 'windows'` still failed from the Codex
sandbox with local socket permission denial. The automatic escalation retry was
rejected by the reviewer due the current usage limit, so this continuation used
ordinary local source inspection rather than the RepoPrompt socket.

Follow-up checks:

- V6 / NU7 transaction deserialization: still a duplicate/future-only boundary.
  The semantic verifier runs `consensus_branch_id()` before state/script/proof
  work, so V6 transactions carrying an older NU branch ID fail closed in normal
  transaction verification. The pre-verification `hash()` / `auth_digest()`
  panic family remains covered by public #10534 and the local routing ledger.
- `zebra-script` FFI: still eliminated beyond the known SIGHASH_SINGLE issue.
  The wrapper preflights input/output alignment, allowlists V5 hash-type bytes,
  and converts callback failures into the existing random-digest rejection path.
- RPC auth/body ordering: wrong-cookie requests still return before request-body
  collection. Valid-auth or auth-disabled requests retain the already-recorded
  bounded pre-guard body-collection / `text/plain` compatibility hardening
  issues.
- Anchor/nullifier reorg handling: still eliminated by existing A3 notes. The
  current source checks parent-chain plus finalized state, and rollback removes
  trees, anchors, and nullifiers symmetrically.
- Async/backpressure: the two best fresh local-only leads remain the Halo2
  weighted-admission mismatch and the `AdvertiseBlockToAll` stale-unready-peer
  lifetime issue. Direct source rechecks matched the local notes:
  `tower-batch-control` admits one queue permit per request while Halo2 flush
  weight is per Orchard action, and `PeerSet` queued broadcasts prune banned or
  ready peers but not removed/disconnected peers or closed receivers.

No public GitHub issue, comment, or advisory was posted from this continuation.

## Mempool standardness parser follow-up

Detailed note:

- `docs/analysis/mempool-standardness-parser-followup-note.md`

Result: no new live vulnerability promoted.

This local follow-up checked the P2SH/scriptSig standardness path after the
earlier `spent_outputs.is_empty()` candidate:

- `OP_1` through `OP_16` are represented by `zcash_script` as
  `Opcode::PushValue(PushValue::SmallValue(_))`, so Zebra's push counting does
  not miss small-number pushes.
- `Code::is_push_only()` intentionally accepts `PushSize(_)` and `OP_RESERVED`
  for zcashd parity, but malformed or oversize pushes are rejected by script
  evaluation before normal mempool storage receives a `VerifiedUnminedTx`.
- The empty previous-output vector bypass remains a synthetic storage-boundary
  hardening issue, not a fresh live remote finding, because the production
  transaction verifier fills previous-output slots by input index and
  `CachedFfiTransaction::is_valid()` rejects length mismatch.

Recommended hardening is still worthwhile: reject transparent `PrevOut`
transactions with empty `spent_outputs` at the storage boundary, and consider
making the push-count helper return an error on parse errors.

No public GitHub issue, comment, or advisory was posted from this slice.

## Disk-format upgrade / startup validation RepoPrompt slice

Detailed note:

- `docs/analysis/state-startup-validation-admission-barrier-note.md`

Result: one local-only startup hardening finding retained; one proposed V27
partial-read finding routed to existing B5/value-pool notes.

RepoPrompt focused on disk-format upgrades, version-file detection,
feature-toggle transitions, partial column-family deletion, and mixed old/new
state exposure. The clearest fresh source-level issue is that
`CheckOpenCurrent` and `Downgrade` state databases are opened, marked
`finished_format_upgrades`, and exposed to state-service reads before detailed
format validation finishes in the background. If a current-version database is
malformed in a way only detailed validation catches, Zebra can briefly serve
reads, and potentially accept finalized writes, before the checker panic is
observed.

This is not a peer-triggered private-disclosure candidate on current evidence.
It requires local malformed disk state or unusual cross-version reuse, and the
checker should eventually fail. The recommended hardening is to make
current-version and downgrade validation an admission barrier, or return an
explicit not-ready state until detailed validation succeeds.

Local proof added:

```sh
cargo test -p zebra-state check_open_current_marks_upgrades_finished_before_validation_panics_today --lib
```

Result on 2026-05-09: passed. The proof creates a current-version database with
raw block/header/transaction data but missing detailed-format data. Standalone
detailed validation fails while `finished_format_upgrades()` remains false, but
`CheckOpenCurrent::run_format_change_or_check()` marks
`finished_format_upgrades()` true before the validation failure is observed.

The V27 `BlockInfo` / address-received partial-read surface is real but already
recorded in the state-format migration B5 notes. The per-height migration batch
keeps same-height data atomic, but historical `BlockInfo` and received totals
can be incomplete while the background migration is still running.

No public GitHub issue, comment, or advisory was posted from this slice.

## ZIP-244 transparent sighash RepoPrompt slice

Detailed note:

- `docs/analysis/repoprompt-zip244-sighash-slice-2026-05-09.md`

Result: no new confirmed vulnerability beyond the already-disclosed V5
`SIGHASH_SINGLE` missing-corresponding-output issue.

RepoPrompt focused on transparent ZIP-244 sighash behavior outside the known
missing-output finding: raw hash-type bytes, callback fallback behavior,
`OP_CODESEPARATOR`, P2SH `script_code`, previous-output alignment, and
panic-on-invariant boundaries.

The pass retained audit gaps rather than a new reportable issue:

- fallible sighash API hardening would reduce reliance on panic-only
  precondition contracts;
- V5 `OP_CODESEPARATOR` and P2SH `script_code` parity need direct zcashd or
  upstream `zcash_script` fixtures;
- mixed state/mempool previous-output ordering appears fixed by input-indexed
  slot filling, but still deserves a regression test.

Malformed V5 hash-type acceptance and stale callback-buffer reuse remain
eliminated by current callback allowlisting, random fallback digest behavior,
and existing regressions.

No public GitHub issue, comment, or advisory was posted from this slice.

## Async / backpressure RepoPrompt slice

Detailed notes:

- `docs/analysis/repoprompt-async-backpressure-slice-2026-05-09.md`
- `docs/analysis/halo2-batch-queue-weight-admission-note.md`
- `docs/analysis/peer-set-advertiseblocktoall-stale-unready-note.md`

Result: two new local-only availability hardening notes recorded; no private
disclosure candidate promoted.

RepoPrompt focused on async backpressure, cancellation, timeout, and task
lifetime boundaries in `tower-batch-control`, `tower-fallback`, primitive
verifiers, `PeerSet`, RPC, and mempool background components.

The retained findings are:

- Halo2 batch queue admission is request-count based even though Halo2 flush
  weight is Orchard-action-count based.
- `AdvertiseBlockToAll` queued-unready state can keep a broadcast future pending
  after an originally unready peer disconnects or the caller drops interest.

The RPC server-listener hypothesis was demoted after checking jsonrpsee source:
`Server::start()` runs until its `ServerHandle` is stopped or dropped, and
Zebra's spawned waiter future owns the consumed handle. Aborting the task should
drop the handle and stop the server. The remaining issue in that path is the
already-recorded cookie cleanup lifecycle gap, plus the already-recorded
`submitblock` / proposal-mode `getblocktemplate` verifier timeout behavior.

No public GitHub issue, comment, or advisory was posted from this slice.
