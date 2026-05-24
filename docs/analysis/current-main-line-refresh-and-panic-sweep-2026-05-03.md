# Current Main Line Refresh And Panic Sweep

Date: 2026-05-03

Scope: follow-up execution for
`docs/analysis/post-v4.4.0-security-audit-pass-5-plan.md`, focused on two
remaining confidence gaps:

- refresh sensitive line references against current Zebra `origin/main`;
- re-check RPC/P2P process-fatal panic hypotheses that were not already
  confirmed as direct remote issues.

## Source Snapshot

Local Zebra:

- `origin/main` fetched from `ZcashFoundation/zebra`.
- `HEAD`, `origin/main`, and `FETCH_HEAD` all resolved to
  `589d64b9b7ea6ab4c32ecab41ba6b74f26907940` after refresh.
- The worktree has unrelated local modifications, so dirty files were checked
  through `origin/main:<path>` / `git grep` rather than working-tree line
  numbers where needed.

Upstream comparison sources:

- zcashd `master` resolved through GitHub API to
  `1396d7920057011f7dd604f9dbcf1298e3794418`.
- librustzcash `main` resolved through GitHub API to
  `87c0a882a197f3a6b82ff6cad9c62a291a435844`.

## Refreshed Private / Heads-Up Candidates

### V5 `SIGHASH_SINGLE` Missing Corresponding Output

Classification unchanged: already privately disclosed; high-confidence
consensus-divergence candidate.

Current Zebra line references:

- `zebra-script/src/lib.rs:211-218`: Zebra maps the C++ callback hash type to
  the local ZIP-244 sighasher and calls
  `.sighash(our_hash_type, Some((input_index, script_code_vec)))` without an
  explicit wrapper-level missing-output rejection.
- `zebra-consensus/src/transaction.rs:404-456`: the full transaction verifier
  performs branch ID, expiry, locktime, and conflict checks before async
  verification.
- `zebra-consensus/src/transaction.rs:481-509`: the verifier builds the
  `CachedFfiTransaction` and sends V5 transactions through
  `verify_v5_transaction(...)`.

Current upstream comparison:

- zcashd `src/script/interpreter.cpp:1237-1259` documents the ZIP-244
  `hash_type` restrictions and throws if `SIGHASH_SINGLE` or
  `SIGHASH_SINGLE|ANYONECANPAY` is used with no corresponding output.
- librustzcash
  `zcash_primitives/src/transaction/sighash_v5.rs:100-106` still computes
  `transparent_outputs_hash::<TxOut>(&[])` when `SIGHASH_SINGLE` has no
  corresponding output. That confirms the lower-level digest routine remains an
  insufficient backstop by itself.

Smallest fix direction: add an explicit V5 transparent-sighash precheck before
calling the digest helper, rejecting `SIGHASH_SINGLE` variants where
`input_index >= outputs.len()`, matching zcashd's wrapper-level behavior.

### RPC `longpollid` Unicode Panic

Classification unchanged: private RPC availability heads-up candidate.

Current Zebra line references:

- `zebra-rpc/src/methods/types/long_poll.rs:22` defines
  `LONG_POLL_ID_LENGTH` as 46 bytes.
- `zebra-rpc/src/methods/types/long_poll.rs:265-286` implements
  `FromStr for LongPollId`, checks `long_poll_id.len()`, and then slices the
  original UTF-8 string at fixed byte offsets `0..10`, `10..18`, `18..28`,
  `28..38`, and `38..LONG_POLL_ID_LENGTH`.

Confidence remains high: byte-length validation does not guarantee those byte
offsets are UTF-8 character boundaries.

Smallest fix direction: parse from `long_poll_id.as_bytes()` or first require
ASCII with a normal RPC parameter error before any fixed-offset slicing.

### RPC `z_listunifiedreceivers` Invalid Sapling Receiver Panic

Classification unchanged: private RPC availability heads-up candidate.

Current Zebra line references:

- `zebra-rpc/src/methods.rs:2867-2877`: the method decodes the caller-supplied
  Unified Address string with `zcash_address::unified::Encoding::decode(...)`.
