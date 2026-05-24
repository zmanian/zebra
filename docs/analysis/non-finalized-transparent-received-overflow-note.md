# Non-finalized transparent received overflow note

Date: 2026-05-04

Public issue: <https://github.com/ZcashFoundation/zebra/issues/10556>

## Summary

Non-finalized transparent address accounting sums the `received` total with
ordinary `u64` addition, while finalized accounting uses saturating addition.
For an address that receives the same value repeatedly through transparent
self-transfer churn inside the non-finalized window, `getaddressbalance` can
therefore report a wrapped `received` total in release builds, or panic in
debug builds with overflow checks.

This is not a consensus issue. It affects address-index RPC correctness and
debug-build availability for nodes serving `getaddressbalance` over recent
non-finalized state.

## Evidence

- `TransparentTransfers::received()` sums created UTXO values with
  `.sum::<u64>()` in
  `zebra-state/src/service/non_finalized_state/chain/index.rs:233-237`.
- `Chain::partial_transparent_balance_change()` combines per-address non-finalized
  totals with `received + transfers.received()` in
  `zebra-state/src/service/non_finalized_state/chain.rs:1353-1365`.
- The final merge with finalized state uses `saturating_add()` and explicitly
  documents that addresses can receive more than the max money supply by sending
  to themselves in `zebra-state/src/service/read/address/balance.rs:137-147`.
- Finalized transparent address-balance disk-format addition also uses
  `saturating_add()` for `received` in
  `zebra-state/src/service/finalized_state/disk_format/transparent.rs:249-255`.
- The workspace sets `panic = "abort"` for the dev profile in
  `Cargo.toml:183-184`. Release builds do not globally enable overflow checks,
  so ordinary `u64` addition wraps instead of panicking.
- `zebra-state/src/service/non_finalized_state/tests/vectors.rs` now includes
  the current-behavior proof
  `non_finalized_transparent_received_panics_on_high_churn_today`.

Focused proof command:

```sh
cargo test -p zebra-state non_finalized_transparent_received_panics_on_high_churn_today --lib
```

Result on 2026-05-07:

```text
test service::non_finalized_state::tests::vectors::non_finalized_transparent_received_panics_on_high_churn_today - should panic ... ok
```

The test repeatedly indexes synthetic transparent receipts to one address and
spends each receipt back out so the address balance stays bounded while the
non-finalized `received` counter crosses `u64::MAX`.

## Reachability notes

This requires a high-churn but consensus-valid transparent address pattern: the
same address repeatedly receives value in blocks that are still non-finalized.
The total Zcash money supply is about 2,100,000,000,000,000 zatoshis, so
overflowing `u64::MAX` takes roughly 8,785 max-value receipts to the same
address.

That is not impossible at the protocol-size level. `MAX_BLOCK_BYTES` is
2,000,000 bytes (`zebra-chain/src/block/serialize.rs:24`), and Zebra's own
minimum transparent transaction-size constants put a transparent transaction at
54 bytes before real script sizes:

- `zebra-chain/src/transaction/serialize.rs:1136-1148`
- `zebra-chain/src/transaction/serialize.rs:1160-1168`

So the limiting factor is not the integer threshold by itself; it is producing a
valid block/chain with enough transparent self-transfer churn and acceptable
scripts/fees. That makes this a miner- or custom-chain-triggered RPC correctness
issue, not an ordinary low-cost peer or RPC-only attack.

## Impact

- In release builds, `getaddressbalance` can return a wrapped `received` value
  for affected addresses while the actual balance remains constrained by checked
  `Amount` arithmetic.
- In debug builds, the same addition can panic, and the workspace's dev profile
  aborts on panic.
- Downstream indexers or test infrastructure that rely on non-finalized
  `received` totals can see incorrect recent-chain accounting until the affected
  blocks finalize and the saturating finalized accounting takes over.

Severity is low for default public-node security because the attacker needs
valid recent blocks with extreme transparent churn, not just a malformed RPC
request. It is still worth fixing because the finalized path already chose the
safer semantics.

## Suggested fix

- Change `TransparentTransfers::received()` to use `saturating_add()`, or a
  small helper that sums transparent output values saturating at `u64::MAX`.
- Change `Chain::partial_transparent_balance_change()` to combine
  per-address totals with `received.saturating_add(transfers.received())`.
- Keep the current-behavior proof test and invert it after the fix so synthetic
  transfers that exceed `u64::MAX` saturate rather than panic or wrap.
- Consider documenting `received` as a saturating counter consistently across
  finalized and non-finalized address-index reads.

## Confidence

Confidence: high on the arithmetic mismatch and release/debug behavior.
Confidence: low-medium on practical exploitability in default public
deployments, because the trigger requires valid recent-chain construction with
extreme transparent self-transfer churn.
