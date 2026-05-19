# Baseline Crosslink Quint Completeness Audit

This audit tracks what remains before the baseline fixed-sigma/sticky
Crosslink Quint spec reaches the same completeness target as the upstream
Tendermint Quint model.

## Objective

The baseline deliverable is not only a collection of witnesses. It should be a
reviewable Crosslink baseline specification with:

- a named fixed-sigma/sticky Tenderlink model
- explicit `head - sigma` PoW sampling
- Crosslink finalized-prefix semantics at explicit BFT consensus heights
- Tendermint-style safety, validity, agreement, and accountability properties
- upstream-style small model instances
- automated quick and symbolic proof gates
- documented limitations for fork recovery, PoW reorgs, and nil-precommit
  behavior

## Upstream Tendermint Reference

The current upstream reference is:

```text
https://github.com/informalsystems/quint/tree/main/examples/cosmos/tendermint
```

That directory contains a Quint port of the CometBFT accountability TLA+ spec.
The relevant upstream surface is:

- `Tendermint.qnt`
  - parameterized process sets: `Corr`, `Faulty`, `N`, `T`
  - value sets: `ValidValues`, `InvalidValues`
  - round/proposer parameters: `MaxRound`, `Proposer`
  - quorum constants: `THRESHOLD1 = T + 1`, `THRESHOLD2 = 2 * T + 1`
  - consensus state: `round`, `step`, `decision`, `lockedValue`,
    `lockedRound`, `validValue`, `validRound`
  - message/evidence state for proposals, prevotes, and precommits
  - nondeterministic faulty proposal, prevote, and precommit injection in `Init`
  - full transition surface:
    `StartRound`, `InsertProposal`, proposal handlers, prevote quorum handlers,
    precommit quorum handlers, timeout handlers, and round catchup
  - accountability predicates for equivocation and amnesia
  - properties for agreement, validity, accountability, and false-invariant
    counterexample exploration
- `TendermintModels.qnt`
  - small model instances such as `n4_f1`, `n4_f2`, and `n5_f2`
- `TendermintTest.qnt`
  - witness tests for normal decision, no double proposal, and timeout progress

## Current Baseline Artifacts

The current branch has these baseline-specific files:

- `CrosslinkBaseline.qnt`
  - packages the fixed-sigma/sticky variant with
    `ResampleOnNilPrecommit = false`
  - includes stable-stream and stream-change witnesses
- `CrosslinkBaselineTenderlink.qnt`
  - adds an upstream-shaped parameter shell for `Corr`, `Faulty`, `N`, `T`,
    value sets, rounds, proposers, sigma, best tips, heights, and ancestors
  - derives `BaselineStream(round)` from the Crosslink fixed-sigma
    `head - sigma` rule while keeping sticky nil-precommit semantics
- `CrosslinkBaselineModels.qnt`
  - defines small stable and forking baseline model instances over the
    parameter shell
  - currently covers `n4_f1_stable`, `n4_f1_forking`, and `n5_f1_forking`
- `CrosslinkBaselineTest.qnt`
  - adds upstream-style smoke tests for fixed-sigma sampling, normal decision,
    no double proposal, fork-derived stream change, and sticky stale-sample
    carryover
- `CrosslinkBaselinePowSampling.qnt`
  - derives `Stream(round)` from an explicit `head - sigma` ancestor
  - records the fork-switch stale-sample behavior
- `CrosslinkBaselineAccountability.qnt`
  - records that baseline nil precommit preserves same-round value locks
  - records conflicting commit accountability through amnesia evidence
  - records that faulty proposal, prevote, and nil/value precommit evidence
    reaches the equivocation predicates
  - adds a tiny `InitWithFaultyEvidence` harness with nondeterministically
    injected faulty proposal, prevote, and precommit powersets
- `CrosslinkBaselineBftHeights.qnt`
  - gives baseline finality explicit BFT consensus heights
  - rejects skipped consensus heights and fork finality after a finalized prefix
- `CrosslinkBaselineFinality.qnt`
  - composes the sticky Tenderlink baseline with finalized-prefix semantics
  - records stable finality and the stream-change finality stall
- `CrosslinkResampling.qnt`
  - shared focused Tenderlink model used by both baseline and proposed
    nil-precommit resampling variants
- `check.sh`
  - provides `quick-baseline` and `symbolic-baseline` gates
