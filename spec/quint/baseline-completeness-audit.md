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

`baseline-upstream-crosswalk.md` records the exact upstream commit used for the
line-item comparison and maps the current baseline artifacts against each
upstream surface.

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
  - currently covers `n4_f1_stable`, `n4_f1_forking`, `n5_f1_forking`,
    `n4_f2_forking`, `n5_f2_forking`, and `n7_f2_forking`
- `CrosslinkBaselineTest.qnt`
  - adds upstream-style smoke tests for fixed-sigma sampling, normal decision,
    no double proposal, nil prevote quorum handling, timeout-driven nil
    votes and round advance, future-round catchup, fork-derived stream change,
    and sticky stale-sample carryover
  - adds a false-invariant witness for the Crosslink-specific stale
    fixed-sigma proposal after a stream switch
  - adds `f = 2` boundary witnesses distinguishing `n4_f2` and `n5_f2`
    above-live-boundary behavior from a proper `n7_f2` decision path
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
  - adds a fixed-sigma/forking `n4_f1` faulty-init harness with a bounded
    nondeterministic faulty-evidence domain
  - adds representative bounded `n4_f2`, `n5_f2`, and `n7_f2` faulty-init
    harnesses to the symbolic gate so f=2 faulty evidence is checked beyond
    the tiny/one-fault shape
  - adds single-/pair-/triple-faulty `n4_f2` symbolic harnesses that select
    one to three arbitrary faulty proposals, prevotes, and precommits from the
    full `n4_f2` faulty evidence domains
  - adds single-/pair-/triple-faulty `n5_f2` symbolic harnesses that select one
    to three arbitrary faulty proposals, prevotes, and precommits from the
    larger full `n5_f2` faulty evidence domains
  - adds single-/pair-faulty `n7_f2` symbolic harnesses that select one or two
    arbitrary faulty proposals, prevotes, and precommits from the larger full
    `n7_f2` faulty evidence domains
  - adds quick-check-only fixed-sigma/forking faulty-init harnesses for `n4_f1`,
    `n4_f2`, `n5_f1`, `n5_f2`, and `n7_f2` using the full faulty proposal,
    prevote, and precommit powerset domains
  - adds false-invariant counterexample tests for conflicting commits,
    amnesia, equivocation, agreement, agreement-or-amnesia,
    amnesia-implies-equivocation, amnesia-without-equivocation, and undecided
    max-round behavior
- `CrosslinkBaselineBftHeights.qnt`
  - gives baseline finality explicit BFT consensus heights
  - rejects skipped consensus heights and fork finality after a finalized prefix
  - adds a false-invariant witness for a fork-finality attempt after prefix
    finality
- `CrosslinkBaselineFinality.qnt`
  - composes the sticky Tenderlink baseline with finalized-prefix semantics
  - records stable finality and the stream-change finality stall
- `CrosslinkResampling.qnt`
  - shared focused Tenderlink model used by both baseline and proposed
    nil-precommit resampling variants
- `check.sh`
  - provides `quick-baseline`, `symbolic-baseline`,
    `symbolic-baseline-core`, and `symbolic-baseline-accountability` gates
  - supports `APALACHE_PORT_BASE` for sequential symbolic checker ports during
    long local runs
- `.github/workflows/quint-crosslink.yml`
  - runs baseline quick checks and parallel core/accountability symbolic checks
    on the personal fork
- `baseline-upstream-crosswalk.md`
  - maps the upstream Tendermint Quint surface to the baseline Crosslink
    artifacts and names the remaining upstream-quality gaps

## Coverage Matrix

