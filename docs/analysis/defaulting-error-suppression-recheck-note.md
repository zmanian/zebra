# Defaulting and error-suppression recheck

Date: 2026-05-03

Scope: follow-up on four `unwrap_or_default()` / default-to-zero candidates
that looked security-relevant during the post-v4.4.0 audit. This note records
the source-to-sink review so these paths do not keep reappearing as fresh
findings.

## Summary

No new remotely exploitable vulnerability was confirmed in this recheck.

Three candidates are eliminated:

- transparent address-balance `received` defaulting is a local finalized-state
  disk-format compatibility path;
- ZIP-317 `unpaid_actions` defaulting is the intended negative-to-zero clamp;
- `proposal_block_from_template()` defaulting uses `CurTime` for an internal
  block-construction helper and is not the external proposal-validation path.

The only remotely influenced sink is verbose `getrawmempool` descendant-fee
formatting. Its impossible-path overflow fallback can silently report zero, but
the overflow appears unreachable from valid mempool contents under Zebra's value
and conflict invariants, and the value is RPC accounting output only.

## Transparent address-balance `received`

Candidate:

- `zebra-state/src/service/finalized_state/disk_format/transparent.rs:787-799`

`AddressBalanceLocationInner::from_bytes()` reads the fixed balance/location
prefix and then uses `split_at_checked(size_of::<u64>()).unwrap_or_default()`
for the trailing `received` field. The adjacent comment says this exists for
backwards compatibility: old values without the trailing field default to
`received = 0`.

Control surface:

- not peer-controlled;
- not RPC-controlled;
- controlled by local RocksDB contents and upgrade history.

Verdict: eliminated as a remote issue. A truncated or legacy local value can
under-report the transparent `received` total, but it does not affect consensus
validation, mempool acceptance, or block construction.

## ZIP-317 unpaid actions

Candidate:

- `zebra-chain/src/transaction/unmined/zip317.rs:90-105`

The code computes:

```text
unpaid_actions = conventional_actions - floor(miner_fee / marginal_fee)
```

as an `i64`, then converts to `u32` with `try_into().unwrap_or_default()`.

This is the intended `max(0, ...)` operation. The conversion can fail when the
intermediate value is negative, and zero is the correct result. A positive value
above `u32::MAX` is not reachable because `conventional_actions` is already a
`u32`.

Verdict: eliminated; no incorrect validation outcome.

## GBT proposal helper time default

Candidate:

- `zebra-rpc/src/methods/types/get_block_template/proposal.rs:201-205`

`proposal_block_from_template()` defaults a missing `time_source` to `CurTime`.
External RPC proposal validation does not use this helper; `mode=proposal` with
`data` goes through `validate_block_proposal()`, which deserializes the supplied
block bytes and sends them through consensus proposal verification.

Verdict: eliminated as an external proposal-validation issue.

## Verbose `getrawmempool` descendant fees

Candidate:

- `zebra-rpc/src/methods/types/get_raw_mempool.rs:80-105`

Verbose mempool formatting sums direct dependent fees using:

```rust
(fee1 + fee2).unwrap_or_default()
(deps_fees + unmined_tx.miner_fee).unwrap_or_default()
```

`Amount<NonNegative>` addition returns an error on overflow, so these fallbacks
would silently turn an overflowed aggregate into zero.

Control surface:

- mempool contents are influenced by remote transaction submissions;
- the sink is RPC output from `getrawmempool(verbose=1)`.

Reachability assessment:

- each `miner_fee` is a verified `Amount<NonNegative>`;
- valid mempool transactions cannot create arbitrary independent spend value;
- conflict handling prevents accepting mutually conflicting transactions as a
  fee-amplifying set;
- the output is not reused by mempool admission, ZIP-317 mining selection,
  `getblocktemplate`, consensus, or state.

Verdict: not a confirmed security issue. If the invariant were already broken,
the consequence would be misleading RPC accounting (`descendantfees = 0`), not
consensus acceptance or node crash. A defense-in-depth cleanup could replace the
silent default with an explicit invariant error or log, but this is not private
disclosure material on its own.
