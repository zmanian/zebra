# Crosslink Quint Specs

This directory contains focused Quint models for Crosslink/Tenderlink round
recovery and Crosslink finality value semantics.

`CrosslinkResampling.qnt` is not a full Tenderlink implementation spec yet. It
isolates the Ebb-and-Flow-specific question that came up during fork recovery
design:

- Crosslink proposals sample a moving PoW stream, modelled as `Stream(round)`.
- A `2f + 1` `PRECOMMIT nil` quorum is modelled as a round-abandon certificate.
- The sticky model keeps a same-round Crosslink proposal cache across the round
  increment and preserves same-round Tendermint lock/valid state.
- The resampling model treats the nil precommit quorum as an unlock certificate
  for the abandoned round: it clears the same-round proposal cache,
  `validValue`, and `lockedValue`, so the next proposer can sample the current
  stream.

The model preserves older Tendermint locks. A nil precommit certificate only
clears state whose round is exactly the abandoned round; it does not erase
earlier safety-carrying locks. It also keeps the Tendermint quorum-intersection
argument explicit: retained locks must be backed by value precommit evidence,
and a nil certificate can coexist with at most `f` correct same-round value
locks, not with a commit-capable value-lock quorum. The model also ports the
upstream Tendermint accountability shape over the Crosslink evidence surface:
transition-carried proposal, prevote, and precommit evidence feed equivocation
and amnesia predicates, while a nil-precommit certificate for the abandoned
round is treated as valid unlock evidence rather than amnesia.

`CrosslinkForkFinality.qnt` is a separate value-semantics model. It abstracts
PoW snapshots as a finite fork tree, then checks that Crosslink finality can skip
heights on one branch while rejecting finalization of a fork after a block is
final.

`CrosslinkPowForkSchedule.qnt` is the first step from fixed PoW fixtures toward
generated reorg schedules. It derives rollback depth from a bounded sequence of
best-tip changes, then checks whether a selected sigma is deep enough to survive
that fork switch.

`CrosslinkPowBranchCompetition.qnt` replaces the hand-declared best-tip switch
with a bounded branch-competition fixture. Published tips compete by honest plus
adversarial work, a hidden adversarial branch cannot become best until it is
published, and releasing an outworking adversarial branch derives the same
rollback-depth signal used by dynamic sigma.

`CrosslinkComposed.qnt` connects those two pieces: a Tenderlink decision over a
resampled PoW snapshot becomes the input to Crosslink finality, which can then
advance to a tail-confirmed snapshot while preserving the finalized prefix.

`CrosslinkBftHeights.qnt` adds the missing BFT-height dimension for finality.
It checks that successive Tenderlink decisions at consecutive consensus heights
can update Crosslink finality directly, while rejecting skipped consensus
heights and fork decisions after a prefix is final.

`CrosslinkDynamicSigma.qnt` sketches the third Crosslink variant: a
dynamic-sigma controller. It treats the percentage of total PoW hash power that
is participating in Crosslink as an explicit controller input. Low hash-power
participation raises the minimum sigma floor because the finalizers' observed
PoW stream is less representative of the global longest-chain race. Round
failures can still raise sigma, but they are not the only signal; below a
critical hash-participation threshold, the model forces the maximum sigma and
marks the controller state as degraded. The bounded fixture also makes the
`head - sigma` sample explicit: base sigma sees adjacent tip churn, while a
raised sigma samples a deeper ancestor that remains stable across the same
round boundary. The controller also accepts an observed reorg-depth schedule:
if an adversarial or accidental reorg reaches the current depth, sigma moves to
the next configured floor even when hash participation is healthy and the
previous BFT round did not fail. It also includes a bounded stochastic-risk
score over hash-power coverage, recent round-failure rate, block-interval
variance, and observed rollback depth; the score can raise sigma when combined
signals become risky even if no single hard floor fires.

`CrosslinkDynamicSigmaCalibration.qnt` gives the dynamic-sigma controller a
bounded calibration contract. It treats hash-power participation, round-failure
rate, block-interval variance, and observed reorg depth as measured windows,
then checks that the selected risk weights and thresholds classify those
windows into the expected sigma floors.

`CrosslinkDynamicSigmaTelemetry.qnt` makes that calibration contract more
production-shaped. It derives participation from Crosslink-participating PoW
work over total observed PoW work, requires conservative coverage and
round-failure estimates, and adds an explicit acceptable rollback-risk target
plus expected-loss budget that the selected sigma must satisfy whenever the
configured ladder can satisfy both.

