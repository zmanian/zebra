# Checkpoint Subsidy Output Validation Gap Note

Date: 2026-05-09

Status: local-only. Do not post publicly without explicit user direction.

## Summary

Zebra's full block verifier validates the actual coinbase outputs for Canopy+
funding streams and NU6.1 one-time lockbox disbursements. The checkpoint
verifier does not. Instead, checkpoint verification derives a synthetic deferred
pool balance change from the configured schedule and commits the checkpoint block
with that precomputed value.

This means a checkpointed Canopy+ block can omit or redirect required
non-deferred funding-stream outputs, and a custom NU6.1 checkpointed block can
omit or redirect required lockbox disbursement outputs, while still being
accepted by checkpoint verification if the block hash matches the trusted
checkpoint chain.

This is not a live peer-only Mainnet/default-Testnet exploit. The attacker needs
control over, or a mistake in, the trusted checkpoint set or custom-network
checkpoint configuration. But within that trust boundary, checkpoint validation
can finalize economic accounting that the full semantic verifier would reject.

## Evidence

Checkpoint path:

- `zebra-consensus/src/checkpoint.rs:613-624` computes
  `funding_stream_values(height, network, block_subsidy(...))`, removes only the
  deferred receiver, subtracts `network.lockbox_disbursement_total_amount(height)`,
  and stores the resulting optional `DeferredPoolBalanceChange` in
  `CheckpointVerifiedBlock::new(...)`.
- `zebra-consensus/src/checkpoint.rs:626-630` then checks Merkle-root validity,
  but the checkpoint path never calls the full coinbase subsidy validators.

Full semantic path:

- `zebra-consensus/src/block/check.rs:247-254` computes the expected deferred
  pool contribution from the funding-stream schedule.
- `zebra-consensus/src/block/check.rs:261-281` handles the NU6.1 activation
  block by requiring non-empty lockbox disbursements, checking that each
  expected disbursement amount is present in the coinbase outputs, and mapping
  lockbox underflow to `SubsidyError::Underflow`.
- `zebra-consensus/src/block/check.rs:284-299` checks every non-deferred
  funding-stream receiver by deriving its expected address and requiring the
  expected amount in the coinbase outputs.
- `zebra-consensus/src/block/check.rs:312-368` later validates total coinbase
  input/output equality using the expected deferred pool change.

State sink:

- `zebra-state/src/request.rs:518-527` stores the checkpoint verifier's optional
  deferred pool balance change in `CheckpointVerifiedBlock`.
- `zebra-state/src/service.rs:1048-1070` accepts
  `Request::CommitCheckpointVerifiedBlock`, checks pending UTXO waiters against
  newly-created outputs, and queues the checkpoint block for finalized commit.

## Current-Behavior Proof

Added a focused comparison proof:

- `zebra-consensus/src/checkpoint/tests.rs` now has
  `checkpoint_check_block_accepts_missing_funding_stream_outputs_today`.
- The test builds a custom Testnet with proof-of-work disabled, mutates a
  funding-stream-era block's coinbase so the required funding-stream outputs are
  missing, and updates the Merkle root so the block body and header are
  internally consistent.
- It confirms full semantic subsidy validation rejects the block with
  `FundingStreamNotFound`.
- It then constructs a checkpoint verifier whose checkpoint list trusts the
  mutated block hash and confirms the checkpoint pre-check currently accepts the
  same malformed coinbase.

Verification:

```sh
cargo test -p zebra-consensus checkpoint_check_block_accepts_missing_funding_stream_outputs_today --lib
```

Result on 2026-05-09: passed.

## Duplicate and Overlap Check

This is distinct from existing local notes:

- `docs/analysis/checkpoint-lockbox-underflow-defaults-deferred-note.md` covers
  checkpoint arithmetic underflow when the configured lockbox total exceeds the
  expected deferred amount.
- `docs/analysis/value-pool-error-suppression-note.md` covers value-pool error
  suppression and replay/commit trust-boundary behavior.
- `docs/analysis/custom-network-implicit-nu6-1-lockbox-boundary-note.md` covers
  omitted custom `nu6_1` activation inheriting a later configured upgrade.
- `docs/analysis/custom-network-parameter-panic-sweep-note.md` and
  `docs/analysis/configured-funding-streams-config-panic-note.md` cover
  malformed custom-network configuration panics.

Targeted GitHub searches on 2026-05-09 did not find an exact public tracker for
this checkpoint-specific validation gap:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra checkpoint funding stream subsidy validation'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra CheckpointVerifier subsidy_is_valid lockbox'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra CommitCheckpointVerifiedBlock funding stream'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra checkpoint lockbox disbursement'
```

The closest search hits were broad historical subsidy/coinbase tracking issues
and NU6.1 implementation/release items, not a checkpoint-path validation issue.

## Impact

Likely severity: low-to-medium, depending on checkpoint trust assumptions.

Preconditions:

- a checkpoint list includes the affected block hash, or a custom
  Testnet/Regtest operator supplies such a checkpoint list;
- the checkpointed block is at a height where funding-stream outputs are
  consensus-required, or at a custom NU6.1 activation height with lockbox
  disbursements;
- the block's header hash and Merkle root match the trusted checkpointed block
  contents.

Potential effects:

- full validation would reject the block for missing or redirected funding
  stream / lockbox outputs;
- checkpoint verification can accept the same block because it does not inspect
  the actual coinbase payout structure;
- finalized state can persist schedule-derived deferred-pool accounting rather
  than accounting derived from a coinbase that satisfied the full rules.

This does not let an ordinary peer bypass default-network validation unless the
checkpoint set itself is malicious, incorrect, or supplied by an untrusted
custom-network source.

## Recommended Fix Direction

- In checkpoint verification, run the coinbase-output portions of
  `subsidy_is_valid()` that are still applicable to checkpointed blocks:
  funding-stream output presence and NU6.1 lockbox output presence.
- Alternatively, factor the funding-stream and lockbox output checks into a
  helper shared by the full semantic path and checkpoint path.
- Treat missing NU6.1 lockbox disbursements as a checkpoint verification error,
  even when the configured total amount is zero.
- Add regression coverage comparing full semantic validation and checkpoint
  validation for:
  - a Canopy+ block missing a required non-deferred funding-stream output;
  - a custom NU6.1 activation block with empty configured lockbox disbursements;
  - a custom NU6.1 activation block with redirected configured lockbox outputs.

## Confidence

Confidence is high for the source-level discrepancy. The full semantic verifier
checks actual coinbase funding-stream and lockbox outputs, while the checkpoint
verifier computes only a synthetic deferred-pool delta and does not call those
checks. The focused comparison test now confirms that a trusted checkpoint hash
for a Merkle-consistent malformed coinbase can pass the checkpoint pre-check
while full semantic subsidy validation rejects the same block.

Confidence is medium-low on practical exploitability because the affected
boundary is trusted checkpoints and custom-network checkpoint configuration, not
ordinary untrusted peer traffic on bundled networks.
