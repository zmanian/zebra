# Private Advisory Drafts After Maintainer Reply

Date: 2026-05-07

Status: submitted as GitHub Security Advisory private reports, based on the
maintainer reply that `longpollid`, `z_listunifiedreceivers`, and ZIP-235 are
already known or public, while the `invalidateblock` / `reconsiderblock`,
address-book ban, and value-pool items were not recognized as already reported.

Submitted reports:

- GHSA-f2mf-6pmp-v5h4: Trusted RPC state-control methods can trigger
  process-fatal non-finalized-state panics
- GHSA-765j-7mqh-5hpp: Address-book ban path can panic when
  `max_connections_per_ip` is greater than 1
- GHSA-7fxc-hmq3-8rm3: Block value-pool aggregation can suppress transaction
  value-balance errors
- GHSA-g6vw-jj63-r8c4: RPC cookie auth material can inherit loose file
  permissions and remain after server failure or task abort

Do not publish exact repro details publicly until maintainers decide disclosure
handling.

## Routing Summary

Do not re-report:

- `getblocktemplate` `longpollid` Unicode panic: maintainer says already
  reported.
- `z_listunifiedreceivers` invalid Sapling receiver panic: maintainer says
  already reported.
- ZIP-235 miner-fee share overflow: already public as
  <https://github.com/ZcashFoundation/zebra/issues/10519>.

Recommended private submissions:

1. Trusted RPC state-control panics in `invalidateblock` / `reconsiderblock`.
2. Address-book ban panic when `network.max_connections_per_ip > 1`.
3. Block value-pool error suppression, marked as lower confidence for remote
   exploitability but high confidence as validator accounting correctness.
4. RPC cookie auth material retention and loose-permission inheritance, marked
   low severity and local/deployment-dependent.

## Draft 1: Trusted RPC `invalidateblock` / `reconsiderblock` Panics

### Title

Trusted RPC state-control methods can trigger process-fatal non-finalized-state
panics

### Summary

Several authenticated/trusted RPC state-control sequences can panic in
non-finalized-state chain handling. Zebra release profiles use `panic = "abort"`,
so these panics are process-fatal in `zebrad`.

The confirmed variants are:

- `invalidateblock` on the root block of a tracked non-finalized chain.
- `invalidateblock` on two same-height sibling fork tips that reduce to the same
  parent chain.
- `reconsiderblock` repeated for the same successfully reconsidered invalidated
  root.

This is not a consensus failure and not unauthenticated P2P. It is an
availability bug reachable by an RPC caller with access to trusted control
methods. It becomes remote if RPC is exposed or credentials are compromised.

### Affected Component

- `zebra-rpc` state-control RPC methods:
  `invalidateblock`, `reconsiderblock`
- `zebra-state` non-finalized state chain-set and invalidation handling

### Evidence

Current source evidence:

- `zebra-state/src/service/non_finalized_state.rs:375-414` handles
  `invalidate_block()`. When the target is the non-finalized root, it calls
  `self.chain_set.remove(&chain)`.
- `zebra-state/src/service/non_finalized_state.rs:388-392` inserts a shortened
  chain before filtering sibling chains that contain the invalidated hash.
- `zebra-state/src/service/non_finalized_state.rs:421-503` handles
  `reconsider_block()`.
- `zebra-state/src/service/non_finalized_state.rs:439-444` removes an
  invalidated entry from a cloned `IndexMap`, not from the live
  `self.invalidated_blocks`.
- `zebra-state/src/service/non_finalized_state.rs:481-485` replays invalidated
  blocks into a chain with an `expect(...)`.
- `zebra-state/src/service/non_finalized_state/chain.rs:2320-2344` implements
  `Ord for Chain` and treats equal tip hashes as unreachable.

### Local Repro Tests

Current HEAD verification on 2026-05-07:

```sh
cargo test -p zebra-state service::non_finalized_state::tests::vectors --lib
```

Result:

```text
18 passed
```

The passing vector module includes these current-behavior `#[should_panic]`
tests:

- `invalidating_chain_root_panics_when_removing_existing_chain_today`
- `invalidating_same_height_fork_tips_panics_today`
- `reconsider_block_twice_replays_stale_invalidated_entry_today`

### Impact

An authenticated RPC caller can abort Zebra using normal state-control RPC
methods and ordinary non-finalized-state shapes.

Severity is lower than unauthenticated peer-only DoS because RPC access is
required. It is still stronger than ordinary hardening because the impact is a
confirmed process-fatal panic in supported RPC methods.

### Suggested Fix Direction

- Avoid using a `BTreeSet<Arc<Chain>>` lookup/removal path that compares a chain
  with itself using an ordering that panics on equal tips.
- Make `Chain` ordering total and non-panicking, and enforce unique tip hashes at
  insertion boundaries.
- Make invalidation idempotent when the shortened parent chain already exists.
- Remove invalidated records from the live `self.invalidated_blocks`, not a
  clone.
- Replace replay `expect(...)` calls with typed `ReconsiderError` results.
- Add regression tests proving the three sequences return success or typed
  errors without panicking.

### References In Our Notes

- `docs/analysis/rpc-invalidateblock-chain-root-panic-finding.md`
- `docs/analysis/rpc-invalidateblock-same-height-fork-panic-finding.md`
- `docs/analysis/rpc-reconsiderblock-stale-invalidated-entry-panic-finding.md`

## Draft 2: Address-Book Ban Panic With Multiple Connections Per IP

### Title

Address-book ban path can panic when `network.max_connections_per_ip > 1`

### Summary