- `zebra-rpc/src/methods.rs:2891-2894`: for a decoded Sapling receiver, Zebra
  calls `Address::try_from_sapling(network, data).expect("using data already
  decoded as valid")`.

Confidence remains high from the local temporary repro described in
`docs/analysis/rpc-z-listunifiedreceivers-invalid-sapling-panic-finding.md`.
The refreshed source still has the same `expect` at the RPC boundary.

Smallest fix direction: map failed Sapling receiver semantic validation into an
RPC invalid-parameter error, just as the outer Unified Address decode already
does.

### Address-Book Misbehavior Ban Panic With `max_connections_per_ip > 1`

Classification unchanged: private maintainer heads-up candidate for non-default
network availability.

Current Zebra line references:

- `zebra-network/src/address_book.rs:76-82`: `most_recent_by_ip` is documented
  as only supporting `max_connections_per_ip == 1` and being `None` for larger
  configured values.
- `zebra-network/src/address_book.rs:160-168`: `most_recent_by_ip` is only
  created when `should_limit_outbound_conns_per_ip` is true.
- `zebra-network/src/constants.rs:389-391`: the ban threshold is
  `MAX_PEER_MISBEHAVIOR_SCORE = 100`.
- `zebra-network/src/meta_addr.rs:315-321`: `UpdateMisbehavior` carries the
  score increment.
- `zebra-network/src/meta_addr.rs:958-966`: `UpdateMisbehavior` contributes
  the configured misbehavior score.
- `zebra-network/src/address_book.rs:443-458`: when the updated score reaches
  the ban threshold, Zebra unconditionally unwraps `most_recent_by_ip` before
  removing the banned IP.
- `zebra-network/src/address_book_updater.rs:101-114`: the updater applies the
  change while holding the address-book mutex and expects the mutex to remain
  unpoisoned.
- `zebra-network/src/peer_set/initialize.rs:149-155`: accumulated
  misbehavior reports are forwarded as `UpdateMisbehavior`.

Smallest fix direction: in the ban branch, remove from `most_recent_by_ip` only
when the optional cache exists.

### Mempool Infrastructure Failures Become Exact-Tip Rejections

Classification unchanged: private maintainer heads-up candidate if ZF wants
transient operational availability issues through the security inbox.

Current Zebra line references:

- `zebrad/src/components/mempool/downloads.rs:387-393`: the download verifier
  maps verifier errors into `TransactionDownloadVerifyError::Invalid`.
- `zebra-consensus/src/error.rs:248-257`: boxed transaction-verifier errors
  that do not downcast to known transaction errors become
  `TransactionError::InternalDowncastError`.
- `zebrad/src/components/mempool.rs:641-649`: invalid download/verify results
  are treated as verification failures and can drive peer scoring.
- `zebrad/src/components/mempool/storage.rs:865-868`: every `Invalid` result is
  stored as `ExactTipRejectionError::FailedVerification(error)`.
- `zebra-consensus/src/transaction.rs:649-651`: mempool locktime state-service
  failures become `ValidateMempoolLockTimeError`.
- `zebra-consensus/src/transaction.rs:705-707`: state-service failures while
  looking up `UnspentBestChainUtxo` become `TransparentInputNotFound`.
- `zebra-consensus/src/transaction.rs:742-748`: `AwaitOutput` timeout becomes
  `TransparentInputNotFound`.

Smallest fix direction: preserve the distinction between verifier
infrastructure errors and semantic invalidity before reaching storage rejection
caches.

### Mining RPC Verifier Timeout Gap

Classification unchanged: private maintainer heads-up if bundled with miner
availability concerns; otherwise public RPC hardening.

Current Zebra line references:

- `zebra-rpc/src/methods.rs:2553-2578`: `submitblock` deserializes the block,
  awaits `block_verifier_router.ready()`, and then awaits
  `.call(Request::Commit(...))` without an RPC-level timeout.
- `zebra-rpc/src/methods.rs:2989-2998`: getblocktemplate proposal mode builds a
  proposal block and reuses `submit_block(...)`, inheriting the same wait
  behavior.