`dynamic-sigma-telemetry-integration.md` maps those telemetry inputs to
production data sources and documents the consensus-safety requirements before
a deployed controller can replace the prototype's fixed sigma parameter.
`zebra-crosslink/src/dynamic_sigma.rs` is the matching pure Rust controller
prototype: it derives conservative coverage and round-failure estimates from
raw counters, validates telemetry windows, and selects the same sigma floor as
the Quint telemetry fixture. It also includes a proposal-carried evidence
verifier that rejects selected sigma values below the controller-required floor.
`DynamicSigmaTelemetryComponents` and `DynamicSigmaRoundCounters` are the first
production-shaped assembly boundary: they require explicit total and
Crosslink-participating hash work, reject inconsistent round counters, and only
then build raw controller telemetry. `DynamicSigmaRoundEvent` accumulates
started, decided, nil-precommit, stale-proposal, timeout, invalid-proposal, and
mixed-evidence labels into those counters, while rejecting failure-reason
overcounts. `observed_hash_work_participation` aggregates source-side PoW work
observations into the total-work denominator and verified-participating
numerator, so work without objective Crosslink participation evidence is counted
conservatively as non-participating. `DynamicSigmaBestTipTransition` derives the
observed reorg-depth input from explicit old-tip, new-tip, and common-ancestor
heights, with a helper for taking the maximum rollback depth over a transition
window. `telemetry_components_from_observation_window` composes these
source-shaped hash-work observations, round counters, and best-tip transitions
into telemetry components before proposal evidence selection. The branch also
includes a
`BftBlock::try_from_with_confirmation_depth` construction hook and tagged payload
envelope. The live Tenderlink proposal, validation, and decided-block
callbacks now route through a config-aware payload path: default config still
emits and accepts the legacy fixed-sigma `BftBlock`, while
`dynamic_sigma_prototype` emits and validates the tagged dynamic-sigma envelope
using shared prototype parameters and proposal-carried evidence. The decoded
payload carries its selected confirmation depth into voting-time stale checks,
so a prototype dynamic proposal is checked against `head - selected_sigma`. The
prototype proposer now runs the dynamic-sigma controller over its fixture
telemetry components before selecting that sigma; production telemetry sources
are still the missing deployment step. Invalid telemetry assembly or
telemetry-to-evidence selection prevents prototype proposal emission instead of
falling back to the base sigma.

`CrosslinkDynamicSigmaForkSchedule.qnt` composes the dynamic-sigma controller
with the derived PoW fork schedule. In this model, dynamic sigma consumes
rollback depth computed from best-tip transitions instead of a supplied
`ObservedReorgDepth` map.

`CrosslinkDynamicSigmaBranchCompetition.qnt` feeds the generated PoW
branch-competition model into dynamic sigma. The controller now consumes a
rollback-depth signal produced by published-tip work competition, including the
adversarial branch release witness.

`CrosslinkDynamicSigmaResampling.qnt` composes the derived fork signal with the
nil-precommit resampling path. It checks that a fork switch can raise sigma
before validators advance the abandoned Tenderlink round, and that the
resampling path can still decide the fresh stream value. It also carries the
hash-power participation floor into the composed model, so low participation by
Crosslink-aware miners can raise sigma even when the latest best-tip transition
does not add a new fork-switch signal. Its best-tip fixture is backed by
generated published-tip work competition.

`CrosslinkDynamicSigmaFinality.qnt` composes dynamic sigma, nil-precommit
resampling, and Crosslink finality. It uses the live `dynSigma` value as the
tail-confirmation depth for finality, so fork-derived and hash-participation
sigma increases both delay finalization until the fresh decision is confirmed
deeply enough. Its fork signal is now backed by generated published-tip work
competition, and finality advances at explicit BFT consensus heights.

## Upstream Base

The best current Tendermint Quint base is the Quint repository's Cosmos example:

```text
https://github.com/informalsystems/quint/tree/main/examples/cosmos/tendermint
```

That example is a Quint port of the CometBFT accountability spec. The older
`informalsystems/tendermint-spec` repository is useful background, but its CSMI
library does not typecheck cleanly under Quint `0.31.0`.

## Toolchain

Quint's command-line package is still a JavaScript CLI, but the simulator uses
the Rust backend with `--backend=rust`. On this machine, Node `26.0.0` exposes a
`yargs` packaging issue in the globally installed Quint CLI, so the working local
command is:

```sh
QUINT="node /private/tmp/quint-global-patched/dist/src/cli.js"
```

If the global CLI works in your shell, use `quint` instead.

Apalache verification also needs Java on the path. The working local environment
is:

```sh
export HOME=/private/tmp/quint-home2
export JAVA_HOME=/opt/homebrew/opt/openjdk/libexec/openjdk.jdk/Contents/Home
export PATH=/opt/homebrew/opt/openjdk/bin:$PATH
```

Apalache starts a local checker server on port `8822` during `quint verify`.

## Checks

Run the quick local sweep:

```sh
QUINT="$QUINT" spec/quint/check.sh quick
```

Run the bounded Apalache sweep:

```sh
QUINT="$QUINT" JVM_ARGS=-Xmx8192m spec/quint/check.sh symbolic
```

Typecheck:

```sh
$QUINT typecheck spec/quint/CrosslinkResampling.qnt
$QUINT typecheck spec/quint/CrosslinkForkFinality.qnt
$QUINT typecheck spec/quint/CrosslinkPowForkSchedule.qnt
$QUINT typecheck spec/quint/CrosslinkPowBranchCompetition.qnt
$QUINT typecheck spec/quint/CrosslinkComposed.qnt
$QUINT typecheck spec/quint/CrosslinkBftHeights.qnt
$QUINT typecheck spec/quint/CrosslinkDynamicSigma.qnt
$QUINT typecheck spec/quint/CrosslinkDynamicSigmaCalibration.qnt
$QUINT typecheck spec/quint/CrosslinkDynamicSigmaTelemetry.qnt
$QUINT typecheck spec/quint/CrosslinkDynamicSigmaForkSchedule.qnt
$QUINT typecheck spec/quint/CrosslinkDynamicSigmaBranchCompetition.qnt
$QUINT typecheck spec/quint/CrosslinkDynamicSigmaResampling.qnt
$QUINT typecheck spec/quint/CrosslinkDynamicSigmaFinality.qnt
```

Witness the current sticky behavior:

```sh
$QUINT test spec/quint/CrosslinkResampling.qnt \
  --main=CrosslinkStickyModel \
  --max-samples=100 \
  --backend=rust
```

This runs:

- `abandonedRoundProposalTest`
- `currentProtocolCarriesStaleSampleTest`
- `currentProtocolSameRoundLockBlocksFreshDecisionTest`

