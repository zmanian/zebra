# Local Audit Continuation - 2026-05-09

Status: local-only. Do not post public GitHub issues, PR comments, advisories,
or repo-facing comments from this note without explicit user direction.

## Scope

Continuation pass after the standing policy changed back to local-only issue
tracking. The goal of this pass was to avoid duplicate filing while checking
whether several fresh-looking source leads could be upgraded into unique
security findings.

## Results

### RPC `sendrawtransaction` retry queue

Result: duplicate / already bounded.

The retry queue still uses `CHANNEL_AND_QUEUE_CAPACITY = 20`, and both the
broadcast channel and retained retry queue are bounded. `Queue::insert()` evicts
the oldest entry when the queue exceeds the cap. Existing property tests cover
the size limit and queue ordering behavior:

- `zebra-rpc/src/queue.rs`
- `zebra-rpc/src/queue/tests/prop.rs`
- `docs/analysis/post-v4.4.0-security-audit-followups.md`
- `docs/analysis/post-v4.4.0-security-audit-continuation-2026-05-03.md`

This remains a bounded retryability-semantics hardening issue, not a fresh
unbounded memory or panic finding.

### Mempool eviction-list invariant panics

Result: duplicate elimination.

The `EvictionList` asserts that `unique_entries` and `ordered_entries` remain in
sync and that an already-evicted transaction is not inserted again. A new source
pass did not find a storage entry point that can insert the same mined ID into
the same list twice without first being blocked by rejection-cache checks or
without the older entry expiring/being evicted.

This matches the earlier elimination in
`docs/analysis/post-v4.4.0-security-audit-continuation-2026-05-03.md`.

### P2P gossiped-address unwraps and GetAddr cache

Result: duplicate / already ledgered.

The `MetaAddr` service-bit and last-seen unwrap candidates are already covered
by `docs/analysis/p2p-gossiped-address-services-unwrap-elimination-note.md`.
Remote `addr` and `addrv2` deserialization constructs gossipable addresses with
the expected service and timestamp fields before the candidate set unwraps them.

The cached `getaddr` empty-refresh behavior is also already captured in
`docs/analysis/p2p-getaddr-response-amplification-note.md` and backed by
`empty_getaddr_refresh_leaves_refresh_time_stale_today`.

### Serialization and protocol strictness

Result: no new cap bypass in this pass.

A targeted scan of `zebra-network/src/protocol/external` and `zebra-chain/src`
found the already-known classes: `TrustedPreallocate` caps are widely used, and
the remaining interesting strictness issues are the existing notes around
header/body reservation, counted headers with nonzero transaction counts, and
trailing bytes in `block` / `tx` messages.

No fresh uncapped vector allocation or new strictness mismatch was promoted in
this pass.

### Filesystem / local-material handling

Result: duplicate or low-value local threat model.

The RPC cookie file permission and lifecycle issues are already documented and
private-reported. The state-cache cleanup path explicitly avoids deleting
canonicalized paths outside the configured cache directory; the remaining
TOCTOU comment is a local attacker / elevated-privilege caveat and did not meet
the bar for a new Zebra vulnerability note in this pass.

### Optional internal miner

Result: duplicate of miner-RPC liveness family.

The internal miner still composes with `getblocktemplate` template construction
and `submit_block()` submission, so it inherits the known miner-facing timeout
and classification concerns. I did not find a separate miner-only externally
influenced panic or retained-state issue beyond the existing miner-RPC notes:

- `docs/analysis/submitblock-timeout-security-note.md`
- `docs/analysis/rpc-generate-uncapped-disabled-pow-note.md`
- `docs/analysis/gbt-custom-pre-canopy-panic-note.md`
- `docs/analysis/nu7-custom-activation-v5-serialization-panic-note.md`

### Amount arithmetic panics

Result: one new local sibling finding, plus one duplicate.

The panicking ZIP-235 `((fees * 6).unwrap() / 10).unwrap()` paths in block
validation and transaction builder code are duplicates of the existing
future-gated miner-fee-share overflow note:

- `zebra-consensus/src/block/check.rs`
- `zebra-chain/src/transaction/builder.rs`
- `docs/analysis/zip235-miner-fee-share-intermediate-overflow-panic-note.md`

The fresher result is a configured-Testnet `slow_start_interval` gap. Zebra
accepts custom `slow_start_interval` values, but does not validate that they
preserve funding-stream address-period assumptions or that early slow-start
subsidies remain exactly divisible by the founders reward denominator.

Proof tests were added locally:

- `configured_slow_start_interval_can_make_funding_stream_validation_panic_today`
- `configured_slow_start_interval_can_make_founders_reward_panic_today`

Verification:

```sh
cargo test -p zebra-chain configured_slow_start_interval_can_make --lib
```

Result: passed on 2026-05-09.

Full details are in
`docs/analysis/custom-network-parameter-panic-sweep-note.md`.

## Next Promising Local Passes

1. Revisit lower-confidence state/read-service race notes where current tests
   prove primitives but not full RPC/P2P reachability.
2. Focus on one proof upgrade at a time: either a no-PoW path into queued block
   retention, or a practical timeout path for mempool downloader cancel-handle
   retention.
3. Continue the arithmetic pass outside `Amount` itself, focusing on raw
   `usize` / `u64` sizing math in peer, mempool, and RPC paths.

## Follow-up: Sync Numeric Config Bounds

Result: new local low-severity config-hardening finding.

`ChainSync::new()` lower-bounds sync concurrency config values but does not
upper-bound them. A local proof now shows
`full_verify_concurrency_limit = usize::MAX` can make
`ChainSync::lookahead_limit()` panic in debug/test builds when crossing the
checkpoint/full-verification boundary:

- `zebrad/src/components/sync.rs`
- `huge_full_verify_concurrency_limit_can_overflow_lookahead_limit_today`

I also added a lower-level downloader proof for the same config family:
`Downloads::new(..., lookahead_limit = usize::MAX, ...)` can panic in its height
filter when there is no best tip yet, because it converts
`lookahead_limit - 1` to `u32` with `expect("fits in u32")`. Top-level
`ChainSync::new()` passes `max(checkpoint_verify_concurrency_limit,
full_verify_concurrency_limit)` into that downloader lookahead, so an oversized
checkpoint verifier limit reaches this primitive even though the earlier
`lookahead_limit()` proof used `full_verify_concurrency_limit`.

- `zebrad/src/components/sync/downloads.rs`
- `huge_lookahead_limit_can_panic_downloader_height_filter_today`

Verification:

```sh
cargo test -p zebrad huge_full_verify_concurrency_limit_can_overflow_lookahead_limit_today --lib
cargo test -p zebrad huge_lookahead_limit_can_panic_downloader_height_filter_today --lib
```

Result: both passed on 2026-05-09.

Full details are in
`docs/analysis/sync-concurrency-config-overflow-note.md`.

## Follow-up: Network Peerset Numeric Config Bounds

Result: new local low-severity config-hardening sibling.

`peerset_initial_target_size` is accepted as a `usize` and only zero-checked.
A TOML value of `9223372036854775807` parses on 64-bit targets, then
`Config::peerset_total_connection_limit()` overflows in debug/test builds while
deriving inbound and outbound connection limits.

Local proof:

- `zebra-network/src/config/tests/vectors.rs`
- `oversized_peerset_initial_target_size_overflows_connection_limits_today`

Verification:

```sh
cargo test -p zebra-network oversized_peerset_initial_target_size_overflows_connection_limits_today --lib
```

Result: passed on 2026-05-09.

Full details are in
`docs/analysis/network-peerset-config-overflow-note.md`.

## Follow-up: RPC / State Range Query Caps

Result: duplicate / already covered locally.

The fresh scan of RPC-to-state range and collection query paths did not produce
a new unique finding. The promising candidates are already tracked in local
notes and, where practical, backed by focused current-behavior tests:

- `getaddresstxids`, `getaddressutxos`, and `getaddressbalance` still lack
  method-level address-count, returned-item, and height-range caps, but this is
  already documented in `docs/analysis/rpc-address-index-query-bounds-note.md`.
- `z_getsubtreesbyindex` still treats explicit `start_index + limit` overflow as
  the omitted-limit unbounded suffix case, but this is already documented in
  `docs/analysis/rpc-subtree-limit-overflow-hardening-note.md`.
- `getnetworksolps` / `getnetworkhashps` still forward large positive
  `num_blocks` windows to state, but this is already documented in
  `docs/analysis/rpc-solution-rate-window-bounds-note.md` and overlaps the old
  public #6688 history.

No additional RPC/state collection path in this focused scan had a stronger
fresh shape than those existing notes. Keep this area local-only unless the user
explicitly asks to post or escalate one of the already-recorded items.

## Follow-up: Mempool Downloader Timeout Retention

Result: existing private-report finding remains high-confidence; no new public
action.

The timeout path still returns `Elapsed` without the transaction ID, so
`Downloads::poll_next()` cannot remove the corresponding `cancel_handles`
entry. The existing local tests already prove the important behavioral claims:

- sequential outer timeouts accumulate stale cancel handles while `in_flight()`
  returns to zero;
- a direct-pushed `Gossip::Tx` timeout retains the full transaction request;
- the service-level mempool path exposes the same retained pushed-transaction
  request and then rejects the same txid as `AlreadyQueued`.

Verification refreshed on 2026-05-09:

```sh
cargo test -p zebrad timed_out --lib
cargo test -p zebrad mempool_timeout_retains_pushed_transaction_request_today --lib
```

Both commands passed. Full details remain in
`docs/analysis/mempool-downloader-timeout-cancel-handle-retention-finding.md`.

## Follow-up: Indexer Idle Stream Retention Proof

Result: proof upgrade for an existing local-only indexer hardening note.

`ChainTipChange` now has a focused test showing that dropping the client response
stream does not promptly release the spawned stream task while the stream is
idle. The test then sends a tip event and confirms the task exits once it can
observe the closed response channel:

- `zebra-rpc/src/indexer/tests/vectors.rs`
- `dropped_chain_tip_change_stream_retains_task_until_tip_event_today`

`MempoolChange` has a focused test showing that dropping the client response
stream does not promptly release the server-side mempool broadcast subscription
while the stream is idle:

- `zebra-rpc/src/indexer/tests/vectors.rs`
- `dropped_mempool_change_stream_retains_subscription_today`

`NonFinalizedStateChange` now has the analogous state-listener proof: the test
answers the RPC task's `ReadRequest::NonFinalizedBlocksListener` with a local
listener, drops the client response stream, and confirms the state listener
receiver is still alive while the stream is idle:

- `zebra-rpc/src/indexer/tests/vectors.rs`
- `dropped_non_finalized_state_stream_retains_state_listener_today`

`MempoolChange` lag behavior is also proof-backed now. The test uses the
capacity-one test broadcast channel, sends two changes before yielding to the
stream task, and confirms the lagged stream exits with `Code::Unavailable` and
the same `"mempool_change channel has closed"` status text used for upstream
channel closure:

- `zebra-rpc/src/indexer/tests/vectors.rs`
- `lagged_mempool_change_stream_ends_as_unavailable_today`

Verification:

```sh
cargo test -p zebra-rpc dropped_chain_tip_change_stream_retains_task_until_tip_event_today --lib
cargo test -p zebra-rpc dropped_mempool_change_stream_retains_subscription_today --lib
cargo test -p zebra-rpc dropped_non_finalized_state_stream_retains_state_listener_today --lib
cargo test -p zebra-rpc lagged_mempool_change_stream_ends_as_unavailable_today --lib
```

Result: passed on 2026-05-09.

This upgrades the chain-tip, mempool-change idle retention,
non-finalized-state listener retention, and mempool-change lag portions of
`docs/analysis/indexer-idle-stream-disconnect-retention-note.md` from
source-evidence-only to local-proof-backed. Keep it local-only unless the user
explicitly asks to post or escalate it.

## Follow-up: TrustedChainSync Validation Boundary Proof

Result: proof upgrade for the existing local-only trusted-indexer validation
boundary note.

The `BlockAndHash` hash/body mismatch proof already covered the stream decoder.
A new lower-level non-finalized-state proof now covers the skipped recent-chain
height check: a block whose header points at finalized genesis but whose
coinbase height is 2 is rejected by the normal state write helper as
`NonSequentialBlock`, while direct `NonFinalizedState::commit_new_chain()`
accepts it and records it as the best tip at height 2.

This does not make the issue a default full-node consensus vulnerability.
`TrustedChainSync` is still an opt-in trusted-indexer mirror path. But it raises
confidence that the syncer’s direct call into the lower-level non-finalized
commit API really does skip at least one concrete `initial_contextual_validity`
recent-chain check.

Verification:

```sh
cargo test -p zebra-state lower_level_commit_skips_recent_chain_height_check_today --lib
```

Result: passed on 2026-05-09. Full details remain in
`docs/analysis/trusted-chain-sync-indexer-validation-boundary-note.md`.

## Follow-up: TrustedChainSync End-to-End Stream Proof

Result: upgraded the trusted-indexer validation boundary from component proofs
plus call-graph reasoning to live syncer proof.

Added two tests in `zebra-rpc/src/sync.rs`:

- `trusted_chain_sync_stream_propagates_transmitted_hash_today`
- `trusted_chain_sync_stream_accepts_nonsequential_child_of_genesis_today`

The tests stand up a local tonic indexer server and instantiate
`TrustedChainSync` directly, spawning only `sync()` so the finalized-tip
forwarder does not affect deterministic cleanup. The stream proof uses the same
V4 height-2 child-of-genesis fixture as the state-layer proof. With a mismatched
transmitted hash, the syncer publishes the transmitted hash in the non-finalized
mirror tip. With the actual header hash, the syncer publishes the
non-sequential height-2 child of finalized genesis.

Verification:

```sh
cargo test -p zebra-rpc trusted_chain_sync_stream_propagates_transmitted_hash_today --lib
cargo test -p zebra-rpc trusted_chain_sync_stream_accepts_nonsequential_child_of_genesis_today --lib
```

