# Pre-Heartwood Sapling Root Checkpoint Coverage Note

Date: 2026-05-03

Last updated: 2026-05-09

Disposition: local-only. Do not post publicly without explicit
re-authorization and a fresh duplicate check.

Scope: pass-5 follow-up on block commitment validation before Heartwood,
especially Sapling/Blossom `hashLightClientRoot` validation and custom network
checkpoint coverage.

## Finding

Zebra's semantic/contextual validation does not compare a Sapling or Blossom
block header's `hashLightClientRoot` against the computed final Sapling note
commitment tree root. The code intentionally returns success for
`Commitment::FinalSaplingRoot(_)`, relying on checkpoints to cover all
pre-Canopy blocks on production networks.

For default Mainnet and default Testnet, this is not a live consensus divergence:
mandatory checkpoints cover the relevant pre-Canopy range, and the verifier
router sends blocks through checkpoint verification at least through the
mandatory checkpoint height.

For custom Regtest, checkpoint coverage is weaker. `Network::new_regtest()` can
build a network with a pre-Heartwood Sapling/Blossom window and only the genesis
checkpoint. With the default `consensus.checkpoint_sync = true`, the router's
maximum checkpoint height is then the checkpoint list maximum, so blocks above
genesis can be routed toward semantic validation even if they are still before
Heartwood. The canonical commit path has a state-side mandatory-checkpoint
height assertion, so this shape is more likely to become a custom-network abort
than a canonical-chain acceptance path. But proposal validation and direct
contextual helper paths can still evaluate the block and treat a structurally
valid but incorrect Sapling final root as acceptable.

## Evidence

- `zebra-state/src/service/check.rs:137-159` returns `Ok(())` for
  `Commitment::FinalSaplingRoot(_)`. The nearby comment quotes the consensus
  rule and says Zebra does not need to validate it because Zebra checkpoints on
  Canopy.
- `zebra-chain/src/block/commitment.rs:117-120` parses Sapling and Blossom
  header commitment bytes as `FinalSaplingRoot(root)` if the bytes are a
  structurally valid Sapling root.
- `zebra-chain/src/block/arbitrary.rs:487-496` sets pre-Heartwood generated
  block commitment bytes to a fixed well-formed value, not to the generated
  Sapling tree root, because this rule is expected to be checkpoint-covered.
- `zebra-state/src/service/finalized_state/tests/prop.rs:58-133` exercises fake
  activation heights, but its wrong-commitment failure cases cover Heartwood and
  NU5 boundaries, not Sapling/Blossom final Sapling root equality.
- `zebra-chain/src/parameters/network.rs:247-263` defines the mandatory
  checkpoint height as immediately before Canopy activation.
- `zebra-consensus/src/router.rs:194-215` routes blocks at or below
  `max_checkpoint_height` through checkpoint verification, and rejects proposals
  at or below that height.
- `zebra-consensus/src/router.rs:396-408` chooses `list.max_height()` when
  `checkpoint_sync` is true, and otherwise chooses the first checkpoint at or
  above the mandatory checkpoint height.
- `zebra-state/src/service.rs:877-885` asserts that
  `CommitSemanticallyVerifiedBlock` heights are above the mandatory checkpoint
  height, so canonical commit is guarded against semantic processing of
  pre-Canopy blocks.
- `zebra-state/src/service.rs:1655-1691` handles
  `CheckBlockProposalValidity` on a cloned read-state snapshot without applying
  the same mandatory-checkpoint height gate before contextual validation.
- `zebra-chain/src/parameters/network/testnet.rs:864-870` enforces configured
  Testnet checkpoint coverage through the mandatory checkpoint height.
- `zebra-chain/src/parameters/network/testnet.rs:986-1018` builds Regtest
  parameters without the same final checkpoint-coverage check.
- `zebra-chain/src/parameters/network/testnet.rs:374-398` shows Regtest default
  activation heights collapse Overwinter through Canopy to height 1 unless the
  operator configures custom values; custom values can create a longer
  Sapling/Blossom pre-Heartwood interval.

## Local Verification

Commands:

```sh
cargo test -p zebra-state all_upgrades_and_wrong_commitments_with_fake_activation_heights --lib
cargo test -p zebra-chain checkpoint_list_hard_coded_mandatory --lib
cargo test -p zebra-state proof_pre_heartwood_final_sapling_root_is_structural_only_today --lib
```

Observed result:

- rerun on 2026-05-09: the fake-upgrade finalized-state commitment test passed,
  consistent with
  pre-Heartwood commitments only needing to be well-formed in that test setup;
- rerun on 2026-05-09: both hard-coded Mainnet/Testnet mandatory checkpoint
  coverage tests passed;
- a temporary proof test added and then removed showed that, on a custom Regtest
  with Sapling at height 1 and Heartwood later, a height-1 block whose
  commitment bytes are `[1, 0, ...]` parses as `FinalSaplingRoot(_)` and passes
  `block_commitment_is_valid_for_chain_history()`.

## Duplicate Check

Read-only GitHub searches refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "pre-Heartwood" "Sapling root" "Regtest"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "FinalSaplingRoot" "checkpoint"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "hashLightClientRoot" checkpoint Regtest'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "mandatory checkpoint" "Regtest" "Canopy"'
```

Closest overlaps:

- #2092 is historical provenance: it proposed implementing
  `FinalSaplingRoot`, but explicitly notes the rule was not needed for
  production validation because Zebra checkpoints on Canopy.
- #8428, #8475, #8485, and #8629 cover Regtest/custom-network and mandatory
  checkpoint design history, including validating Regtest from NU5 / Canopy and
  lowering mandatory checkpoint height.
- #902 is an old state-updates RFC. No direct custom-Regtest
  `FinalSaplingRoot` checkpoint-coverage hardening issue was found.

## Impact

Default Mainnet and default Testnet: eliminated by mandatory checkpoint coverage.

Configured Testnet built through `ParametersBuilder::to_network()`: eliminated
by the checkpoint coverage check.

Custom Regtest with a pre-Heartwood Sapling/Blossom interval and insufficient
checkpoints: unsupported configuration gap. Canonical commit has a state assert
that prevents quiet best-chain acceptance, but the router/config combination can
still reach a process-abort shape, and proposal validation can false-accept a
pre-Heartwood block proposal with an incorrect but structurally valid Sapling
root. This is public custom-network hardening rather than a private Zcash
mainnet/testnet divergence.

## Suggested Fix

Either:

- enforce Regtest checkpoint coverage through `mandatory_checkpoint_height()` in
  the same way configured Testnet does; or
- make `init_checkpoint_list()` refuse to initialize a verifier whose checkpoint
  list does not cover the mandatory height, regardless of `checkpoint_sync`; or
- reject `CheckBlockProposalValidity` requests at heights that require
  checkpoint verification, matching `CommitSemanticallyVerifiedBlock`; or
- implement direct `FinalSaplingRoot` equality validation by passing the
  computed Sapling tree root into the block commitment check for both
  non-finalized and finalized commit paths.

The checkpoint-coverage fixes preserve the current design assumption that
pre-Canopy rules are checkpoint-covered. Direct Sapling-root equality validation
is more complete, but touches contextual validation sequencing because the
current commitment check only receives the history tree, not the updated Sapling
note commitment tree root.