The second test shows that after a round-0 nil-precommit certificate, the round-1
proposer still proposes `s0`.

The third test shows the liveness failure mode: if the same-round Tendermint
lock is preserved after the stream has changed, the sticky model cannot continue
to a fresh `s1` decision in round 1.

Witness the proposed resampling behavior:

```sh
$QUINT test spec/quint/CrosslinkResampling.qnt \
  --main=CrosslinkNilResamplingModel \
  --max-samples=100 \
  --backend=rust
```

This runs:

- `abandonedRoundProposalTest`
- `nilPrecommitResamplesFreshStreamTest`
- `nilPrecommitPreservesOlderTendermintValueLockTest`
- `nilPrecommitUnlocksSameRoundTendermintStateTest`
- `lateNilPrecommitCertificateUnlocksAbandonedRoundTest`
- `nilPrecommitUnlockResamplesAndDecidesFreshValueTest`
- `conflictingCommitsExposeInvalidUnlockEvidenceTest`
- `conflictWithBogusNilUnlockExposesEquivocationEvidenceTest`
- `nilPrecommitCertificateJustifiesSameRoundSwitchTest`
- `laterNilCertificateDoesNotUnlockOlderValueLockTest`

The second test shows that after the same round-0 nil-precommit certificate, the
round-1 proposer proposes `s1`.

The third and fourth tests are the guardrails for the Tendermint lock rule: a
nil precommit certificate preserves older `validValue`/`lockedValue` state, but
does clear same-round `validValue`/`lockedValue` state. The same-round unlock
witness is quorum-faithful: the nil certificate is formed by `2f + 1`
precommits, while only a minority correct validator keeps a same-round value
lock before recovery. The fifth test is the bounded liveness witness: after a
same-round nil certificate and stream change, the resampling model reaches a
fresh `s1` decision.

The late-certificate test covers a real implementation race: a validator may
timeout into the next round before it receives the `2f + 1` nil-precommit
certificate for the abandoned round. The certificate still clears lock and valid
state whose round equals the certificate round, but it does not rewind or
re-propose the current round.

The final four tests are the accountability witnesses. They check that:

- two conflicting value commits across rounds expose Tendermint-style amnesia
  evidence when there is no nil-precommit unlock certificate for the older lock
- a bogus nil certificate that coexists with a same-round value commit exposes
  nil/value equivocation evidence
- a valid same-round nil certificate justifies switching away from a minority
  same-round value lock without falsely reporting amnesia
- a later nil certificate does not justify abandoning an older value lock and
  still leaves amnesia evidence for the invalid switch

One limitation is intentional: a mixed precommit set with some value precommits
and some nil precommits is not treated as unlock evidence unless nil itself has
quorum. A mixed set does not rule out a hidden value-commit quorum under
Byzantine equivocation, so unlocking on it would be a safety change rather than
the nil-certificate liveness improvement modelled here.

Witness Crosslink finality value semantics:

```sh
$QUINT test spec/quint/CrosslinkForkFinality.qnt \
  --main=CrosslinkForkFinalityModel \
  --max-samples=100 \
  --backend=rust
```

This runs:

- `canSkipHeightsOnSamePowBranchTest`
- `extendsFinalizedPrefixTest`
- `rejectsFinalizingForkAfterFinalBlockTest`
- `rejectsUnconfirmedTailTest`

Witness derived PoW fork rollback depth:

```sh
$QUINT test spec/quint/CrosslinkPowForkSchedule.qnt \
  --main=CrosslinkPowForkScheduleModel \
  --max-samples=100 \
  --backend=rust
```

This runs:

- `forkSwitchDerivesRollbackDepthTest`
- `sameBranchExtensionHasZeroRollbackDepthTest`
- `raisedSigmaSurvivesForkSwitchThatBaseSigmaDoesNotTest`
- `scheduleDerivesRollbackDepthAcrossRoundsTest`

The model has a same-branch extension from `a3` to `a4`, then a fork switch from
`a4` to `b4` whose last common ancestor is at height 2. It derives rollback
depth 2 from that best-tip transition and checks that sigma 1 does not survive
the switch while sigma 3 does.

Witness generated PoW branch competition:

```sh
$QUINT test spec/quint/CrosslinkPowBranchCompetition.qnt \
  --main=CrosslinkPowBranchCompetitionModel \
  --max-samples=100 \
  --backend=rust
```

This runs:

- `hiddenAdversarialWorkDoesNotWinUntilPublishedTest`
- `releasedAdversarialBranchOutworksHonestTipTest`
- `generatedCompetitionDerivesRollbackDepthTest`
- `raisedSigmaSurvivesGeneratedAdversarialSwitchTest`
- `adversarialCatchupProducesForkSwitchTest`

The fixture lets an adversarial `b4` branch accumulate hidden work while the
published best tip remains `a4`. When `b4` is published with higher total work,
the generated best tip switches to `b4`, deriving rollback depth 2 from the
`a4 -> b4` transition.

Witness the composed nil-precommit-to-finality flow:

```sh
$QUINT test spec/quint/CrosslinkComposed.qnt \
  --main=CrosslinkComposedResamplingModel \
  --max-samples=100 \
  --backend=rust
```

This runs:

- `resamplingNilPrecommitFinalizesFreshCandidateTest`

The composed witness forms a round-0 nil-precommit certificate, carries one
minority same-round value lock, advances all correct validators to round 1,
resamples `a2`, decides it, and finalizes `a2` using `a3` as the tail-confirming
PoW tip. This also demonstrates height skipping in the composed flow: finality
moves from genesis `g` directly to `a2`.

Witness BFT-heighted Crosslink finality:

```sh
$QUINT test spec/quint/CrosslinkBftHeights.qnt \
  --main=CrosslinkBftHeightsModel \
  --max-samples=100 \
  --backend=rust
```

This runs:

- `successiveBftDecisionsAdvanceCrosslinkFinalityTest`
- `rejectsForkDecisionAfterPrefixFinalityTest`
- `rejectsSkippingConsensusHeightTest`

The fixture applies scheduled consensus-height-1 decision `a2` and
consensus-height-2 decision `a3`, advancing Crosslink finality twice. The
negative witnesses reject finalizing a later fork after `a2` is final and reject
skipping directly from consensus height 0 to height 2.

Witness the dynamic-sigma hash-participation controller:

```sh
$QUINT test spec/quint/CrosslinkDynamicSigma.qnt \
  --main=CrosslinkDynamicSigmaHashParticipationModel \
  --max-samples=100 \
  --backend=rust
```

This runs:

- `highHashParticipationStartsAtBaseSigmaTest`
- `roundFailureEscalatesSigmaEvenWithHighHashParticipationTest`
- `lowHashParticipationRaisesSigmaFloorWithoutRoundFailureTest`
- `criticalHashParticipationForcesMaxSigmaTest`
- `adversarialReorgDepthRaisesSigmaFloorTest`
- `hashParticipationSigmaFloorIsMonotoneTest`
- `calibratedRiskScoreIsMonotoneInParticipationTest`
- `combinedStochasticRiskRaisesSigmaWithoutSingleHardSignalTest`
- `criticalStochasticRiskForcesMaxSigmaTest`
- `blockIntervalVarianceCanRaiseSigmaTest`
- `raisedSigmaCanStabilizeMovingPowSampleTest`
- `observedReorgRaisesLiveSigmaTest`
- `observedLowHashParticipationRaisesLiveSigmaTest`
- `observedCriticalHashParticipationForcesLiveMaxSigmaTest`

The model separates two signals that should both feed a production controller:
round failures tell the protocol that the current sampled stream is not stable
enough for Tenderlink to decide, while hash-power participation estimates how
much of the global PoW race is actually represented in the Crosslink-visible
stream. Lower participation therefore raises the sigma floor even if the current
round has not failed. The `raisedSigmaCanStabilizeMovingPowSampleTest` fixture
shows the expected sampling effect directly: `head - baseSigma` changes across
adjacent rounds, but `head - raisedSigma` remains on the same deeper ancestor.
The reorg-depth witnesses add a third input: observed rollback depth forces the
controller to move one rung deeper, so an execution that is still healthy by
participation can nevertheless raise sigma after a reorg reaches the current
floor. The stochastic-risk witnesses add the combined-signal case: moderate
coverage risk, recent round failures, and block-interval variance can raise
sigma together even when none of the old individual floors would have done so
alone; sufficiently high combined risk forces the maximum sigma.

Witness dynamic-sigma calibration over measured windows:

```sh
$QUINT test spec/quint/CrosslinkDynamicSigmaCalibration.qnt \
  --main=CrosslinkDynamicSigmaCalibrationModel \
  --max-samples=100 \
  --backend=rust
```

This runs:

- `healthyMeasuredWindowKeepsBaseSigmaTest`
- `marginalHashParticipationRaisesSigmaTest`
- `combinedMeasuredRiskRaisesSigmaTest`
- `deepReorgMeasuredWindowForcesMaxSigmaTest`
- `criticalParticipationMeasuredWindowForcesMaxSigmaTest`
- `criticalCombinedRiskForcesMaxSigmaTest`
- `calibrationMatchesAllMeasuredWindowsTest`

The calibration fixture covers six measured windows: healthy baseline,
marginal hash-power participation, combined stochastic risk from coverage plus
round-failure plus block-variance signals, deep observed reorgs, critical
hash-power participation, and critical combined stochastic risk. The harness
checks that the chosen weights and thresholds map each measured window to the
expected sigma floor, while keeping each signal monotone and materially
weighted.

Witness production-shaped dynamic-sigma telemetry:

```sh
$QUINT test spec/quint/CrosslinkDynamicSigmaTelemetry.qnt \
  --main=CrosslinkDynamicSigmaTelemetryModel \
  --max-samples=100 \
  --backend=rust
```

This runs:

- `healthyTelemetryWindowKeepsBaseSigmaTest`
- `sourceHashWorkDerivesTelemetryComponentsTest`
- `sourceRoundCountersAreConsistentTest`
- `sourceRollbackDepthDerivesTelemetryComponentsTest`
- `hashWorkParticipationRaisesSigmaTest`
- `combinedTelemetryRiskRaisesSigmaTest`
- `economicTargetRaisesSigmaAboveSignalFloorTest`
- `criticalParticipationForcesMaxSigmaTest`
- `economicTargetCanForceMaxSigmaTest`
- `unreachableEconomicTargetFallsBackToMaxSigmaTest`
- `deepReorgTelemetryWindowForcesMaxSigmaTest`
- `expectedLossBudgetRaisesSigmaEvenWithinPpmTargetTest`
- `telemetryMatchesAllExpectedWindowsTest`

The telemetry fixture covers nine windows: healthy baseline, marginal
participating hash work, combined telemetry risk, an economic target that raises
sigma above the hard-signal floor, critical participating hash work, an economic
target that forces max sigma, an unreachable risk target that falls back to max
sigma, a deep reorg, and a high-value-at-risk window where the rollback
probability is within the PPM cap but the expected-loss budget still forces a
deeper sigma. It checks that conservative telemetry estimates upper-bound raw
sampled work and round failures, rollback risk is monotone in sigma, and the
selected sigma satisfies the configured rollback-risk and expected-loss targets
when the ladder can satisfy them. It also now derives the telemetry component
inputs from source-shaped hash-work samples, round counters, and best-tip
transition heights, matching the Rust source observation-window boundary.