Result on 2026-05-09: both passed.

Triage unchanged: local-only trusted-indexer/read-state mirror issue, not a
default full-node consensus vulnerability. Confidence is now high for the live
gRPC stream-to-mirror composition.

## Follow-up: TrustedChainSync / Indexer Adjacent Sweep

Result: no stronger fresh sibling in this pass.

The `TrustedChainSync` best-tip forwarding task still has the same
source-evidence-only shape documented in
`docs/analysis/trusted-chain-sync-best-tip-forwarder-note.md`: it subscribes to
an upstream best-tip stream, exits when the streamed hash is not in the mirror's
finalized DB, and its helper `JoinHandle` is discarded. A focused runtime proof
would need a broader gRPC/read-state mirror harness and a way to observe the
discarded helper task after it exits, so this pass did not add a test for that
sibling.

The adjacent indexer stream paths did not produce a better new finding:

- `ChainTipChange` streams the current best tip as documented by
  `LatestChainTip`, so the finalized/non-finalized mismatch remains part of the
  trusted-sync helper issue rather than an independent indexer bug.
- `NonFinalizedBlocksListener::unwrap()` can panic if a listener is cloned
  before unwrapping, but the normal `ReadStateService` creates a fresh listener
  for the indexer RPC path; this stayed an internal API invariant rather than an
  externally reachable issue.
- Slow-consumer stream drops and idle stream retention are already covered by
  the existing indexer idle-stream note, with the mempool-change and
  non-finalized-state listener portions now proof-backed.

Keep the best-tip-forwarder note local-only and source-evidence-only unless a
future pass builds the larger mirror harness or the user explicitly asks to
prioritize it.

## Follow-up: RPC Performance Candidate Recheck

Result: no proof upgrade; one local note tightened.

`getrawmempool(true)` now has a focused proof for the high-confidence shape
documented in `docs/analysis/rpc-getrawmempool-verbose-quadratic-note.md`:
verbose assembly rebuilds a transaction-ID lookup map once per returned mempool
transaction. The new local test constructs eight non-coinbase mempool
transactions, builds one verbose object per transaction, and confirms eight
lookup-map builds over eight inputs each:

- `cargo test -p zebra-rpc verbose_mempool_object_rebuilds_lookup_for_each_transaction_today --lib`

The GBT dependency-DAG amplification note now has a focused proof of the local
scan shape. The source has an early `selected_txs.len() < deps.len()` return
before the selected-vector scan, so the expensive repeated-scan shape requires a
small direct-dependency set or enough unrelated selected transactions to make
the selected vector at least as large as the candidate's dependency set. The new
test constructs 12 unrelated selected transactions and a four-parent dependent
shape, then confirms each parent encounter scans the entire selected vector
after the length gate is satisfied:

- `docs/analysis/gbt-dependency-dag-selection-amplification-note.md`
- `cargo test -p zebra-rpc multi_parent_dependency_check_repeatedly_scans_selected_transactions_today --lib`

No public action. These remain local hardening candidates unless the user
explicitly asks to post or escalate them.

## Follow-up: Queued-Block Retention / Scope Recheck

Result: existing local-only queued-block findings still stand; no stronger
private-disclosure candidate found in this pass.

The same-height secondary-index desync remains proof-backed:

- `zebra-state/src/service/queued_blocks/tests/vectors.rs`
- `dequeue_drops_height_index_for_other_parents_today`

`QueuedBlocks::dequeue_children()` still removes the whole `by_height` bucket
for each dequeued child's height, so a same-height child under a different
parent can remain in `blocks` and `by_parent` while becoming invisible to
`prune_by_height()`. This strengthens the retention story, but it is still
availability hardening on default public networks because queue entry requires
semantic block verification, including proof of work.

The queued-only `AwaitUtxo` scope issue was also rechecked. The queue-global
UTXO map can influence block/proposal semantic verification, but normal state
mutation still goes through `validate_and_commit_non_finalized()`, which calls
`check::initial_contextual_validity()` and rebuilds transparent spends from the
selected parent chain plus finalized state. I did not find an invalid-chain
acceptance path in this pass.

Keep these local-only unless explicitly re-authorized:

- `docs/analysis/state-queued-block-timeout-retention-note.md`
- `docs/analysis/queued-block-height-index-desync-note.md`
- `docs/analysis/queued-block-awaitutxo-scope-note.md`

### Checkpoint verifier queued-block recheck

Result: duplicate-control / eliminated as a fresh cheap no-PoW retention path
on default networks.

I rechecked whether the checkpoint verifier provides a cheaper route into
queued-block retention than the semantic non-finalized queue. The important
distinction is that the checkpoint verifier queues below-checkpoint blocks, but
`CheckpointVerifier::check_block()` still verifies the coinbase height, block
difficulty, Equihash solution, funding-stream arithmetic, and Merkle root before
placing the block in `self.queued`.

Default Mainnet/Testnet therefore still require proof of work before a block can
occupy a checkpoint-queue slot. The queue also has explicit local bounds:

- `MAX_CHECKPOINT_HEIGHT_GAP = 400` keeps hard-coded checkpoint intervals small.
- `MAX_QUEUED_BLOCKS_PER_HEIGHT = 4` limits side-chain candidates at each
  queued height.
- The production block-verifier router is wrapped in a small Tower buffer, and
  sync/inbound callers add their own timeout/lookahead controls before reaching
  the verifier.

The real exposed liveness shape remains the already-recorded mining-RPC timeout
issue: `submitblock` and `getblocktemplate` proposal mode call the verifier
without the sync/inbound `BLOCK_VERIFY_TIMEOUT`, so an out-of-order checkpoint
block can leave the RPC future pending while the checkpoint verifier waits for a
contiguous range. That path is covered by
`docs/analysis/submitblock-timeout-security-note.md`, including a current
behavior test.

The residual edge is configuration/test-network hardening: if a custom network
or Regtest setup disables proof of work and exposes miner RPC, the checkpoint
queue becomes cheaper to fill, but the per-height/checkpoint-gap caps still
bound the verifier-local queue. I do not see a new default-network
private-disclosure candidate here.

Verification refreshed on 2026-05-09:

```sh
cargo test -p zebra-rpc rpc_submitblock_waits_without_timeout_for_out_of_order_checkpoint_block_today --lib
cargo test -p zebra-consensus checkpoint_drop_cancel_test --lib
```

Both commands passed.

## Follow-up: Primitive Verifier Failure Taxonomy Recheck

Result: no new acceptance or crash finding; keep as local hardening.

The primitive verifier stack still looks fail-closed: primary batch verifier
failures route through `tower_fallback`, and invalid or failed single-item
verification returns an error to transaction verification. The remaining issue
is still taxonomy and diagnostics stability, not invalid transaction acceptance:
some boxed primitive/script errors become
`TransactionError::InternalDowncastError`, and `AsyncChecks` returns whichever
independent async error completes first.

I also rechecked the mempool handling side. The focused backstop confirms that
an internal verifier error becomes an exact-tip rejection today rather than a
silent accept:

```sh
cargo test -p zebrad mempool_internal_verifier_error_is_exact_tip_rejected_today --lib
```

Result: passed on 2026-05-09.

Keep `docs/analysis/primitive-verifier-failure-taxonomy-note.md` local-only
unless explicitly re-authorized.

## Follow-up: Mempool Cascading Removal Notification Recheck

Result: existing local-only finding remains one of the cleaner public-hardening
candidates, but no public action without explicit direction.

The deterministic expiry side still reproduces: an expired mempool parent can
remove a non-expired dependent through dependency cascading, while
`remove_expired_transactions()` reports only the parent as expired. That means
RPC/indexer subscribers can miss the dependent invalidation if they build state
only from mempool change notifications.

Verification refreshed:

```sh
cargo test -p zebrad expired_parent_removes_unreported_non_expired_dependent_today --lib
```

Result: passed on 2026-05-09.

The insertion-time ZIP-401 false-added shape is now proof-backed. A new focused
test uses a test-only eviction-key hook to force the otherwise random
`evict_one()` choice to the parent of a newly inserted dependent. The real
`Storage::insert()` loop records only the selected eviction victim, returns
`Ok(child_id)`, and leaves the child neither stored nor cached as rejected after
the parent eviction cascades through `VerifiedSet::remove()`.

Additional verification:

```sh
cargo test -p zebrad evicted_parent_reports_dependent_inserted_today --lib
```

Result: passed on 2026-05-09.

Keep `docs/analysis/mempool-cascading-removal-notification-note.md` local-only
unless explicitly re-authorized.

## Follow-up: Observability / Metrics Cardinality Recheck

Result: existing local-only Prometheus/tracing findings remain test-backed; no
new private-disclosure issue found.

Refreshed the three focused current-behavior tests that cover the clearest
attacker-influenced metric paths:

```sh
cargo test -p zebra-network handshake_connected_metric_uses_remote_user_agent_label_today --lib
cargo test -p zebrad mempool_failed_verify_metric_reason_uses_raw_transaction_error_today --lib
cargo test -p zebra-rpc rpc_metrics_method_label_uses_raw_unknown_method_today --lib
```

All passed on 2026-05-09. I also refreshed the adjacent cancellation-gauge
backstop:

```sh
cargo test -p zebra-rpc rpc_active_requests_gauge_not_decremented_when_response_future_dropped_today --lib
```

Result: passed on 2026-05-09.

This keeps the observability severity where it was: public hardening for nodes
that enable metrics/telemetry and expose P2P, mempool, or RPC surfaces. It is
not a consensus or default crash issue. Keep these local-only unless explicitly
re-authorized:

- `docs/analysis/prometheus-cardinality-security-note.md`
- `docs/analysis/rpc-tracing-method-attribute-amplification-note.md`
- `docs/analysis/mempool-metrics-full-scan-amplification-note.md`

## Follow-up: RPC Tracing Method Attribute Proof

Result: upgraded tracing side from source-evidence-only to local-proof-backed.

`RpcTracingMiddleware` copies `request.method_name()` into the `rpc.method` span
attribute. The metrics sibling was already proof-backed, but the tracing note
still needed a subscriber harness. The new focused test installs a local
tracing subscriber layer, sends two different unknown JSON-RPC method names
through `RpcTracingMiddleware`, and confirms both unknown names are recorded
directly as `rpc.method` span attributes:

- `cargo test -p zebra-rpc rpc_tracing_method_attribute_uses_raw_unknown_method_today --lib`

This stays local-only and remains public observability hardening rather than a
private-disclosure candidate, because RPC is opt-in/authenticated by default and
telemetry export requires explicit runtime configuration.

## Follow-up: Optional HTTP Endpoint Hardening Recheck

Result: existing local-only endpoint notes remain proof-backed; no default
private-disclosure issue found.

Refreshed the health endpoint idle-connection proof:

```sh
cargo test -p zebrad idle_health_connection_waits_without_request_timeout_today --lib
```

Result: passed on 2026-05-09.

Refreshed the optional tracing filter endpoint large-body proof:

```sh
cargo test -p zebrad tracing_filter_endpoint_read_filter_accepts_large_body_today --features filter-reload --lib
```

Result: passed on 2026-05-09.

The metrics endpoint note remains source-evidence-only because the listener is
delegated to `metrics-exporter-prometheus` and a Zebra unit proof would require
mutating the global metrics recorder or wrapping the dependency listener. These
endpoint findings are still public hardening: disabled or feature-gated by
default, but meaningful for copied Docker/cloud/internal-probe deployments that
bind utility endpoints broadly.

Keep these local-only unless explicitly re-authorized:

- `docs/analysis/metrics-endpoint-connection-hardening-note.md`
- `docs/analysis/health-endpoint-connection-hardening-note.md`
- `docs/analysis/tracing-filter-endpoint-security-note.md`

## Follow-up: P2P Connection State Metric Labels

Result: no new reportable finding from the peer connection shutdown/state
metric sibling; keep as a hardening observation only.

I rechecked whether `zebra.net.connection.state` can be driven into
high-cardinality labels through peer response errors. The obvious candidate was
`PeerError::NotFoundResponse(Vec<InventoryHash>)`, because its `Display`
includes the missing inventory values. That error is request-scoped in
`Handler::Finished(Err(_))`: the handler state label uses `error.kind()`, and
the request response is sent back to the internal client without closing the
connection. Existing focused connection tests also confirm unrelated
`notfound` messages complete the active block/transaction request but leave the
shared connection error slot unset.

The connection shutdown path still calls `update_state_metrics(error.to_string())`
after the event loop exits, so it would be better if it used a normalized error
kind. However, the remote-driven shutdown variants found in this pass are fixed
strings (`ConnectionClosed`, `DuplicateHandshake`, overload/timeouts) or
serialization errors with finite/static parse messages from
`SerializationError`. I did not find a direct path where a remote peer can place
arbitrary inventory hashes or user-controlled strings into the shutdown metric
label.

This means the previously confirmed Prometheus cardinality findings remain the
important observability items. This sibling should stay local-only unless a
future pass finds a concrete remote-controlled `SerializationError::Io` message
or another dynamic shutdown error source.

## Follow-up: Mempool Metrics Full-Scan Proof

Result: upgraded successful-insert growth and independent-removal shrink shapes
from source-evidence-only to local-proof-backed.

`VerifiedSet::update_metrics()` recomputes bucketed mempool metrics by iterating
over every stored transaction after successful inserts and removals. The new
focused test inserts five accepted transactions into eviction-free storage and
confirms the metrics path runs once per insert and visits `1 + 2 + 3 + 4 + 5`
stored transactions while the verified set grows. The sibling removal test
removes the same five independent transactions through `Storage::remove_exact()`
and confirms another five metric recomputations over `4 + 3 + 2 + 1 + 0`
remaining transactions while the verified set shrinks:

- `cargo test -p zebrad mempool_insert_recomputes_metrics_over_growing_set_today --lib`
- `cargo test -p zebrad mempool_remove_recomputes_metrics_over_shrinking_set_today --lib`

