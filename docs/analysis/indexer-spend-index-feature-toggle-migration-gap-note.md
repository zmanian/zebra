# Indexer Spend-Index Feature-Toggle Migration Gap Note

Date: 2026-05-09

Status: local-only audit note. Do not post publicly without explicit user
direction.

## Summary

The indexer and non-indexer database-format compatibility migrations can leave
transparent spend-to-transaction lookup data permanently incomplete after an
interrupted feature-toggle startup.

The risky sequence is:

1. Open an indexer-built finalized-state database with a non-indexer binary.
2. `drop_tx_locs_by_spends` globally deletes the transparent
   `tx_loc_by_spent_output_loc` column family range.
3. The process is cancelled or crashes before every height's shielded nullifier
   values are rewritten from indexed `TransactionLocation` values to non-indexer
   unit values.
4. Reopen the same database with an indexer binary.
5. `track_tx_locs_by_spends` treats a whole height as already indexed if the
   first spend/nullifier it probes still resolves through any spend index.

If the first surviving evidence in a height is a shielded nullifier that still
has its old indexed value, the indexer rebuild migration can skip the entire
height even though transparent spend mappings for later transactions in that
height were deleted globally.

## Source Evidence

- `zebra-state/src/service/finalized_state/disk_format/upgrade.rs:477` only runs
  the non-indexer drop migration when the on-disk build metadata still contains
  `indexer`.
- `zebra-state/src/service/finalized_state/disk_format/upgrade/drop_tx_locs_by_spends.rs:27`
  opens `tx_loc_by_spent_output_loc_cf`, and
  `zebra-state/src/service/finalized_state/disk_format/upgrade/drop_tx_locs_by_spends.rs:29`
  deletes the whole transparent spent-output-location range before the
  per-height loop starts.
- `zebra-state/src/service/finalized_state/disk_format/upgrade/drop_tx_locs_by_spends.rs:55`
  rewrites shielded nullifier batches per height, so an interrupted run can
  leave the transparent index deleted while some shielded nullifier entries
  still have indexer-era values.
- `zebra-state/src/service/finalized_state/disk_format/upgrade.rs:460` runs
  `track_tx_locs_by_spends` on indexer opens.
- `zebra-state/src/service/finalized_state/disk_format/upgrade/track_tx_locs_by_spends.rs:36`
  starts each height with `should_index_at_height = false`.
- `zebra-state/src/service/finalized_state/disk_format/upgrade/track_tx_locs_by_spends.rs:48`
  probes only until it finds the first spend/nullifier evidence for that height.
- `zebra-state/src/service/finalized_state/disk_format/upgrade/track_tx_locs_by_spends.rs:58`
  calls `read::spending_transaction_hash(None, zebra_db, spend)`.
- `zebra-state/src/service/finalized_state/disk_format/upgrade/track_tx_locs_by_spends.rs:62`
  returns `Ok(())` for the whole height when that probe succeeds.
- If the probe returns `Some`, the migration returns `Ok(())` for the whole
  height instead of rebuilding later transactions in that height.
- `zebra-state/src/service/finalized_state/disk_format/upgrade/track_tx_locs_by_spends.rs:85`
  through `:90` is the transparent-spend and nullifier rebuild path that gets
  skipped.
- Normal indexer commits do write transparent spend mappings:
  `zebra-state/src/service/finalized_state/zebra_db/transparent.rs:719` through
  `:720`.

The source-to-sink path is finalized-state disk format corruption after a local
feature-toggle interruption, then indexer/RPC reads through
`spending_transaction_hash`:

- `zebra-state/src/service/read/block.rs:305` multiplexes non-finalized and
  finalized spend lookups.
- `zebra-state/src/service/finalized_state/zebra_db/block.rs:373` through
  `:381` uses the transparent spent-output-location index for
  `Spend::OutPoint`, and nullifier indexes for shielded spends.

## Impact

This is not a default peer-only remote exploit. It requires local deployment
conditions: an indexer database, a non-indexer open, an interrupted migration,
a later indexer open, and an affected height whose first surviving spend
evidence is shielded while later transparent spends need rebuild.

The impact is persistent index integrity loss for affected transparent spends.
Indexer consumers asking for the transaction that spent a transparent outpoint
can receive absence for a spend that exists in finalized chain data. This can
mislead services that rely on Zebra's optional indexer read API for historical
transparent spend tracking.

## Duplicate Check

Public issue searches returned zero results for:

- `repo:ZcashFoundation/zebra track_tx_locs_by_spends`
- `repo:ZcashFoundation/zebra tx_loc_by_spent_output_loc indexer migration`
- `repo:ZcashFoundation/zebra spending transaction id migration indexer`
- `repo:ZcashFoundation/zebra feature toggle indexer spend index`

Local docs contain a prior address-index spending-ID recheck in
`docs/analysis/local-audit-continuation-2026-05-09.md`, but that recheck only
covered normal upgrade assumptions and database consistency. It did not cover
the interrupted non-indexer drop plus indexer rebuild mixed-state sequence.

## Recommended Fix

- Make `track_tx_locs_by_spends` idempotently rebuild all spend-index entries
  for every height instead of skipping a whole height based on one successful
  probe.
- Alternatively, probe both transparent and shielded index families for the
  specific representation expected by the current build before treating a height
  as complete.
- Make the non-indexer drop migration update/deletion order more crash-safe:
  avoid a global transparent delete before per-height nullifier rewrites, or
  write an explicit migration-in-progress marker that forces a full rebuild on
  any later indexer open.
- Add a mixed-state migration test that simulates a database where the
  transparent spent-output-location CF is empty but a height still has old
  indexed shielded nullifier values.

## Confidence

High confidence in the source-level mixed-state hazard and persistence impact.
Medium-low confidence in practical prevalence because it depends on unusual
local feature toggling and interrupted startup, not ordinary network traffic.