Witness dynamic sigma consuming derived PoW rollback depth:

```sh
$QUINT test spec/quint/CrosslinkDynamicSigmaForkSchedule.qnt \
  --main=CrosslinkDynamicSigmaForkScheduleModel \
  --max-samples=100 \
  --backend=rust
```

This runs:

- `derivedReorgDepthFeedsDynamicSigmaTest`
- `forkScheduleDerivedReorgRaisesDynamicSigmaTest`
- `derivedRaisedSigmaSurvivesForkSwitchTest`

The composed fixture advances from `a3` to `a4` with rollback depth 0 and keeps
base sigma. It then switches from `a4` to `b4`, derives rollback depth 2 from
the fork schedule, and raises dynamic sigma from 1 to 3 without relying on a
separately supplied observed-reorg map.

Witness dynamic sigma consuming generated PoW branch competition:

```sh
$QUINT test spec/quint/CrosslinkDynamicSigmaBranchCompetition.qnt \
  --main=CrosslinkDynamicSigmaBranchCompetitionModel \
  --max-samples=100 \
  --backend=rust
```

This runs:

- `generatedCompetitionFeedsDynamicSigmaTest`
- `generatedCompetitionForkSwitchRaisesDynamicSigmaTest`
- `generatedRaisedSigmaSurvivesAdversarialSwitchTest`

The composed fixture keeps base sigma while published work extends from `a3` to
`a4`, then releases the adversarial `b4` branch. The generated best-tip switch
derives rollback depth 2 and raises dynamic sigma from 1 to 3.

Witness dynamic sigma composing with nil-precommit resampling:

```sh
$QUINT test spec/quint/CrosslinkDynamicSigmaResampling.qnt \
  --main=CrosslinkDynamicSigmaResamplingModel \
  --max-samples=100 \
  --backend=rust
```

This runs:

- `derivedForkSignalRaisesSigmaBeforeResamplingDecisionTest`
- `derivedForkSignalThenNilResamplingDecidesFreshValueTest`
- `criticalHashParticipationRaisesSigmaWithoutNewForkSwitchTest`
- `generatedCompetitionBacksResamplingForkSignalTest`

The witness forms the same-round nil-precommit recovery scenario, derives a
rollback-depth signal from the PoW fork fixture, raises sigma from 1 to 3, then
advances the validators to round 1 and decides fresh stream value `s1`.
The hash-participation witness then advances over a same-branch transition with
rollback depth 0 and still raises sigma to the maximum when participating hash
power falls below the configured critical threshold.
The generated-competition witness checks that hidden `s1` work does not become
the best tip until `s1` is published, then derives the same rollback-depth
signal from that work-backed best-tip switch.

Witness the full dynamic-sigma/resampling/finality composition:

```sh
$QUINT test spec/quint/CrosslinkDynamicSigmaFinality.qnt \
  --main=CrosslinkDynamicSigmaFinalityModel \
  --max-samples=100 \
  --backend=rust
```

This runs:

- `dynamicSigmaResamplingFinalizesTailConfirmedFreshCandidateTest`
- `dynamicSigmaRejectsUnderconfirmedFreshCandidateTest`
- `dynamicSigmaRejectsSkippedBftHeightFinalityTest`
- `hashParticipationSignalRaisesFullCompositionSigmaTest`
- `hashParticipationSigmaCanDelayFullFinalityTest`
- `generatedCompetitionBacksFullCompositionForkSignalTest`

The witness forms a nil-precommit recovery scenario, derives a rollback-depth
signal from an `a3 -> b4` fork switch, raises dynamic sigma from 1 to 3,
resamples and decides fresh `b2`, then finalizes `b2` only with tail-confirming
tip `b5`. The under-confirmed test rejects finalizing the same `b2` decision
against tip `b4`, showing that finality uses the raised dynamic sigma rather
than the base confirmation depth. The generated-competition witness checks that
the fork signal is backed by published work: hidden `b4` does not win at round
0, but published `b4` becomes the generated best tip at round 1. The skipped
BFT-height witness rejects trying to finalize the decided value at consensus
height 2 when the current full-composition height is still 0. The
hash-participation witnesses then advance to a round with no new fork rollback
but only 45% Crosslink-participating hash power; the controller raises sigma to
the maximum, and the previously tail-confirmed `b2` candidate is rejected
against `b5` because it is no longer deep enough under the live sigma.

Randomized Rust-backend safety simulation:

```sh
$QUINT run spec/quint/CrosslinkResampling.qnt \
  --main=CrosslinkStickyModel \
  --init=Init \
  --step=Next \
  --max-steps=10 \
  --max-samples=1000 \
  --invariant=Safety \
  --backend=rust \
  --verbosity=0

$QUINT run spec/quint/CrosslinkResampling.qnt \
  --main=CrosslinkNilResamplingModel \
  --init=Init \
  --step=Next \
  --max-steps=10 \
  --max-samples=1000 \
  --invariant=Safety \
  --backend=rust \
  --verbosity=0

$QUINT run spec/quint/CrosslinkForkFinality.qnt \
  --main=CrosslinkForkFinalityModel \
  --init=Init \
  --step=Next \
  --max-steps=6 \
  --max-samples=1000 \
  --invariant=Safety \
  --backend=rust \
  --verbosity=0

$QUINT run spec/quint/CrosslinkPowForkSchedule.qnt \
  --main=CrosslinkPowForkScheduleModel \
  --init=Init \
  --step=Next \
  --max-steps=4 \
  --max-samples=1000 \
  --invariant=Safety \
  --backend=rust \
  --verbosity=0

$QUINT run spec/quint/CrosslinkPowBranchCompetition.qnt \
  --main=CrosslinkPowBranchCompetitionModel \
  --init=Init \
  --step=Next \
  --max-steps=4 \
  --max-samples=1000 \
  --invariant=Safety \
  --backend=rust \
  --verbosity=0

$QUINT run spec/quint/CrosslinkResampling.qnt \
  --main=CrosslinkNilResamplingLivenessModel \
  --init=LivenessInit \
  --step=LivenessStep \
  --max-steps=15 \
  --max-samples=1 \
  --invariant=LivenessSafety \
  --backend=rust \
  --verbosity=0

$QUINT run spec/quint/CrosslinkComposed.qnt \
  --main=CrosslinkComposedResamplingModel \
  --init=ComposedInit \
  --step=ComposedNext \
  --max-steps=10 \
  --max-samples=1000 \
  --invariant=ComposedSafety \
  --backend=rust \
  --verbosity=0

$QUINT run spec/quint/CrosslinkComposed.qnt \
  --main=CrosslinkComposedLivenessModel \
  --init=LivenessInit \
  --step=LivenessStep \
  --max-steps=16 \
  --max-samples=1 \
  --invariant=LivenessSafety \
  --backend=rust \
  --verbosity=0

$QUINT run spec/quint/CrosslinkBftHeights.qnt \
  --main=CrosslinkBftHeightsModel \
  --init=Init \
  --step=Next \
  --max-steps=5 \
  --max-samples=1000 \
  --invariant=Safety \
  --backend=rust \
  --verbosity=0

$QUINT run spec/quint/CrosslinkDynamicSigma.qnt \
  --main=CrosslinkDynamicSigmaHashParticipationModel \
  --init=Init \
  --step=Next \
  --max-steps=7 \
  --max-samples=1000 \
  --invariant=Safety \
  --backend=rust \
  --verbosity=0

$QUINT run spec/quint/CrosslinkDynamicSigmaCalibration.qnt \
  --main=CrosslinkDynamicSigmaCalibrationModel \
  --init=Init \
  --step=Next \
  --max-steps=8 \
  --max-samples=1000 \
  --invariant=Safety \
  --backend=rust \
  --verbosity=0

$QUINT run spec/quint/CrosslinkDynamicSigmaTelemetry.qnt \
  --main=CrosslinkDynamicSigmaTelemetryModel \
  --init=Init \
  --step=Next \
  --max-steps=8 \
  --max-samples=1000 \
  --invariant=Safety \
  --backend=rust \
  --verbosity=0

$QUINT run spec/quint/CrosslinkDynamicSigmaForkSchedule.qnt \
  --main=CrosslinkDynamicSigmaForkScheduleModel \
  --init=DerivedInit \
  --step=DerivedNext \
  --max-steps=4 \
  --max-samples=1000 \
  --invariant=DerivedSafety \
  --backend=rust \
  --verbosity=0

$QUINT run spec/quint/CrosslinkDynamicSigmaBranchCompetition.qnt \
  --main=CrosslinkDynamicSigmaBranchCompetitionModel \
  --init=BranchCompetitionDynamicInit \
  --step=BranchCompetitionDynamicNext \
  --max-steps=4 \
  --max-samples=1000 \
  --invariant=BranchCompetitionDynamicSafety \
  --backend=rust \
  --verbosity=0

$QUINT run spec/quint/CrosslinkDynamicSigmaResampling.qnt \
  --main=CrosslinkDynamicSigmaResamplingModel \
  --init=DynamicResamplingInit \
  --step=DynamicResamplingNext \
  --max-steps=8 \
  --max-samples=1000 \
  --invariant=DynamicResamplingSafety \
  --backend=rust \
  --verbosity=0

$QUINT run spec/quint/CrosslinkDynamicSigmaFinality.qnt \
  --main=CrosslinkDynamicSigmaFinalityModel \
  --init=FullComposedInit \
  --step=FullComposedNext \
  --max-steps=10 \
  --max-samples=1000 \
  --invariant=FullComposedSafety \
  --backend=rust \
  --verbosity=0
```

Bounded Apalache verification:

```sh
$QUINT verify spec/quint/CrosslinkResampling.qnt \
  --main=CrosslinkStickyModel \
  --max-steps=3 \
  --init=Init \
  --step=Next \
  --invariant=Safety

$QUINT verify spec/quint/CrosslinkResampling.qnt \
  --main=CrosslinkNilResamplingModel \
  --max-steps=3 \
  --init=Init \
  --step=Next \
  --invariant=Safety

$QUINT verify spec/quint/CrosslinkForkFinality.qnt \
  --main=CrosslinkForkFinalityModel \
  --max-steps=4 \
  --init=Init \
  --step=Next \
  --invariant=Safety

$QUINT verify spec/quint/CrosslinkPowForkSchedule.qnt \
  --main=CrosslinkPowForkScheduleModel \
  --max-steps=4 \
  --init=Init \
  --step=Next \
  --invariant=Safety

$QUINT verify spec/quint/CrosslinkPowBranchCompetition.qnt \
  --main=CrosslinkPowBranchCompetitionModel \
  --max-steps=4 \
  --init=Init \
  --step=Next \
  --invariant=Safety

$QUINT verify spec/quint/CrosslinkResampling.qnt \
  --main=CrosslinkNilResamplingLivenessModel \
  --max-steps=15 \
  --init=LivenessInit \
  --step=LivenessStep \
  --invariant=LivenessSafety

$QUINT verify spec/quint/CrosslinkComposed.qnt \
  --main=CrosslinkComposedResamplingModel \
  --max-steps=5 \
  --init=ComposedInit \
  --step=ComposedNext \
  --invariant=ComposedSafety

$QUINT verify spec/quint/CrosslinkComposed.qnt \
  --main=CrosslinkComposedLivenessModel \
  --max-steps=16 \
  --init=LivenessInit \
  --step=LivenessStep \
  --invariant=LivenessSafety

$QUINT verify spec/quint/CrosslinkBftHeights.qnt \
  --main=CrosslinkBftHeightsModel \
  --max-steps=5 \
  --init=Init \
  --step=Next \
  --invariant=Safety

$QUINT verify spec/quint/CrosslinkDynamicSigma.qnt \
  --main=CrosslinkDynamicSigmaHashParticipationModel \
  --max-steps=7 \
  --init=Init \
  --step=Next \
  --invariant=Safety

$QUINT verify spec/quint/CrosslinkDynamicSigmaCalibration.qnt \
  --main=CrosslinkDynamicSigmaCalibrationModel \
  --max-steps=8 \
  --init=Init \
  --step=Next \
  --invariant=Safety

$QUINT verify spec/quint/CrosslinkDynamicSigmaTelemetry.qnt \
  --main=CrosslinkDynamicSigmaTelemetryModel \
  --max-steps=8 \
  --init=Init \
  --step=Next \
  --invariant=Safety

$QUINT verify spec/quint/CrosslinkDynamicSigmaForkSchedule.qnt \
  --main=CrosslinkDynamicSigmaForkScheduleModel \
  --max-steps=4 \
  --init=DerivedInit \
  --step=DerivedNext \
  --invariant=DerivedSafety

$QUINT verify spec/quint/CrosslinkDynamicSigmaBranchCompetition.qnt \
  --main=CrosslinkDynamicSigmaBranchCompetitionModel \
  --max-steps=4 \
  --init=BranchCompetitionDynamicInit \
  --step=BranchCompetitionDynamicNext \
  --invariant=BranchCompetitionDynamicSafety

$QUINT verify spec/quint/CrosslinkDynamicSigmaResampling.qnt \
  --main=CrosslinkDynamicSigmaResamplingModel \
  --max-steps=8 \
  --init=DynamicResamplingInit \
  --step=DynamicResamplingNext \
  --invariant=DynamicResamplingSafety

JVM_ARGS=-Xmx8192m $QUINT verify spec/quint/CrosslinkDynamicSigmaFinality.qnt \
  --main=CrosslinkDynamicSigmaFinalityModel \
  --max-steps=8 \
  --init=FullComposedInit \
  --step=FullComposedNext \
  --invariant=FullComposedSafety

JVM_ARGS=-Xmx8192m $QUINT verify spec/quint/CrosslinkDynamicSigmaFinality.qnt \
  --main=CrosslinkDynamicSigmaFinalityModel \
  --max-steps=10 \
  --init=FullComposedInit \
  --step=FullComposedNext \
  --invariant=FullProtocolProjectionSafety

JVM_ARGS=-Xmx8192m $QUINT verify spec/quint/CrosslinkDynamicSigmaFinality.qnt \
  --main=CrosslinkDynamicSigmaFinalityModel \
  --max-steps=10 \
  --init=FullComposedInit \
  --step=FullComposedNext \
  --invariant=FullFinalityProjectionSafety

JVM_ARGS=-Xmx8192m $QUINT verify spec/quint/CrosslinkDynamicSigmaFinality.qnt \
  --main=CrosslinkDynamicSigmaFinalityModel \
  --max-steps=10 \
  --init=FullComposedInit \
  --step=FullComposedNext \
  --invariant=FullWorkCompetitionProjectionSafety
```

The full dynamic-sigma finality composition is substantially heavier under
Apalache once the resampling model includes accountability evidence. The checked
symbolic bound above passes with an 8G JVM heap. The projection checks split the
full composition into protocol/accountability, finalized-prefix, and generated
work-competition obligations so each can be pushed to a deeper bound before
rerunning the full conjunction.

The bounded resampling checks currently report no violation for `Safety`, which
combines:

- validity of correct-validator decisions
- agreement on decided values
- upstream-style accountability: if agreement fails, at least `f + 1`
  validators are detectable by equivocation or amnesia evidence
- no correct-validator precommit equivocation
- no same-round quorum for both nil and a concrete value
- any retained Tendermint lock keeps its matching valid value across
  nil-precommit round recovery
- any retained Tendermint lock is backed by value-precommit evidence
- a nil certificate leaves at most `f` correct same-round value locks, so
  same-round unlock is not discarding a commit-capable value-lock quorum
- observed proposal, prevote, and precommit messages are covered by
  transition-carried evidence sets
- every pair of conflicting value commit quorums has accountability evidence:
  either correct-validator precommit equivocation, nil/value equivocation in the
  purported unlock round, or a correct-validator value switch without a valid
  same-round nil unlock certificate

The bounded fork-finality check reports no violation for its `Safety`, which
combines:

- finalized snapshots are prefix-linear
- the latest final snapshot extends every previously finalized snapshot
- the initial final snapshot remains finalized

The bounded PoW fork-schedule check reports no violation for its `Safety`, which
combines:

- the live best tip matches the configured best-tip schedule
- the live rollback depth is derived from the previous and current best tips
- the rollback depth stays within the configured PoW height bound

The bounded PoW branch-competition check reports no violation for its `Safety`,
which combines:

- the live best tip matches the work-derived published-tip competition
- the live best-tip work matches honest plus adversarial work
- the live rollback depth is derived from generated best-tip changes
- the rollback depth stays within the configured PoW height bound