This stays local-only and remains public availability hardening, not private
disclosure. The work is bounded by mempool cost limits and accepted transaction
cost, but it is avoidable hot-path metric accounting.

## Follow-up: Value-Pool `getblockchaininfo` Chain-Supply Sibling

Result: new proof-backed sibling to the existing local/private value-pool
accounting note; not a standalone direct-RPC finding on current evidence.

`GetBlockchainInfoBalance::chain_supply()` assumes the sum of all non-negative
chain value pools cannot overflow `Amount<NonNegative>`, but
`ValueBalance<NonNegative>` only proves each individual pool is non-negative.
It does not encode the cross-pool total-supply invariant. A state value with
three individually valid half-`MAX_MONEY` pools therefore panics while
constructing the `getblockchaininfo` `chain_supply` field.

Local proof added and run:

```sh
cargo test -p zebra-rpc chain_supply_panics_when_pool_sum_exceeds_max_money_today --lib
```

Result: passed as a `#[should_panic]` current-behavior test on 2026-05-09.

This is most useful as supporting evidence for
`docs/analysis/value-pool-error-suppression-note.md`: if value-pool
error-suppression, migration replay fallback, checkpoint replay, or local DB
corruption produces cross-pool-invalid state, an ordinary
`getblockchaininfo` call can become process-fatal. I did not find a direct path
where a remote RPC caller can choose these pool values, so keep this local-only
and bundled with the value-pool accounting issue unless maintainers ask for
separate detail.

## Follow-up: Address-Index Read Assertions

Result: no new address-index panic finding in this pass.

I rechecked the `getaddresstxids` / `getaddressutxos` path after the broad
range-cap note, focusing on the raw `unwrap()` and `assert!()` sites in:

- `zebra-state/src/service/read/address/tx_id.rs`
- `zebra-state/src/service/read/address/utxo.rs`
- the RPC response ordering assertions in `zebra-rpc/src/methods.rs`

The state helpers unwrap the optional non-finalized chain only after checking
that a chain exists, and the "empty finalized state must not have a
non-finalized chain" assertion depends on the state initialization invariant
rather than direct RPC input. The overlap assertions are reached only after the
helpers have checked that the required finalized/non-finalized range is present.
The RPC ordering assertions consume `BTreeMap`-ordered state results, so a
normal internal state response cannot reorder them through the public RPC
parameters.

This leaves the existing address-index issue unchanged: missing address-count,
height-range, and returned-item caps are still the meaningful local hardening
item. I did not find a fresh process-fatal RPC path here.

## Follow-up: RPC Queue and Cookie/Auth Parser Recheck

Result: no new RPC queue or cookie-auth bypass finding in this pass.

The `sendrawtransaction` retry queue has a few timing `expect()` calls, but they
operate on consensus target-spacing durations and a fixed 20-entry queue. I did
not find an RPC-controlled way to make `spacing.to_std()` or the queue expiry
time overflow. The existing retry semantics stay bounded by
`CHANNEL_AND_QUEUE_CAPACITY`, and broader mempool queue behavior is already
covered by the mempool downloader and exact-tip rejection notes.

I also rechecked `HttpRequestMiddleware::check_credentials()`. The parser is
permissive: it does not require the authorization scheme string to be exactly
`Basic`, and it ignores the username portion after Base64 decoding. However, it
still requires the random cookie password to match `Cookie::authenticate()`.
That makes it protocol-strictness hardening rather than an authentication bypass
on current evidence. The stronger cookie-auth issues remain the already-recorded
local credential lifecycle and existing-file permission findings.

## Follow-up: P2P Inbound Routing Recheck

Result: no fresh P2P inbound request finding in this pass; the interesting
paths map to existing local notes or already-filed public issues.

I rechecked the `zebra-network` message-to-request boundary and the
`zebrad::components::inbound` request dispatcher, looking for request types that
force expensive state, mempool, or response construction before a local cap is
applied.

Routing summary:

- `Request::Peers`: already covered by
  `docs/analysis/p2p-getaddr-response-amplification-note.md`. The global cache
  avoids full address-book rescans on normal non-empty responses, but the
  empty-refresh residual and per-request cached response clone/send work remain
  local public-hardening items.
- `Request::BlocksByHash`: has the strongest cap in this family. Inbound uses
  `GETDATA_MAX_BLOCK_COUNT` before state lookups and also stops once
  `GETDATA_SENT_BYTES_LIMIT` is reached. I did not find a fresh pre-lookup block
  amplification issue here.
- `Request::TransactionsById`: already covered by
  `docs/analysis/p2p-getdata-transaction-request-amplification-note.md` and
  public issue #10566. Zebra forwards the full requested transaction-ID set to
  mempool before response-size trimming.
- `Request::FindBlocks` / `Request::FindHeaders`: already covered by
  `docs/analysis/p2p-block-locator-length-hardening-note.md` and public issue
  #10549. State caps the response, but the request locator can still be large
  before the chain-intersection scan.
- `Request::PushTransaction`: one full transaction reaches the mempool queue.
  The remaining findings in this lane are the existing mempool downloader
  timeout-retention, direct-push attribution, metrics-label, and eager full-`tx`
  decode notes; I did not find a new routing-layer cap bypass.
- `Request::AdvertiseTransactionIds`: already covered by
  `docs/analysis/p2p-transaction-inv-queue-amplification-note.md`; the V5
  same-effects sibling is covered by
  `docs/analysis/mempool-v5-same-effects-pending-amplification-note.md` and
  public issue #10565. The current behavior is proof-backed by local tests, but
  it is not fresh.
- `Request::AdvertiseBlock`: the downloader deduplicates by block hash, caps
  pending gossiped block downloads, and enforces one in-flight download per
  advertiser IP. The missed misbehavior-scoring and eager full-block decode
  siblings are already recorded in
  `docs/analysis/inbound-gossiped-block-router-error-misbehavior-note.md` and
  `docs/analysis/p2p-unsolicited-block-decode-hardening-note.md`.
- `Request::MempoolTransactionIds`: already covered by
  `docs/analysis/p2p-mempool-request-enumeration-note.md`. The connection layer
  caps the outbound `inv`, but only after the mempool service materializes the
  full local ID set.

The duplicate map is now clearer than the first-pass scan: the inbound surface
still has multiple worthwhile availability-hardening candidates, but this
recheck did not produce a unique, higher-priority vulnerability beyond the
existing local ledger. Keep this local-only unless the user explicitly selects
one of these candidates for public reporting.

## Follow-up: Peer/Count Arithmetic and Discovery Recheck

Result: no fresh peer-discovery or raw-count arithmetic finding in this pass.

I rechecked the places where peer-controlled counts, address payload lengths,
inventory status, and peer-set readiness can grow state or steer routing before
Zebra applies local limits. This pass was mostly duplicate control.

Routing summary:

- `addrv2` parsing: eliminated as a fresh allocation issue in this checkout.
  `AddrV2` advertises `MAX_ADDRS_IN_MESSAGE` as its trusted preallocation cap,
  `read_addrv2()` rejects address byte strings over `MAX_ADDR_V2_ADDR_SIZE`, and
  the codec rejects `addr` / `addrv2` messages with more than
  `MAX_ADDRS_IN_MESSAGE` entries. The remaining `addrv2`-adjacent items are the
  existing service-bit unwrap and address-gossip sanitation notes.
- Address-book response growth: already covered by
  `docs/analysis/p2p-getaddr-response-amplification-note.md` and the new inbound
  routing recheck above. The interesting residual is response construction and
  cache refresh behavior, not an uncapped parser count.
- `notfound` / inventory routing: already covered by
  `docs/analysis/p2p-inventory-routing-poisoning-note.md` and
  `docs/analysis/p2p-notfound-request-correlation-issue.md`. The registry is
  size-bounded and time-rotated; the real issue is request-correlation, where
  unsolicited or unrelated `notfound` can influence missing-inventory state or
  complete an active request.
- Peer-set unready retention: already covered by
  `docs/analysis/p2p-peer-set-unready-availability-note.md`. This remains an
  availability-hardening item around continuously unready services, not a newly
  discovered unbounded map or arithmetic overflow.
- Stale gossiped address churn: already covered by
  `docs/analysis/p2p-stale-gossiped-address-dial-churn-note.md`. I did not find
  a stronger same-lane issue than repeated connection-attempt waste from stale
  or polluted candidate addresses.
- Ban-watch / lazy disconnect: already covered by
  `docs/analysis/peer-set-ban-watch-lazy-disconnect-note.md` and related
  address-book cleanup notes. The current residual is delayed cleanup until the
  peer set is next polled, not a new ban bypass proof.

This reinforces the current local-only posture: these paths are useful
background for future hardening, but this pass did not add a unique,
higher-confidence public candidate. Do not post from this section without
explicit user direction.

## Follow-up: Error-Suppression, Empty-Locator, and Shielded-Constructor Recheck

Result: no fresh unique finding in this pass.

I rechecked silent/defaulting paths and panic-looking constructors after the
peer/count sweep. The goal was to avoid missing a sibling to the value-pool
`flat_map(Result)` bug or the Sapling `TransmissionKey` public API panic.

Routing summary:

- `Block::chain_value_pool_change()` remains the primary live
  error-suppression finding. The other `unwrap_or_default()` / `.ok()` sites I
  rechecked either route to the existing value-pool note, the existing
  defaulting-error recheck, or non-security response-shaping behavior.
- `ReadRequest::BlockLocator` maps a missing locator to an empty vector via
  `read::block_locator(...).unwrap_or_default()`. `ChainSync::obtain_tips()`
  later expects at least one locator hash, but the normal sync loop calls
  `request_genesis()` before `obtain_tips()`, so an empty fresh state is guarded
  by the explicit genesis-download path. I did not promote this as a remotely
  triggerable sync panic.
- `FindBlocks` / `FindHeaders` state lookup still maps to the existing
  block-locator length and junk-response steering notes. I did not find a new
  state-side error defaulting behavior beyond those already-recorded P2P
  availability items.
- Orchard/Pallas public constructors checked in
  `zebra-chain/src/orchard/{note,commitment,keys,note/nullifiers}.rs` generally
  follow the safer pattern: call `from_bytes()` / `from_repr()`, check the
  `CtOption`, then unwrap only after confirming it is present. This does not
  reproduce the Sapling `TransmissionKey::try_from([u8; 32])` panic shape.
- Sapling `EphemeralPublicKey` also checks the `CtOption` before unwrapping.
  The known remaining Sapling public API panic is still
  `docs/analysis/sapling-transmission-key-public-api-panic-note.md`.
- Generic allocation/reserve sites mostly route to existing notes:
  P2P body reservation, transaction/getdata response preallocation, inbound
  transaction `inv` queue work, address-index RPC vectors, batch RPC request
  parsing, and getrawmempool/template construction costs.

This pass is therefore duplicate-control only. It does not justify a public
issue or maintainer ping without a new explicit user direction.

## Follow-up: TODO / Deferred-Security Comment Sweep

Result: no fresh unique finding in this pass.

I swept security-sensitive `TODO`, `FIXME`, and "temporary / unsupported" style
comments in the consensus, state, network, RPC, and zebrad component crates. The
goal was to catch intentionally deferred checks that had not already been
captured in the local ledger.

Routing summary:

- `zebra-chain/src/block/commitment.rs` asks whether exposing `expected` and
  `actual` commitment roots in `CommitmentError` is a security risk. I did not
  promote this: these are block-validation diagnostics over public commitment
  material, and the nearby consensus-relevant commitment gaps are already
  covered by the pre-Heartwood Sapling-root checkpoint note and NU5+
  block-commitment validation paths.
- `zebra-consensus/src/transaction/check.rs` still has a TODO saying a
  coinbase output plaintext `0x01` lead byte is allowed during the ZIP-212
  grace period because of librustzcash behavior. The current helper in
  `zebra-chain/src/primitives/zcash_note_encryption.rs` now passes
  `Zip212Enforcement::On` from Canopy onward, so this appears to be stale
  commentary or already addressed by the dependency path rather than a fresh
  consensus under-enforcement finding.
- Mixed `inv` and `getdata` handling in
  `zebra-network/src/peer/connection.rs` remains a protocol-cleanup TODO, but
  I did not find a stronger vulnerability than the existing local notes.
  Transaction inventories are collected into `HashSet`s before inbound work,
  block `getdata` is capped before lookup, and the transaction `getdata`
  pre-lookup work is already covered by
  `docs/analysis/p2p-getdata-transaction-request-amplification-note.md`.
- Response-list uniqueness TODOs in
  `zebra-network/src/protocol/internal/{request,response}.rs` mostly describe
  type-shape cleanup. The relevant live behaviors are already captured as
  request-correlation, request-size, and response-materialization notes:
  `notfound` correlation, block-locator length, transaction `getdata`,
  mempool request enumeration, and transaction `inv` queue amplification.
- `zebra-state/src/service/check/anchors.rs` marks Sprout anchor checking as
  expensive for attacker-controlled mempool transactions. The code already
  moves the work into a Rayon scope and the remaining broader primitive-worker
  failure taxonomy is tracked in
  `docs/analysis/primitive-verifier-failure-taxonomy-note.md`.
- The queued-block `known_utxos` cleanup TODOs in
  `zebra-state/src/service/queued_blocks.rs` are already represented by the
  queued-block family:
  `docs/analysis/queued-block-awaitutxo-scope-note.md`,
  `docs/analysis/queued-block-height-index-desync-note.md`, and
  `docs/analysis/state-queued-block-timeout-retention-note.md`.
- The mempool rejected-list cleanup TODO in
  `zebrad/src/components/mempool/storage.rs` routes to the existing exact-tip
  rejection family, especially infrastructure errors being recorded as
  verification failures. I did not find a distinct cleanup-bypass proof in this
  pass.
- `zebra-chain/src/primitives/zcash_primitives.rs` still has public/internal
  precondition panics around sighash and conversion helpers. The V5
  `SIGHASH_SINGLE` issue, V5 parser backstop, and V6 auth-digest panic notes
  already cover the interesting transaction-version/conversion edges I saw.