| Requirement | Current evidence | Status |
| --- | --- | --- |
| Upstream Tendermint crosswalk | `baseline-upstream-crosswalk.md` | Covered |
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
| Automated local baseline symbolic gate | `check.sh symbolic-baseline`; `check.sh symbolic-baseline-core`; `check.sh symbolic-baseline-accountability` | Covered |
| Automated CI baseline gates | `.github/workflows/quint-crosslink.yml` | Covered structurally; requires green run evidence per commit |
| Parameterized `Corr/Faulty/N/T` validator model | `CrosslinkBaselineTenderlink.qnt`; `BaselineInitWithFaultyEvidence`; `BaselineStartRound`; `BaselineNext`; baseline-prefixed proposal, vote, timeout, nil, round-advance, catchup, and decision aliases; `BaselineFaultyInitSafety`; `CrosslinkBaselineParameterizedShellTest` | Partial; parameter shell now exposes the full faulty-init evidence surface plus the focused transition surface through baseline-prefixed aliases, while the full upstream-identical transition surface is still not ported |
| Upstream-style model instances (`n4_f1`, `n4_f2`, `n5_f2`) | `CrosslinkBaselineModels.qnt`; `n4_f1_stable`, `n4_f1_forking`, `n5_f1_forking`, `n4_f2_forking`, `n5_f2_forking`, `n7_f2_forking` | Covered as named focused instances with depth-3 symbolic coverage for the `f = 2` safety gates |
| Upstream-style normal decision/no-double-proposal/StartRound/nil-prevote/timeout/catchup/validRound tests | `CrosslinkBaselineTest.qnt`; `decisionTest`; `noProposeTwiceTest`; `parameterizedShellStartRoundAdvancesToProposeTest`; `parameterizedShellTransitionAliasesDriveDecisionPathTest`; `parameterizedShellStreamChangeAliasPrecommitsNilTest`; `parameterizedShellNilTimeoutAliasesStartNextRoundTest`; `parameterizedShellLateNilCertificateAliasStaysDisabledTest`; `parameterizedShellTimeoutPrecommitAndCatchupAliasesTest`; `parameterizedShellConcreteValidRoundAliasAcceptsPrevoteQuorumTest`; `nilPrevoteQuorumPrecommitsNilTest`; `timeoutPrevotePathFormsNilPrecommitCertTest`; `timeoutPrecommitAdvancesWithoutPrecommitQuorumTest`; `roundCatchupStartsFutureRoundTest`; `validRoundProposalWithoutPrevoteQuorumIsRejectedTest`; `validRoundProposalWithPrevoteQuorumIsAcceptedTest`; `nilValidRoundProposalHandlerPrevotesTest`; `validRoundProposalHandlerWithoutPrevoteQuorumIsRejectedTest`; `validRoundProposalHandlerWithPrevoteQuorumIsAcceptedTest`; `correctValuePrevotesRequireJustifiedProposalTest` | Covered for `n4_f1_stable` plus shell-level StartRound, transition-alias, timeout, catchup, stream-change, late nil-certificate disabled-baseline, and validRound coverage |
| Upstream-style stream-change/sticky-sample test | `CrosslinkBaselineTest.qnt`; `streamChangeDerivesFreshHeadMinusSigmaTest`; `stickyBaselineCarriesStaleFixedSigmaSampleTest` | Covered for `n4_f1_forking` |
| Fault-boundary behavior for `f = 2` | `CrosslinkBaselineTest.qnt`; `n4F2DocumentsAboveLiveFaultBoundaryTest`; `n5F2CatchupEvidenceButNoCorrectValueQuorumTest`; `n7F2DecisionPathTest`; `symbolic-baseline` depth-3 checks for `BaselineN4F2ForkingSafety`, `BaselineN5F2ForkingSafety`, and `BaselineN7F2ForkingSafety` | Covered by Rust-backed witnesses and bounded symbolic gates |
| Faulty proposal/prevote/precommit evidence reaches equivocation predicates | `CrosslinkBaselineAccountability.qnt`; `baselineFaultyProposalEvidenceFeedsEquivocationTest`; `baselineFaultyPrevoteEvidenceFeedsEquivocationTest`; `baselineFaultyNilValuePrecommitEvidenceFeedsEquivocationTest` | Covered as witnesses |
| Nondeterministic faulty message injection in `Init` | `BaselineInitWithFaultyEvidence`; `BaselineNext`; `CrosslinkBaselineParameterizedShellTest`; `InitWithFaultyEvidence`; `InitWithSingleN4F2FaultyEvidence`; `InitWithPairN4F2FaultyEvidence`; `InitWithTripleN4F2FaultyEvidence`; `InitWithSingleN5F2FaultyEvidence`; `InitWithPairN5F2FaultyEvidence`; `InitWithTripleN5F2FaultyEvidence`; `InitWithSingleN7F2FaultyEvidence`; `InitWithPairN7F2FaultyEvidence`; `CrosslinkBaselineFaultyInitTinyModel`; `CrosslinkBaselineFaultyInitForkingModel`; `CrosslinkBaselineBoundedFaultyInitN4F2ForkingModel`; `CrosslinkBaselineSingleFaultyInitN4F2ForkingModel`; `CrosslinkBaselinePairFaultyInitN4F2ForkingModel`; `CrosslinkBaselineTripleFaultyInitN4F2ForkingModel`; `CrosslinkBaselineBoundedFaultyInitN5F2ForkingModel`; `CrosslinkBaselineSingleFaultyInitN5F2ForkingModel`; `CrosslinkBaselinePairFaultyInitN5F2ForkingModel`; `CrosslinkBaselineTripleFaultyInitN5F2ForkingModel`; `CrosslinkBaselineBoundedFaultyInitN7F2ForkingModel`; `CrosslinkBaselineSingleFaultyInitN7F2ForkingModel`; `CrosslinkBaselinePairFaultyInitN7F2ForkingModel`; `CrosslinkBaselineFullFaultyInitForkingModel`; `CrosslinkBaselineFullFaultyInitN4F2ForkingModel`; `CrosslinkBaselineFullFaultyInitN5F1ForkingModel`; `CrosslinkBaselineFullFaultyInitN5F2ForkingModel`; `CrosslinkBaselineFullFaultyInitN7F2ForkingModel`; `BaselineFaultyInitSafety`; `BaselineForkingFaultyInitSafety`; `BaselineBoundedN4F2ForkingFaultyInitSafety`; `BaselineSingleN4F2ForkingFaultyInitSafety`; `BaselinePairN4F2ForkingFaultyInitSafety`; `BaselineTripleN4F2ForkingFaultyInitSafety`; `BaselineBoundedN5F2ForkingFaultyInitSafety`; `BaselineSingleN5F2ForkingFaultyInitSafety`; `BaselinePairN5F2ForkingFaultyInitSafety`; `BaselineTripleN5F2ForkingFaultyInitSafety`; `BaselineBoundedN7F2ForkingFaultyInitSafety`; `BaselineSingleN7F2ForkingFaultyInitSafety`; `BaselinePairN7F2ForkingFaultyInitSafety`; `BaselineFullForkingFaultyInitSafety`; `BaselineFullN4F2ForkingFaultyInitSafety`; `BaselineFullN5F1ForkingFaultyInitSafety`; `BaselineFullN5F2ForkingFaultyInitSafety`; `BaselineFullN7F2ForkingFaultyInitSafety` | Partial; exposed through the parameterized shell and covered by a tiny full-powerset instance, bounded symbolic fixed-sigma/forking instances for `n4_f1`, representative `n4_f2`, `n5_f2`, and `n7_f2`, single-/pair-/triple-faulty full-domain `n4_f2` abstractions, single-/pair-/triple-faulty full-domain `n5_f2` abstractions, single-/pair-faulty full-domain `n7_f2` abstractions, and quick-check full-powerset fixed-sigma/forking `n4_f1`, `n4_f2`, `n5_f1`, `n5_f2`, and `n7_f2` instances; not yet lifted into symbolic gates for the full-powerset larger instances |
| Full Tendermint transition surface | Current model covers StartRound-style round initialization, value prevote quorum, nil prevote quorum, split nil-valid-round and concrete-valid-round proposal handlers, validRound proposal justification, correct-value-prevote proposal provenance, propose/prevote/precommit timeout paths, stream-change nil precommit, late nil-precommit certificate handling as a disabled sticky-baseline alias, round advance after precommit quorum, timeout round advance, future-round catchup, and decision | Covered for the focused baseline shell; still not a full upstream port |
| Full agreement/validity/accountability invariant suite over arbitrary evidence | `Safety` includes `Agreement`, `Validity`, and `Accountability` in the focused symbolic gates; current suite also has bounded faulty-init gates plus focused accountability witnesses | Partial; larger full-powerset faulty-evidence surfaces are still quick-check-only rather than symbolic |
| False-invariant/counterexample harnesses for amnesia/equivocation/agreement | `CrosslinkBaselineCounterexampleModel`; `falseNoConflictingCommitsInvariantFailsTest`; `falseNoAmnesiaEvidenceInvariantFailsTest`; `falseNoEquivocationEvidenceInvariantFailsTest`; `falseAgreementInvariantFailsTest`; `falseAgreementOrAmnesiaInvariantFailsTest`; `falseAmnesiaImpliesEquivocationInvariantFailsTest`; `falseShowMeAmnesiaWithoutEquivocationInvariantFailsTest`; `falseNeverUndecidedInMaxRoundInvariantFailsTest`; `falseNilPrecommitClearsSameRoundLockInvariantFailsTest` | Covered as seeded Rust witnesses; not yet an arbitrary-evidence symbolic suite |
| Crosslink-specific false-invariant witnesses | `falseNoStaleFixedSigmaProposalInvariantFailsTest`; `falseForkFinalityAttemptIsValidTest`; `falseNilPrecommitClearsSameRoundLockInvariantFailsTest` | Covered as seeded Rust witnesses |
| Generated/adversarial PoW schedule for baseline long reorgs | `CrosslinkBaselinePowSampling.qnt`; `CrosslinkBaselinePowSamplingModel`; `CrosslinkBaselinePowLongReorgModel`; `CrosslinkBaselinePowGeneratedScheduleModel`; `CrosslinkBaselinePowRepeatedGeneratedScheduleModel`; `BaselinePowSamplingSafety`; `BaselinePowLongReorgSafety`; `BaselinePowGeneratedScheduleSafety`; `BaselinePowRepeatedGeneratedScheduleSafety` | Covered, bounded; fork-switch, long-reorg, generated adversarial work-competition, and repeated generated stream-change fixtures are covered |
| Stochastic PoW block-production model | `CrosslinkBaselinePowStochasticProductionModel`; `BaselinePowStochasticProductionSafety` | Covered, bounded; finite hash-participation, hidden-work-risk, and block-variance buckets derive honest extension and hidden-work release windows |
| Inductive or deeper multi-height finality argument | Current BFT-height model is bounded | Partial |

