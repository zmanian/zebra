# Baseline Crosslink / Upstream Tendermint Crosswalk

This crosswalk maps the fixed-sigma/sticky baseline Crosslink spec against the
current upstream Tendermint Quint example. It is meant to keep the baseline work
honest about which parts are already modeled, which parts are intentionally
Crosslink-specific, and which parts are still incomplete.

## Upstream Snapshot

The upstream reference checked for this crosswalk was:

```text
https://github.com/informalsystems/quint/tree/main/examples/cosmos/tendermint
commit db6f80b2120a3cda86ae577c6f99e322f70d6a9d
checked 2026-05-19
```

The reference directory contains:

- `Tendermint.qnt`
- `TendermintModels.qnt`
- `TendermintTest.qnt`
- `README.md`
- the source TLA+ accountability spec under `tla/`

The README describes `Tendermint.qnt` as a Quint version of the CometBFT
accountability TLA+ specification.

## Crosslink Baseline Rule

The baseline is not a verbatim Tendermint value model. It preserves the current
Crosslink/Tenderlink behavior:

```text
BaselineStream(round) =
  ancestor(bestTip(round), height(bestTip(round)) - sigma)
```

Consensus values are fixed-sigma PoW snapshots. The sticky baseline keeps
`ResampleOnNilPrecommit = false`, so a nil-precommit quorum advances the round
without clearing same-round value or proposal-cache state.

## Surface Crosswalk

