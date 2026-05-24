# Post-v4.4.0 security audit pass 5 plan

Date: 2026-05-02

## Goal

Run a fifth audit pass that broadens beyond the verifier-result taxonomy and
waiter-cleanup work of passes 3 and 4 into surfaces that have not yet been
exercised: the rest of the ZIP-244 transparent sighash matrix, activation-height
boundaries, anchor and nullifier handling under reorg, value-pool arithmetic,
FFI safety, semantic network DoS, RPC auth ordering, mempool DoS parity, and
time-related consensus.

The pass is split into two tracks so disclosure stays clean:

- Track A is consensus-critical. Findings stay private until maintainers confirm
  handling.
- Track B is availability and hardening. Findings can become public maintainer
  issues directly when confirmed.

This pass stays local: no public exploit payloads, no mainnet testing, and no
PR until the Zebra contribution gate is satisfied.

## Outputs

- A pass-5 findings document in `docs/analysis/`, with Track A and Track B
  sections clearly separated.
- One reproducer or regression-test sketch per confirmed issue.
- A table of eliminated leads with the exact code backstop that makes them safe.
- A fix-priority list ordered by consensus impact, then miner availability,
  then RPC/mempool availability, then defense-in-depth.

## Disclosure ground rules

- Track A findings: do not commit reproducer payloads or exact line-pointers to
  exploitable conditions until ZF confirms. Keep tests in private notes.
- Track B findings: file directly as maintainer issues with the reproducer
  sketch. The contribution gate still applies before any PR.
- Refresh live `main` / GitHub source and verify line citations before any
  disclosure text leaves the laptop. v4.4.0 release shifted line numbers;
  pass-1 through pass-4 citations need a refresh.

---

## Track A: consensus-critical

### Workstream A1: ZIP-244 transparent sighash matrix beyond SIGHASH_SINGLE

Question: are there other ZIP-244 wrapper-level rules whose only enforcement in
Zebra is implicit in `zcash_primitives` digest behavior, the way SIGHASH_SINGLE
missing-output was?

Primary files:

- `zebra-script/src/lib.rs`
- `zebra-script/src/tests.rs`
- `zebra-chain/src/transaction/sighash.rs`
- `zebra-chain/src/primitives/zcash_primitives.rs`
- zcashd source for ZIP-244 wrapper (compare against current zcashd `master`)

Commands:

```bash
rg -n "SignedOutputs|HashType::|raw_bits|from_bits|sighash_v5|sighash_v4|SIGHASH" zebra-script/src zebra-chain/src -S
rg -n "OP_CODESEPARATOR|script_code|FindAndDelete" zebra-script/src zebra-chain/src -S
```

Matrix to cover (each row tested against zcashd, not against
`zcash_primitives`):

| Tx version | Hash byte | script_code shape | Notes |
| --- | --- | --- | --- |
| V5+ | `0x00` | any | undefined; expect rejection |
| V5+ | high-bit set without ANYONECANPAY base | any | undefined |
| V5+ | `0x81` (ALL\|ANYONECANPAY) | empty | edge |
| V5+ | `0x82` (NONE\|ANYONECANPAY) | empty | edge |
| V5+ | `0x03` valid | empty `script_code` | edge |
| V5+ | `0x03` valid | `script_code` containing OP_CODESEPARATOR | parity check |
| V5+ | `0x03` valid | P2SH redeem script | parity check |
| V5+ | `SIGHASH_SINGLE` valid index | many outputs | regression |

Evidence to collect:

- Whether Zebra rejects each undefined V5+ hash byte before computing a digest,
  not after.
- Whether `OP_CODESEPARATOR` is removed from `script_code` for V5+ in the same
  way zcashd does, if at all required by ZIP-244.
- Whether P2SH `script_code` is the redeem script (not the scriptSig) at the
  Zebra/FFI boundary for the SIGHASH digest.

Reproducer plan:

- Extend `zebra-script` test coverage in the same style as the SIGHASH_SINGLE
  reproducers: build a V5 transaction, sign for a specific edge, run
  `CachedFfiTransaction::is_valid`, assert pass or fail.
- Compare each row to zcashd source-level expected behavior; do not use mainnet.

Stop condition:

- Confirmed for any row where Zebra accepts and zcashd rejects, or vice versa.
- Eliminated only when each row's behavior is validated against current zcashd
  source.

### Workstream A2: NU6.1 / NU7 activation-boundary correctness

Question: is consensus behavior at the exact activation height boundary
identical between mempool, block verification, and proposal validation?

Primary files:

- `zebra-chain/src/parameters/network_upgrade.rs`
- `zebra-chain/src/parameters/network/testnet.rs`
- `zebra-chain/src/parameters/subsidy.rs`
- `zebra-consensus/src/transaction.rs`
- `zebra-consensus/src/block.rs`
- `zebra-consensus/src/checkpoint.rs`
- `zebra-state/src/service/check/`
- `zebrad/src/components/mempool/storage.rs`

Commands:

```bash
rg -n "NetworkUpgrade::|activation_height|Branch|consensus_branch_id|Nu6|Nu7|Nu5|target_height" zebra-chain/src zebra-consensus/src zebra-state/src zebrad/src/components -S
rg -n "current_network_upgrade|next_network_upgrade|activation_block" zebra-chain/src zebra-consensus/src -S
```

Checks:

- Branch ID derivation at `H = activation_height` and `H = activation_height-1`
  is consistent across mempool transaction verification, block transaction
  verification, GBT proposal mode, and submit-block.
- Mempool same-effects and exact-tip rejection caches are cleared at activation
  height crossings. Pass-2 noted this directionally; verify the actual code path
  fires on the boundary block, not one block late.
- Subsidy / fee / lockbox accounting at the boundary.
- Expiry-height defaults change correctly across upgrades.

Reproducer plan:

- Build a unit test on a synthetic regtest-like network parameter set with an
  activation height set inside a controlled block range.
- Verify the same canonical transaction returns identical accept/reject across
  mempool and block paths at `H-1`, `H`, and `H+1`.

Stop condition:

- Confirmed if any path uses the wrong branch ID or skips upgrade-driven cache
  clearing at the boundary.
- Eliminated if all five paths derive the same branch ID and clear caches in
  sync.

### Workstream A3: anchor and nullifier handling under reorg

Question: do Sapling/Orchard anchor lookups and nullifier set membership stay
consistent across reorgs, including reorgs that cross a network upgrade?

Primary files:

- `zebra-state/src/service/check/anchors.rs`
- `zebra-state/src/service/check/nullifier.rs`
- `zebra-state/src/service/non_finalized_state.rs`
- `zebra-state/src/service/finalized_state/zebra_db/shielded.rs`
- `zebra-state/src/service.rs`
- `zebra-consensus/src/transaction.rs`
- `zebra-consensus/src/block.rs`

Commands:

```bash
rg -n "anchor|note_commitment_tree|sapling_tree|orchard_tree|final_sapling_root|final_orchard_root" zebra-state/src zebra-consensus/src -S
rg -n "nullifier|sprout_nullifier|sapling_nullifier|orchard_nullifier|nullifier_set" zebra-state/src zebra-consensus/src -S
rg -n "rollback|invalidate|reorg|fork|best_chain_change|chain_tip" zebra-state/src -S
```

Checks:

- Empty-tree behavior at activation: the first block that introduces a Sapling
  or Orchard anchor.
- Anchor lookup path during proposal validation uses the same tree snapshot as
  commit.
- Nullifier set is restored exactly on reorg rollback. A nullifier that was
  spent on a discarded chain must be available again on the new chain.
- Reorgs that span a network upgrade boundary do not leave a stale anchor set
  cached for the wrong upgrade.
- `final_sapling_root` and `final_orchard_root` in block headers are recomputed,
  not pulled from cache when the underlying tree changes.

Reproducer plan:

- Construct a small synthetic chain with two competing branches:
  - branch X: spends nullifier N at height h.
  - branch Y: spends nullifier N at height h.
- Apply branch X, then reorg to branch Y. Assert N appears once and only once
  in the active nullifier set, and the nullifier-double-spend check on branch Y
  passes for the legitimate inclusion.
- Construct a transaction whose anchor exists only in branch X; verify it is
  rejected on branch Y.

Stop condition:

- Confirmed if a reorg leaves the nullifier set inconsistent, leaks an anchor
  across branches, or causes a proposal-mode anchor lookup to disagree with
  commit.
- Eliminated if branch isolation holds across all three reorg shapes (deep,
  upgrade-crossing, finalization-adjacent).

### Workstream A4: deep reorgs and the finalization boundary

Question: do any code paths assume the finalized chain does not move?

Primary files:

- `zebra-state/src/service/finalized_state.rs`
- `zebra-state/src/service/non_finalized_state.rs`
- `zebra-state/src/service.rs`
- `zebra-state/src/constants.rs`
- `zebrad/src/components/sync.rs`

Commands:

```bash
rg -n "MAX_BLOCK_REORG_HEIGHT|finalized|finalize_and_commit|root_block|tip_height" zebra-state/src zebrad/src -S
rg -n "assume|invariant|expect\\(\"finaliz" zebra-state/src zebra-consensus/src -S
```

Checks:

- Identify all `expect()` and `unwrap()` whose justification depends on the
  finalized chain being immutable.
- Identify whether reorgs of exactly `MAX_BLOCK_REORG_HEIGHT` blocks are handled
  the same way as smaller reorgs (off-by-one risk).
- Identify whether a deep reorg combined with a network upgrade activation in
  the discarded segment is handled correctly.

Stop condition:

- Confirmed if any consensus-critical path panics or returns a misleading error
  at the boundary.
- Eliminated when each "finalized is immutable" assumption is justified by an
  explicit depth check at a higher layer.

### Workstream A5: value-pool and balance arithmetic

Question: can attacker-controlled value balances trigger overflow in checked
arithmetic, or be bypassed by an `as` cast, in any consensus check?

Primary files:

- `zebra-chain/src/amount.rs`
- `zebra-chain/src/transaction/serialize.rs`
- `zebra-chain/src/value_balance.rs` (if present; otherwise locate in
  `zebra-chain/src/transaction/`)
- `zebra-consensus/src/transaction.rs`
- `zebra-consensus/src/block/check.rs`
- `zebra-state/src/service/check/utxo.rs`
- `zebra-state/src/service/check/value_balance.rs` (if present)

Commands:

```bash
rg -n "as i64|as u64|as i32|as u32|saturating_|checked_|wrapping_|overflow" zebra-chain/src zebra-consensus/src zebra-state/src -S
rg -n "value_balance|value_pool|transparent_value_balance|sapling_value_balance|orchard_value_balance|lockbox" zebra-chain/src zebra-consensus/src zebra-state/src -S
```

Checks:

- Every `as` cast on a value type is annotated with a comment explaining why it
  cannot wrap, per AGENTS.md.
- Every arithmetic operation on attacker-controlled amounts uses `checked_*` or
  `saturating_*`.
- Value-pool transitions across network upgrades preserve the invariant that
  total transparent + Sapling + Orchard + Lockbox value matches the supply
  schedule.
- Coinbase value pool transitions at the activation boundary.

Reproducer plan:

- Property tests over `Amount` arithmetic boundaries, exercising the maximum
  positive, maximum negative, and zero cases.
- Targeted unit tests for value-pool transitions at NU5, NU6, NU6.1, NU7
  boundaries (whichever are active in the audited branch).

Stop condition:

- Confirmed if any value-related path can overflow, wrap, or accept an invalid
  balance.
- Eliminated when all amount arithmetic on untrusted input is checked and all
  `as` casts are justified.

### Workstream A6: zebra-script FFI safety

Question: can any FFI callback or `zcash_script` return value cause undefined
behavior, hidden acceptance, or process abort?

Primary files:

- `zebra-script/src/lib.rs`
- `zebra-script/src/tests.rs`
- `zebra-chain/src/primitives/zcash_primitives.rs`

Commands:

```bash
rg -n "extern \"C\"|callback|catch_unwind|panic|unwind|c_int|c_uint|raw::|std::ffi" zebra-script/src zebra-chain/src/primitives -S
rg -n "ZCASH_SCRIPT_ERR|zcash_script_error|verify_callback" zebra-script/src -S
```

Checks:

- Every Rust callback invoked via FFI is wrapped in `catch_unwind` (or proven
  panic-free at compile time) so unwinding cannot cross the FFI boundary.
- All raw pointers passed to or received from `zcash_script` have validated
  lifetimes.
- All `ZCASH_SCRIPT_ERR_*` codes are mapped to a Rust enum variant. Unknown
  codes do not silently become `Ok`.
- `previous_outputs` and `script_code` byte buffers are alive for the duration
  of the FFI call.

Stop condition:

- Confirmed if a callback can panic into FFI, a return code can be silently
  ignored, or a buffer can outlive its borrow.
- Eliminated when each callback is panic-safe, every error code is mapped, and
  buffer lifetimes are explicit.

### Workstream A7: proof and signature batch verifier failure semantics

Question: can a malformed proof/signature item, batch verifier shutdown, worker
panic, or channel error be reported as consensus acceptance, hidden rejection,
or a misleading infrastructure error?

Primary files:

- `zebra-consensus/src/primitives/`
- `zebra-consensus/src/transaction.rs`
- `zebra-chain/src/primitives/`

Commands:

```bash
rg -n "Batch|batch|Verifier|spawn_blocking|JoinError|oneshot|channel|panic|InternalDowncastError" zebra-consensus/src/primitives zebra-consensus/src/transaction.rs -S
rg -n "sapling|orchard|halo2|groth16|ed25519|redjubjub|reddsa" zebra-consensus/src/primitives zebra-consensus/src/transaction.rs -S
```

Checks:

- Batch verification worker failures are distinguishable from consensus-invalid
  proofs or signatures.
- A worker panic, dropped request, or shutdown does not silently pass.
- All boxed primitive verifier errors downcast into stable `TransactionError`
  variants, or remain infrastructure failures at public boundaries.
- Cancelling one item does not poison unrelated queued verifier work.

Stop condition:

- Confirmed if any primitive verifier failure can be accepted, misclassified as
  a stable consensus rejection when it is infrastructure, or panic across an
  async boundary.
- Eliminated when each primitive verifier has explicit failure taxonomy and
  batch-worker panic/error paths are covered by tests or code invariants.

---

## Track B: availability and hardening

### Workstream B1: semantic network DoS

Question: can a peer hold node resources or cause expensive lookups using
protocol-valid messages?

Primary files:

- `zebra-network/src/peer/connection.rs`
- `zebra-network/src/peer_set/`
- `zebra-network/src/protocol/external/`
- `zebra-network/src/address_book.rs`
- `zebra-network/src/protocol/external/addr/v2.rs`

Commands:

```bash
rg -n "getdata|getheaders|getblocks|inv|notfound|mempool message|MAX_INV|MAX_GETDATA|MAX_HEADERS_PER_MESSAGE" zebra-network/src -S
rg -n "AddrV2|services|transport_id|tor|i2p|cjdns" zebra-network/src -S
rg -n "rate.?limit|cooldown|backoff|throttle" zebra-network/src -S
```

Checks:

- `getdata` / `inv` replay: can a peer repeatedly request the same hashes to
  force lookups without progress?
- `getheaders` reply state machine: can a peer keep Zebra "waiting for next
  message" indefinitely without sending one?
- Address book: does adding many `addrv2` entries with weird-but-legal service
  flags or transport IDs poison peer selection?
- Per-peer concurrency caps on inbound requests.

Reproducer plan:

- Local-only mock peer test that sends a long sequence of duplicate `getdata`
  requests. Assert request count is bounded per connection.
- Local-only mock peer test that sends a `getheaders` and never follows up.
  Assert Zebra times out the peer's slot.

Stop condition:

- Confirmed if any protocol-valid sequence holds a connection slot indefinitely
  or forces unbounded lookups.
- Eliminated when rate limits, timeouts, and per-peer concurrency caps cover
  each shape.

### Workstream B2: RPC auth ordering and exposure

Question: is cookie auth checked before expensive request handling, and are
response sizes bounded?

Primary files:

- `zebra-rpc/src/config/rpc.rs`
- `zebra-rpc/src/server.rs`
- `zebra-rpc/src/server/http_request_compatibility.rs`
- `zebra-rpc/src/methods.rs`
- `zebrad/src/commands/start.rs`

