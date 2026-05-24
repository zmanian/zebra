# State Startup Validation Admission Barrier Note

Date: 2026-05-09

Status: local-only audit note. Do not post publicly without explicit user
direction. Proof-backed for the non-upgrade `finished_format_upgrades` ordering.

## Summary

Zebra opens finalized state and returns usable `ZebraDb` / state-service handles
before the background disk-format check has finished. For normal upgrades this
is an intentional background migration model, but for `CheckOpenCurrent` and
`Downgrade` paths it means a database that should be rejected by detailed
format validation can briefly serve reads, and potentially accept finalized
writes, before the checker thread reports a panic.

This is not a peer-triggered remote exploit on current evidence. It requires an
already-current or downgrade-opened database that is malformed in a way the
detailed validators catch. It is best treated as startup hardening and local
data-integrity defense.

## Source Evidence

- `zebra-state/src/service/finalized_state/zebra_db.rs:103` through `:116`
  determines the disk version and builds a `DbFormatChange`.
- `zebra-state/src/service/finalized_state/zebra_db.rs:124` through `:154`
  opens RocksDB and spawns the format-change task.
- `zebra-state/src/service/finalized_state/zebra_db.rs:156` returns the
  database handle immediately after spawning that task.
- `zebra-state/src/service/finalized_state/zebra_db.rs:160` through `:177`
  launches the format-change worker in the background. A source comment at
  `:165` through `:167` explicitly notes that new blocks can be committed as
  soon as this method returns.
- `zebra-state/src/service/finalized_state/disk_format/upgrade.rs:384`
  through `:388` marks non-upgrade paths as finished before the match body and
  before detailed checks run.
- `zebra-state/src/service/finalized_state/disk_format/upgrade.rs:416`
  through `:423` handles `CheckOpenCurrent` by logging that validity will be
  checked below, not before the handle is returned.
- `zebra-state/src/service/finalized_state/disk_format/upgrade.rs:641`
  through `:654` runs detailed validators and returns an error result only
  after all validators have been executed.
- `zebra-state/src/service.rs:1281` through `:1303` shows read-service
  `poll_ready()` checking for already-reported format-change panics, but it
  does not block while the checker is still running.
- `zebra-state/src/service.rs:1307` and later dispatch read requests against a
  cloned read state immediately after readiness.

## Impact

The issue is an admission-barrier gap: detailed validation is asynchronous, so
the system can briefly treat a not-yet-validated current-format database as
operational.

Possible effects if a malformed current-version database is present:

- RPC/read callers can observe data before the validator rejects the database.
- The block write path can append finalized blocks before a delayed checker
  failure is propagated.
- For non-upgrade paths, `finished_format_upgrades()` is set before validation,
  so address-balance writes use the post-upgrade `Insert` mode rather than the
  migration-safe `Merge` mode.

This does not currently look like a private-disclosure candidate by itself:
the attacker model is local disk corruption, interrupted operator workflows, or
unsupported cross-version reuse. The security relevance is that a consensus-
critical node should reject invalid state before exposing normal read/write
services.

## Duplicate Check

This is adjacent to, but distinct from, the B5 state-format migration notes:

- `docs/analysis/state-format-migration-b5-revisit-note.md` and the related
  continuation notes already cover live reads during the V27
  `BlockInfo` / address-received background migration.
- This note covers `CheckOpenCurrent` and `Downgrade` admission behavior when a
  database is already marked current or compatible but still needs detailed
  validation.
- Searches for `admission barrier`, `CheckOpenCurrent`,
  `format_validity_checks_detailed`, and `mark_finished_format_upgrades` in
  `docs/analysis` did not find an existing local note focused on this exact
  startup validation ordering.

## Current-Behavior Proof

Added focused test:

```sh
cargo test -p zebra-state check_open_current_marks_upgrades_finished_before_validation_panics_today --lib
```

Result on 2026-05-09: passed.

The test creates a current-version `ZebraDb` with raw block/header/transaction
data but missing detailed-format data such as tree/block-info rows. Standalone
detailed validation fails while `finished_format_upgrades()` remains false.
Then `DbFormatChange::CheckOpenCurrent::run_format_change_or_check()` is run
against the same database. The format check panics as expected, but only after
the non-upgrade path has already marked `finished_format_upgrades()` true.

This does not prove a live RPC timing race by itself; it proves the admission
ordering and early finished-upgrade flag that make the race possible in a
background startup checker.

## Remaining Proof Sketch

1. Create or modify a persistent state database whose version file matches the
   running Zebra format.
2. Introduce a validator-visible disk-format defect, such as duplicate legacy
   and current tree-tip keys or a missing subtree row, without changing the
   version file.
3. Start Zebra.
4. Before the background `format_validity_checks_detailed()` thread fails, make
   a normal read request or allow a block-commit harness to advance finalized
   state.
5. Expected behavior from the source ordering: the request can proceed until
   the checker thread finishes and its panic is later observed through
   `check_for_panics()`.

## Recommended Fix

- Treat startup format validation as an admission barrier for
  `CheckOpenCurrent` and `Downgrade` paths, or expose an explicit "state not
  ready" mode that blocks read/write services until validation succeeds.
- Delay `mark_finished_format_upgrades()` for non-upgrade paths until after
  detailed validation succeeds.
- Keep true data migrations interruptible, but make user-facing query surfaces
  report "upgrading" or "not ready" for derived indexes that are being rebuilt.

## Confidence

Medium source confidence in the ordering gap: the code clearly opens the
database, spawns validation, and returns before checks complete.

Low-to-medium practical severity: exploiting it requires an already malformed
or unusual on-disk database, and the validator thread should eventually panic
rather than silently accepting the state forever.