`AddressBook::update()` can panic when applying a peer-misbehavior update that
reaches the ban threshold if `network.max_connections_per_ip` is configured
above the default value of 1.

The default configuration is not affected. The affected setting is accepted by
configuration and documented as supported but security-sensitive.

### Affected Component

- `zebra-network` address book and peer misbehavior scoring

### Evidence

Current source evidence:

- `zebra-network/src/address_book.rs:76-82` documents that
  `most_recent_by_ip` only supports `max_connections_per_ip == 1` and is `None`
  for larger values.
- `zebra-network/src/address_book.rs:158-169` constructs
  `most_recent_by_ip` only when `max_connections_per_ip == 1`.
- `zebra-network/src/address_book.rs:443-458` enters the ban path when
  misbehavior reaches the threshold, then unconditionally unwraps
  `self.most_recent_by_ip.as_mut().expect(...).remove(&banned_ip)`.

Remote-influenced misbehavior scoring reaches this path through ordinary peer
management:

- invalid transaction and block verification errors can produce nonzero
  misbehavior scores;
- `zebrad` forwards those scores with the peer address;
- the peer-set initializer batches and sends misbehavior updates to the
  address-book updater.

### Local Repro Test

Current HEAD verification on 2026-05-07:

```sh
cargo test -p zebra-network misbehavior_ban_panics_with_max_connections_per_ip_above_one_today --lib
```

Result:

```text
test address_book::tests::vectors::misbehavior_ban_panics_with_max_connections_per_ip_above_one_today - should panic ... ok
```

### Impact

A peer that can trigger a ban-threshold misbehavior update can panic the
address-book updater on nodes configured with
`network.max_connections_per_ip > 1`.

The panic occurs while the shared address-book mutex is held. Other users of the
shared address book expect the mutex not to be poisoned, so the initial panic can
degrade or cascade into peer-management failures.

This is a network-facing availability issue for a supported non-default
configuration. It does not affect consensus validation and does not affect
default nodes.

### Suggested Fix Direction

- In the ban branch, remove from `most_recent_by_ip` only if the optional cache
  exists.
- Keep IP ban insertion and `by_addr` removal behavior unchanged.
- Add a regression test for `max_connections_per_ip = 2` that applies a
  ban-threshold `UpdateMisbehavior` and asserts no panic, the IP is banned, and
  matching entries are removed.

### Reference In Our Notes

- `docs/analysis/address-book-misbehavior-ban-max-connections-panic-finding.md`

## Draft 3: Block Value-Pool Error Suppression

### Title

`Block::chain_value_pool_change()` suppresses transaction value-balance errors

### Summary

`Block::chain_value_pool_change()` computes the block value-pool delta with
`flat_map(|tx| tx.value_balance(utxos))`. Because `Result<T, E>` implements
`IntoIterator`, `Ok(value)` contributes one item and `Err(_)` contributes no
items. Therefore a transaction-level `ValueBalanceError` can be silently omitted
from the block-level value-pool sum.

This breaks the function's apparent contract: it already returns
`Result<ValueBalance<NegativeAllowed>, ValueBalanceError>` and state callers map
that error into contextual validation failures.

### Affected Component

- `zebra-chain` block value-pool aggregation
- `zebra-state` finalized and non-finalized contextual value-pool checks
- state-format replay/migration code that recomputes derived value-pool data

### Evidence

Current source evidence:

- `zebra-chain/src/block.rs:228-244` uses `flat_map` over
  `Transaction::value_balance(utxos)`.
- `zebra-state/src/service/finalized_state/zebra_db/chain.rs:256-270` expects
  `chain_value_pool_change()` errors to become
  `ValidateContextError::CalculateBlockChainValueChange`.
- `zebra-state/src/service/non_finalized_state.rs:591-604` has the equivalent
  non-finalized contextual error mapping.

### Local Repro Test

Current HEAD verification on 2026-05-07:

```sh
cargo test -p zebra-chain chain_value_pool_change_drops_transaction_value_balance_errors_today --lib
```

Result:

```text
test block::tests::vectors::chain_value_pool_change_drops_transaction_value_balance_errors_today ... ok
```

The test constructs a transaction whose own `value_balance()` returns `Err`, then
shows `Block::chain_value_pool_change()` returns `Ok` with a zero value-pool
delta.

### Impact

Confidence is high for the code bug and direct reproducer.

Current exploitability is less certain:

- The normal semantic block path rejected our concrete synthetic transparent
  coinbase overflow before state commit.
- Non-coinbase transaction value-balance failures appear to be checked earlier
  while deriving miner fees and again before contextual block construction.
- Checkpointed historical blocks are constrained by trusted checkpoint hashes.

The remaining risk is validator accounting and state integrity if this helper is
reached from checkpoint/finalized/replay paths with malformed, inconsistent, or
future transaction data. In that case, callers expecting an error can instead
persist or build on an undercounted block value-pool delta.

This should be treated as lower severity than the process-fatal panic reports,
but it is consensus-adjacent accounting code and worth fixing.

### Suggested Fix Direction

Replace the `flat_map(Result)` aggregation with error-propagating iteration, for
example:

```rust
let tx_pool_sum =
    self.transactions
        .iter()
        .try_fold(ValueBalance::zero(), |acc, tx| {
            let tx_balance = tx.value_balance(utxos)?;
            acc + tx_balance
        })?;
```

Also remove replay/migration fallbacks that convert
`chain_value_pool_change()` failures into zero deltas.

### Reference In Our Notes

- `docs/analysis/value-pool-error-suppression-note.md`
- `docs/analysis/state-format-migration-b5-revisit-note.md`
