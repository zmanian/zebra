# Post-v4.4.0 Security Audit Pass 5 Addendum

Date: 2026-05-04

Scope: continuation of pass 5 after the initial private SIGHASH_SINGLE disclosure
and the public-hardening notes already collected in `docs/analysis/`.

## Summary

This continuation did not identify a brand-new consensus divergence issue. It
did strengthen three already-open private-leaning availability areas:

- mempool downloader outer-timeout stale cancel-handle retention, including
  direct pushed-transaction retention;
- address-book misbehavior-ban panic when `max_connections_per_ip > 1`; and
- RPC `invalidateblock` / `reconsiderblock` process-fatal panics in
  non-finalized state chain-set handling.

Other rechecked branches had existing backstops or remain public-hardening
notes.

## Rechecked Branches

### RPC Subsidy Height Handling

`get_block_subsidy` accepts caller-supplied heights, but far-future heights are
handled by checked subsidy arithmetic and return zero subsidy rather than
overflowing or panicking.

Evidence:

- `zebra-rpc/src/methods.rs:2800-2860`
- `zebra-chain/src/parameters/network/subsidy.rs:412-478`

Existing snapshot coverage includes excessive and future-height subsidy cases.

### RPC `generate`

The uncapped `generate(num_blocks: u32)` loop remains a public hardening issue
only. It is already documented in
`docs/analysis/rpc-generate-uncapped-disabled-pow-note.md`.

Evidence:

- `zebra-rpc/src/methods.rs:2946-3013`

### Activation Boundary Reset

The exact activation-height boundary has the expected reset backstop:
`ChainTipChange::action()` returns `TipAction::Reset` when the new tip height is
an activation height, and the mempool reset path clears state, cancels in-flight
downloads, and requeues retries.

Evidence:

- `zebra-state/src/service/chain_tip.rs:585-603`
- `zebrad/src/components/mempool.rs:528-586`
- `zebrad/src/components/mempool/tests/vector.rs:679-760`

Targeted tests:

```sh
cargo test -p zebra-chain activates_network_upgrades_correctly --lib
cargo test -p zebrad mempool_cancel_downloads_after_network_upgrade
```

Result: both passed.

### Mempool Dependency / Eviction Shape

The dependency-removal reporting issue was independently rediscovered during
this pass, but it is already captured in
`docs/analysis/mempool-cascading-removal-notification-note.md` and summarized in
the pass-5 findings. It remains public hardening rather than private disclosure.

Evidence:

- `zebra-node-services/src/mempool/transaction_dependencies.rs:84-118`
- `zebrad/src/components/mempool/storage/verified_set.rs:218-238`
- `zebrad/src/components/mempool/storage.rs:463-495`

### Mempool Downloader Timeout Retention

The stale cancel-handle candidate remains a private maintainer heads-up
candidate rather than a fully confirmed default-node exploit. The source bug is
high confidence: outer timeout results do not carry a txid, so
`Downloads::poll_next()` cannot remove the corresponding `cancel_handles` entry.

This continuation added a direct pushed-transaction proof showing that if the
outer timeout wins for `Gossip::Tx`, the retained request still contains the
full `UnminedTx`, not merely an ID.

Evidence:

- `zebrad/src/components/mempool/downloads.rs:179`
- `zebrad/src/components/mempool/downloads.rs:215-228`
- `zebrad/src/components/mempool/downloads.rs:413-450`
- `zebrad/src/components/mempool/downloads.rs:595-696`
- `docs/analysis/mempool-downloader-timeout-cancel-handle-retention-finding.md`

Targeted tests:

```sh
cargo test -p zebrad timed_out_downloads_accumulate_cancel_handles_today --lib
cargo test -p zebrad timed_out_pushed_transaction_retains_full_request_today --lib
cargo test -p zebrad components::mempool::downloads::tests --lib
```

Result: all passed.

### Address-Book Misbehavior Ban Panic