Conclusion: this pass improved duplicate routing, but did not add a unique
finding worth public filing. Keep this local-only unless the user explicitly
chooses one of the already-recorded candidates for disclosure or public issue
work.

## Follow-up: Mempool Dependency-Depth Policy Recheck

Result: duplicate of existing mining-RPC and mempool-dependency notes.

I rechecked the open plan item about whether Zebra caps unconfirmed ancestor or
descendant chains in the mempool. The answer in this checkout is still "not by
explicit depth or fanout policy"; the effective bounds are the total mempool
cost/size limits, per-transaction standardness checks, transaction-size limits,
block-template limits, and conflict tracking.

Source notes:

- `TransactionDependencies` stores direct dependencies and direct dependents,
  and its `add()` method does not enforce maximum ancestor depth, descendant
  count, or total dependency edges.
- `VerifiedSet::insert()` only checks for spend conflicts and verifies that
  spent mempool outpoints already exist in `created_outputs` before recording
  dependency edges. This makes ordinary accepted dependency graphs acyclic, but
  it does not impose a zcashd-style chain-depth cap.
- Same-input / same-nullifier replacement is not implemented as RBF. Once a
  transaction is accepted, `has_spend_conflicts()` blocks same-effects conflicts
  in storage. The already-filed V5 pending-window issue covers the sharper
  before-acceptance case where distinct same-effects witnessed IDs can occupy
  concurrent verifier slots.
- `getblocktemplate` dependency selection still walks direct dependents and
  scans selected transactions for each dependency readiness check. That is
  already captured in
  `docs/analysis/gbt-dependency-dag-selection-amplification-note.md`.
- Verbose `getrawmempool(true)` still reports direct dependents rather than
  transitive descendants. That is already captured in
  `docs/analysis/rpc-verbose-response-shape-correctness-note.md`.
- Dependency-removal notification gaps are already covered by
  `docs/analysis/mempool-cascading-removal-notification-note.md`.

Conclusion: the missing explicit ancestor/descendant policy is real, but I did
not find a new exploit shape beyond the existing local ledger. It remains
public/local hardening evidence, not a private disclosure candidate on current
evidence, and should not be posted without explicit user direction.

## Follow-up: Storage Migration / Multi-CF Recheck

Result: duplicate of the existing B5 state-format note and value-pool replay
finding.

I rechecked the storage-format and multi-column-family lane from the pass-5
plan, focusing on version-marker ordering, restart behavior, v27 `BlockInfo`
replay, background upgrade exposure, and whether multi-CF writes can leave
consensus-sensitive data in a torn or contradictory state.

Source notes:

- The dedicated B5 note already covers this lane:
  `docs/analysis/state-format-migration-b5-revisit-note.md`.
- The pass-5 findings file classifies the B5 storage/migration item as
  eliminated for a fresh private disclosure, with the caveat that v27
  `BlockInfo` replay strengthens the existing value-pool error-suppression
  concern.
- The upgrade framework runs each migration's `prepare()`, `run()`, and
  `validate()` before advancing the on-disk format-version marker, so I did not
  find a new partial-upgrade-as-complete path.
- The checked v27 block-info/address-received path writes each height's derived
  `BlockInfo` and address-balance updates with a `DiskWriteBatch`, so this did
  not produce a new multi-CF torn-write finding.
- Live finalized block commits use merge operands while upgrades are unfinished,
  which avoids the most direct race where normal sync overwrites migration
  address-balance writes.
- The meaningful residual issue is still that the v27 replay migration calls
  `Block::chain_value_pool_change(...).unwrap_or_default()` and can resume from
  existing non-default `BlockInfo` as an authoritative cumulative baseline.
  That belongs with the already-recorded value-pool error-suppression finding,
  not a separate new B5 report.
- A secondary operational caveat also remains local/hardening: the database
  handle can be returned while the background format-change task is still
  deriving historical metadata, so historical `BlockInfo` / received-balance
  reads can observe missing or partial derived data until the relevant upgrade
  reaches those heights.
- Disk decode panics in `FromDisk` paths still look like malformed local RocksDB
  bytes or manual/test corruption hazards. I did not find a valid peer or RPC
  input path to create those malformed bytes.

Conclusion: no fresh unique vulnerability in this lane. Keep routing B5
questions to `docs/analysis/state-format-migration-b5-revisit-note.md` and
`docs/analysis/value-pool-error-suppression-note.md`. Suggested fixes remain:
fail migration replay loudly on value-pool calculation errors, recompute or
sample-check exact historical `BlockInfo` during validation, and avoid exposing
derived read semantics before the relevant upgrade has completed.

## Follow-up: Remaining Abort / Error-Suppression / Checkpoint Recheck

Result: no fresh unique finding in this pass.

I did a narrow recheck of three bug classes after the storage lane:

1. externally influenced abort surfaces that were not already in the RPC/state
   panic notes;
2. error-suppression patterns such as `unwrap_or_default()`, `.ok()`, and
   `flat_map(Result)`;
3. checkpoint-vs-semantic verification divergence, with a short adjacent check
   for whether the custom NU7/V5 serialization panic broadens beyond
   `getblocktemplate`.

Routing summary:

- The remaining production `panic!`/`unwrap()`/`expect()` hits in RPC, P2P,
  mempool, read-state, and selected `zebrad` components mostly reduce to
  already-recorded issues: trusted-RPC finalization panics, longpoll Unicode,
  invalid Sapling receiver UA, address-book ban panic, indexer stream/resource
  notes, optional endpoint bind panics, and local RocksDB corruption hazards.
- `NonFinalizedBlocksListener::unwrap()` in the indexer path looks scary because
  it panics if the listener `Arc` has more than one strong reference. The live
  `ReadStateService` constructs a fresh listener for each
  `ReadRequest::NonFinalizedBlocksListener`, `oneshot()` moves the response, and
  the existing indexer test already exercises multiple subscriptions. I did not
  find a remote indexer-client path that clones the returned listener before
  unwrap.
- The `flat_map(Result)` pattern still routes to
  `docs/analysis/value-pool-error-suppression-note.md`; I did not find another
  consensus-sensitive `Result` iterator being silently dropped in the checked
  `zebra-chain`, `zebra-consensus`, `zebra-state`, or RPC paths.
- Other defaulting/error-suppression candidates route to
  `docs/analysis/defaulting-error-suppression-recheck-note.md`: transparent
  received-balance backward compatibility, ZIP-317 unpaid-action clamping, GBT
  proposal-helper time defaults, and verbose `getrawmempool` descendant-fee
  formatting.
- Checkpoint NU5/V5 authorizing-data binding remains eliminated as a bad-state
  persistence issue. The checkpoint verifier defers auth-data binding, but the
  finalized-state commit path recomputes `hashBlockCommitments` before
  `write_block()`. Keep routing that question to
  `docs/analysis/checkpoint-auth-data-binding-note.md`.
- The pre-Heartwood Sapling-root checkpoint-coverage concern remains the custom
  Regtest/proposal-validation caveat already recorded in
  `docs/analysis/pre-heartwood-sapling-root-checkpoint-coverage-note.md`.
- The custom NU7/V5 serialization panic still looks scoped to constructed
  internal transactions, especially `getblocktemplate` / internal miner
  coinbase generation on custom Regtest/Testnet. I did not find a broader
  ordinary peer/RPC raw-block serialization path that can create or persist the
  impossible normal-build `V5 { network_upgrade: Nu7 }` transaction first.

Conclusion: this pass improved duplicate routing but did not produce a new
private-disclosure or public-issue candidate. Keep these items local-only unless
the user explicitly names a specific item to post.

## Follow-up: Read-State Find Assertions / Locator Recheck

Result: duplicate-control / eliminated as a fresh state-read panic finding.

I rechecked the `FindBlockHashes` / `FindBlockHeaders` source-to-sink path
because `zebra-state/src/service/read/find.rs` still contains assertion-looking
postconditions in a P2P-requested read path.

The externally influenced input is the peer-supplied `known_blocks` locator:

- `zebra-network/src/protocol/external/codec.rs` deserializes `getblocks` and
  `getheaders` locators without an explicit locator-count cap beyond the
  protocol message-size / trusted-preallocation bounds.
- `zebrad/src/components/inbound.rs` forwards the locator unchanged into
  `zebra_state::Request::FindBlockHashes` or `FindBlockHeaders`.
- `zebra-state/src/service.rs` uses fixed response caps,
  `MAX_FIND_BLOCK_HASHES_RESULTS = 500` and
  `MAX_FIND_BLOCK_HEADERS_RESULTS = 160`, when calling the read helpers.

The fresh panic-looking assertions did not upgrade:

- `find_chain_height_range()` asserts `max_len > 0`, but live callers pass the
  fixed non-zero constants above, not peer-supplied lengths.
- The `response_len <= max_len` assertion is a postcondition over a height range
  already capped by those constants.
- The "list must not contain the intersection hash" and "stop hash must be
  final if included" assertions depend on block hashes having a unique block
  height. Concurrent state movement can make the helper return an empty or
  partial response, and the code already logs those cases, but I did not find a
  valid peer-only input that makes the returned sequence violate those
  postconditions.

The real issue in this path remains the request-cost amplification already
tracked in `docs/analysis/p2p-block-locator-length-hardening-note.md`: Zebra can
scan an oversized locator before returning a capped response. That is already
publicly covered by #10549, so this pass should not produce another report.

## Follow-up: Misbehavior Report Transport Sibling Proof

Result: strengthened existing local-only finding.

The lossy misbehavior-report transport note already had direct source evidence
for sync, mempool, and the adjacent inbound `VerifyBlockError` branch, plus a
focused sync-path proof. I added sibling mempool-path and inbound branch-level
proofs so the transport-loss shape is no longer source-only outside sync:

- `zebrad/src/components/mempool/tests/vector.rs`
- `full_misbehavior_channel_drops_score_bearing_mempool_report_today`
- `zebrad/src/components/inbound/tests.rs`
- `full_misbehavior_channel_drops_score_bearing_inbound_branch_today`

Verification:

```sh
cargo test -p zebrad full_misbehavior_channel_drops_score_bearing_mempool_report_today --lib
cargo test -p zebrad full_misbehavior_channel_drops_score_bearing_inbound_branch_today --lib
```

Result: both passed on 2026-05-09. The mempool test fills the bounded
misbehavior channel with a sentinel, queues a transaction by ID so the normal
download path carries an advertiser address, returns a score-bearing
`TransactionError::BadBalance` from the verifier, and confirms the sentinel
remains the only queued report. The inbound branch-level test injects a
score-bearing `VerifyBlockError` into the extracted inbound reporting helper
with the channel already full, and confirms that report is also dropped.

The local finding now has current-behavior proofs for sync and mempool and a
branch-level proof for inbound. The live inbound semantic verifier still returns
`RouterError`, and the separate inbound-router-error proof shows that normal
score-bearing verifier errors do not downcast to `VerifyBlockError`; that
ordinary-reachability caveat remains documented separately. Keep this item
local-only unless the user explicitly asks to post it.

## Follow-up: Mempool Verified-Set Accounting Overflow Recheck

Result: eliminated as a practical security finding.

I rechecked the unchecked additions in `VerifiedSet::insert()`:

- `transactions_serialized_size += transaction.transaction.size`
- `total_cost += transaction.cost()`

The live insertion path does perform these additions before ZIP-401 eviction,
and tests commonly set `tx_cost_limit = u64::MAX`, so the code initially looked
like a supported-configuration panic candidate. The concrete transaction bounds
make it a weak finding:

- `VerifiedUnminedTx::cost()` is `max(transaction.size, 10_000)`.
- `transaction.size` is derived from Zcash serialization size.
- Deserialization and ZIP-317 checks keep ordinary transactions within the
  `MAX_BLOCK_BYTES = 2_000_000` serialization envelope.

Overflowing `total_cost: u64` would require trillions of accepted mempool
transactions at real transaction sizes, and memory exhaustion or ordinary
operator resource limits would occur first. Overflowing
`transactions_serialized_size: usize` is even less useful on 64-bit targets.

Keep this as a code-hardening observation only. It is not worth reporting as a
security issue without a separate path that admits artificially inflated
`UnminedTx::size` values into live storage.

## Follow-up: Peer-Set / Address-Book Duplicate Routing Recheck

Result: no fresh unique finding.

I rechecked three P2P availability-adjacent candidates because they still have
remote-peer shape and process-fatal-looking assertions:

- `PeerSet::update_metrics()` still panics if ready plus unready peers exceed
  the configured peer-set connection limit, but `poll_discover()` only inserts
  peers after duplicate-address and per-IP checks, and inbound/outbound
  admission happens before peer-set insertion. This remains the already-recorded
  internal invariant / defensive-hardening item in
  `docs/analysis/p2p-peer-set-unready-availability-note.md`, not a new
  remote-triggered peer-count panic.
- The meaningful residual in the same area is still long-lived unready peers:
  `poll_peers()` expects peers to become ready or time out and has the explicit
  TODO to drop peers that overload Zebra with inbound messages and never become
  ready. That is already publicly tracked by #7822 and locally documented in
  `docs/analysis/p2p-peer-set-unready-availability-note.md`.
- The `network.max_connections_per_ip > 1` sibling surface still routes to the
  existing address-book artifacts: the privately reported ban-path panic,
  non-contiguous same-IP ban cleanup, and lazy connected-peer disconnect after
  ban publication. I did not find another `most_recent_by_ip` unwrap or same-IP
  cleanup path outside those notes.

I also rechecked the indexer gRPC unwrap/stream surface while looking for a
fresh opt-in service issue. `tonic_reflection::build_v1().unwrap()` is startup
configuration, `NonFinalizedBlocksListener::unwrap()` still looks like an
internal single-owner invariant in the live RPC path, and the unauthenticated
stream, idle-disconnect, per-subscriber listener, `MempoolChange` privacy, and
`TrustedChainSync` boundary issues are already covered by the indexer notes.