Smallest fix direction: put a mining-RPC deadline around both `ready()` and the
verifier call, and preserve enough error taxonomy to avoid reporting verifier
infrastructure failures as ordinary consensus rejection.

### RPC Cookie Permission / Lifecycle Heads-Up

Classification unchanged: private local-credential hardening heads-up.

Current Zebra line references:

- `zebra-rpc/src/server.rs:127-154`: `RpcServer::start()` writes the cookie
  before building the server. If build/bind fails after the write, no cleanup is
  performed in this path.
- `zebra-rpc/src/server.rs:158-161`: the returned task only awaits
  `server.start(...).stopped()` and returns; it does not own cleanup.
- `zebra-rpc/src/server.rs:187-204`: cleanup exists in
  `shutdown_blocking_inner(...)`.
- `zebra-rpc/src/server.rs:219-225`: `Drop for RpcServer` calls shutdown, but
  the live `start()` API returns only `ServerTask`.
- `zebra-rpc/src/server/cookie.rs:43-59`: cookie writing rejects symlinks and
  writes `__cookie__:<secret>`.
- `zebra-rpc/src/server/cookie.rs:71-78`: the `OpenOptionsExt::mode(0o600)`
  mode applies when creating a file, but opening an existing regular file with
  `.create(true).truncate(true)` does not chmod it tighter.

Smallest fix direction: chmod/fchmod existing regular cookie files to `0600`
before or after rewriting, and make the live server task own cookie cleanup on
startup failure and normal shutdown.

### Block Chain Value-Pool Calculation Suppresses Transaction Errors

Classification unchanged: private maintainer heads-up because it touches
validator value-pool accounting, while direct remote exploitability through
normal semantic verification remains unproven.

Current Zebra line references:

- `zebra-chain/src/block.rs:228-244`: `Block::chain_value_pool_change()` uses
  `.flat_map(|t| t.value_balance(utxos))` over per-transaction `Result`s,
  causing `Err` values to produce no item before the block sum.
- `zebra-state/src/service/finalized_state/disk_format/upgrade/block_info_and_address_received.rs:200-210`:
  the v27 block-info/address-received migration adds
  `block.chain_value_pool_change(...).unwrap_or_default()` to the running value
  pool, suppressing any helper error into a zero delta.

Smallest fix direction: replace `flat_map` with an iterator that collects or
sums `Result` values without dropping errors; remove the migration
`unwrap_or_default()` and fail the upgrade loudly if value accounting cannot be
computed.

### ZIP-235 Miner-Fee Share Intermediate Overflow Panic

Classification unchanged: conservative private future-activation heads-up.

Current Zebra line references:

- `zebra-consensus/src/block/check.rs:337-341`: under
  `zcash_unstable = "zip235"` at NU7 activation, validation computes
  `((block_miner_fees * 6).unwrap() / 10).unwrap()`.
- `zebra-chain/src/transaction/builder.rs:90-95`: V6 coinbase generation uses
  the same overflow-prone expression when no explicit ZIP-233 amount is
  supplied.
- `zebra-chain/src/amount.rs:377-394`: `Amount * u64` performs checked
  multiplication and returns `MultiplicationOverflow` when the intermediate
  product leaves the amount range.
- `zebra-chain/src/amount.rs:580-610`: `Amount<NonNegative>` is constrained to
  `0..=MAX_MONEY`.
- `zebra-rpc/src/methods/types/get_block_template.rs:811-830`: template
  generation passes selected mempool fees into V6 coinbase generation under the
  combined NU7 / tx-v6 / ZIP-235 configuration.
- `zebrad/Cargo.toml:144`: `tx_v6` remains an explicit opt-in feature.

Smallest fix direction: compute the 60% share in a wider integer type or with a
ratio helper that constrains only the final amount, and share that helper
between consensus validation and coinbase generation.

## Fresh RPC / P2P Panic Surface Sweep

An independent context-builder pass re-examined remote-triggered
`panic!` / `assert!` / `expect` / `unwrap` surfaces in the selected RPC and P2P
boundary paths. I checked its output against the source and did not add a new
private disclosure item from that sweep.