| Upstream Tendermint surface | Baseline Crosslink artifact | Status | Notes |
| --- | --- | --- | --- |
| Parameter sets `Corr`, `Faulty`, `N`, `T` | `CrosslinkBaselineTenderlink.qnt` exposes `BaselineCorr`, `BaselineFaulty`, `BaselineN`, `BaselineT`; `CrosslinkResampling.qnt` imports them as `Corr`, `Faulty`, `N`, `T` | Covered | The focused model assumes `size(Faulty) <= T` and quorum intersection explicitly. |
| Value sets `ValidValues`, `InvalidValues` | `BaselineValidSnapshots`, `BaselineInvalidSnapshots`, `Snapshots`, `NilSnapshot` | Covered with Crosslink specialization | Values are PoW snapshots, not arbitrary application values. |
| Round/proposer parameters `MaxRound`, `Proposer` | `BaselineMaxRound`, `BaselineProposer`, `Rounds`, `Proposer` | Covered | Small instances use the upstream-style proposer schedule. |
| Crosslink value selection | `BaselineSigma`, `BaselineBestTip`, `BaselineHeight`, `BaselineAncestorAt`, `BaselineHeadMinusSigma`, `BaselineStream` | Crosslink-specific addition | This is the main baseline deviation from upstream Tendermint. |
| Consensus state `round`, `step`, `decision`, `lockedValue`, `lockedRound`, `validValue`, `validRound` | Same state in `CrosslinkResampling.qnt` | Covered | Baseline also adds `cachedProposal` and `cachedProposalRound` for sticky Crosslink proposal carryover. |
| Message/evidence state for proposals, prevotes, precommits | `msgsPropose`, `msgsPrevote`, `msgsPrecommit`, `evidencePropose`, `evidencePrevote`, `evidencePrecommit` | Covered | Evidence is used by equivocation, amnesia, and Crosslink-specific accountability witnesses. |
| Faulty proposal/prevote/precommit domains | `FaultyProposals`, `FaultyPrevotes`, `FaultyPrecommits`, `AllFaulty*` | Covered structurally | The full domains exist in the shared model. |
| Nondeterministic faulty message injection in `Init` | `BaselineInitWithFaultyEvidence`, `BaselineNext`, `BaselineFaultyInitSafety`, `CrosslinkBaselineParameterizedShellTest`, `InitWithFaultyEvidence`, `InitWithSingleN4F2FaultyEvidence`, `CrosslinkBaselineFaultyInitTinyModel`, `CrosslinkBaselineFaultyInitForkingModel`, `CrosslinkBaselineBoundedFaultyInitN4F2ForkingModel`, `CrosslinkBaselineSingleFaultyInitN4F2ForkingModel`, `CrosslinkBaselineBoundedFaultyInitN5F2ForkingModel`, `CrosslinkBaselineFullFaultyInitForkingModel`, `CrosslinkBaselineFullFaultyInitN4F2ForkingModel`, `CrosslinkBaselineFullFaultyInitN5F1ForkingModel`, `CrosslinkBaselineFullFaultyInitN5F2ForkingModel`, `CrosslinkBaselineFullFaultyInitN7F2ForkingModel` | Partial | Exposed through the parameterized baseline shell and covered in a tiny full-powerset harness, bounded symbolic forking harnesses for `n4_f1` plus representative `n4_f2` and `n5_f2`, a single-faulty full-domain `n4_f2` symbolic abstraction, and full-powerset quick-check forking `n4_f1`, `n4_f2`, `n5_f1`, `n5_f2`, and `n7_f2` harnesses; not lifted into symbolic checking for every full-powerset larger instance. |
| `StartRound` | `StartRound`; `BaselineStartRound`; `parameterizedShellStartRoundAdvancesToProposeTest`; `StartNextRoundAfterPrecommitQuorum`, `TimeoutPrecommitStartNextRound`, `CatchUpToRound` | Covered with Crosslink specialization | Baseline now exposes a named upstream-shaped round-initialization helper through the shell. The round-advance transitions still wrap Crosslink-specific nil-certificate and catchup preconditions. |
| `BroadcastProposal`, `BroadcastPrevote`, `BroadcastPrecommit` | Same named broadcast actions | Covered | Message evidence is updated alongside observed messages. |
| `InsertProposal(p, v)` | `InsertProposal(p)` using `StickyOrStreamProposal(p)` | Intentional Crosslink deviation | A correct Crosslink proposer samples `Stream(round)` or reuses sticky cached/valid state; it does not choose arbitrary `v`. |
| Proposal handling in propose step | `UponProposalInPropose`; `UponProposalInProposeAndPrevote`; `UponProposalPrevote`; `BaselineUponProposalInPropose`; `BaselineUponProposalInProposeAndPrevote`; `BaselineUponProposalPrevote`; `HasPrevoteJustifiedProposal`; `CorrectValuePrevotesHaveJustifiedProposal` | Covered with Crosslink specialization | Covers the upstream-shaped split between nil-valid-round proposals and proposals justified by an earlier prevote quorum, while preserving Crosslink freshness and lock checks. |
| Upstream valid-round proposal path | `validValue`, `validRound`, `StickyOrStreamProposal`, `HasNilValidRoundProposal`, `HasConcreteValidRoundProposal`, `HasPrevoteJustifiedProposal`, `CorrectValuePrevotesHaveJustifiedProposal`, `BaselineValidValueOf`, `BaselineValidRoundOf`, `BaselineLockedValueOf`, `BaselineLockedRoundOf`, `validRoundProposalWithoutPrevoteQuorumIsRejectedTest`, `validRoundProposalWithPrevoteQuorumIsAcceptedTest`, `nilValidRoundProposalHandlerPrevotesTest`, `validRoundProposalHandlerWithoutPrevoteQuorumIsRejectedTest`, `validRoundProposalHandlerWithPrevoteQuorumIsAcceptedTest`, `correctValuePrevotesRequireJustifiedProposalTest`, `parameterizedShellConcreteValidRoundAliasAcceptsPrevoteQuorumTest` | Covered with Crosslink specialization | Baseline preserves lock/valid state and requires concrete validRound proposals to be backed by an earlier prevote quorum; the Crosslink-equivalent safety lemma is part of `Safety`, and `Next` now uses the split proposal handlers. |
| Any prevote quorum handling | `UponValuePrevoteQuorum`, `UponNilPrevoteQuorum`, `TimeoutPrevotePrecommitNil`, `BaselineUponValuePrevoteQuorum`, `BaselineUponNilPrevoteQuorum`, `BaselineTimeoutPrevotePrecommitNil` | Covered for focused baseline shell | Value prevote quorums lock and precommit; nil prevote quorums precommit nil. |
| Any precommit quorum handling | `StartNextRoundAfterPrecommitQuorum`, `Decide`, `BaselineStartNextRoundAfterPrecommitQuorum`, `BaselineDecide` | Partial | Baseline distinguishes value commit decision from nil/any-precommit round advance, but does not take arbitrary evidence-set parameters like upstream. |
| Timeout propose | `TimeoutProposePrevoteNil`, `BaselineTimeoutProposePrevoteNil` | Covered | Correct processes can prevote nil when the proposal step times out. |
| Timeout precommit | `TimeoutPrecommitStartNextRound`, `BaselineTimeoutPrecommitStartNextRound` | Covered | Correct processes can advance rounds without a precommit quorum. |
| Nil prevote quorum | `UponNilPrevoteQuorum`, `BaselineUponNilPrevoteQuorum` | Covered | This is one of the explicit baseline witnesses. |
| Round catchup | `RoundCatchupEvidence`, `CatchUpToRound`, `BaselineRoundCatchupEvidence`, `BaselineCatchUpToRound` | Covered for focused shell | Catchup requires `T + 1` observed activity in the target round. |
| System transition `Next` | `CrosslinkResampling.qnt` `Next`; `CrosslinkBaselineTenderlink.qnt` `BaselineNext`; baseline-prefixed focused transition aliases | Partial | Includes focused Crosslink proposal, vote, timeout, nil, stream-change, catchup, and decision paths behind baseline-prefixed shell aliases; still not an upstream-identical transition surface. |
| Agreement | `Agreement`, `BaselineAgreement`, symbolic `BaselineSafety`/`ComposedSafety` gates | Covered, bounded | Agreement is checked directly in the focused and composed baseline gates. |
| Validity | `Validity`, `BaselineValidity`, `Safety` | Covered, bounded | Valid decisions must be modeled snapshots. |
| Accountability | `EquivocationBy`, `AmnesiaBy`, `DetectableFaults`, `Accountability`, `ConflictingCommitsAccountable` | Partial | Baseline adapts accountability to Crosslink nil certificates; broader arbitrary-evidence checking remains incomplete. |
| False invariant examples | `CrosslinkBaselineCounterexampleModel` | Covered as seeded witnesses | Covers false no-conflicting-commit, no-amnesia, no-equivocation, agreement, agreement-or-amnesia, amnesia-implies-equivocation, amnesia-without-equivocation, and undecided max-round witnesses. |
| Small `n4_f1`, `n4_f2`, `n5_f2` model handles | `CrosslinkBaselineModels.qnt` has `n4_f1_stable`, `n4_f1_forking`, `n5_f1_forking`, `n4_f2_forking`, `n5_f2_forking`, `n7_f2_forking` | Covered with Crosslink variants | `n4_f2` and `n5_f2` are above the live BFT boundary for correct-only value commits under `T = 2`; `n7_f2` records the corresponding decision path. |
| Normal decision test | `decisionTest`, `baselineStableStreamDecidesSampledSnapshotTest`, `n7F2DecisionPathTest` | Covered | Baseline decision values are stream snapshots. |
| No double proposal test | `noProposeTwiceTest` | Covered | Checks a correct proposer cannot insert two proposals for the same round. |
| Timeout progress test | `timeoutPrevotePathFormsNilPrecommitCertTest`, `timeoutPrecommitAdvancesWithoutPrecommitQuorumTest` | Covered | The baseline splits upstream timeout behavior into prevote-nil and precommit-timeout paths. |
| Crosslink-specific stale-sample negative witness | `falseNoStaleFixedSigmaProposalInvariantFailsTest` | Covered as seeded witness | Shows the sticky baseline violates the false claim that proposals always equal the current `head - sigma` sample. |
| Crosslink-specific fork-finality negative witness | `falseForkFinalityAttemptIsValidTest` | Covered as seeded witness | Shows a fork-finality attempt is invalid once the prefix has finalized on the other branch. |