Conclusion: keep this as duplicate routing. No public issue, advisory, or
comment without an explicit user-selected item.

## Follow-up: Address-Index Spending-ID Duplicate Routing Recheck

Result: no fresh unique finding.

I rechecked the optional address-index and `indexer` spending-transaction-ID
surface because it combines RPC-visible data, database-format upgrades, and
process-fatal-looking assertions:

- `getaddressutxos` still iterates through `AddressUtxos::utxos()`, where the
  response path expects every indexed UTXO to have an address and a matching
  transaction ID. This is the same state-index consistency shape already
  captured in `docs/analysis/rpc-response-construction-panic-sweep-note.md`.
- The non-finalized address index stores created and spent transparent outputs
  in ordered maps/sets keyed by `OutputLocation`, and the RPC layer consumes
  those ordered maps. I did not find a normal request parameter that can
  reorder or desynchronize the maps.
- The finalized spending-ID index is built from already-validated finalized
  blocks during normal commits, with same-block spends handled by looking up
  output locations from the current block's transaction indexes. The
  `track_tx_locs_by_spends` database-format upgrade has several sharp
  `expect()` calls, but they depend on existing finalized-state validity or DB
  consistency, not directly on a remote request.
- The genesis transparent coinbase sentinel case remains eliminated because
  finalized block commit skips genesis UTXO/address-index updates and
  address-height queries start at height 1.

Conclusion: this routes to existing address-index/RPC hardening notes and is
not a new private-disclosure candidate on its own. Keep local-only unless a
future proof finds a valid-chain path that creates inconsistent address indexes.

## Follow-up: Mempool Crawler Response-Contract Recheck

Result: no fresh unique finding.

I rechecked the mempool crawler because it has two process-fatal-looking
`unreachable!()` branches on a remote-facing path:

- `zebrad/src/components/mempool/crawler.rs` expects
  `Request::MempoolTransactionIds` to return `zn::Response::TransactionIds`.
- The same crawler expects `mempool::Request::Queue` to return
  `mempool::Response::Queued`.

The network-side branch is an internal Tower response-contract invariant, not a
peer-controlled response variant:

- `zebra-network/src/protocol/internal/request.rs` documents
  `MempoolTransactionIds` as returning only `Response::TransactionIds`, and the
  internal API explicitly treats other variants as network-code bugs.
- `zebra-network/src/peer/connection.rs` sends a wire `mempool` message for this
  request and only completes the handler with `Response::TransactionIds` when
  the peer replies with an `inv` whose items all decode as unmined transaction
  IDs.
- Non-transaction `inv` messages and unrelated peer messages are ignored as
  non-responses and the crawler's peer-set call is wrapped in the existing
  `PEER_RESPONSE_TIMEOUT`.

The mempool-side branch is also a service-contract invariant. The concrete
`Mempool` service returns `Response::Queued` for `Request::Queue` in both the
enabled path and the disabled path; the disabled path reports per-item errors
inside the queued result vector rather than changing the outer response variant.

The meaningful crawler-adjacent issues remain the already-documented inventory,
pending-download, same-effects, and peer-set availability notes. I did not find
a new peer-only path to the crawler `unreachable!()` branches.

## Follow-up: Peer-Connection Response-State Panic Recheck

Result: eliminated as a fresh remote-triggered panic.

I rechecked the peer connection state machine because it has several
process-fatal-looking `panic!()` / `unreachable!()` branches while handling
peer messages and client requests:

- `zebra-network/src/peer/connection.rs` panics if `handle_client_request()`
  receives a new internal request while the connection is already
  `AwaitingResponse`.
- The same file has `unreachable!()` branches after matching an
  `AwaitingResponse` state and while moving timed-out responses back to
  `AwaitingRequest`.
- `zebra-network/src/peer/client.rs` panics if `Client::call()` is invoked
  without `poll_ready()`.

The current peer-set routing keeps this out of peer control:

- `PeerSet::route_p2c()`, `route_inv()`, and `send_multiple()` remove a selected
  client from `ready_services`, call it once, and push it into the unready set
  until the request future completes.
- `PeerSet::poll_ready_peer_errors()` explicitly treats a ready client becoming
  unready without first being moved to `unready_peers` as a peer-set bug.
- `Client::poll_ready()` only reports readiness when the request channel has
  space and the connection/heartbeat tasks have not failed; callers that bypass
  Tower readiness can panic, but that is an internal service contract violation,
  not a wire-message transition.
- Malformed or unexpected peer messages during `AwaitingResponse` are first
  offered to the active handler, then optionally to inbound request handling,
  and otherwise ignored or timed out. I did not find a peer message that can
  directly replace the connection state between the outer match arm and the
  later `unreachable!()` checks.

Conclusion: these assertions remain useful crash indicators for internal
peer-set misuse, but this pass did not find a valid P2P input sequence that
reaches them. Do not report separately from the existing peer-set availability
and response-contract notes.

## Follow-up: Duplicate-Routing Sweep After Crawler

Result: no fresh unique finding.

After closing the crawler response-contract lead, I checked several nearby
surfaces that initially looked interesting but already route to existing notes:

- Dynamic Prometheus labels: still limited to the known peer user-agent/address,
  peer message/error, mempool failure `reason`, raw RPC `method`, and metrics
  endpoint exposure families. No new label source appeared in the current
  `metrics::counter!` / `gauge!` / `histogram!` sweep.
- `addrv2` parser `unreachable!()` sites: serialization is test-only for
  `AddrV2`, unsupported remote network IDs deserialize to `AddrV2::Unsupported`,
  and the live receive path filters unsupported entries before conversion. This
  remains covered by the gossiped-address services and `addrv2` allocation-cap
  notes.
- Unknown P2P command `Ok(None)` and BIP37/empty-body parser leniency: both are
  already covered by local notes and the earlier explicitly authorized public
  issue batch.
- Funding-stream address index assertions: already covered by the configured
  funding-stream and Regtest validation-bypass notes. The input is local custom
  network configuration, not peer/RPC traffic on default networks.
- Queued-block UTXO cleanup TODOs: adjacent to the existing queued-block
  retention, height-index desync, and queue-global `AwaitUtxo` scope notes. The
  interesting default-network limitation remains that queue insertion requires
  semantic block verification and proof of work.

This pass adds no new public or private report candidate. Keep these as
duplicate/eliminated routes unless a later proof changes the reachability story.

## Follow-up: OpenTelemetry Sampler Environment Variable Semantics

Result: new local-only observability hardening finding.

Disposition: do not post publicly without explicit user direction and a fresh
duplicate check.

Zebra intentionally exposes OpenTelemetry sampling as a percentage from 0 to
100, and its Docker observability docs use values such as
`OTEL_TRACES_SAMPLER_ARG=10` for 10% sampling. That behavior works for
Zebra-specific documentation, but it overloads a standard OpenTelemetry
environment variable whose usual `traceidratio` / `parentbased_traceidratio`
argument is a floating-point ratio from 0.0 to 1.0.

The runtime code currently reads `OTEL_TRACES_SAMPLER_ARG`, parses it as a
`u8`, and treats parse failure as "no sample percentage configured":

- `zebrad/src/components/tracing/component.rs:286-290` reads
  `OTEL_TRACES_SAMPLER_ARG` and calls `s.parse().ok()`.
- `zebrad/src/components/tracing/component.rs:293-297` logs
  `sample_percent.unwrap_or(100)`.
- `zebrad/src/components/tracing/otel.rs:66-86` converts the optional
  percentage to a ratio by dividing by 100, defaulting to `100`, and then uses
  `Sampler::AlwaysOn` for rates at or above 1.0.
- `zebrad/src/components/tracing.rs:214-226` documents the Zebra config field
  as a percentage and explicitly notes that this differs from the standard
  OpenTelemetry ratio syntax.

This means an operator using normal OpenTelemetry conventions, for example
`OTEL_TRACES_SAMPLER=traceidratio` with
`OTEL_TRACES_SAMPLER_ARG=0.1`, gets a silent parse miss in Zebra. With an OTLP
endpoint configured, the resulting `None` falls back to 100% sampling rather
than the likely intended 10% sampling. Zebra also ignores
`OTEL_TRACES_SAMPLER`, so setting the standard sampler selector does not rescue
that configuration.

I do not think this is a private disclosure issue:

- OpenTelemetry export is still opt-in through `tracing.opentelemetry_endpoint`
  or `OTEL_EXPORTER_OTLP_ENDPOINT`.
- The input is local deployment configuration, not peer or unauthenticated RPC
  traffic.
- Impact is operational: unexpected trace volume, larger telemetry egress, more
  collector/storage load, and a wider privacy surface than the operator meant
  to enable.

It is still a useful hardening item because it composes with the existing
telemetry notes: a node under heavy remote activity, or a node with the optional
filter-reload endpoint exposed, can export much more telemetry than an operator
intended if they used standard OTEL ratio syntax.

Suggested fix direction:

- Prefer a Zebra-namespaced environment variable for the percentage form, for
  example `ZEBRA_TRACING__OPENTELEMETRY_SAMPLE_PERCENT`.
- If Zebra continues reading `OTEL_TRACES_SAMPLER_ARG`, parse standard ratio
  values when `OTEL_TRACES_SAMPLER` is `traceidratio` or
  `parentbased_traceidratio`, or at least warn loudly and keep export disabled
  on invalid values instead of defaulting to 100%.
- Document that Zebra ignores `OTEL_TRACES_SAMPLER` today.
- Add a small unit test around sampler-env resolution so `0.1` cannot silently
  become full sampling again.

Confidence: high for source behavior, medium for deployment impact. This is a
low-severity public hardening candidate only if the user explicitly asks to post
it later.

## Follow-up: Tokio Console Optional Listener Recheck

Result: eliminated as a fresh report candidate.

Zebra's `tokio-console` support initially looked like another optional
observability listener worth checking, because `console_subscriber::spawn()`
starts a gRPC server that streams async runtime diagnostics to console clients.

The current reachability story is bounded:

- `zebrad/src/components/tracing/component.rs:313-316` only installs
  `console_subscriber::spawn()` when both the `tokio-console` Cargo feature and
  `tokio_unstable` cfg are enabled.
- `zebrad/Cargo.toml:131-142` documents the required opt-in build command and
  keeps `tokio-console` out of the default release feature set.
- `book/src/dev/tokio-console.md:9-25` describes it as a developer diagnostic
  tool and says Zebra uses the default options.
- The upstream `console-subscriber` docs for the builder and `spawn()` say the
  default listener is `127.0.0.1:6669`, with `TOKIO_CONSOLE_BIND` as an
  environment override.

That leaves only local deployment footguns: if an operator intentionally builds
the feature, passes `RUSTFLAGS="--cfg tokio_unstable"`, and sets
`TOKIO_CONSOLE_BIND` to a non-loopback address, the console gRPC service can
expose task/runtime diagnostics to that network. This is not enabled in normal
Zebra release artifacts and is controlled by local build/runtime configuration,
so I am not treating it as a unique vulnerability.

Suggested future hardening, if maintainers are already touching this area:
mention `TOKIO_CONSOLE_BIND` in `book/src/dev/tokio-console.md` and recommend
loopback or a Unix-domain socket for any developer use. Keep it grouped with
the existing feature-gate release-variable note rather than filing a separate
issue.

## Follow-up: SubmitBlock Gossip Channel Backpressure Recheck

Result: eliminated as a fresh vulnerability; keep as miner/RPC result
semantics hardening.

The `submitblock` path commits the block first, then tries to notify the block
gossip task:

- `zebra-rpc/src/methods.rs:2573-2594` awaits
  `Request::Commit(Arc::new(block))`; on success it calls
  `self.gbt.advertise_mined_block(hash, height)` before returning
  `SubmitBlockResponse::Accepted`.
- `zebra-rpc/src/methods/types/get_block_template.rs:548-556` implements
  `advertise_mined_block()` as `try_send((block, height))` on a bounded
  `mpsc::Sender`.
- `zebra-rpc/src/methods/types/submit_block.rs:83-96` sizes that channel at
  10,000 messages.
- `zebrad/src/components/sync/gossip.rs:50-153` reads submitted-block messages,
  broadcasts them as `AdvertiseBlockToAll`, and uses a `Timeout` around the
  peer-set service readiness/call path.

This produces an odd but bounded outcome: if the local mined-block gossip
channel is full after the block verifier accepted a submitted block, Zebra maps
the `try_send` failure into a JSON-RPC error even though the block has already
been accepted into state. That can confuse mining software, but I do not see a
cheap remote DoS or consensus-impacting path:

- Invalid or malformed blocks do not reach the channel.
- Filling the channel requires many successful `submitblock` commits, not just
  cheap invalid submissions.
- The channel is process-local, large, and drained by the gossip task.
- The already documented `submitblock` timeout/result-taxonomy note is the more
  important miner-facing availability issue.

Suggested hardening if this area is changed later: return the block verification
result independently from best-effort gossip notification, log/metric a full
gossip channel, and avoid turning post-commit gossip backpressure into an RPC
failure for the already-accepted block.

## Follow-up: Address-Index Balance Arithmetic Recheck

Result: eliminated as a fresh panic; existing address-index bounds note remains
the right local record.

I rechecked the raw `expect()` in finalized transparent balance accounting:

- `zebra-state/src/service/finalized_state/zebra_db/transparent.rs:338-353`
  sums finalized balances for a caller-supplied address set and unwraps the
  `Amount` addition with the invariant that the address total should not
  overflow.
- `zebra-state/src/service/read/address/balance.rs:35-57` passes a
  `HashSet<transparent::Address>` through the read service.
- `zebra-rpc/src/methods.rs:1132-1148` parses `getaddressbalance` parameters and
  forwards only the set returned by `valid_addresses()`.
- `zebra-rpc/src/methods.rs:3535-3553` converts the request's vector of address
  strings into a `HashSet<Address>` before creating the state request.

