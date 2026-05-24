# Value Pool Error Suppression Note

Date: 2026-05-02

Scope: follow-up on pass-5 Workstream A5, covering block-level value-pool
aggregation, deferred/lockbox accounting, and state persistence paths.

## Finding

`Block::chain_value_pool_change()` suppresses transaction-level value-balance
errors while computing the block's chain value-pool delta.

The current implementation maps each transaction through
`Transaction::value_balance(utxos)` and then uses `flat_map`:

```rust
self
    .transactions
    .iter()
    .flat_map(|t| t.value_balance(utxos))
    .sum::<Result<ValueBalance<NegativeAllowed>, _>>()?
```

In Rust, `Result<T, E>` implements `IntoIterator`: `Ok(value)` yields one item
and `Err(_)` yields zero items. As a result, a transaction whose
`value_balance()` returns `Err` is omitted from the block sum instead of causing
`chain_value_pool_change()` to return that error.

This breaks the intended contract of `chain_value_pool_change()`, whose return
type is already `Result<ValueBalance<NegativeAllowed>, ValueBalanceError>`.

## Evidence

- `zebra-chain/src/block.rs:228-244` contains the `flat_map` aggregation and
  then injects the deferred pool change.
- `zebra-chain/src/transaction.rs:1463-1470` composes transaction value balances
  from transparent, Sprout, Sapling, and Orchard components and can return
  `ValueBalanceError`.
- `zebra-chain/src/value_balance.rs:435-449` and `zebra-chain/src/value_balance.rs:487-498`
  propagate pool-wise arithmetic errors.
- `zebra-state/src/service/finalized_state/zebra_db/chain.rs:256-279` expects
  `chain_value_pool_change()` failures to become
  `ValidateContextError::CalculateBlockChainValueChange` or
  `ValidateContextError::AddValuePool`.
- `zebra-state/src/service/non_finalized_state.rs:591-604` similarly maps
  block chain-value calculation failures into contextual validation errors.

## Local Proof

Added a durable current-behavior `zebra-chain` unit test:
`zebra-chain/src/block/tests/vectors.rs:95`.

The test shape:

- construct a V1 coinbase transaction with two transparent outputs,
- each output is individually `MAX_MONEY`, so it is a valid
  `Amount<NonNegative>`,
- the transaction-level output sum exceeds `MAX_MONEY`, so
  `coinbase.value_balance(&HashMap::new())` returns `Err`,
- put that transaction in a block and call
  `block.chain_value_pool_change(&HashMap::new(), None)`.

Observed current behavior:

```text
cargo test -p zebra-chain chain_value_pool_change_drops_transaction_value_balance_errors_today --lib
test block::tests::vectors::chain_value_pool_change_drops_transaction_value_balance_errors_today ... ok
```

The test asserted that transaction-level value-balance calculation returned
`Err`, while block-level `chain_value_pool_change()` returned `Ok` with a zero
value-pool delta.

I also added and removed a short-lived `zebra-consensus` probe for the same
coinbase transparent-overflow shape. It confirmed that the normal semantic block
path's `miner_fees_are_valid()` rejects this concrete coinbase case before state
commit:

```text
cargo test -p zebra-consensus miner_fees_reject_coinbase_output_sum_over_max_money_probe --lib
test block::tests::miner_fees_reject_coinbase_output_sum_over_max_money_probe ... ok
```

That semantic-path probe was not kept because it proves a mitigation for this
specific synthetic coinbase shape, not the helper bug itself.

## Impact

This is a consensus-adjacent correctness bug, but I do not currently have
evidence that it lets a remote peer get an invalid block accepted on the normal
semantic verification path.

Important mitigations and reachability conclusions:

- Non-coinbase transaction value-balance failures are checked earlier by the
  transaction verifier when deriving miner fees. Any `Err` or invalid remaining
  transaction value becomes `TransactionError::IncorrectFee`, so semantic block
  verification does not reach state commit for those transactions.
- The state contextual check also verifies non-coinbase
  `remaining_transaction_value()` before building `ContextuallyVerifiedBlock`,
  so there is a second non-coinbase backstop before block-level value-pool
  aggregation.