The non-default `network.max_connections_per_ip > 1` address-book panic remains
a private maintainer heads-up candidate. The code accepts positive
`max_connections_per_ip` values above 1, disables `most_recent_by_ip` for those
configs, but the ban-threshold misbehavior path still unconditionally unwraps
that optional map.

This continuation made the current-behavior repro durable as a focused
`#[should_panic]` test.

Evidence:

- `zebra-network/src/address_book.rs:76-82`
- `zebra-network/src/address_book.rs:158-169`
- `zebra-network/src/address_book.rs:443-458`
- `zebra-network/src/address_book/tests/vectors.rs:40-50`
- `docs/analysis/address-book-misbehavior-ban-max-connections-panic-finding.md`

Targeted test:

```sh
cargo test -p zebra-network misbehavior_ban_panics_with_max_connections_per_ip_above_one_today --lib
```

Result: passed.

### Finalization / Reorg Read Boundary

The finalization-adjacent anchor/nullifier disappearance race remains
eliminated for the reviewed read paths. Readers use published watch snapshots;
the writer-private post-finalize/pre-DB-commit state is not exposed to mempool
or proposal-validation reads.

Evidence:

- `zebra-state/src/service/write.rs:428-450`
- `zebra-state/src/service.rs:923-932`
- `zebra-state/src/service.rs:1573-1588`
- `zebra-state/src/service.rs:1655-1691`

The low-severity invalidated-record retention off-by-one remains documented in
`docs/analysis/finalization-invalidated-record-retention-note.md`.

Targeted tests:

```sh
cargo test -p zebra-state service::check::tests::anchors --lib
cargo test -p zebra-state service::check::tests::nullifier --lib
cargo test -p zebra-state service::non_finalized_state::tests::prop::forked_equals_pushed --lib
```

Result: all passed.

### Prometheus Dynamic Labels

The attacker-influenced metric-label issue is already documented in
`docs/analysis/prometheus-cardinality-security-note.md`. The relevant current
families are peer handshake/message labels, mempool verification failure
`reason`, and RPC `method`.

Evidence:

- `zebra-network/src/peer/handshake.rs:760-814`
- `zebrad/src/components/mempool.rs:656-658`
- `zebra-rpc/src/server/rpc_metrics.rs:44-86`

### RPC Cookie File / Lifecycle

The RPC cookie existing-file and lifecycle findings now have durable
current-behavior tests instead of temporary proof snippets.

Evidence:

- `zebra-rpc/src/server/tests/cookie.rs:51`
- `zebra-rpc/src/server/tests/vectors.rs:213`
- `zebra-rpc/src/server/tests/vectors.rs:271`
- `docs/analysis/rpc-cookie-existing-file-permissions-note.md`
- `docs/analysis/rpc-cookie-lifecycle-cleanup-note.md`

Targeted tests:

```sh
cargo test -p zebra-rpc cookie_write_preserves_existing_regular_file_permissions_today --lib
cargo test -p zebra-rpc rpc_server_start_failure_leaves_cookie_today --lib
cargo test -p zebra-rpc rpc_server_task_abort_leaves_cookie_today --lib
```

Result: all passed.

### ZIP-235 Miner-Fee Share Panic

The future-gated ZIP-235 miner-fee share intermediate-overflow panic now has a
durable current-behavior test in `zebra-consensus/src/block/tests.rs`.

Evidence:

- `zebra-consensus/src/block/check.rs:337-344`
- `zebra-consensus/src/block/tests.rs:702`
- `docs/analysis/zip235-miner-fee-share-intermediate-overflow-panic-note.md`

Targeted tests:

```sh
RUSTFLAGS='--cfg zcash_unstable="nu7" --cfg zcash_unstable="zip235"' \
  cargo test -p zebra-consensus \
  miner_fees_validation_panics_when_zip233_fee_share_intermediate_overflows_today \
  --features tx_v6 --lib

RUSTFLAGS='--cfg zcash_unstable="nu7" --cfg zcash_unstable="zip235"' \
  cargo test -p zebra-consensus miner_fees_validation --features tx_v6 --lib
```