So duplicate RPC address strings cannot multiply one real address balance until
the finalized summation overflows. The remaining issue is still the existing
low-severity resource hardening: address-index RPCs accept large unique address
sets and broad ranges unless callers or deployment-level limits cap them. That
is already tracked in `docs/analysis/rpc-address-index-query-bounds-note.md` and
the local ledger.

## Follow-up: LongPoll Raw-Sizing / Conversion Recheck

Result: no new sibling to the already known `longpollid` Unicode parser panic.

The raw conversion sites in `zebra-rpc/src/methods/types/long_poll.rs` look
sharp, but the reachable ones are bounded by fixed-size internal data:

- `LongPollInput::generate_id()` casts the mempool transaction count to `u32`;
  this is intentionally lossy long-poll invalidation state, and the actual
  mempool size is independently bounded.
- `update_checksum()` chunks a `[u8; 32]` hash into four-byte chunks, so
  `chunk.try_into().expect("chunk is u32 size")` is not attacker-reachable with
  a short chunk.
- The externally supplied `LongPollId` parser still checks byte length and then
  slices the UTF-8 string at fixed byte offsets. That is the already documented
  process-fatal Unicode panic in
  `docs/analysis/rpc-longpollid-unicode-panic-finding.md`.

No new public or private report candidate from this pass.

## Follow-up: Sync Progress Chain-Tip Estimate Panic Recheck

Result: eliminated as a fresh process-fatal panic candidate.

The progress task has a sharp-looking `expect()` after estimating the network
tip:

- `zebrad/src/components/sync/progress.rs:140-147` calls
  `estimate_network_chain_tip_height()` and then unwraps
  `best_tip_height()` with the invariant that a successful estimate requires a
  height.
- `zebra-chain/src/chain_tip.rs:82-92` implements the default estimator by
  first calling `best_tip_height_and_block_time()?`, so an estimate is only
  returned after a height and block time are both available.
- `zebra-state/src/service/chain_tip.rs:380-405` implements the production
  `LatestChainTip` getters from one shared `Option<ChainTipBlock>`, so the
  combined getter and height-only getter are views over the same watched value.
- `zebra-state/src/service/chain_tip.rs:228-249` refuses to publish `None`
  updates. Once the production chain-tip sender has published a concrete tip,
  later finalized or non-finalized `None` values do not reset the public
  `LatestChainTip` back to empty.

There is therefore no observed production path where `estimate_network_chain_tip_height()`
returns `Some(_)` and the immediately following `best_tip_height()` can be
`None`. The mock chain tip can be made inconsistent by tests, but it is not used
in the `zebrad` production progress task.

## Follow-up: RPC / GBT Duplicate-Control Rechecks

Result: no fresh finding promoted.

I rechecked several sharp-looking RPC and mining-template paths that showed up
in the production panic/defaulting sweep:

- The optional tracing filter endpoint's unauthenticated `POST /filter` and
  large-body behavior is already recorded in
  `docs/analysis/tracing-filter-endpoint-security-note.md` and
  `docs/analysis/tracing-filter-reload-telemetry-amplification-note.md`.
- Inbound transaction `getdata` work before response trimming is already
  recorded in `docs/analysis/p2p-getdata-transaction-request-amplification-note.md`
  and the local ledger entry for issue #10566.
- The verbose Orchard RPC action/signature lookup remains a real bounded
  `O(n^2)` response-construction shape, but it is already captured in
  `docs/analysis/rpc-verbose-orchard-action-quadratic-note.md`. The consensus
  block-size cap keeps the action count finite, and RPC is disabled/authenticated
  by default, so this stays public hardening rather than private disclosure.
- The `getblocktemplate` time-envelope mismatch is already proof-backed in
  `docs/analysis/gbt-time-envelope-mismatch-note.md`, including the local
  future-time ceiling and Testnet target-spacing activation boundary tests.
- The `getblocktemplate` chain-info history-tree `expect()` is not a fresh
  panic path on current review: `read::tree::history_tree()` falls back to
  `Some(db.history_tree())`, and the surrounding before/after best-tip check
  catches finalized-tip movement before the result is used.
- The `z_listunifiedreceivers` Orchard sibling is already documented as an
  invalid-receiver echo rather than a process abort. The Sapling branch remains
  the fatal path that was privately reported.
- RPC `unwrap_or_default()` / `.ok()` response-builder candidates route to the
  existing `defaulting-error-suppression` and `value-pool-error-suppression`
  notes. I did not find another consensus-sensitive `Result` iterator being
  silently dropped in the checked RPC/state paths.

These checks were local-only duplicate control; nothing was posted publicly.

## Follow-up: V5 `getrawtransaction` Blockhash Exactness Proof

Result: upgraded the residual V5 exactness note from source-evidence-only to
local proof-backed.

I added the current-behavior test
`getrawtransaction_blockhash_can_return_different_v5_auth_variant_today` in
`zebra-rpc/src/methods/tests/vectors.rs`. The test constructs two V5
transactions with the same mined transaction ID but different authorizing data,
then mocks the same read-state sequence used by `getrawtransaction(txid,
verbose=1, blockhash)`.

Observed behavior:

- the caller-supplied block membership check succeeds for one V5 auth variant,
- the later global `AnyChainTransaction(txid)` lookup returns the other auth
  variant,
- the RPC response reports the caller's `blockhash` and `in_active_chain`, but
  the `hex` and `authdigest` are from the global lookup result.

Verification:

```sh
cargo test -p zebra-rpc getrawtransaction_blockhash_can_return_different_v5_auth_variant_today --lib
```

Result on 2026-05-09: passed after formatting.

This remains local-only. It is RPC exactness hardening rather than a private
consensus issue on current evidence.

## Follow-up: Value-Pool Upgrade Replay Proof

Result: upgraded the migration/replay part of the value-pool suppression note
from source-evidence-only to local proof-backed.

I added the current-behavior test
`block_info_upgrade_persists_zero_value_pool_when_recomputed_block_value_errors_today`
in `zebra-state/src/service/finalized_state/zebra_db/block/tests/vectors.rs`.
The test creates an older/raw finalized database shape with real genesis data
and a height-1 malformed historical block whose transaction-level value balance
fails. It then runs the `block_info_and_address_received::Upgrade`.

Observed behavior:

- the block value-pool recomputation problem is converted into a zero block
  delta by `chain_value_pool_change(...).unwrap_or_default()`,
- the upgrade writes `BlockInfo` for the malformed height,
- the upgrade's own `validate()` accepts the resulting `BlockInfo` because the
  serialized size keeps it from being `Default::default()`.

Verification:

```sh
cargo test -p zebra-state block_info_upgrade_persists_zero_value_pool_when_recomputed_block_value_errors_today --lib
cargo test -p zebra-chain chain_value_pool_change_drops_transaction_value_balance_errors_today --lib
cargo test -p zebra-rpc chain_supply_panics_when_pool_sum_exceeds_max_money_today --lib
```

Result on 2026-05-09: all passed after formatting.

This strengthens the private/local value-pool follow-up: the helper bug and the
upgrade fallback are high confidence, while normal remote invalid-block
acceptance remains unproven because the semantic verifier rejects the concrete
synthetic value-balance failures checked so far.

## Follow-up: Error-Suppression Sibling Sweep

Result: no fresh sibling promoted beyond the value-pool upgrade replay proof.

After strengthening the value-pool finding, I swept nearby consensus/state
`unwrap_or_default()`, `flat_map()`, `filter_map(Result::ok)`, and `.ok()`
patterns.

Eliminated or already-covered items:

- `zebra-consensus/src/block/check.rs` and `zebra-consensus/src/checkpoint.rs`
  default a missing deferred-pool funding-stream entry to zero before subtracting
  lockbox disbursements. This is the expected absent-entry shape, not a dropped
  arithmetic error.
- `zebra-state/src/service/finalized_state/disk_format/transparent.rs` initially
  looked like it split `AddressBalanceLocationInner` using the wrong width, but
  `BALANCE_DISK_BYTES` and `OUTPUT_LOCATION_DISK_BYTES` are both 8 in the current
  format, so this is semantically brittle rather than a current corruption bug.
- Primitive batch verifier `result.ok()` sends turn `JoinError` failures into
  the existing batch-failure channel and metrics failure label. I did not find
  an invalid-acceptance path in that pattern.
- `ReadRequest::BlockLocator` still defaults a block-locator calculation failure
  to an empty locator, but this routes to sync/peer behavior rather than
  consensus acceptance and is already adjacent to the existing locator hardening
  notes.

The only promoted item from this family remains the value-pool path, because it
has both a real `Result`-as-iterator suppression bug and a proof-backed
upgrade/replay fallback that persists a zero delta.

## Follow-up: Value-Pool Normal Checkpoint Backstop

Result: remote checkpoint-block injection remains eliminated on current
evidence; the value-pool finding stays at the trusted-input / replay boundary.

I rechecked the normal checkpoint verifier before trying to promote the
value-pool replay proof into a remote invalid-block path:

- `zebra-consensus/src/checkpoint.rs::queue_block()` calls `check_block()` before
  adding a block to the checkpoint queue.
- `check_block()` verifies coinbase height, difficulty / Equihash, expected
  deferred-pool calculation, and then calls `merkle_root_validity()` before
  producing a `CheckpointVerifiedBlock`.
- `FinalizedState::commit_finalized_direct()` performs the state commit for an
  already-constructed `CheckpointVerifiedBlock`, and it does not repeat Merkle
  validation. That is intentional trust in the checkpoint verifier / caller
  boundary.

So a remote peer cannot take a trusted checkpoint header hash and swap in the
malformed transaction body used by the value-pool proof: the header hash commits
to the Merkle root, and the checkpoint verifier checks the transaction Merkle
root before state commit.

The meaningful remaining risk is narrower but still worth fixing:

- raw/older finalized DB replay can persist zero value-pool deltas when
  recomputation fails,
- direct construction of `CheckpointVerifiedBlock` trusts its caller and can
  reach the same state write helper; this is now proof-backed by
  `direct_checkpoint_commit_accepts_bad_value_balance_block_today`,
- the helper contract is wrong because `Block::chain_value_pool_change()` has a
  `Result` return type but drops per-transaction errors.

No new remote/private escalation from this checkpoint-path recheck.

## Follow-up: Checkpoint Same-Hash V5 Auth-Data Replacement

Result: verifier-level replacement proof added; not promoted as a fresh remote
report.

Detailed note:
`docs/analysis/checkpoint-auth-data-binding-note.md`.

I revisited the checkpoint auth-data boundary after the value-pool checkpoint
backstop pass, focusing on liveness rather than persisted-state corruption.

Current proof:

- Added
  `same_hash_checkpoint_auth_data_variant_replaces_queued_block_today` in
  `zebra-consensus/src/checkpoint/tests.rs`.
- The test constructs two height-1 checkpoint block bodies that differ only in a
  V5 transaction's Orchard authorizing data.
- The two bodies have the same V5 mined transaction ID and the same block header
  hash.
- While the checkpoint range is incomplete, the newer same-hash body replaces the
  older queued body.
- Once the range completes, the older future returns `NewerRequest`; the newer
  replacement body reaches the state-commit path and surfaces
  `CommitCheckpointVerified` when the mocked state rejects it.

Verification:

```sh
cargo test -p zebra-consensus same_hash_checkpoint_auth_data_variant_replaces_queued_block_today --lib
```

Result on 2026-05-09: passed.

## Follow-up: P2P Peer-Path `try_send()` Backpressure Audit

Result: no new independent vulnerability found.

Detailed note:
`docs/analysis/p2p-try-send-backpressure-audit-note.md`.

RepoPrompt suggested a final bounded-channel pass over the P2P peer-path
`try_send()` sites. The direct proof work covered:

- `PeerSet::poll_ready()` `demand_signal.try_send(MorePeers)`;
- `dial()` redemand after a failed outbound connection;
- `route_p2c()` stall-event transport into `drain_stall_events()`;
- `Client::call()` when the per-peer request channel is disconnected;
- `send_one_heartbeat()` when the per-peer request channel is full.

The result is an elimination pass rather than a new report. `MorePeers` loss in
the direct full-channel case is duplicate-demand elision: the already-queued
demand token remains queued. Failed dials requeue a consumed demand token when
there is room. Stall events are poll-deferred but delivered through an
unbounded channel and enforced on the next peer-set poll. The client request
path returns an explicit peer error on disconnection. The heartbeat path waits
on a full channel and then times out/reports the peer.

Verification:

```sh
cargo test -p zebra-network full_more_peers_channel_preserves_existing_demand_today --lib
cargo test -p zebra-network failed_dial_requeues_consumed_demand_token_today --lib
cargo test -p zebra-network stall_events_are_deferred_until_next_poll_then_disconnect_today --lib
cargo test -p zebra-network client_call_on_disconnected_server_tx_returns_error_today --lib
cargo test -p zebra-network heartbeat_full_server_tx_times_out_and_reports_error_today --lib
```

Result on 2026-05-09: all five focused tests passed.

Triage: no new public issue recommended. The remaining availability overlap is
the existing unready-peer capacity class already documented in
`docs/analysis/p2p-peer-set-unready-availability-note.md` and public #7822. The
confirmed lossy score-bearing signal path remains the separate `zebrad`
misbehavior-report transport note.

## Follow-up: GBT ZIP-317 Weighted-Index Rebuild Proof

Result: upgraded the ZIP-317 weighted-index selection note from source-evidence
plus broad sanity tests to a focused current-behavior proof.

Detailed note:
`docs/analysis/gbt-zip317-selection-quadratic-note.md`.

Current proof:

- Added test-only instrumentation around `setup_fee_weighted_index()` in
  `zebra-rpc/src/methods/types/get_block_template/zip317.rs`.
- Added
  `independent_transaction_selection_rebuilds_weighted_index_after_each_candidate_today`
  in `zebra-rpc/src/methods/types/get_block_template/zip317/tests.rs`.