- `.github/workflows/quint-crosslink.yml`
  - runs baseline quick and symbolic checks on the personal fork

## Coverage Matrix

| Requirement | Current evidence | Status |
| --- | --- | --- |
| Named fixed-sigma/sticky baseline variant | `CrosslinkBaseline.qnt`; `ResampleOnNilPrecommit = false` | Covered |
| Stable-stream decision path | `baselineStableStreamDecidesSampledSnapshotTest` | Covered |
| Stream-change halt/stale-sample limitation | `baselineCarriesStaleSampleAfterStreamChangeTest`; `baselineSameRoundLockBlocksFreshDecisionAfterStreamChangeTest` | Covered |
| Explicit fixed `head - sigma` sampling | `CrosslinkBaselinePowSampling.qnt` | Covered |
| Fork switch rolls back sampled ancestor | `forkSwitchRollsBackFixedSigmaSampleTest` | Covered |
| Sticky baseline carries rolled-back sample | `stickyBaselineCarriesRolledBackHeadMinusSigmaTest` | Covered |
| Crosslink finalized-prefix safety | `CrosslinkBaselineFinality.qnt`; `ComposedSafety` | Covered, bounded |
| Explicit BFT consensus-height progression | `CrosslinkBaselineBftHeights.qnt`; `BaselineBftHeightSafety` | Covered, bounded |
| Reject skipped BFT heights | `baselineRejectsSkippedBftHeightTest` | Covered |
| Reject fork finality after prefix finality | `baselineRejectsForkAfterPrefixFinalityTest` | Covered |
| Baseline nil precommit preserves same-round value locks | `baselineNilPrecommitDoesNotClearSameRoundValueLockTest` | Covered |
| Conflicting commits expose accountability evidence | `baselineConflictingCommitsWithoutUnlockExposeAmnesiaTest` | Covered as a witness |
| Automated local baseline quick gate | `check.sh quick-baseline` | Covered |
| Automated local baseline symbolic gate | `check.sh symbolic-baseline` | Covered |
| Automated CI baseline gates | `.github/workflows/quint-crosslink.yml` | Covered structurally; requires green run evidence per commit |
| Parameterized `Corr/Faulty/N/T` validator model | `CrosslinkBaselineTenderlink.qnt` | Partial; parameter shell exists, full transition/faulty injection is still focused |
| Upstream-style model instances (`n4_f1`, `n4_f2`, `n5_f2`) | `CrosslinkBaselineModels.qnt`; `n4_f1_stable`, `n4_f1_forking`, `n5_f1_forking` | Partial; above-threshold faulty instances are still missing |
| Upstream-style normal decision/no-double-proposal tests | `CrosslinkBaselineTest.qnt`; `decisionTest`; `noProposeTwiceTest` | Covered for `n4_f1_stable` |
| Upstream-style stream-change/sticky-sample test | `CrosslinkBaselineTest.qnt`; `streamChangeDerivesFreshHeadMinusSigmaTest`; `stickyBaselineCarriesStaleFixedSigmaSampleTest` | Covered for `n4_f1_forking` |
| Faulty proposal/prevote/precommit evidence reaches equivocation predicates | `CrosslinkBaselineAccountability.qnt`; `baselineFaultyProposalEvidenceFeedsEquivocationTest`; `baselineFaultyPrevoteEvidenceFeedsEquivocationTest`; `baselineFaultyNilValuePrecommitEvidenceFeedsEquivocationTest` | Covered as witnesses |
| Nondeterministic faulty message injection in `Init` | `InitWithFaultyEvidence`; `CrosslinkBaselineFaultyInitTinyModel`; `BaselineFaultyInitSafety` | Partial; covered in a tiny proof-gated instance, not yet lifted into the larger parameterized baseline instances |
| Full Tendermint transition surface | Current model isolates the Crosslink fork-recovery question | Missing |
| Full agreement/validity/accountability invariant suite over arbitrary evidence | Current suite has bounded safety plus focused accountability witnesses | Partial |
| False-invariant/counterexample harnesses for amnesia/equivocation/agreement | No baseline equivalent yet | Missing |
| Generated/adversarial PoW schedule for baseline long reorgs | Baseline uses a bounded fork-switch fixture | Partial |
| Stochastic PoW block-production model | Not modeled in baseline | Missing |
| Inductive or deeper multi-height finality argument | Current BFT-height model is bounded | Partial |