Result: both passed. The first passes as a `#[should_panic]` proof; the second
confirms the surrounding unstable miner-fee tests still pass.

### RPC Longpoll Unicode Panic

The `getblocktemplate` `longpollid` parser panic remains a private RPC
availability heads-up candidate. The current checkout already has durable
`#[should_panic]` tests proving both the direct `LongPollId` parser panic and
the RPC parameter-deserialization path.

Evidence:

- `zebra-rpc/src/methods/types/long_poll.rs:265-287`
- `zebra-rpc/src/methods/types/long_poll.rs:331-351`
- `docs/analysis/rpc-longpollid-unicode-panic-finding.md`

Targeted test:

```sh
cargo test -p zebra-rpc non_ascii_long_poll_id --lib
```

Result: passed.

### RPC Unified Address Sapling Receiver Panic

The `z_listunifiedreceivers` invalid Sapling receiver panic is also stronger
after this continuation. The previously temporary proof is now a durable
current-behavior test in `zebra-rpc/src/methods/tests/vectors.rs`.

The test constructs a syntactically valid Unified Address with a length-valid
but semantically invalid Sapling receiver (`Receiver::Sapling([0; 43])`). The
Unified Address parser accepts the structure, then `z_listunifiedreceivers`
unwraps `Address::try_from_sapling()` and panics with
`using data already decoded as valid`.

Evidence:

- `zebra-rpc/src/methods.rs:2867-2915`
- `zebra-chain/src/primitives/address.rs:67-75`
- `zebra-chain/src/primitives/address.rs:77-131`
- `zebra-rpc/src/methods/tests/vectors.rs:3220`
- `docs/analysis/rpc-z-listunifiedreceivers-invalid-sapling-panic-finding.md`

Targeted test:

```sh
cargo test -p zebra-rpc rpc_z_listunifiedreceivers_panics_on_invalid_sapling_receiver_today --lib
```

Result: passed.

### P2P Inventory `notfound` Registration

The unsolicited-`notfound` routing-poisoning proof is now a durable
current-behavior test in
`zebra-network/src/peer_set/inventory_registry/tests/vectors.rs`.

The test sends `Message::NotFound(vec![block_hash])` through
`register_inventory_status()` with an inbound direct peer address and no request
correlation context. The wrapper forwards the message and the inventory
registry records the peer in `missing_peers(block_hash)`.

Evidence:

```sh
cargo test -p zebra-network unsolicited_notfound_registers_missing_inventory_today --lib
cargo test -p zebra-network peer_set::inventory_registry::tests::vectors --lib
```

Result: passed.

### RPC Invalidation / Reconsideration Panics

The `invalidateblock` and `reconsiderblock` panic cluster is stronger after this
continuation. The three previously scratch-only proof shapes are now durable
current-behavior tests in
`zebra-state/src/service/non_finalized_state/tests/vectors.rs`.

Confirmed shapes:

- invalidating a non-finalized chain root calls `BTreeSet::remove(&chain)` and
  panics when `Chain::cmp` compares the stored chain with itself;
- invalidating two same-height sibling fork tips in sequence tries to insert the
  same shortened parent-only chain twice; and
- reconsidering the same invalidated block twice replays a stale invalidated
  entry because `reconsider_block()` removes from
  `self.invalidated_blocks.clone()` rather than the live map.

Evidence:

- `zebra-state/src/service/non_finalized_state.rs:375-414`
- `zebra-state/src/service/non_finalized_state.rs:421-503`
- `zebra-state/src/service/non_finalized_state/chain.rs:2320-2345`
- `zebra-state/src/service/non_finalized_state/tests/vectors.rs:402`
- `zebra-state/src/service/non_finalized_state/tests/vectors.rs:434`
- `zebra-state/src/service/non_finalized_state/tests/vectors.rs:473`
- `docs/analysis/rpc-invalidateblock-chain-root-panic-finding.md`
- `docs/analysis/rpc-invalidateblock-same-height-fork-panic-finding.md`
- `docs/analysis/rpc-reconsiderblock-stale-invalidated-entry-panic-finding.md`