Eliminated or downgraded leads:

- `zebra-rpc/src/server/rpc_call_compatibility.rs` still has `expect` sites for
  framework-produced error response shape, but malformed client JSON is parsed
  by jsonrpsee before this middleware sees a `MethodResponse`.
- `zebra-rpc/src/server/http_request_compatibility.rs` has JSON-RPC 2.0
  response-shape assertions, but the selected path operates on internal
  framework responses. Existing tests cover body collection and oversized body
  errors returning `Err`, not panicking.
- `zebra-rpc/src/methods.rs` address-index order asserts are externally
  callable through RPC, but the ordering is produced by state `BTreeMap` /
  address-index helpers. Bad RPC parameters do not directly control returned
  order.
- `zebra-rpc/src/methods.rs` `invalidate_block` asserts the success response
  matches `Response::Invalidated(block_hash)`, and
  `zebra-state/src/service.rs` maps the matching request success path directly
  to that response variant.
- `zebra-network/src/protocol/external/codec.rs` converts malformed peer bytes
  into parse/serialization errors.
- `zebra-network/src/peer/handshake.rs` preserves decode failures as
  `Result<Message, SerializationError>`.
- `zebra-network/src/peer/connection.rs` handles `Some(Err(e))` by failing the
  peer connection, so bad wire bytes do not become internal state-machine
  `Message` values.
- `zebra-rpc/src/indexer/server.rs` has a tonic reflection `unwrap()` during
  server startup, not in a runtime client-input path.

Residual hardening recommendation: replace RPC/state invariant panics that sit
behind externally callable methods with explicit internal errors and telemetry,
especially address-index consistency assertions. I do not currently classify
those as private disclosure without a separate state-corruption or concurrency
proof.

## Address-Index Assertion Follow-Up

I did one extra source pass on the strongest residual panic-surface lead from
the RPC/P2P sweep: address-index queries are externally callable through RPC,
and their state helpers contain `assert!` / `expect` sites.

Result: still eliminated as a fresh private issue on current evidence.

Relevant code backstops:

- `zebra-state/src/service/read/address/tx_id.rs:1-10` and
  `zebra-state/src/service/read/address/utxo.rs:1-10` explicitly document the
  intended stale-chain/finalized-DB overlap: the block write task can commit
  blocks to finalized state after a read request has cloned the cached
  non-finalized chain.
- `zebra-state/src/service/write.rs:421-449` publishes the latest
  non-finalized chain to readers before the finalization loop commits roots to
  disk. That explains why readers can see blocks in both places.
- `zebra-state/src/service/read/address/tx_id.rs:49-62` and
  `zebra-state/src/service/read/address/utxo.rs:126-186` retry finalized DB
  reads when finalization movement creates an overlap window.
- `zebra-state/src/service/read/address/tx_id.rs:168-185` and
  `zebra-state/src/service/read/address/utxo.rs:289-307` return explicit errors
  when the finalized query moved but there is no non-finalized chain to
  compensate.
- `zebra-state/src/service/read/address/tx_id.rs:225-243` and
  `zebra-state/src/service/read/address/utxo.rs:346-360` return explicit errors
  when the required overlap extends above the cloned non-finalized chain tip.
- The remaining overlap `assert!` sites require a broken internal `Chain`
  contiguity/index invariant after those bounds checks. A malformed RPC request
  controls addresses and height ranges, but not the `BTreeMap` ordering or the
  `Chain.blocks` membership for the required overlap heights.

Hardening recommendation unchanged: convert these RPC-reachable internal
assertions into explicit internal errors, but do not disclose them as a
standalone remote crash without a separate proof that peer-driven state movement
can violate the chain-contiguity invariant.

## Completion Status Update

This note satisfies the pass-5 line-refresh criterion for the current sensitive
candidate set I rechecked in this follow-up. It does not claim all public
hardening line references across the large pass-5 report have been refreshed.
Before opening any public issues or sending additional private detail, refresh
the exact lines for the specific subset being disclosed one more time against
the then-current `origin/main`.