## Remaining Work

To finish a focused baseline artifact, the remaining work is:

1. Keep `quick-baseline`, `symbolic-baseline-core`, and
   `symbolic-baseline-accountability` green locally and in CI for the current
   commit.
2. Keep the baseline limitation explicit: stream changes between prevote and
   precommit can leave the sticky baseline carrying a stale fixed-sigma sample
   and can halt fresh finality.
3. Keep the upstream-shaped shell documented as a shell, not as a complete port
   of the upstream Tendermint transition system.

To finish an upstream-quality baseline spec, the remaining work is larger:

Use `baseline-upstream-crosswalk.md` as the controlling checklist for the
remaining upstream-quality gaps.

1. Extend the parameterized baseline shell into a full transition model.
   - Keep the current upstream-shaped constants and assumptions:
     `Corr`, `Faulty`, `N`, `T`, value sets, `MaxRound`, `Proposer`, `sigma`,
     best tips, heights, and ancestors.
   - Continue to keep Crosslink value selection as `Stream(round) =
     ancestor(bestTip(round), height(bestTip(round)) - sigma)` rather than an
     arbitrary Tendermint value.
2. Add the remaining baseline model instances.
   - The branch now includes focused `n4_f2`, `n5_f2`, and `n7_f2` instances
     without violating the focused shell's `size(Faulty) <= T` assumption
     silently.
   - The `n4_f2` and `n5_f2` witnesses document why those smaller `f = 2`
     layouts are above the live fault boundary for correct-only value commits;
     `n7_f2` records the corresponding 2f+1 correct decision path.
   - The `f = 2` safety invariants are now `symbolic-baseline` gates at max
     depth 3; deeper bounds remain a tractability question.