Targeted tests:

```sh
cargo test -p zebra-state invalidating_chain_root_panics_when_removing_existing_chain_today --lib
cargo test -p zebra-state invalidating_same_height_fork_tips_panics_today --lib
cargo test -p zebra-state reconsider_block_twice_replays_stale_invalidated_entry_today --lib
cargo test -p zebra-state service::non_finalized_state::tests::vectors --lib
```

Result: all passed.

### Value-Pool Error Suppression

The `Block::chain_value_pool_change()` error-suppression bug is stronger after
this continuation. The helper bug was previously proven with a temporary
`zebra-chain` test; it is now covered by a durable current-behavior test in
`zebra-chain/src/block/tests/vectors.rs`.

The test constructs a V1 coinbase transaction with two individually valid
`MAX_MONEY` transparent outputs. The transaction-level
`Transaction::value_balance()` returns `Err`, but block-level
`chain_value_pool_change()` currently drops that error and returns a zero
value-pool delta.

Evidence:

- `zebra-chain/src/block.rs:228-244`
- `zebra-chain/src/transaction.rs:1463-1470`
- `zebra-chain/src/block/tests/vectors.rs:95`
- `docs/analysis/value-pool-error-suppression-note.md`

Targeted test:

```sh
cargo test -p zebra-chain chain_value_pool_change_drops_transaction_value_balance_errors_today --lib
```

Result: passed.

Disclosure triage remains conservative: private maintainer heads-up because
this is validator value-pool accounting code, while direct remote exploitability
through normal semantic block verification remains low on current evidence.

### GBT High-Fee Coinbase Overflow Panic

The `getblocktemplate` high-fee coinbase overflow panic remains public mining
RPC availability hardening, not private disclosure. This continuation converted
the previous temporary helper proof into a durable current-behavior test in
`zebra-rpc/src/methods/types/get_block_template/tests.rs`.

The test uses a custom NU6-active network and calls
`standard_coinbase_outputs()` with `miner_fee = MAX_MONEY`. The helper panics
when it computes `miner_subsidy + miner_fee` and unwraps the overflowing
`Amount` result.

Evidence:

- `zebra-rpc/src/methods/types/get_block_template.rs:867-881`
- `zebra-rpc/src/methods/types/get_block_template/tests.rs:49`
- `docs/analysis/getblocktemplate-high-fee-coinbase-overflow-panic-note.md`

Targeted tests:

```sh
cargo test -p zebra-rpc standard_coinbase_outputs_panics_when_fee_plus_subsidy_exceeds_max_money_today --lib
cargo test -p zebra-rpc methods::types::get_block_template::tests --lib
```

Result: both passed.

## Disclosure Update

No new consensus-private issue came out of this continuation.

The existing private-disclosure set should still include the previously
identified SIGHASH issue. It should also include private maintainer heads-up
coverage for the availability and validator-accounting candidates strengthened
here:

- mempool downloader timeout stale cancel-handle retention, because a remote
  path reaches the downloader and the direct pushed-transaction timeout shape
  retains full transaction contents; and
- address-book misbehavior-ban panic for supported but non-default
  `max_connections_per_ip > 1` deployments; and
- RPC invalidation/reconsideration panics, because trusted RPC methods can reach
  process-fatal `Chain::cmp` duplicate-tip panics through normal state APIs; and
- RPC `z_listunifiedreceivers` invalid Sapling receiver panic, because a
  structurally valid caller-supplied Unified Address can still contain receiver
  bytes that fail semantic Sapling address validation and hit an `expect()`; and
- value-pool error suppression, because a transaction-level
  `ValueBalanceError` can be silently omitted from block-level chain value-pool
  accounting even though default semantic verification appears to reject the
  concrete synthetic overflow shape earlier.

The already-documented RPC `longpollid` Unicode parser panic remains in the
private heads-up bucket as well.

The remaining branches above are either eliminated or public hardening.