- The concrete coinbase transparent-output overflow probe is independently
  rejected by `miner_fees_are_valid()`, which sums coinbase transparent outputs
  directly and returns `SubsidyError::Overflow`.
- Checkpointed blocks do not run the full transaction semantic value-balance
  path, but accepted checkpoint blocks are constrained by trusted checkpoint
  hashes. This is not a practical arbitrary-remote-block injection path unless
  the checkpoint set or finalized database is already wrong.
- The normal checkpoint verifier also checks block Merkle-root validity before
  queuing a checkpoint block for state commit. Since the checkpoint hash commits
  to the header, and the header commits to the transaction Merkle root, a remote
  peer cannot keep a trusted checkpoint hash while swapping in a malformed
  transaction body. The remaining checkpoint concern is therefore a trusted-input
  boundary: a bad checkpoint list, direct `CheckpointVerifiedBlock` construction,
  or older/raw finalized DB data.
- A direct state-service test now confirms that if a caller constructs
  `CheckpointVerifiedBlock` directly, state commit trusts that boundary, does
  not repeat Merkle validation, and reaches the same zero-delta value-pool path.
- Transparent missing-output cases tend to panic at the direct UTXO lookup
  helper rather than return `ValueBalanceError`, so the concrete dropped-error
  class is mostly arithmetic/value-balance composition failures.

The more realistic risk is in validator internals and derived state:

- Checkpointed blocks bypass full semantic subsidy/fee validation and rely on
  trusted checkpoint hashes plus later state persistence.
- Finalized and non-finalized state callers expect block value-pool errors to
  propagate, but this helper can undercount instead.
- Database upgrade/replay code currently masks `chain_value_pool_change()`
  failure with `unwrap_or_default()`, which can convert a failed recomputation
  into a zero block delta. This fallback is silently corrupting if the error
  branch is ever reached: the zero-adjusted cumulative pool is immediately
  persisted into `BlockInfo`, and later heights inherit that cumulative value.
  This path is now locally proof-backed by a raw finalized-DB upgrade test.

Current reachability confidence:

- Code bug in `Block::chain_value_pool_change()`: high.
- Arbitrary remote invalid-block acceptance on the full semantic path: low.
- Checkpoint/finalized-history integrity impact if trusted historical data is
  malformed or the database is inconsistent: medium.
- Migration replay corruption if recomputation returns `Err`: high for impact of
  the fallback and proof-backed for the older/raw finalized-DB upgrade path;
  still unproven for reachability from normal finalized DB contents.

### RPC `chain_supply` panic sibling

Follow-up on 2026-05-09 found a small RPC response-construction sibling:
`GetBlockchainInfoBalance::chain_supply()` sums the five non-negative pool
balances with `Amount` addition and unwraps the result:

```rust
(a.chain_value_zat + b.chain_value_zat)
    .expect("sum of value balances should not overflow")
```

`ValueBalance<NonNegative>` only proves each individual pool is non-negative;
it does not prove the cross-pool sum is at most `MAX_MONEY`. If an earlier
value-pool accounting bug, migration fallback, trusted checkpoint replay issue,
or local database corruption produces individually valid pools whose total is
too large, `getblockchaininfo` can panic while building `chain_supply`.

Local proof added:

```sh
cargo test -p zebra-rpc chain_supply_panics_when_pool_sum_exceeds_max_money_today --lib
```

Result on 2026-05-09: passed as a `#[should_panic]` current-behavior test. The
test builds a `ValueBalance<NonNegative>` with three half-`MAX_MONEY` pools:
each pool amount is representable, but the chain-supply total overflows.

This does not change the main disclosure posture on its own. A remote caller
cannot choose `value_pools` directly through `getblockchaininfo`; the meaningful
risk is that the value-pool suppression/replay bugs leave state in a shape that
turns a routine RPC query into a process-fatal panic.

### Upgrade replay zero-delta proof

Follow-up on 2026-05-09 added a current-behavior migration proof:

```sh
cargo test -p zebra-state block_info_upgrade_persists_zero_value_pool_when_recomputed_block_value_errors_today --lib
```