## Remaining Work

To finish a focused baseline artifact, the remaining work is:

1. Keep `quick-baseline` and `symbolic-baseline` green locally and in CI for the
   current commit.
2. Keep the baseline limitation explicit: stream changes between prevote and
   precommit can leave the sticky baseline carrying a stale fixed-sigma sample
   and can halt fresh finality.
3. Keep the upstream-shaped shell documented as a shell, not as a complete port
   of the upstream Tendermint transition system.

To finish an upstream-quality baseline spec, the remaining work is larger:

1. Extend the parameterized baseline shell into a full transition model.
   - Keep the current upstream-shaped constants and assumptions:
     `Corr`, `Faulty`, `N`, `T`, value sets, `MaxRound`, `Proposer`, `sigma`,
     best tips, heights, and ancestors.
   - Continue to keep Crosslink value selection as `Stream(round) =
     ancestor(bestTip(round), height(bestTip(round)) - sigma)` rather than an
     arbitrary Tendermint value.
2. Add the remaining baseline model instances.
   - Add above-threshold or accountability-focused analogues for upstream
     `n4_f2` and `n5_f2` without violating the focused shell's current
     `size(Faulty) <= T` assumption silently.
   - Keep at least one model where the PoW stream changes across a round
     boundary.
3. Add upstream-style faulty message injection.
   - Nondeterministically seed faulty proposals, prevotes, and precommits in
     `Init`.
   - Ensure proposal, prevote, and precommit evidence is carried into
     Crosslink accountability predicates.
4. Port the full transition surface.
   - Include proposer selection, proposal insertion, proposal handling,
     prevote quorum handling, precommit quorum handling, timeouts, nil prevotes,
     and round catchup.
   - Preserve the baseline sticky rule: nil precommit does not clear same-round
     valid or locked state.
5. Strengthen properties.
   - Check agreement, validity, and accountability over the parameterized model.
   - Keep Crosslink finalized-prefix safety separate from Tenderlink agreement
     so failures are easier to diagnose.
6. Add false-invariant/counterexample modules.
   - Port the upstream negative checks for amnesia, equivocation, agreement, and
     undecided max-round behavior.
   - Add Crosslink-specific negative checks for stale fixed-sigma samples and
     fork finality attempts.
7. Expand the PoW environment.
   - Replace the baseline single fork-switch fixture with generated bounded PoW
     schedules.
   - Add long-reorg fixtures where the rollback depth exceeds the configured
     sigma.
   - Add a simple stochastic or adversarial block-production abstraction so
     repeated stream changes are not only hand-authored witnesses.
8. Push finality beyond a bounded fixture.
   - Either add deeper symbolic projection checks or split the model into
     smaller lemmas that make the multi-height finalized-prefix argument more
     obviously inductive.

## Recommended Order

1. Add upstream-style faulty message injection to the parameterized shell.
   - Existing focused witnesses prove that manually seeded faulty proposal,
     prevote, and precommit evidence reaches equivocation predicates.
   - A tiny proof-gated `InitWithFaultyEvidence` harness now covers
     nondeterministic faulty proposal, prevote, and precommit powersets.
   - The missing piece is lifting that init path into the larger parameterized
     baseline instances without making the symbolic gate unusably large.
2. Port the full Tendermint transition surface into the parameterized shell
   while preserving the baseline sticky nil-precommit rule.
3. Add remaining model instances, including above-threshold or
   accountability-focused analogues of upstream `n4_f2` and `n5_f2`.
4. Add the full agreement/validity/accountability checks to
   `symbolic-baseline`.
5. Add the false-invariant/counterexample harnesses.
6. Generalize baseline PoW schedules for long reorgs and repeated stream
   changes.
7. Revisit CI timeout and split symbolic jobs if the parameterized model makes
   Apalache too heavy.

## Completion Standard

The baseline should be treated as complete only when:

- the focused baseline witnesses still pass
- the upstream-shaped parameterized baseline model typechecks
- every baseline model instance has quick witness coverage
- bounded symbolic checks pass for agreement, validity, accountability,
  fixed-sigma sampling, and finalized-prefix safety
- the counterexample harnesses produce the expected failures
- CI runs the quick and symbolic baseline gates on the personal fork
- the issue and gist link to the current spec folder and this audit
