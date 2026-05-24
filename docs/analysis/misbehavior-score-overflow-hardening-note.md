# Misbehavior Score Overflow Hardening Note

Date: 2026-05-09

Status: local-only. Do not post publicly without explicit user direction.

## Summary

Peer misbehavior scoring uses raw `u32` addition before ban enforcement in two
places: the peer-set misbehavior batcher and `MetaAddrChange::UpdateMisbehavior`
application. Normal score producers use small values and the ban threshold is
low, so this is not a practical standalone vulnerability on current evidence.
It is still worth hardening with saturating addition because an already-large
score panics in debug builds and wraps before the ban check in release builds.

## Evidence

The peer-set misbehavior batcher coalesces score increments with raw `+=`:

- `zebra-network/src/peer_set/initialize.rs:141-145`

`MetaAddrChange::UpdateMisbehavior` is later applied by adding the previous
stored score to the incoming score increment:

- `zebra-network/src/meta_addr.rs:1154`
- `zebra-network/src/meta_addr.rs:1180`

Address-book ban enforcement happens after that combined score is constructed:

- `zebra-network/src/address_book.rs:444`

## Current-Behavior Proofs

Added focused tests:

```sh
cargo test -p zebra-network misbehavior_update_addition_overflows_before_ban_today --lib
cargo test -p zebra-network misbehavior_batch_accumulator_overflows_before_flush_today --lib
```

Result on 2026-05-09: both passed.

The `MetaAddrChange` test creates an existing `MetaAddr` with
`misbehavior_score = u32::MAX` and then applies an `UpdateMisbehavior` increment
of `1`. In debug builds, the raw addition panics before ban enforcement observes
the score. In release builds, the same unchecked addition wraps to zero before
the ban check.

The peer-set batcher test exercises the same helper used by the misbehavior
batch task. It seeds the pending batch map with `u32::MAX` for one peer, applies
an increment of `1`, and confirms the debug-panic / release-wrap behavior before
the batch can flush a ban-threshold score to the address-book updater.

## Duplicate Check

Read-only GitHub searches on 2026-05-09 returned no exact issue hits:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "misbehavior_score" overflow'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "UpdateMisbehavior" overflow'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra misbehavior score saturating addition'
```

The closest local notes are:

- `docs/analysis/misbehavior-reporting-lossy-channel-note.md`, which covers
  dropped reports on a full channel rather than arithmetic overflow.
- `docs/analysis/address-book-misbehavior-ban-max-connections-panic-finding.md`,
  which covers a separate ban-path panic after the threshold is reached.

## Impact

Suggested severity: low hardening.

Current remote practicality is low because normal score increments are `100` or
`0`, and `MAX_PEER_MISBEHAVIOR_SCORE` is `100`, so ordinary live scores should
ban before approaching `u32::MAX`. The remaining concern is defensive robustness:
large existing scores, future score producers, or corrupted/internal state can
panic in debug builds or wrap in release builds before the intended ban check.

## Suggested Fix Direction

- Use `saturating_add()` in the peer-set misbehavior batcher.
- Use `saturating_add()` when applying `MetaAddrChange::UpdateMisbehavior`.
- Add regression tests proving `u32::MAX + 1` remains at `u32::MAX` and still
  triggers ban enforcement.

## Confidence

Confidence is high on both unchecked arithmetic sites and their debug/release
behavior. Confidence is low on current practical exploitability as an
attacker-triggered availability issue.
