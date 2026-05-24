# Time Consensus Parity Note

Date: 2026-05-03

Last updated: 2026-05-09

Disposition: local-only eliminated lead. Do not post publicly without explicit
re-authorization.

## Summary

No confirmed time-related consensus or mempool-policy vulnerability was found in
this pass.

The reviewed paths line up in the important places:

- mined transaction locktime is checked against the candidate block height and
  candidate block time;
- mempool time-lock validation uses best-chain next median-time-past, not local
  wall-clock time;
- transaction expiry is checked before UTXO/cache work, so a stale verified UTXO
  result cannot bypass expiry in the current checkout;
- contextual block validation enforces both strict median-time-past and the
  90-minute maximum time-since-MTP rule; and
- the context-free two-hour node-local future-time check is a separate
  non-deterministic admission rule, not the consensus MTP rule.

This is a useful eliminated lead, not a private disclosure item.

## Evidence

### Transaction Locktime

`zebra-consensus/src/transaction.rs:422-456` runs expiry first, then locktime:

- `coinbase_expiry_height()` / `non_coinbase_expiry_height()` run before UTXO
  lookup and script/proof work.
- For mined blocks, `check::lock_time_has_passed(&tx, req.height(), block_time)`
  uses the candidate block's supplied time.
- For mempool requests, Zebra only asks state for
  `BestChainNextMedianTimePast` when `tx.lock_time_is_time()` is true, then runs
  the same `lock_time_has_passed()` check.

`zebra-consensus/src/transaction/check.rs:67-99` implements strict boundary
semantics:

- height locks pass only when `block_height > unlock_height`;
- time locks pass only when `block_time > unlock_time`; and
- time locks fail when no candidate block time / next MTP is available.

`zebra-chain/src/transaction.rs:412-458` matches zcashd's effective locktime
behavior by ignoring locktime unless at least one transparent input has a
sequence number other than `u32::MAX`.

`zebra-chain/src/transaction/lock_time.rs:120-133` deserializes the wire `u32`
locktime into a valid `Height` or `Time`, so attacker-supplied wire data cannot
construct an out-of-range `LockTime::Time`.

### Expiry

`zebra-consensus/src/transaction/check.rs:373-441` enforces coinbase and
non-coinbase expiry-height rules:

- NU5+ coinbase expiry must equal the mined height.
- Non-coinbase transactions are rejected once `block_height > expiry_height`.
- The maximum expiry-height bound is checked before acceptance.

The prior local stale-cache probe in
`zebra-consensus/src/transaction/tests.rs:4006-4117` is stale for this checkout:
the current verifier checks expiry before cache-sensitive UTXO work.

### Block Time

`zebra-consensus/src/block/check.rs:400-407` calls
`Header::time_is_valid_at()`, which is the node-local "not more than two hours
in the future according to this node's clock" admission rule.
`zebra-chain/src/block/header.rs:107-126` documents that this rule is
non-deterministic and can become valid later.

The actual consensus time checks happen in state contextual validation.
`zebra-state/src/service/check.rs:267-321` computes median-time-past from the
candidate's parent context and rejects:

- `candidate_time <= median_time_past` for all non-genesis blocks; and
- `candidate_time > median_time_past + 90 minutes` after the network-specific
  enforcement height.

`zebra-state/src/service/check/difficulty.rs:338-363` computes the MTP median
from the recent parent times after sorting. `zebra-state/src/service/read/find.rs:621-697`
uses the same median function for mempool's best-chain next-MTP query and
checks the finalized tip before/after the multi-step read, retrying if it moved.

### Reorg / Best-Chain Source

The mempool MTP query is served by
`zebra-state/src/service/read/find.rs:621-697`, which builds the relevant chain
from the latest non-finalized state plus finalized DB, then detects concurrent
tip movement. That keeps the mempool locktime decision tied to the current best
chain rather than a cached wall-clock or stale MTP value.

## Eliminated Hypotheses

| Hypothesis | Result | Backstop |
| --- | --- | --- |
| Mempool time locks use wall-clock time | Eliminated | Mempool requests `BestChainNextMedianTimePast` only for time locks and then calls the same locktime helper. |
| Equal-to-boundary locktime is accepted too early | Eliminated | `lock_time_has_passed()` requires strict `>` for height and time. |
| Expired transactions can use cached UTXO verification to bypass expiry | Eliminated in current checkout | Expiry checks run before UTXO/cache-sensitive work in `zebra-consensus/src/transaction.rs`. |
| Wire locktime can create an invalid out-of-range `LockTime::Time` | Eliminated | Deserialization maps every `u32` to either height or valid UTC timestamp. |
| Header future-time consensus is only the local two-hour rule | Eliminated | State contextual validation separately enforces MTP and MTP+90 minutes. |
| Mempool MTP can be stale across reorg/finalization movement | Eliminated as an obvious bug | State detects tip movement during the read and retries. |

## Verification

Targeted tests rerun locally on 2026-05-09:

- `cargo test -p zebra-consensus mempool_request_with_invalid_lock_time_is_rejected --lib`
- `cargo test -p zebra-consensus transaction_is_rejected_based_on_lock_time --lib`
- `cargo test -p zebra-consensus time_is_valid_for_historical_blocks --lib`
- `cargo test -p zebra-consensus mempool_cached_result_bypasses_expiry_check_for_block_at_next_height --lib`

All four passed. The last test is a stronger regression for the old
mempool-cache expiry-bypass shape: a transaction cached as valid near the tip is
presented inside a later block where its expiry height has passed, and the block
request must still return `ExpiredTransaction`.

## Recommendation

No private disclosure. Keep this as an eliminated-lead note.

Useful local hardening would be small regression coverage around the exact
block-time consensus boundaries in `zebra-state`:

- candidate time equal to MTP is rejected;
- candidate time MTP+90 minutes is accepted at enforced heights; and
- candidate time MTP+90 minutes+1 second is rejected.

## Duplicate Check

Read-only duplicate search refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra time consensus locktime median time past future time mempool'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "BestChainNextMedianTimePast" locktime mempool'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "median time past" "mempool"'
```

Closest historical hits:

- closed #3060 validates transaction lock times;
- closed #5984 reports the original missed mempool `nLockTime`/MTP check; and
- closed #6027 implements `BestChainNextMedianTimePast` and mempool locktime
  validation.

These are direct historical coverage for the locktime/MTP portion of this lead,
not fresh unresolved findings.

The remaining local hardening idea is still a direct state-level boundary test
for the MTP+90-minute contextual rule.