- The test runs `select_mempool_transactions()` over eight independent
  non-coinbase test transactions and confirms the weighted-index builder sees
  candidate counts `n, n - 1, ..., 1` across the conventional-fee and low-fee
  partitions.

Verification:

```sh
cargo test -p zebra-rpc independent_transaction_selection_rebuilds_weighted_index_after_each_candidate_today --lib
```

Result on 2026-05-09: passed.

Triage unchanged: local-only mining RPC availability hardening, not private
disclosure. The path requires mining RPC access and remains bounded by mempool
and block limits, but the avoidable repeated weighted-index rebuild is now
measured directly.

## Follow-up: RPC HTTP Compatibility Parse Amplification

Result: upgraded from middleware-local current-behavior proofs to production
server-stack composition proof.

Detailed note:
`docs/analysis/rpc-http-compatibility-parse-amplification-note.md`.

RepoPrompt's broader non-duplicate sweep pointed at the HTTP compatibility
middleware rather than the already-covered pre-guard retention or batch-count
issues. The current middleware fully buffers request bodies, parses them as a
compatibility envelope, reserializes recognized requests, and rebuilds an
`HttpBody` before jsonrpsee performs normal dispatch. The response path also
fully buffers, parses, reserializes, and rebuilds response bodies.

The sharper point is that strict JSON-RPC 2.0 traffic pays this cost even when
no legacy compatibility rewrite is needed.

Current proof:

- Added
  `strict_json_rpc_2_request_is_reserialized_before_inner_service_today` in
  `zebra-rpc/src/server/tests/http_request_compatibility.rs`.
- Added
  `strict_json_rpc_2_response_is_reserialized_before_client_today` in the same
  file.
- The request test captures the raw body received by a mock inner service and
  shows a strict 2.0 request is canonicalized before inner dispatch.
- The response test returns a noncanonical strict 2.0 response from a mock
  inner service and shows the middleware canonicalizes it before returning it to
  the caller.
- Added
  `rpc_server_text_plain_strict_json_rpc_reaches_dispatch_today` in
  `zebra-rpc/src/server/tests/vectors.rs`.
- The server-boundary test starts the actual auth-disabled `RpcServer`, sends a
  strict JSON-RPC 2.0 request as `Content-Type: text/plain; charset=utf-8`, and
  receives a JSON-RPC method-not-found response for a nonexistent method while
  all backend mocks remain idle.

Verification:

```sh
cargo test -p zebra-rpc strict_json_rpc_2 --lib
cargo test -p zebra-rpc rpc_server_text_plain_strict_json_rpc_reaches_dispatch_today --lib
```

Result on 2026-05-09: both passed.

Triage: local-only hardening, not private disclosure. RPC is disabled or
authenticated by default and request/response bodies are size-bounded, but the
extra parse/serialize/body-copy work is deterministic and avoidable for normal
strict 2.0 callers.

## Follow-up: Indexer/Trusted-Sync Duplicate-Control Sweep

Result: no fresh candidate from the selected indexer and trusted-sync surfaces.

RepoPrompt reviewed the indexer gRPC server bootstrap, streaming RPC methods,
protobuf block/hash conversion helpers, and `TrustedChainSync` commit loop
against the current local ledger. Every sharp edge in that selected surface
mapped cleanly to an existing owning note:

- `docs/analysis/indexer-idle-stream-disconnect-retention-note.md` owns idle
  stream disconnect retention and `MempoolChange` lag/closure conflation.
- `docs/analysis/indexer-non-finalized-state-stream-amplification-note.md` owns
  per-subscriber non-finalized listener/task/channel multiplication.
- `docs/analysis/trusted-chain-sync-indexer-validation-boundary-note.md` owns
  the protobuf hash/body trust boundary and skipped recent-chain contextual
  validation boundary.
- The finalized-tip forwarder permanent-exit sibling is already recorded in
  the continuation ledger and should not be split into a new note from the same
  evidence.

Conclusion: do not create a new finding from this pass unless a future sweep can
show a different trigger, different affected state/resource, and clear
non-overlap from the four existing families above.

## Follow-up: Pre-Version Full-Block Handshake Proof

Result: upgraded the existing unsolicited full-block eager-decode note with a
raw handshake-level proof.

Detailed note:
`docs/analysis/p2p-unsolicited-block-decode-hardening-note.md`.

The previous note already recorded that `negotiate_version()` decodes and
ignores non-`version` and non-`verack` messages, but the proof was mostly source
evidence plus a post-handshake connection-level test. I added
`handshake_decodes_and_ignores_pre_version_block_today` in
`zebra-network/src/peer/handshake/tests.rs`.

The test builds an in-memory framed connection, has the remote side send a real
encoded `Message::Block` before its `Version` message, then sends normal
`Version` and `Verack`. Zebra completes the handshake, which proves the full
block frame was decoded into a non-handshake `Message` and ignored before peer
admission.

Verification:

```sh
cargo test -p zebra-network handshake_decodes_and_ignores_pre_version_block_today --lib
```

Result on 2026-05-09: passed. The first attempt used Tokio's current-thread
test runtime and failed because block decoding calls `tokio::task::block_in_place`;
the committed proof uses `#[tokio::test(flavor = "multi_thread")]`, matching the
codec's runtime requirement.

Triage unchanged: public P2P availability hardening, not private disclosure.
The work is bounded by the outer message size, connection limits, and handshake
timeout, but it happens before normal peer admission and before the inbound
service can shed block-verification work.

Additional proof:

- Added
  `mismatched_block_response_is_decoded_then_ignored_today` in
  `zebra-network/src/peer/connection/tests/vectors.rs`.
- The test requests block A, sends already-decoded block B, confirms the active
  client response is still pending and the inbound service sees no request, then
  sends block A and confirms the original request completes.

Verification:

```sh
cargo test -p zebra-network mismatched_block_response_is_decoded_then_ignored_today --lib
```

Result on 2026-05-09: passed.

## Follow-up: P2P Addr/AddrV2 Count-Cap Regression Triage

Result: eliminated as a fresh vulnerability; keep as duplicate/regression
verification.

Detailed note:
`docs/analysis/p2p-addr-decode-count-cap-regression-note.md`.

RepoPrompt investigated the apparent late-cap shape where `read_addr()` and
`read_addrv2()` deserialize `Vec<AddrV1>` / `Vec<AddrV2>` before their visible
`MAX_ADDRS_IN_MESSAGE` length checks. The current tree is protected earlier:
`Vec<T>::zcash_deserialize()` calls `zcash_deserialize_external_count()`, and
that helper rejects counts above `T::max_allocation()` before
`Vec::with_capacity()`. Both `AddrV1` and `AddrV2` now return
`MAX_ADDRS_IN_MESSAGE` from `TrustedPreallocate::max_allocation()`.

Reachability is still worth recording: unauthenticated peers can exercise the
decoder during handshake because `negotiate_version()` decodes and ignores
non-`version` / non-`verack` messages while waiting for the expected handshake
message. After handshake, unsolicited `addr` is consumed by the peer connection
and bounded in the per-connection cache. The current residual risk is bounded
message parsing/cache churn, not oversized upfront heap allocation.

Read-only duplicate checks found the same family covered by GHSA-xr93-pcq3-pxf8
and related public work: #10545, #10563, #10570, and the older addrv2 parsing
PR #3022. No public issue should be opened for this shape unless a distinct
bypass is found.

Verification:

```sh
cargo test -p zebra-network poc_remote_addrv2_resource_exhaustion --lib
cargo test -p zebra-network addr_v --lib
```

Result on 2026-05-09: both passed.

## Follow-up: Address UTXO Txid History Over-Broad Lookup

Result: upgraded the address-index RPC bounds note with a more specific
state-level work-amplification proof.

Detailed note:
`docs/analysis/rpc-address-index-query-bounds-note.md`.

RepoPrompt's raw-size/count pass found that `getaddressutxos` has a sharper
internal amplification than generic missing address-count caps:
`lookup_tx_ids_for_utxos()` derives the exact `TransactionLocation`s for the
returned live UTXOs, but then asks the non-finalized chain for
`partial_transparent_tx_ids(addresses, ADDRESS_HEIGHTS_FULL_RANGE)`. That lookup
is proportional to full address activity history in the non-finalized chain,
not just to the returned live UTXO set.

Existing deterministic proof:

- A test-only counter wraps the full-history non-finalized txid lookup in
  `zebra-state/src/service/read/address/utxo.rs`.
- `address_utxos_queries_chain_tx_history_even_for_empty_utxos_today` calls the
  txid helper with a non-finalized chain and an empty UTXO
  map, confirms no txids are needed, and still observes one full-history chain
  txid lookup.

Duplicate search:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "lookup_tx_ids_for_utxos"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "getaddressutxos" "partial_transparent_tx_ids"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "address UTXO" "txid" "history"'
```

Result: no hits.

Verification:

```sh
cargo test -p zebra-state address_utxos_queries_chain_tx_history_even_for_empty_utxos_today --lib
```

Result on 2026-05-09: passed.

Triage: local-only public RPC availability hardening, not a demonstrated remote
DoS. RPC is disabled and cookie-authenticated by default, but the fix target is
crisp: resolve txids only for the returned UTXO transaction locations and skip
non-finalized address history scans when the UTXO set is empty or sparse.

Reachability check:

- The sync downloader rejects duplicate pending block hashes through
  `cancel_handles.contains_key(&hash)` before queuing another download.
- The inbound gossiped-block downloader has the same duplicate-hash pending
  check.
- So the normal P2P downloaders do not obviously feed two competing same-hash
  block bodies into checkpoint verification while the first one is still waiting
  for the range to complete.

Triage: this remains a useful regression around the deferred checkpoint
auth-data boundary and a possible direct-verifier/RPC hardening thought, but not
a high-confidence fresh remote vulnerability. It also overlaps historical closed
design issues recorded in the detailed note, especially same-hash checkpoint
replacement and checkpoint auth-data commitment validation.

## Follow-up: Finalization Invalidation Service-Level Proof

Result: confidence raised from lower-level state proof to service-level trusted
control-plane proof.

Detailed note:
`docs/analysis/finalization-invalidated-record-retention-note.md`.

Current proof:

- Added a subprocess test in `zebra-state/src/service/tests.rs`:
  `state_service_invalidate_side_chain_then_finalization_aborts_today`.
- The parent test launches the same test binary with an ignored async helper,
  because the service-level failure aborts the child process rather than
  surfacing as a catchable `#[should_panic]`.
- The helper drives normal `StateService::call()` requests:
  checkpoint genesis commit, semantically verified non-finalized commits,
  `Request::InvalidateBlock` for the same-root side-chain tip, and enough best
  chain descendants to trigger automatic finalization in the async writer.
- The parent observes `SIGABRT`, matching the release-profile availability
  story for a block write task panic.

Verification:

```sh
cargo test -p zebra-state state_service_invalidate_side_chain_then_finalization_aborts_today --lib
cargo test -p zebra-state finalize_retains_invalidated_record_at_finalized_height_today --lib
```

Result on 2026-05-09: both commands passed. The child process prints
`thread caused non-unwinding panic. aborting.` and the parent test treats the
abort signal as the expected current behavior. The newer direct retention proof
inserts a test invalidated record at the exact height of the next finalized
root, finalizes a normal two-block non-finalized chain, and confirms the record
is still retained. This isolates the off-by-one predicate separately from the
empty-side-chain panic path.

Caveat:

- The service-level proof uses a synthetic private testnet with Canopy active at
  height 1 plus fake semantically verified blocks, so it is not a P2P
  invalid-block acceptance proof.
- It does exercise the normal state-service queue, invalidation request, async
  non-finalized writer, and automatic finalization path. That is enough to move
  the finding from "unit-only plus source route" to a high-confidence
  trusted-RPC/control-state availability issue.

## Follow-up: Address-Book Ban Updater Poisoning Proof

Result: confidence raised from direct address-book panic to live updater
panic-plus-poisoning proof.

Detailed note:
`docs/analysis/address-book-misbehavior-ban-max-connections-panic-finding.md`.

Current proof:

- Added `misbehavior_ban_panics_updater_and_poisons_address_book_today` in
  `zebra-network/src/address_book/tests/vectors.rs`.
- The test spawns the real `AddressBookUpdater` with
  `network.max_connections_per_ip = 2`.
- It sends a ban-threshold `MetaAddrChange::UpdateMisbehavior` over the updater
  channel, rather than calling `AddressBook::update()` directly.
- It awaits the updater task and confirms the task exited by panic.
- It then confirms the shared `Arc<Mutex<AddressBook>>` is poisoned.

Verification:

```sh
cargo test -p zebra-network misbehavior_ban_panics_updater_and_poisons_address_book_today --lib
```

Result on 2026-05-09: passed.

Triage: still a non-default configuration issue, not default-node consensus or
P2P integrity. But the live updater proof makes the availability story much
cleaner: a ban-threshold remote-influenced misbehavior update can crash the
normal updater task and poison the shared address-book mutex under the supported
multi-connection-per-IP setting.

## Follow-up: Indexer Listener Unwrap RepoPrompt Triage

Result: eliminated as a vulnerability finding; keep as internal invariant
cleanup only.

RepoPrompt and the local trace agree on the ownership route:

- `zebra-rpc/src/indexer/methods.rs` accepts an empty
  `NonFinalizedStateChange` request, clones the read-state service handle, and
  sends `ReadRequest::NonFinalizedBlocksListener`.
- `zebra-state/src/service.rs` handles that request through an early-return path
  that creates a fresh `NonFinalizedBlocksListener` and returns it directly in
  `ReadResponse::NonFinalizedBlocksListener`.
- `zebra-rpc/src/indexer/methods.rs` then matches the read response by value and
  immediately calls `listener.unwrap()`.
- The service traits require cloneable services, not cloned responses. The
  production indexer wiring passes the read-state service directly; no selected
  runtime wrapper clones, caches, logs, or broadcasts the listener response
  before the unwrap.

