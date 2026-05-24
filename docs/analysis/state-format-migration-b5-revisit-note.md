# State Format Migration B5 Revisit Note

Date: 2026-05-03

Scope: follow-up on pass-5 Workstream B5, focused on storage migrations,
format-version drift, and multi-column-family consistency.

## Summary

I did not find a new remotely triggerable state-format vulnerability beyond the
already documented value-pool error-suppression issue. The current migration
framework generally marks upgrades complete only after `prepare()`, `run()`, and
`validate()` succeed, and uses per-height or per-block RocksDB write batches for
the data reviewed here.

The main remaining concern is not a separate remotely triggerable B5 item: the
v27 `BlockInfo` replay migration still depends on
`Block::chain_value_pool_change()`, and converts a failure into a zero delta
with `unwrap_or_default()`. That strengthens the existing private maintainer
heads-up for value-pool accounting, because the migration can persist derived
metadata that later reads trust, while validation only checks recent presence
and nonzero fields rather than exact historical value-pool correctness.

## Evidence

Upgrade registration and completion ordering:

- `zebra-state/src/service/finalized_state/disk_format/upgrade.rs:95-105`
  lists format upgrades in order: prune trees, add subtrees, tree key/cache
  fixes, v26 no-migration marker, and v27 block-info/address-received data.
- `zebra-state/src/service/finalized_state/disk_format/upgrade.rs:571-592`
  runs `prepare()`, `run()`, and `validate()` for each migration before calling
  `mark_as_upgraded_to()`.
- `zebra-state/src/service/finalized_state/disk_format/upgrade.rs:860-865`
  has a unit test asserting upgrade versions are strictly increasing.

Live verification:

```sh
cargo test -p zebra-state format_upgrades_are_in_version_order --lib
```

Result: passed, one test run.

v27 block-info/address-received upgrade behavior:

- `zebra-state/src/service/finalized_state/disk_format/upgrade/block_info_and_address_received.rs:62-153`
  loads each height up to the initial finalized tip, skipping heights that
  already have `BlockInfo`.
- `zebra-state/src/service/finalized_state/disk_format/upgrade/block_info_and_address_received.rs:155-229`
  writes each height's `BlockInfo` and transparent address received-balance
  merge operands in a single `DiskWriteBatch`.
- `zebra-state/src/service/finalized_state/zebra_db/block.rs:536-549`
  makes live block commits use merge operands while format upgrades are still
  running, avoiding overwrite races with the migration's address-balance writes.
- `zebra-state/src/service/finalized_state/zebra_db/block.rs:676-680` and
  `zebra-state/src/service/finalized_state/zebra_db/chain.rs:256-299` write
  `BlockInfo` for newly finalized blocks through the ordinary block commit path.

The important caveat is value-pool replay:

- `zebra-state/src/service/finalized_state/disk_format/upgrade/block_info_and_address_received.rs:201-210`
  calls `block.chain_value_pool_change(...).unwrap_or_default()` while deriving
  the cumulative value pool for historical `BlockInfo`.
- `zebra-state/src/service/finalized_state/disk_format/upgrade/block_info_and_address_received.rs:81-87`
  treats any existing `BlockInfo` as authoritative and resumes from its stored
  value pool. If a previous interrupted run wrote wrong-but-nondefault
  `BlockInfo`, a later run can use that value as the cumulative baseline
  instead of recomputing it.
- `zebra-state/src/service/finalized_state/disk_format/upgrade/block_info_and_address_received.rs:260-270`
  validates only that recent blocks have non-default `BlockInfo`; it does not
  recompute expected value pools for every height.
- `zebra-state/src/service/finalized_state/disk_format/upgrade/block_info_and_address_received.rs:275-303`
  validates that recent transparent recipient addresses have nonzero received
  balances, but this does not catch a nonzero-yet-wrong value-pool delta.
- `zebra-state/src/service/finalized_state/zebra_db/chain.rs:256-270`
  shows the normal finalized-commit path propagates value-pool calculation
  errors instead of defaulting them, so the silent fallback is specific to the
  replay migration path reviewed here.

Live-read exposure during upgrade:

- `zebra-state/src/service/finalized_state/zebra_db.rs:154-179` spawns the
  format-change task after opening the database and then returns the `ZebraDb`
  handle.
- `zebra-state/src/service/finalized_state/disk_format/upgrade.rs:384-395`
  marks `finished_format_upgrades` only after upgrade execution completes.
- `zebra-state/src/service/read/block.rs:347-364` and
  `zebra-state/src/service/finalized_state/zebra_db/chain.rs:183-190` read
  `BlockInfo` directly from the finalized database if it is not in the
  non-finalized chain. During a long v27 migration, historical `BlockInfo`
  queries can therefore observe `None` or partial derived metadata until the
  background upgrade reaches those heights.
- `zebra-state/src/service/finalized_state/disk_format/transparent.rs:787-804`
  defaults a missing legacy `received` field to zero for backward
  compatibility, so address received-balance reads can also look plausible
  while still missing the v27-derived total.

Disk-format decode panics:

- `zebra-state/src/service/finalized_state/disk_format/chain.rs:111-123`
  panics on malformed `BlockInfo` byte lengths and unwraps the constrained
  value-pool decode for 40-byte value-pool data.

I did not find a path from valid peer block data or an RPC query to malformed
RocksDB bytes. On current evidence, these panics remain local database
corruption or invalid manual/test write hazards rather than remote
vulnerabilities.

## Eliminated Leads

- **Partial v27 interruption as a direct RocksDB corruption vector:** per-height
  writes are atomic and the version marker is not advanced until validation
  succeeds. The remaining issue is semantic rather than torn-write corruption:
  resumption trusts existing `BlockInfo`, so a previously written wrong value
  can become the new cumulative baseline.
- **Live sync during v27 upgrade:** new finalized blocks write their own
  `BlockInfo`; address balances use merge operands until
  `finished_format_upgrades` is true, so ordinary commits should not overwrite
  concurrent migration updates. This does not prevent read clients from seeing
  partial historical derived metadata while the upgrade is still running.
- **Format upgrade registry drift:** the current upgrade list is ordered and
  covered by the targeted unit test above.
- **Malformed disk bytes from remote inputs:** serialization writes constrained
  types, and the reviewed unwraps require disk corruption or inconsistent local
  state, not just valid attacker-supplied chain/RPC data.

## Triage

No new private disclosure item from B5 alone.

Keep the value-pool migration fallback bundled with the existing private
value-pool maintainer heads-up: if `chain_value_pool_change()` can ever fail
for finalized data, the migration can write wrong `BlockInfo`, resume from that
wrong value on later runs, and pass shallow validation. The migration should
fail loudly with height/hash context instead of writing a zero delta.

Suggested public hardening:

- make the v27 validation recompute and compare sampled or full historical
  `BlockInfo` value pools, especially around upgrade/restart boundaries;
- replace `unwrap_or_default()` in migration replay with an error path;
- avoid exposing derived `BlockInfo` or `received` RPC/read semantics until the
  relevant format upgrade has completed, or document that pre-completion reads
  are incomplete;
- convert disk `FromDisk` panics into structured corruption errors where callers
  can report a clean state-rebuild instruction.
