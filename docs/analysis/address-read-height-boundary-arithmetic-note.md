# Address Read Height Boundary Arithmetic Note

Date: 2026-05-09

Status: local-only hardening. Do not post publicly without explicit user
direction.

## Summary

Address-index read helpers compute the non-finalized overlap start as one block
after the finalized tip. That assumption is ordinary for current chain state,
but the helpers do not all handle terminal or invalid height values the same
way:

- the balance overlay path unwraps `(tip + 1)` and panics at the valid consensus
  terminal `Height::MAX`;
- the txid and UTXO overlay paths use raw `u32 + 1`, so they do not panic at
  `Height::MAX`, but debug builds panic if an invalid public tuple height such
  as `Height(u32::MAX)` reaches the helper.

This is not a normal remote vulnerability. Current finalized storage bounds are
far below `Height::MAX`, and normal height parsing rejects values above
`Height::MAX`. The value is in malformed-state, synthetic-state, and future
storage hardening: address RPC/indexer reads should return a stable error or
"no child height" result instead of panicking or wrapping.

## Evidence

Balance overlay:

- `zebra-state/src/service/read/address/balance.rs` computes
  `required_chain_root` with `(tip + 1).unwrap()`.
- `zebra-chain/src/block/height.rs` makes `Height + 1` return `None` above
  `Height::MAX`.

Txid and UTXO overlays:

- `zebra-state/src/service/read/address/tx_id.rs` computes
  `finalized_tip_range.start().0 + 1`.
- `zebra-state/src/service/read/address/utxo.rs` computes
  `finalized_tip_range.start().0 + 1`.
- `Height` is a public tuple struct, even though its invariants say callers
  should not construct heights above `Height::MAX`.

Current reachability reducers:

- `zebra-chain/src/block/height.rs` defines valid `Height::MAX` as
  `u32::MAX / 2`, not `u32::MAX`.
- Normal `TryFrom<u32>` and string parsing reject heights above `Height::MAX`.
- `zebra-state/src/service/finalized_state/disk_format/block.rs` documents the
  current 3-byte on-disk finalized height limit, far below `Height::MAX`.
- `zebra-state/src/service/finalized_state/zebra_db.rs` quick validity checks
  reject finalized tips that exceed half of that on-disk maximum.

## Current-Behavior Proof

Added three focused tests:

```sh
cargo test -p zebra-state terminal_finalized_tip_panics_balance_overlay_in_debug_today --lib
cargo test -p zebra-state invalid_max_finalized_tip_panics_tx_id_overlay_in_debug_today --lib
cargo test -p zebra-state invalid_max_finalized_tip_panics_utxo_overlay_in_debug_today --lib
```

Result on 2026-05-09: all passed.

The balance test passes `Some(Height::MAX)` as the finalized tip and confirms
debug builds currently panic while computing the capped next height. The txid
and UTXO tests pass `Height(u32::MAX)` as an invalid finalized tip and confirm
debug builds currently panic while computing raw `u32 + 1`. In release builds,
the raw `u32` cases do not use debug overflow checks, so the tests assert only
that the helper returns without panicking.

The initial hypothesis that txid/UTXO would panic at valid `Height::MAX` was
eliminated: `Height::MAX` is `u32::MAX / 2`, so raw `+ 1` is representable.

## Duplicate and Overlap Check

This is a sibling to `docs/analysis/rpc-boundary-arithmetic-hardening-note.md`,
which covers RPC response construction arithmetic. This note is limited to the
address-index read overlay helpers.

The existing local address-index notes cover query bounds, snapshot coherence,
and overlap assertions under ordinary finalized/non-finalized races. They do
not record this exact terminal/invalid-height next-child calculation.

## Impact

Low availability/correctness hardening.

An ordinary remote peer cannot advance Zebra to the required height, and an RPC
caller cannot directly construct these internal finalized-tip ranges. The
plausible sources are malformed/corrupted local state, synthetic test state, an
internal invariant violation, or future storage support for much larger heights.

The failure mode is an address read panic in debug/test builds, and potentially
wrapped or inconsistent overlap calculations in release builds for invalid
public tuple heights. It does not bypass validation or change consensus.

## Suggested Fix Direction

- Replace `(tip + 1).unwrap()` in the balance overlay with explicit
  `Height::next()` handling and a stable "no child height" path.
- Replace raw `.0 + 1` in txid and UTXO overlays with checked `Height::next()`
  or `checked_add(1)` plus an explicit error.
- Add regression tests for valid `Height::MAX` and invalid public tuple heights
  so the three address read helpers converge on the same boundary behavior.

## Confidence

High confidence in the source-level arithmetic and current test behavior.
Low confidence in practical exploitability because current storage and parsing
bounds keep these values out of normal deployments.