Commands:

```bash
rg -n "cookie|enable_cookie_auth|check_credentials|Authorization|basic_auth" zebra-rpc/src zebrad/src -S
rg -n "max_request_body_size|max_response_body_size|content-length|Limited::new" zebra-rpc/src -S
rg -n "getrawtransaction|getblock|verbose|verbosity" zebra-rpc/src/methods.rs -S
```

Checks:

- Cookie file is created with `0600` permissions and rotated as documented.
- Cookie auth check happens before request body collection. (Pass-2 plan flagged
  this; this pass should turn it into a confirm-or-eliminate.)
- No RPC method bypasses auth.
- `getrawtransaction` and `getblock` verbose response sizes are bounded against
  pathological input (large transactions, max-fan-out blocks).
- jsonrpsee per-connection and per-method concurrency settings are explicit and
  documented.

Reproducer plan:

- Local-only test that sends a large unauthenticated request body. Assert the
  body is rejected before parsing.
- Local-only test that requests `getrawtransaction` with the maximum verbose
  flag for a synthetic large transaction. Assert response size is bounded.

Stop condition:

- Confirmed if auth runs after body collection, or any verbose response is
  unbounded.
- Eliminated when auth precedes body parsing and all response sizes have a cap.

### Workstream B3: mempool DoS and policy parity

Question: are mempool DoS limits aligned with block validation, and is mempool
churn from RBF-like behavior bounded?

Primary files:

- `zebrad/src/components/mempool/storage.rs`
- `zebrad/src/components/mempool/storage/verified_set.rs`
- `zebrad/src/components/mempool/storage/policy.rs`
- `zebra-consensus/src/transaction/check.rs`
- `zebra-chain/src/transaction.rs`

Commands:

```bash
rg -n "MAX_TX_SIZE|MAX_BLOCK_SIGOPS|sigops|MAX_STANDARD|standardness|ancestor|descendant" zebrad/src/components/mempool zebra-consensus/src -S
rg -n "rbf|replace_by_fee|same_inputs|conflict|same_effects" zebrad/src/components/mempool -S
```

Checks:

- Per-transaction sigops limit at mempool entry matches block validation.
- Transaction size and weight limits at mempool entry match block validation.
- Maximum unconfirmed-descendant chain depth (zcashd has one; Zebra's behavior
  should be documented and bounded).
- If RBF is not implemented, confirm that same-input-different-witness
  re-submission cannot churn the verifier without bound.

Reproducer plan:

- Build a synthetic transaction at the size or sigops boundary. Assert mempool
  and block paths agree on accept or reject.
- Build a long descendant chain in the mempool and assert depth is bounded.

Stop condition:

- Confirmed if mempool accepts transactions block validation rejects, or
  descendant depth is unbounded.
- Eliminated when both limits match and depth is capped.

### Workstream B4: time-related consensus

Question: are median-time-past, header time bounds, and locktime/nSequence
edges handled consistently across paths and reorgs?

Primary files:

- `zebra-state/src/service/check/difficulty.rs`
- `zebra-state/src/service/check/utxo.rs`
- `zebra-consensus/src/block/check.rs`
- `zebra-consensus/src/transaction.rs`
- `zebra-chain/src/block/header.rs`

Commands:

```bash
rg -n "median_time_past|MTP|max_future_block_time|block_time|MAX_BLOCK_FUTURE" zebra-state/src zebra-consensus/src zebra-chain/src -S
rg -n "lock_time|nSequence|n_sequence|locktime" zebra-chain/src zebra-consensus/src -S
```

Checks:

- Median-time-past computation under reorg: cached values are invalidated on
  rollback.
- Block header `time` upper bound (max future drift) is enforced consistently.
- Locktime / `nSequence` edges for transparent inputs: equal-to-boundary cases.
- Mempool locktime check uses the same MTP semantics as block validation, not
  current wall-clock time.

Stop condition:

- Confirmed if any time-related check disagrees across paths.
- Eliminated when all paths derive time from the same source under reorg.

### Workstream B5: storage, migrations, and format-version drift

Question: can a partial migration, format-version mismatch, or column-family
inconsistency leave the database in a state Zebra reads incorrectly?

Primary files:

- `zebra-state/src/service/finalized_state.rs`
- `zebra-state/src/service/finalized_state/disk_format.rs`
- `zebra-state/src/service/finalized_state/disk_db.rs`
- `zebra-state/src/service/finalized_state/zebra_db/`
- `zebra-state/src/constants.rs`

Commands:

```bash
rg -n "DATABASE_FORMAT_VERSION|format_version|migration|upgrade_db|column_family|ColumnFamily" zebra-state/src -S
rg -n "snapshot|atomic_write|WriteBatch|transaction" zebra-state/src -S
```

Checks:

- All format-version constants and their corresponding migrations are paired.
- Crash mid-migration leaves either the old or the new format, not a hybrid.
- Reads that span column families use a snapshot (or are explicitly safe under
  concurrent writes).

Stop condition:

- Confirmed if a format-version bump exists without a migration, or a multi-CF
  read can observe partial writes.
- Eliminated when each version bump has a migration and multi-CF reads are
  snapshot-bounded.

### Workstream B6: log injection and metric cardinality

Question: can attacker-controlled input become a high-cardinality metric label
or a confusing log line?

Primary files:

- everything under `zebra-network/src` and `zebra-rpc/src` (log call sites near
  user input)
- `zebrad/src/components/` (metrics)

Commands:

```bash
rg -n "metrics::counter!|metrics::gauge!|metrics::histogram!|increment_counter|describe_" zebrad/src zebra-network/src zebra-state/src zebra-rpc/src -S
rg -n "tracing::info!|tracing::warn!|tracing::error!|debug!|info!|warn!|error!" zebra-network/src zebra-rpc/src -S | head -200
```

Checks:

- No metric label is built from unbounded peer input (peer addresses, user
  agents, hash hex strings).
- No log line interpolates raw bytes from a malformed message without quoting
  or truncation.

Stop condition:

- Confirmed if a label or log line can grow with attacker input.
- Eliminated when all such call sites use bounded values.

### Workstream B7: feature-gated code reachable in release

Question: are any test-only or debug-only paths included in the release binary?

Primary files:

- `zebrad/Cargo.toml`
- each crate's `Cargo.toml`
- `.cargo/config.toml`

Commands:

```bash
rg -n "cfg\\(any\\(test|cfg\\(feature|cfg\\(debug_assertions|test-only|test_only" .  -S
rg -n "default-release-binaries|getblocktemplate-rpcs|elasticsearch|tx_v6|nu7|sentry" .  -S
```

Checks:

- The features enabled by `default-release-binaries` are reviewed for any that
  expose test scaffolding, override consensus, or weaken validation.
- No `cfg(debug_assertions)` gate hides a check that should run in release.

Stop condition:

- Confirmed if a release-enabled feature exposes a test path, weakens a check,
  or skips a guard.
- Eliminated when each release feature is justified.

---

## Workstreams already covered or deferred

- Pass-3 findings 1-3 are open and should be tracked separately from this pass.
- Pass-4 findings 1-2 are open and should be tracked separately.

## Triage order

1. A1 sighash matrix beyond SIGHASH_SINGLE (highest expected signal).
2. A6 FFI safety.
3. A7 proof/signature batch verifier failure semantics.
4. B2 RPC auth ordering and exposure.
5. B3 mempool DoS parity.
6. A3 anchor and nullifier under reorg.
7. A5 value-pool arithmetic.
8. A2 activation-boundary correctness.
9. A4 deep reorgs and finalization boundary.
10. B1 semantic network DoS.
11. B4 time-related consensus.
12. B5 storage and migrations.
13. B7 feature-gated release paths.
14. B6 log and metric hygiene.

## Completion criteria

This pass is complete when:

- Every workstream has either a confirmed finding or an eliminated-lead note
  with the specific code backstop.
- Confirmed findings include file/line evidence, a minimal regression sketch,
  and a fix recommendation that names the smallest correct change.
- Track A findings stay private until ZF confirms disclosure handling.
- Track B findings have public maintainer issue drafts ready, gated on the
  contribution gate before any PR.
- Line-pointer citations have been re-verified against the current `main` HEAD
  and, where relevant, current upstream zcashd / librustzcash source.