3. Add upstream-style faulty message injection.
   - The parameterized shell now exposes `BaselineInitWithFaultyEvidence`,
     `BaselineNext`, `BaselineFaultyInitDomainWellFormed`, and
     `BaselineFaultyInitSafety` for nondeterministically seeded faulty
     proposals, prevotes, and precommits in `Init`.
   - Ensure proposal, prevote, and precommit evidence is carried into
     Crosslink accountability predicates.
   - The full faulty-init powerset is now exercised for the fixed-sigma/forking
     `n4_f1`, `n4_f2`, `n5_f1`, `n5_f2`, and `n7_f2` instances in
     `quick-baseline`.
   - The single-/pair-/triple-faulty `n4_f2` harnesses symbolically range over one
     to three arbitrary faulty proposals, prevotes, and precommits from the full
     `n4_f2` domain at max depth 2, single-/pair-/triple-faulty `n5_f2`
     harnesses range over the larger full `n5_f2` domain at max depth 2, and
     single-/pair-faulty `n7_f2` harnesses range over the proper f=2 boundary's
     full faulty-evidence domain at max depth 2;
     remaining work is broader symbolic coverage that stays tractable. A local
     Apalache probe of
     `CrosslinkBaselineFullFaultyInitN4F2ForkingModel` at max depth 1 exhausted
     the default 4GB JVM heap, so the current quick-only boundary for larger
     full-powerset instances is a real tractability limit rather than an
     omitted proof gate.