## Proof Gate Crosswalk

| Upstream-quality expectation | Current baseline gate | Status |
| --- | --- | --- |
| Typecheck the parameterized model | `check.sh quick-baseline` typechecks all baseline files | Covered |
| Run witness tests | `check.sh quick-baseline` runs baseline, accountability, BFT-height, finality, PoW-sampling, and `f = 2` witness modules | Covered |
| Bounded agreement/validity/accountability checks | `check.sh symbolic-baseline` verifies focused baseline safety invariants, and `Safety` includes `Agreement`, `Validity`, and `Accountability` | Covered, bounded |
| Faulty init symbolic checking | Tiny full-powerset faulty init plus bounded forking faulty init for `n4_f1`, representative `n4_f2`/`n5_f2`, and a single-faulty full-domain `n4_f2` abstraction; full forking faulty init remains quick-check-only for `n4_f1`, `n4_f2`, `n5_f1`, `n5_f2`, and `n7_f2` | Partial |
| `f = 2` symbolic checking | `symbolic-baseline` verifies `n4_f2`, `n5_f2`, and `n7_f2` safety invariants at depth 2 | Covered, shallow |
| Full arbitrary-evidence accountability checking | Focused witnesses, upstream-shaped negative witnesses, and bounded faulty-init gates | Partial |
| Full PoW environment checking | Fixed fork switch, long-reorg, generated adversarial work-competition, repeated generated stream-change, finite stochastic-production, and fixed-sigma sampling fixtures | Partial; bounded fixtures rather than an unbounded PoW environment |
| Stochastic or adversarial block production | `CrosslinkBaselinePowStochasticProductionModel`; generated and repeated generated work-competition fixtures | Covered, bounded |
| Inductive multi-height finality proof | Bounded BFT-height and composed-finality fixtures | Partial |

