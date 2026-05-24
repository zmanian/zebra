# Checkpoint Lockbox Underflow Defaults Deferred Note

Date: 2026-05-04

Scope: audit of checkpoint verification value-pool precomputation around NU6.1
lockbox disbursements on custom networks.

## Summary

For checkpoint-verified blocks, Zebra precomputes the deferred pool balance
change from the configured funding streams and NU6.1 lockbox total. If the
configured lockbox total is larger than the deferred funding-stream amount, the
checkpoint path maps the checked-subtraction failure to `None`. Later state
value-pool code treats `None` as a zero deferred-pool change.

The full block-validation path handles the same underflow as a typed
`SubsidyError::Underflow`. This creates a custom-network/checkpoint-path
consistency gap: malformed custom lockbox economics can be silently defaulted
in checkpoint verification instead of rejected.

This is custom configuration/checkpoint hardening, not a default Mainnet or
default Testnet remote vulnerability.

## Evidence

The checkpoint verifier computes the expected deferred amount from funding
streams and subtracts the configured lockbox disbursement total:

- `zebra-consensus/src/checkpoint.rs:614-621`

The key behavior is that `checked_sub(...)` returns `None` on underflow, and
the checkpoint path immediately maps only `Some(...)` into
`DeferredPoolBalanceChange`:

- `zebra-consensus/src/checkpoint.rs:618-624`

`CheckpointVerifiedBlock::new()` stores that optional value for state commit:

- `zebra-state/src/request.rs:517-526`

When state calculates the chain value-pool change, `None` is interpreted as a
default zero deferred-pool amount:

- `zebra-chain/src/block.rs:228-243`

By contrast, the full subsidy validation path subtracts each expected lockbox
amount from the deferred-pool balance and converts underflow into a typed
consensus error:

- `zebra-consensus/src/block/check.rs:270-282`

The finalized-state upgrade/recompute path is also stricter: it expects the
deferred-minus-lockbox subtraction to be valid, and would panic if the
configured total underflowed:

- `zebra-state/src/service/finalized_state/disk_format/upgrade/block_info_and_address_received.rs:184-197`

## Impact

Likely severity: low.

Preconditions:

- custom Testnet/Regtest parameters;
- custom lockbox disbursements whose total is individually valid but larger
  than the deferred funding-stream amount at NU6.1 activation;
- checkpoint verification is used for the affected block height.

Potential effects:

- the semantic/full-validation path would reject the block economics with
  `SubsidyError::Underflow`;
- the checkpoint path can instead commit a checkpoint-verified block with the
  deferred pool change defaulted to zero;
- later value-pool accounting can differ from what the configured subsidy rules
  imply.

This requires trusting or configuring checkpoints for the affected custom
network. It does not let an untrusted peer bypass default-network validation.

## Recommended Fix Direction

- In checkpoint verification, convert lockbox underflow into
  `VerifyCheckpointError` instead of `None`.
- Treat `None` as "not applicable/not computed", not as an arithmetic failure
  fallback.
- Add custom-network regression coverage where the configured lockbox total is
  greater than the deferred amount at NU6.1 activation, and assert checkpoint
  verification fails rather than defaulting the deferred amount.

## Disclosure Triage

Public hardening. This is a custom-checkpoint/custom-configuration correctness
issue, not private-disclosure-worthy by itself.

## Confidence

Medium-high for the source-level mismatch: the checkpoint path, full-validation
path, and state defaulting behavior are direct. Medium-low for real operational
impact, because the preconditions require malformed custom economics and
checkpoint verification at the affected height.