Result: passed.

The test writes an older/raw finalized database shape with:

- real mainnet genesis block data and no `block_info`,
- a height-1 block containing a transaction whose transaction-level value
  balance fails because its transparent output sum exceeds `MAX_MONEY`,
- no existing `block_info` entry for the malformed height.

Then it runs the `block_info_and_address_received::Upgrade`. Current behavior:

- `chain_value_pool_change(...).unwrap_or_default()` converts the recomputation
  problem into a zero block delta,
- the upgrade writes `BlockInfo` for the malformed height,
- the upgrade's own `validate()` method accepts that `BlockInfo` because its
  serialized size keeps it from being equal to `Default::default()`.

This upgrades the replay fallback from source-evidence-only to proof-backed for
manually present/older raw finalized data. It still does not prove that a normal
fully verified Zebra node can arrive at that raw DB shape from remote blocks.

### Direct checkpoint commit trust-boundary proof

Follow-up on 2026-05-09 added a state-service current-behavior proof:

```sh
cargo test -p zebra-state direct_checkpoint_commit_accepts_bad_value_balance_block_today --lib
```

Result: passed.

The test commits real mainnet genesis through the state service, then constructs
a height-1 block with the real block-1 header but a malformed transaction body
whose transaction-level value balance fails. Because block hashes are header
hashes, the block hash still matches the real block-1 header hash even though
the transaction Merkle root no longer matches the body.

Current behavior: when the test directly wraps that block in
`CheckpointVerifiedBlock` and submits `Request::CommitCheckpointVerifiedBlock`,
the state service commits it and stores zero value pools for height 1. This is
not a remote checkpoint-verifier bypass, because the normal checkpoint verifier
checks Merkle-root validity before constructing/queuing the checkpoint-verified
block. It is a proof of the internal trusted-caller boundary and the downstream
state persistence effect.

## Related Hardening Issue

`zebra-state/src/service/finalized_state/disk_format/upgrade/block_info_and_address_received.rs`
uses:

```rust
block
    .chain_value_pool_change(...)
    .unwrap_or_default()
```

That fallback should not be used for derived value-pool replay. If replay cannot
recompute a block's value-pool change, the upgrade should fail loudly with
height/hash context rather than writing metadata from a zero delta.

## Suggested Fix

Replace the `flat_map(Result)` aggregation with an error-propagating fold:

```rust
let tx_pool_sum =
    self.transactions
        .iter()
        .try_fold(ValueBalance::zero(), |acc, tx| {
            let tx_balance = tx.value_balance(utxos)?;
            acc + tx_balance
        })?;

Ok(*tx_pool_sum.neg().set_deferred_amount(
    deferred_pool_balance_change
        .map(DeferredPoolBalanceChange::value)
        .unwrap_or_default(),
))
```

Also remove the upgrade replay `unwrap_or_default()` fallback and make replay
fail when value-pool recomputation fails.

## Suggested Tests

- A direct `zebra-chain` regression proving `Block::chain_value_pool_change()`
  returns `Err` when any transaction's `value_balance()` returns `Err`.
- A finalized-state regression proving `CalculateBlockChainValueChange` is
  reachable from block value-pool calculation failure.
- A migration/replay regression proving a recomputation failure aborts replay
  instead of writing a zero delta.
- An RPC regression proving `getblockchaininfo` handles cross-pool-invalid
  `ValueBalance<NonNegative>` values without panicking, either by returning an
  RPC error or omitting `chain_supply` with clear state-error context.
- Deferred/lockbox equivalence tests comparing semantic, checkpoint-style, and
  migration-style deferred pool formulas around NU6 and NU6.1 activation.

## Disclosure Triage

Recommended handling: private maintainer heads-up or bundled private follow-up,
not a public issue until maintainers confirm impact.

Confidence in the code bug: high.

Confidence in remote exploitability through normal block submission or P2P block
verification: low to medium-low.

Reason: the state helper is consensus-adjacent and should be fixed, but the
normal semantic path appears to reject realistic attacker-controlled
value-balance failures before this helper is used for persistence.