`NonFinalizedBlocksListener::unwrap()` can still panic if in-process code clones
the listener or the `ReadResponse` before consuming it, because the listener
wraps its receiver in an `Arc` and uses `Arc::try_unwrap(...).unwrap()`. I did
not find a gRPC-client-controlled way to create that second strong reference.
Opening many streams creates many independent listeners, which routes to the
already-recorded indexer stream-retention/resource notes rather than to this
panic.

Verification refreshed on 2026-05-09:

```sh
cargo test -p zebra-rpc non_finalized_state_streams_request_one_listener_each_today --lib
cargo test -p zebra-rpc dropped_non_finalized_state_stream_retains_state_listener_today --lib
```

Result: both passed.

## Follow-up: TrustedChainSync Best-Tip Forwarder Runtime Proof

Result: upgraded from source-evidence-only to runtime proof-backed for the
helper's permanent-exit behavior.

Detailed note:
`docs/analysis/trusted-chain-sync-best-tip-forwarder-note.md`.

Current proof:

- Factored the finalized-tip forwarding loop in `zebra-rpc/src/sync.rs` into a
  private `spawn_finalized_tip_forwarder()` helper, preserving the current
  `TrustedChainSync::spawn()` behavior where the returned helper `JoinHandle` is
  immediately dropped and only the non-finalized sync task handle is returned.
- Added
  `trusted_chain_sync_tip_forwarder_exits_on_unfinalized_tip_today` in
  `zebra-rpc/src/sync.rs`.
- The test runs a minimal local indexer gRPC server, creates a real
  primary/secondary state DB pair, confirms secondary catch-up succeeds, and
  confirms the test hash is absent from finalized storage.
- It sends that absent hash over `ChainTipChange` and awaits the forwarding
  task. The task exits without panic, matching the current
  `db.block(hash.into()) == None => return` branch.

Verification:

```sh
cargo test -p zebra-rpc trusted_chain_sync_tip_forwarder_exits_on_unfinalized_tip_today --lib
```

Result on 2026-05-09: passed.

Triage: still not a default-node consensus issue and not a private disclosure
candidate. It is an opt-in trusted-mirror robustness bug: a normal or malicious
trusted upstream best-tip message for a hash absent from the mirror finalized DB
can permanently stop the finalized-tip forwarding helper, leaving the mirror
dependent on the separate non-finalized sync stream for future tip updates.

## Follow-up: Verbose Orchard Action Repeated-Search Proof

Result: upgraded the verbose Orchard action RPC assembly note from
source-evidence-only to local proof-backed.

Detailed note:
`docs/analysis/rpc-verbose-orchard-action-quadratic-note.md`.

Current proof:

- Added a test-only counter around the existing authorized-action `.find()` in
  `zebra-rpc/src/methods/types/transaction.rs`.
- Added
  `verbose_transaction_searches_authorized_orchard_actions_repeatedly_today`.
- The test deserializes the Testnet NU5 vector block
  `BLOCK_TESTNET_1842467_BYTES`, selects the two-action Orchard transaction,
  builds a verbose `TransactionObject`, and confirms the authorized-action
  search performs three comparisons for two actions.

Verification:

```sh
cargo test -p zebra-rpc verbose_transaction_searches_authorized_orchard_actions_repeatedly_today --lib
```

Result on 2026-05-09: passed.

Triage unchanged: this is local-only public RPC availability hardening, not
private disclosure. RPC is disabled/authenticated by default and the work is
bounded by transaction/block size, but the avoidable repeated signature lookup
is now measured by a focused current-behavior test.

## Follow-up: Inbound Gossiped Block RouterError Misbehavior Proof

Result: upgraded the inbound gossiped-block misbehavior note from type-boundary
evidence to service-level current-behavior proof.

Detailed note:
`docs/analysis/inbound-gossiped-block-router-error-misbehavior-note.md`.

Current proof:

- Kept the existing type-boundary test:
  `score_bearing_router_error_does_not_downcast_to_verify_block_error`.
- Added
  `inbound_router_error_score_is_not_reported_today` in
  `zebrad/src/components/inbound/tests.rs`.
- The new test constructs a real `Inbound` service with real state, mocked
  block-download peer set, mocked semantic block verifier, and a real
  misbehavior channel.
- It queues a gossiped `AdvertiseBlock`, returns a block body with
  `Some(advertiser_addr)`, returns a score-bearing `RouterError` from verifier
  `Request::Commit`, drives `Inbound::poll_ready()` until the download queue is
  empty, and confirms the misbehavior channel remains empty.

Verification:

```sh
cargo test -p zebrad inbound_router_error_score_is_not_reported_today --lib
```

Result on 2026-05-09: passed.

Triage unchanged: this is local-only public P2P hardening, not private
disclosure. Invalid gossiped blocks are still rejected; the issue is missed
address-book scoring/banning for the responding peer on the inbound gossip
cleanup path. The sync downloader path is already type-aware and reports
`RouterError::misbehavior_score()`, so the finding should stay scoped to
inbound gossip.

## Follow-up: QueuedBlocks PoW Route Recheck

Result: no default Mainnet/default-Testnet upgrade, but the custom-network
exposure story is sharper.

Detailed note:
`docs/analysis/state-queued-block-timeout-retention-note.md`.

RepoPrompt rechecked the commit routes into the retained queued-block state. The
normal P2P inbound, P2P sync, and RPC `submitblock` routes all converge through
consensus `Request::Commit` and state `Request::CommitSemanticallyVerifiedBlock`.
On Mainnet and the default public Testnet, those routes still require the normal
difficulty and Equihash checks before a missing-parent block can be queued.

Eliminated as queue routes:

- `getblocktemplate` proposal mode, because it uses
  `ReadRequest::CheckBlockProposalValidity` against a cloned non-finalized
  state and does not mutate the shared queue;
- `TrustedChainSync`, because it commits directly into a local
  `NonFinalizedState` and missing parents fail with `NotReadyToBeCommitted`;
- `generate` as a distinct missing-parent path, because it is gated on
  `network.disable_pow()` and normally builds tip children before submitting.

The remaining cheap route is operator-selected no-PoW networking:

- Regtest sets `disable_pow` via
  `Parameters::new_regtest(...).with_disable_pow(true)`.
- Custom Testnet config exposes `[network.testnet_parameters].disable_pow` and
  passes it to `ParametersBuilder::with_disable_pow(disable_pow)`.
- `book/src/user/custom-testnets.md` explicitly documents `disable_pow = true`.

Triage: still local hardening, not a private default-network disclosure. The
finding is more relevant for Regtest/custom-Testnet deployments where P2P or RPC
is reachable by semi-trusted clients. RepoPrompt later confirmed that a stronger
cross-crate RPC `submitblock` cancellation proof is blocked by observability:
the documented `KnownBlock` queue lookup does not currently report blocks
retained in `non_finalized_state_queued_blocks`, and adding that visibility
would be a production behavior change rather than a pure audit proof.

Additional proof added:

- `dropped_semantic_commit_future_retains_queued_missing_parent_today` in
  `zebra-state/src/service/tests.rs`.
- The test calls the real
  `StateService::call(Request::CommitSemanticallyVerifiedBlock)` boundary with
  a post-mandatory-checkpoint missing-parent block, observes that the block is
  queued before the returned future is awaited, drops that future, and confirms
  the queued entry remains.
- `known_block_misses_queued_missing_parent_today` in the same file.
- The test queues a missing-parent block, confirms the queue retains it
  internally, then shows `Request::KnownBlock(block_hash)` returns
  `KnownBlock(None)` today despite the `KnownBlock` request documentation saying
  block queues are checked.

Verification:

```sh
cargo test -p zebra-state dropped_semantic_commit_future_retains_queued_missing_parent_today --lib
cargo test -p zebra-state known_block_misses_queued_missing_parent_today --lib
```

Result on 2026-05-09: passed.

## Follow-up: RPC getrawtransaction No-Blockhash Snapshot Mix

Result: upgraded the residual no-`blockhash` verbose
`getrawtransaction` snapshot-coherence note from source-evidence-only to a
deterministic RPC-boundary proof.

Detailed note:
`docs/analysis/rpc-getrawtransaction-snapshot-consistency-note.md`.

Current proof:

- Added
  `getrawtransaction_no_blockhash_can_mix_mined_tx_and_best_chain_blockhash_today`
  in `zebra-rpc/src/methods/tests/vectors.rs`.
- The test drives the current no-`blockhash`, verbose path with mocked
  services: the mempool mined-ID lookup misses, `AnyChainTransaction(txid)`
  returns a mined transaction from one continuous-mainnet block, and the later
  `BestChainBlockHash(height)` lookup returns a different block hash for the
  same height.
- The response currently combines the transaction bytes, height,
  confirmations, and block time from the mined transaction metadata with the
  mismatched later best-chain block hash, while still reporting
  `in_active_chain = true`.

Verification:

```sh
cargo test -p zebra-rpc getrawtransaction_no_blockhash_can_mix_mined_tx_and_best_chain_blockhash_today --lib
```

Result on 2026-05-09: passed.

Triage unchanged: local-only RPC hardening, not private disclosure. RPC is
disabled or cookie-authenticated by default, and the issue is provenance
metadata coherence rather than consensus validity or arbitrary transaction
substitution. Practical impact still depends on active chain movement and on
downstream clients treating verbose RPC metadata as an atomic chain-membership
statement.

## Follow-up: OpenTelemetry Sampler Env Mismatch Proof

Result: upgraded the `OTEL_TRACES_SAMPLER_ARG` ratio-vs-percent subcase from
source evidence to deterministic tests.

Detailed note:
`docs/analysis/sentry-opentelemetry-privacy-note.md`.

RepoPrompt selected this as the freshest remaining deployment-facing lane
because it is independent from the already proof-backed RPC/tracing metric
cardinality work and has a compact pure-helper proof path.

Current proof:

- Extracted `resolve_otel_runtime_config()` in
  `zebrad/src/components/tracing/component.rs`.
- Added env-guarded tests proving that `OTEL_TRACES_SAMPLER=traceidratio` plus
  `OTEL_TRACES_SAMPLER_ARG=0.1` leaves Zebra with no parsed integer sampling
  percentage and an effective default of 100%.
- Added controls proving integer `OTEL_TRACES_SAMPLER_ARG=10` is treated as
  10%, `OTEL_TRACES_SAMPLER` alone is ignored, and Zebra config overrides the
  OTEL env fallback.
- Extracted `sample_rate_from_percent()` in
  `zebrad/src/components/tracing/otel.rs` and added tests proving
  `None -> 1.0`, `Some(10) -> 0.10`, and oversized percentages clamp to full
  sampling.

Verification:

```sh
cargo test -p zebrad components::tracing::component::tests --lib
cargo test -p zebrad components::tracing::otel::tests --lib
```

Result on 2026-05-09: passed.

Triage unchanged: local-only operational/privacy hardening, not private
disclosure. Telemetry export still requires an explicitly configured OTLP
endpoint, but conventional OpenTelemetry ratio-style sampler configuration can
silently fail open to full sampling under that opted-in deployment.

## Follow-up: Metrics Endpoint Idle-Connection Proof

Result: upgraded the optional Prometheus metrics endpoint note from
source-evidence-only to partial current-behavior proof plus local
dependency-source evidence.

Detailed note:
`docs/analysis/metrics-endpoint-connection-hardening-note.md`.

RepoPrompt cautioned that a bounded black-box wait cannot prove the complete
absence of request/header timeouts, connection caps, or allowlists. The proof
therefore stays narrow.

Current proof:

- Added
  `prometheus_metrics_connection_waits_without_request_timeout_today` in
  `zebrad/src/components/metrics.rs`.
- The test uses `metrics_exporter_prometheus::PrometheusBuilder::build()` rather
  than Zebra's production `install()` path, so it avoids mutating the
  process-global metrics recorder.
- It opens a local TCP connection to the exporter, sends no request bytes for
  250 ms, then sends a normal scrape on the same connection and receives a
  successful metrics response containing a probe metric.
- Local dependency source for `metrics-exporter-prometheus` 0.16.2 confirms the
  listener accepts streams, checks an optional allowlist that defaults to
  allow-all, and spawns Hyper `serve_connection()` directly; the builder's
  `idle_timeout()` is metric-recency cleanup, not connection/request timeout.

Verification:

```sh
cargo test -p zebrad prometheus_metrics_connection_waits_without_request_timeout_today --lib
```

Result on 2026-05-09: passed.

Triage unchanged: local-only deployment hardening, not private disclosure. The
metrics endpoint is disabled by default and normally documented for localhost,
but copied broad-bind deployments can expose a listener whose connection
lifecycle is owned by the dependency rather than Zebra-side timeout/cap logic.

## Follow-up: RPC Pre-Guard Server-Boundary Proof

Result: upgraded the RPC pre-guard HTTP body-retention note from direct
middleware proof plus dependency-source ordering to a real `RpcServer` boundary
proof.

Detailed note:
`docs/analysis/rpc-pre-guard-http-connection-retention-note.md`.

RepoPrompt selected this as the best next local-only target because the overlap
ledger has no public issue match, the path sits on the network edge, and the
existing unit proof did not yet show the behavior across the actual server
composition.

Current proof:

- Added
  `rpc_server_incomplete_body_waits_before_dispatch_today` in
  `zebra-rpc/src/server/tests/vectors.rs`.
- The test starts the actual auth-disabled `RpcServer`, opens three HTTP
  connections, sends complete valid JSON-RPC body bytes, but declares a larger
  `Content-Length`.
- Each connection remains open without a response after a short wait, and the
  mocked mempool, state, read-state, and block-verifier services receive no
  requests.

Verification:

```sh
cargo test -p zebra-rpc rpc_server_incomplete_body_waits_before_dispatch_today --lib
```

Result on 2026-05-09: passed.

Triage unchanged: local-only RPC availability hardening, not private
disclosure. Wrong-auth requests still return before body collection, but
auth-disabled or valid-auth slow bodies can hold server work before method
dispatch and before Zebra-owned outer admission/timeout logic exists.