4. Port the full transition surface.
   - Include proposer selection, proposal insertion, proposal handling,
     prevote quorum handling, precommit quorum handling, timeouts, nil prevotes,
     and round catchup. The current baseline model now covers StartRound-style
     round initialization plus the timeout and catchup paths for the focused
     shell.
   - Preserve the baseline sticky rule: nil precommit does not clear same-round
     valid or locked state.
5. Strengthen properties.
   - The focused and larger-instance `Safety` gates already include agreement,
     validity, and accountability; remaining work is to broaden the
     arbitrary-evidence/accountability shape that can be checked symbolically.
   - Keep Crosslink finalized-prefix safety separate from Tenderlink agreement
     so failures are easier to diagnose.
6. Add deeper Crosslink-specific false-invariant/counterexample modules.
   - The upstream-shaped negative checks for amnesia, equivocation, agreement,
     agreement-or-amnesia, amnesia-implies-equivocation,
     amnesia-without-equivocation, and undecided max-round behavior now have
     seeded witnesses.
   - Seeded witnesses now also cover the false claims that sticky baseline
     proposals always match the current fixed-sigma sample and that
     fork-finality attempts remain valid after prefix finality.
   - Remaining work is to broaden those Crosslink-specific false invariants
     beyond hand-authored fixtures.
7. Expand the PoW environment.
   - The baseline now has a generated bounded PoW schedule where published work
     selects `a3`, then `a4`, then an adversarially released `b4`.
   - The baseline now has a repeated generated bounded PoW schedule where
     published work selects `a3`, then `a4`, then adversarially released `b4`,
     then adversarially released `c4`.
   - A long-reorg fixture now covers rollback depth 3 with sigma 2, where the
     sticky baseline carries the stale `head - sigma` sample across the fork
     switch.
   - A finite stochastic-production fixture now buckets hash-power
     participation, hidden-work risk, and block-time variance, then derives
     honest extension and hidden-work release windows from those buckets.
8. Push finality beyond a bounded fixture.
   - Either add deeper symbolic projection checks or split the model into
     smaller lemmas that make the multi-height finalized-prefix argument more
     obviously inductive.

## Recommended Order

1. Broaden upstream-style faulty message injection beyond the current shell
   aliases and focused harnesses.
   - Existing focused witnesses prove that manually seeded faulty proposal,
     prevote, and precommit evidence reaches equivocation predicates.
   - A tiny proof-gated `InitWithFaultyEvidence` harness now covers
     nondeterministic faulty proposal, prevote, and precommit powersets.
   - A fixed-sigma/forking `n4_f1` harness now covers the same init shape with
     a bounded faulty-evidence domain.
   - Representative bounded `n4_f2`, `n5_f2`, and `n7_f2` harnesses now carry
     the same idea into f=2 symbolic checks without expanding the full powerset.
   - Single-/pair-/triple-faulty `n4_f2` harnesses now symbolically range over the
     full faulty-evidence domain while bounding the selected evidence set size.
   - Single-/pair-/triple-faulty `n5_f2` harnesses now range over the larger
     full faulty-evidence domain at max depth 2.
   - Single-/pair-faulty `n7_f2` harnesses now range over the proper f=2
     boundary's full faulty-evidence domain by selecting one or two arbitrary
     faulty proposals, prevotes, and precommits at max depth 2.
   - The parameterized shell now exposes the full faulty-init surface and
     transition step through baseline-prefixed aliases.
   - Quick-check-only fixed-sigma/forking `n4_f1`, `n4_f2`, `n5_f1`, `n5_f2`,
     and `n7_f2` harnesses now cover the full faulty-evidence domain. The
     missing piece is finding tractable symbolic abstractions beyond the
     representative bounded and selected-evidence domains.
2. Continue porting the full Tendermint transition surface into the
   parameterized shell while preserving the baseline sticky nil-precommit rule.
   The shell now exposes baseline-prefixed aliases for the focused transition
   surface, but it is still not an upstream-identical transition system.
3. Broaden the full-powerset faulty-evidence shapes that can be checked
   symbolically, without dropping agreement, validity, or accountability from
   the checked invariant.
4. Keep watching CI runtime as the symbolic frontier expands. The workflow now
   splits baseline symbolic checks into core and accountability matrix jobs, so
   additional work should preserve that parallel structure or add further
   slices before the 20-minute timeout becomes tight again.

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