## Intentional Deviations

These differences should remain in the Crosslink baseline instead of being
"fixed" toward vanilla Tendermint:

1. Correct proposals are selected from `head - sigma`, not from an arbitrary
   valid-value set.
2. Decision freshness checks require the decided snapshot to match the sampled
   stream for the decision round.
3. The sticky baseline keeps same-round locks, valid values, and cached
   proposals across nil-precommit round advance.
4. A nil-precommit certificate for the abandoned round is valid unlock evidence
   for the proposed resampling variant, but not for the sticky baseline.
5. Crosslink finality has a separate finalized-prefix model at BFT consensus
   heights; it is not only Tendermint single-height agreement.

## Remaining Upstream-Quality Gaps

The crosswalk leaves these concrete gaps:

1. Lift the focused baseline shell into a fuller parameterized transition model
   without losing the Crosslink `head - sigma` value rule.
2. Broaden the tractable symbolic shape for faulty proposal, prevote, and
   precommit injection beyond the parameterized shell quick witness, tiny,
   bounded `n4_f1`, representative `n4_f2`/`n5_f2`, and single-faulty
   full-domain `n4_f2` harnesses. The
   fixed-sigma/forking `n4_f1`, `n4_f2`, `n5_f1`, `n5_f2`, and `n7_f2`
   surfaces now have full-powerset quick coverage. A local depth-1 Apalache
   probe of the full-powerset `n4_f2` surface exhausted the default 4GB JVM
   heap, so the next symbolic step likely needs a smaller abstraction rather
   than simply adding the full-powerset larger instances to CI.
3. Broaden arbitrary-evidence symbolic accountability coverage. The current
   focused and larger-instance `Safety` gates already include agreement,
   validity, and accountability, but the larger full-powerset faulty-evidence
   surfaces are still quick-check-only.
4. Decide whether the `n4_f2`, `n5_f2`, and `n7_f2` symbolic gates should be
   deepened beyond max depth 2.
5. Broaden Crosslink-specific false-invariant witnesses beyond hand-authored
   stale-sample and fork-finality fixtures.
6. Strengthen finalized-prefix reasoning beyond bounded fixtures, either with
   deeper symbolic projections or smaller lemmas.