The bounded liveness harness checks the proposed nil-precommit flow under a
post-GST schedule: a same-round nil certificate is formed for `s0`, one correct
validator may hold a minority same-round value lock, all correct validators
advance to round 1, the proposer resamples `s1`, and the model reaches a fresh
`s1` decision by phase 15 while preserving the safety invariants.

The composed bounded liveness harness checks the Crosslink-level version of the
same argument: after a same-round nil certificate and a stream update from `a1`
to `a2`, resampling reaches a fresh `a2` Tenderlink decision and then a fresh
`a2` finality update by phase 16 while preserving both the Tenderlink lock
safety invariants and the finalized-prefix safety invariants.

The BFT-heighted finality harness checks that consecutive BFT decisions advance
Crosslink finality in height order while preserving finalized-prefix safety.
It also gives negative witnesses for two invalid transitions: skipping a
consensus height and finalizing a fork after a prefix is final.

The dynamic-sigma harness checks a bounded controller invariant: live sigma
remains within the configured ladder, never falls below the floor implied by
observed hash-power participation, observed reorg depth, or the calibrated
stochastic-risk score, tracks the `head - sigma` snapshot selected for the
current round, and uses monotone risk surfaces where lower participation cannot
require a lower sigma than higher participation.

The dynamic-sigma calibration harness reports no violation for `Safety`, which
combines:

- every bounded measurement window maps to its expected sigma floor
- lower hash-power participation never lowers the participation-derived floor
- lower hash-power participation never lowers the calibrated risk score
- round-failure, block-variance, and reorg-depth weights are all material
- the observation-walk invariant preserves the calibrated label for each window

The production-shaped dynamic-sigma telemetry harness reports no violation for
`Safety`, which combines:

- source hash-work samples derive the total-work denominator and
  Crosslink-participating numerator
- source round counters remain internally consistent
- source best-tip transition heights derive observed rollback depth
- conservative coverage estimates upper-bound the raw gap between total PoW
  work and Crosslink-participating PoW work
- conservative round-failure estimates upper-bound raw failed Tenderlink rounds
- rollback-risk estimates are monotone across the sigma ladder
- selected sigma satisfies the explicit rollback-risk and expected-loss targets
  when reachable
- if the target is unreachable at max sigma, the controller falls back to max
  sigma and exposes that status
- sampled hash-work coverage maps to the expected participation floor
- every telemetry window maps to its expected sigma floor

The dynamic-sigma/fork-schedule composition reports no violation for
`DerivedSafety`, which combines:

- the PoW fork-schedule safety invariants
- dynamic sigma stays within the configured ladder
- dynamic sigma respects the hash-participation floor
- dynamic sigma respects the rollback-depth floor derived from the fork schedule
- the controller status matches current hash participation

The dynamic-sigma/branch-competition composition reports no violation for
`BranchCompetitionDynamicSafety`, which combines:

- the PoW branch-competition safety invariants
- dynamic sigma stays within the configured ladder
- dynamic sigma respects the hash-participation floor
- dynamic sigma respects the rollback-depth floor derived from generated
  best-tip work competition
- the controller status matches current hash participation

The dynamic-sigma/resampling composition reports no violation for
`DynamicResamplingSafety`, which combines:

- the nil-precommit resampling safety invariants
- dynamic sigma stays within the configured ladder
- dynamic sigma respects the rollback-depth floor derived from the fork fixture
- dynamic sigma respects the hash-power participation floor
- the participation floor is monotone, so lower participation never requires a
  lower sigma than higher participation
- the controller status matches current hash-power participation
- the current dynamic best tip matches generated published-tip work competition

The full dynamic-sigma/resampling/finality composition reports no violation for
`FullComposedSafety`, which combines:

- the dynamic-sigma/resampling safety invariants
- the generated best-tip work-competition invariant for the current dynamic
  round
- finalized snapshots remain prefix-linear
- the latest finalized snapshot extends all prior finalized snapshots
- the initial finalized snapshot remains finalized
- finality advances exactly one BFT consensus height at a time
- finality uses the live dynamic sigma as the tail-confirmation depth
- hash-participation-driven sigma increases compose with the same finality
  depth rule as fork-derived sigma increases

The split full-composition projection checks report no violation at depth 10.
`FullProtocolProjectionSafety` is the expensive check because it carries the
nil-precommit/accountability evidence obligations. `FullFinalityProjectionSafety`
and `FullWorkCompetitionProjectionSafety` isolate the finalized-prefix and
generated-work-competition obligations and run substantially faster.

## Next Extensions

This model is intentionally narrow. The next useful extensions are:

- wire the pure Rust dynamic-sigma controller, proposal-evidence verifier, and
  selected-sigma BFT block constructor to production telemetry sources and live
  proposal validation, including a consensus-safe Crosslink hash-participation
  metric and a validated economic exposure model. The live prototype proposal
  path now runs the controller over fixture telemetry components, and the pure
  telemetry assembly boundary fails closed on missing participating-work
  evidence or inconsistent round counters. The event accumulator now gives live
  Tenderlink hooks an exact round-counter contract, the hash-work observation
  accumulator gives source producers an exact participation-numerator contract,
  and the pure rollback-depth helper derives the observed reorg-depth input from
  explicit best-tip transition evidence. The observation-window assembler now
  composes those source-shaped inputs into telemetry components. The remaining
  work is replacing the fixture with consensus-safe or proposal-verifiable input
  producers
- refine the split projection checks into smaller inductive lemmas if bounds
  beyond the checked depth-10 projections still need very large JVM/Z3 heaps
