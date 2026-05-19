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

`CrosslinkBaseline.qnt` packages the current fixed-sigma/sticky behavior as a
named baseline variant. It reuses the focused Tenderlink model with
`ResampleOnNilPrecommit = false`, then separates the positive baseline witness
from the known stream-change limitation: a stable stream decides through the
normal propose/prevote/precommit path, while a changed stream carries the stale
sample and can block a fresh decision because same-round Tendermint state is not
cleared.

`CrosslinkBaselineTenderlink.qnt` gives that baseline an upstream-shaped
parameter shell: `BaselineCorr`, `BaselineFaulty`, `BaselineN`, `BaselineT`,
valid and invalid snapshot sets, round/proposer parameters, and the
Crosslink-specific fixed-sigma rule
`BaselineStream(round) = ancestor(bestTip(round), height(bestTip(round)) -
sigma)`. It still delegates to the focused sticky model, so full upstream
faulty-message injection and most of the full Tendermint transition surface
remain future work.

`CrosslinkBaselineModels.qnt` adds small baseline instances over that shell,
including stable and forking `n4_f1` fixtures, a forking `n5_f1` fixture,
above-live-boundary `n4_f2`/`n5_f2` fixtures, and a proper `n7_f2` fixture.
The `f = 2` witnesses distinguish cases where correct validators cannot form a
2f+1 value quorum from the `n7_f2` case where the normal decision path remains
available.

`CrosslinkBaselineTest.qnt` adds upstream-style smoke tests for the baseline
shell: fixed-sigma sampling, normal decision, no double proposal, nil prevote
quorum precommit-nil handling, timeout-driven nil votes and round advance,
future-round catchup, deriving a fresh `head - sigma` value after a fork
switch, and the sticky baseline witness that still carries the stale
fixed-sigma sample.

`CrosslinkBaselineAccountability.qnt` makes the baseline accountability
projection explicit. It checks that a nil-precommit certificate does not clear
same-round value locks in the sticky baseline, and that conflicting value
commits without unlock evidence expose Tendermint-style amnesia evidence. It
also includes focused faulty-evidence witnesses showing that faulty proposals,
prevotes, and nil/value precommit conflicts are carried into the same
equivocation predicates used by the accountability model. Its tiny faulty-init
model adapts the upstream Tendermint pattern of nondeterministically injecting
faulty proposal, prevote, and precommit powersets in the initial state, while
keeping the instance small enough for the baseline proof gate. A second
faulty-init model lifts the same idea into the fixed-sigma/forking `n4_f1`
parameter surface with a bounded faulty-evidence domain so symbolic checking
remains tractable. The same file also includes counterexample tests for false
agreement, amnesia, equivocation, and no-conflicting-commit claims, so the
accountability predicates are checked against known-bad assertions.

`CrosslinkBaselineBftHeights.qnt` gives the baseline variant a named
heighted-finality model. It checks that fixed-sigma finality advances through
consecutive BFT consensus heights while rejecting skipped consensus heights and
fork decisions after a prefix is final.

`CrosslinkBaselineFinality.qnt` composes that baseline Tenderlink behavior with
Crosslink finality. It shows that the fixed-sigma/sticky protocol can finalize a
tail-confirmed stable-stream decision, and records the Crosslink-level failure
mode when a stream change leaves the protocol carrying the stale sample while
finality remains at the prior prefix.

`CrosslinkBaselinePowSampling.qnt` pins the baseline stream to an explicit
fixed `head - sigma` PoW sample. It derives `Stream(round)` from a best-tip
schedule and ancestor map, then shows that a fork switch can roll back the
previous fixed-sigma sample while the sticky baseline still carries that sample
into the next round.

`baseline-completeness-audit.md` maps the current baseline artifacts against
the upstream Tendermint Quint example and tracks the remaining work before the
baseline reaches upstream-style completeness rather than focused witness
coverage.

`baseline-upstream-crosswalk.md` is the line-item map from the current upstream
Tendermint Quint surface to the baseline Crosslink artifacts. Use it as the
working checklist for deciding whether a baseline change is closing an upstream
completeness gap, documenting an intentional Crosslink deviation, or adding a
Crosslink-only proof obligation.

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

`CrosslinkDynamicSigmaHysteresis.qnt` models the stability policy that should
sit on top of those telemetry-derived floors. Worsening evidence, including a
drop in Crosslink-participating hash power, can raise sigma immediately.
Improving evidence lowers sigma only after enough stable lower-risk windows,
and then only one ladder step at a time.

`dynamic-sigma-telemetry-integration.md` maps those telemetry inputs to
production data sources and documents the consensus-safety requirements before
a deployed controller can replace the prototype's fixed sigma parameter.
`zebra-crosslink/src/dynamic_sigma.rs` is the matching pure Rust controller
prototype: it derives conservative coverage and round-failure estimates from
raw counters, validates telemetry windows, and selects the same sigma floor as
the Quint telemetry fixture. It also includes a proposal-carried evidence
verifier that rejects selected sigma values below the controller-required floor.
The pure module now exposes proposal-evidence selection helpers for raw
telemetry and production-shaped components, with optional hysteresis, and the
Tenderlink prototype path calls those helpers rather than duplicating controller
selection logic.
`DynamicSigmaTelemetryComponents` and `DynamicSigmaRoundCounters` are the first
production-shaped assembly boundary: they require explicit total and
Crosslink-participating hash work, reject inconsistent round counters, and only
then build raw controller telemetry. `DynamicSigmaRoundEvent` accumulates
started, decided, nil-precommit, stale-proposal, timeout, invalid-proposal, and
mixed-evidence labels into those counters, while rejecting failure-reason
overcounts. `observed_hash_work_participation` aggregates source-side PoW work
observations into the total-work denominator and verified-participating
numerator, so work without objective Crosslink participation evidence is counted
conservatively as non-participating. The regression tests now include a skewed
window where fewer high-work non-participating observations outweigh more
participating observations, ensuring the sigma input is percentage of hash
power rather than observation count. `DynamicSigmaDecision` also exposes the
same work-threshold view as a conservative lower-bound participation
percentage, rounded down so diagnostics cannot overstate threshold coverage.
`hash_work_observation_from_header` and
`hash_work_observations_from_headers` now derive that source signal from PoW
headers by converting compact difficulty into work and treating the current
non-null Crosslink fat pointer as the objective participation marker. This keeps
the dynamic-sigma input as a work-weighted participating hash-power percentage,
and the `_with_verifier` helpers let a production source require stricter
fat-pointer validation before non-null marker work enters the participating
numerator. `DynamicSigmaHashWorkObservationWindowPolicy` adds source-window
discipline for that percentage by requiring enough recent observations and
enough total observed work before deriving the numerator/denominator pair.
`DynamicSigmaHeaderObservationWindow` lets callers provide headers
directly alongside Tenderlink round counters, best-tip transitions, variance
telemetry, rollback-risk estimates, and economic exposure inputs, then builds
the same telemetry components as the lower-level hash-work observation path.
`telemetry_components_from_header_observation_window_with_hash_work_policy`
applies the same recent-window and minimum-work guardrail to header-derived
participation, so production callers do not have to bypass the policy when the
source signal is PoW headers.
`DynamicSigmaTimedHeaderObservationWindow` derives the variance input from the
same header window by comparing adjacent header timestamps against an expected
target spacing, rejecting too-short or non-increasing windows and capping the
conservative deviation percentage at 100. Its hash-work-policy helper composes
that timestamp-derived variance with the same guarded header participation path.
`target_block_spacing_seconds_from_network_upgrade` and
`target_block_spacing_seconds_for_height` derive the expected spacing from
Zebra's network-upgrade schedule for production-shaped timed windows.
`DynamicSigmaEconomicExposurePolicy` makes the economic-risk boundary explicit:
consensus-critical exposure carries value-at-risk and loss-budget units into
proposal evidence, while service-local exposure maps to zero consensus exposure
and cannot silently change validator validity.
`DynamicSigmaBestTipTransition` derives the observed reorg-depth input from
explicit old-tip, new-tip, and common-ancestor heights, with a helper for taking
the maximum rollback depth over a transition window.
`DynamicSigmaBestTipTransitionRecorder` gives live state hooks a small
state-machine boundary: seed the first observed best tip, require a common
ancestor after that, and reject invalid transition evidence without advancing.
`rollback_depth_history_from_transition_windows` turns a sequence of transition
windows into one rollback-depth sample per measurement window, giving live state
hooks a pure source target for rollback-risk history.
`rollback_risk_curve_from_observed_rollback_depths` derives a deterministic
empirical `RollbackRiskCurve` by measuring how often observed rollback-depth
windows reach each sigma in the ladder, then adding a bounded conservative ppm
margin. `DynamicSigmaRollbackRiskWindowPolicy` wraps that estimator with
minimum-history and maximum-recent-window bounds so old rollback events do not
silently dominate current sigma selection.
`telemetry_components_from_observation_window` composes these source-shaped
hash-work observations, round counters, and best-tip transitions into telemetry
components before proposal evidence selection. The branch also
includes a
`BftBlock::try_from_with_confirmation_depth` construction hook and tagged payload
envelope. The live Tenderlink proposal, validation, and decided-block
callbacks now route through a config-aware payload path: default config still
emits and accepts the legacy fixed-sigma `BftBlock`, while
`dynamic_sigma_prototype` emits and validates the tagged dynamic-sigma envelope
using shared prototype parameters and proposal-carried evidence. The decoded
payload carries its selected confirmation depth into voting-time stale checks,
so a prototype dynamic proposal is checked against `head - selected_sigma`. The
prototype proposer now builds telemetry from a fixture timed-header source
window, applies the hash-work policy, assembles telemetry components, and then
runs the dynamic-sigma controller before selecting sigma; production telemetry
sources are still the missing deployment step. Invalid source observation
assembly, telemetry assembly, or telemetry-to-evidence selection prevents
prototype proposal emission instead of falling back to the base sigma.

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

Apalache starts a local checker server during `quint verify`. By default Quint
uses port `8822`; the local wrapper also accepts `APALACHE_PORT_BASE` to give
each symbolic check a sequential port when a long run would otherwise collide
with a lingering checker process.

## Checks

Run the baseline Crosslink sweep:

```sh
QUINT="$QUINT" spec/quint/check.sh quick-baseline
QUINT="$QUINT" JVM_ARGS=-Xmx8192m spec/quint/check.sh symbolic-baseline
```

This typechecks the shared sticky Tenderlink model plus the named baseline
Crosslink specs, runs the Rust witness tests, runs the Rust safety checks, and
then runs the bounded Apalache checks for the baseline-only proof obligations.

Run the quick local sweep:

```sh
QUINT="$QUINT" spec/quint/check.sh quick
```

Run the bounded Apalache sweep:

```sh
QUINT="$QUINT" JVM_ARGS=-Xmx8192m spec/quint/check.sh symbolic
```

For faster iteration on only the fixed-sigma/sticky baseline variant, use either
half of that baseline sweep:

```sh
QUINT="$QUINT" spec/quint/check.sh quick-baseline
QUINT="$QUINT" JVM_ARGS=-Xmx8192m spec/quint/check.sh symbolic-baseline
```

If a local symbolic run reports an Apalache port collision, rerun with a fresh
base port:

```sh
QUINT="$QUINT" JVM_ARGS=-Xmx8192m APALACHE_PORT_BASE=8830 \
  spec/quint/check.sh symbolic-baseline
```

Typecheck:

```sh
$QUINT typecheck spec/quint/CrosslinkBaseline.qnt
$QUINT typecheck spec/quint/CrosslinkBaselineTenderlink.qnt
$QUINT typecheck spec/quint/CrosslinkBaselineModels.qnt
$QUINT typecheck spec/quint/CrosslinkBaselineTest.qnt
$QUINT typecheck spec/quint/CrosslinkBaselineAccountability.qnt
$QUINT typecheck spec/quint/CrosslinkBaselineBftHeights.qnt
$QUINT typecheck spec/quint/CrosslinkBaselineFinality.qnt
$QUINT typecheck spec/quint/CrosslinkBaselinePowSampling.qnt
$QUINT typecheck spec/quint/CrosslinkResampling.qnt
$QUINT typecheck spec/quint/CrosslinkForkFinality.qnt
$QUINT typecheck spec/quint/CrosslinkPowForkSchedule.qnt
$QUINT typecheck spec/quint/CrosslinkPowBranchCompetition.qnt
$QUINT typecheck spec/quint/CrosslinkComposed.qnt
$QUINT typecheck spec/quint/CrosslinkBftHeights.qnt
$QUINT typecheck spec/quint/CrosslinkDynamicSigma.qnt
$QUINT typecheck spec/quint/CrosslinkDynamicSigmaCalibration.qnt
$QUINT typecheck spec/quint/CrosslinkDynamicSigmaTelemetry.qnt
$QUINT typecheck spec/quint/CrosslinkDynamicSigmaHysteresis.qnt
$QUINT typecheck spec/quint/CrosslinkDynamicSigmaForkSchedule.qnt
$QUINT typecheck spec/quint/CrosslinkDynamicSigmaBranchCompetition.qnt
$QUINT typecheck spec/quint/CrosslinkDynamicSigmaResampling.qnt
$QUINT typecheck spec/quint/CrosslinkDynamicSigmaFinality.qnt
```

Witness the named baseline behavior:

```sh
$QUINT test spec/quint/CrosslinkBaseline.qnt \
  --main=CrosslinkBaselineStableModel \
  --max-samples=100 \
  --backend=rust

$QUINT test spec/quint/CrosslinkBaseline.qnt \
  --main=CrosslinkBaselineStreamChangeModel \
  --max-samples=100 \
  --backend=rust
```

The stable model checks the fixed-sigma/sticky protocol can decide its sampled
snapshot when the PoW stream does not move. The stream-change model records the
baseline limitation: after a nil-precommit quorum, the current protocol carries
the stale sample into the next round and same-round Tendermint state can block a
fresh decision.

Witness the upstream-shaped baseline shell:

```sh
$QUINT test spec/quint/CrosslinkBaselineTest.qnt \
  --main=CrosslinkBaselineN4F1StableTest \
  --max-samples=100 \
  --backend=rust

$QUINT test spec/quint/CrosslinkBaselineTest.qnt \
  --main=CrosslinkBaselineN4F1ForkingTest \
  --max-samples=100 \
  --backend=rust

$QUINT test spec/quint/CrosslinkBaselineTest.qnt \
  --main=CrosslinkBaselineN4F2ForkingTest \
  --max-samples=100 \
  --backend=rust

$QUINT test spec/quint/CrosslinkBaselineTest.qnt \
  --main=CrosslinkBaselineN5F2ForkingTest \
  --max-samples=100 \
  --backend=rust

$QUINT test spec/quint/CrosslinkBaselineTest.qnt \
  --main=CrosslinkBaselineN7F2ForkingTest \
  --max-samples=100 \
  --backend=rust
```

The stable `n4_f1` test checks fixed-sigma sampling, normal decision,
no-double-proposal behavior, and the normal Tendermint transition from a nil
prevote quorum to nil precommits. The forking `n4_f1` test checks that the
shell derives the fresh `head - sigma` sample after a fork switch while the
sticky baseline still carries the old sample into the next round. The `f = 2`
tests record that `n4_f2` and `n5_f2` sit above the live fault boundary for
correct-only value commits, while `n7_f2` still supports a 2f+1 correct
decision path.

Witness the named baseline accountability behavior:

```sh
$QUINT test spec/quint/CrosslinkBaselineAccountability.qnt \
  --main=CrosslinkBaselineAccountabilityModel \
  --max-samples=100 \
  --backend=rust

$QUINT test spec/quint/CrosslinkBaselineAccountability.qnt \
  --main=CrosslinkBaselineFaultyInitTinyModel \
  --max-samples=100 \
  --backend=rust

$QUINT test spec/quint/CrosslinkBaselineAccountability.qnt \
  --main=CrosslinkBaselineFaultyInitForkingModel \
  --max-samples=100 \
  --backend=rust

$QUINT test spec/quint/CrosslinkBaselineAccountability.qnt \
  --main=CrosslinkBaselineCounterexampleModel \
  --max-samples=100 \
  --backend=rust
```

This model checks that baseline nil precommit preserves same-round value-lock
state and that conflicting value commits without nil-unlock evidence are
accountable through the existing amnesia predicates. It also checks that
faulty proposal, prevote, and nil/value precommit evidence feeds the
equivocation detector. The tiny faulty-init model checks the upstream-style
`InitWithFaultyEvidence` path with nondeterministic faulty message powersets.
The forking faulty-init model checks a larger fixed-sigma/forking baseline
instance with bounded faulty proposal, prevote, and precommit powersets; the
remaining upstream-quality step is to lift the unbounded faulty-init path into
the larger parameterized baseline instances without making the symbolic gate
unusably large. The counterexample model uses `.fail()` witnesses for false
no-conflicting-commit, no-amnesia, no-equivocation, and agreement claims.

Witness baseline BFT-heighted finality:

```sh
$QUINT test spec/quint/CrosslinkBaselineBftHeights.qnt \
  --main=CrosslinkBaselineBftHeightsModel \
  --max-samples=100 \
  --backend=rust
```

This model checks that fixed-sigma baseline finality advances at consecutive BFT
heights, rejects skipped BFT heights, and rejects finalizing a fork after a
prefix is final.

Witness the named baseline finality behavior:

```sh
$QUINT test spec/quint/CrosslinkBaselineFinality.qnt \
  --main=CrosslinkBaselineFinalityStableModel \
  --max-samples=100 \
  --backend=rust

$QUINT test spec/quint/CrosslinkBaselineFinality.qnt \
  --main=CrosslinkBaselineFinalityStreamChangeModel \
  --max-samples=100 \
  --backend=rust
```

The stable finality model checks the current fixed-sigma/sticky protocol can
decide and finalize a tail-confirmed sampled snapshot when the PoW stream is
stable. The stream-change finality model records the Crosslink-level limitation:
after a nil-precommit quorum, the current protocol can still carry the stale
sample and leave finality at the previous prefix instead of reaching the fresh
stream value.

Witness fixed-sigma baseline PoW sampling:

```sh
$QUINT test spec/quint/CrosslinkBaselinePowSampling.qnt \
  --main=CrosslinkBaselinePowSamplingModel \
  --max-samples=100 \
  --backend=rust
```

This model makes `Stream(round)` equal to the explicit fixed `head - sigma`
ancestor. Its fork-switch fixture moves the best tip from `a4` to `b4`, derives
round samples `a3` and `b3`, and checks that the sticky baseline still proposes
the rolled-back `a3` sample in round 1.

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
- `sourceBlockIntervalVarianceDerivesTelemetryComponentsTest`
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
inputs from source-shaped hash-work samples, round counters, best-tip
transition heights, and adjacent header timestamps, matching the Rust source
observation-window boundary.
`apply_dynamic_sigma_hysteresis` is a pure Rust policy helper for applying those
required sigma floors across windows: it raises immediately on worse evidence
and only lowers one ladder step after enough stable lower-risk windows. It is
also composed into the prototype dynamic-sigma proposal-evidence builder with an
explicit initial hysteresis state. Proposal validation still only requires that
the carried `selected_sigma` is at least the telemetry-required floor, so a
hysteresis-selected value above the floor remains valid without validators
reconstructing private proposer state. The prototype service now owns an
in-process hysteresis state and advances it after successfully encoding a
dynamic proposal. The hysteresis policy and state now have deterministic Zcash
serialization, giving a production source a stable persistence or
proposal-carried encoding. The typed Rust selection helper now distinguishes
disabled hysteresis from durable-local and proposal-carried state sources:
disabled selection emits no next state, while the enabled modes return the next
state to persist or carry. Production still needs to choose the durable,
consensus-safe or proposal-verifiable source for that state before this becomes
a deployed controller rule. The live proposal callback builds a single proposal
plan, so the same dynamic-sigma evidence supplies both the BFT block
construction depth and the encoded payload instead of running selection twice.

Witness dynamic-sigma hysteresis:

```sh
$QUINT test spec/quint/CrosslinkDynamicSigmaHysteresis.qnt \
  --main=CrosslinkDynamicSigmaHysteresisModel \
  --max-samples=100 \
  --backend=rust
```

This runs:

- `higherRequiredSigmaAppliesImmediatelyTest`
- `lowerRequiredSigmaWaitsForStableWindowTest`
- `stableLowerRequiredSigmaStepsDownOneLevelTest`
- `secondStableDecreaseReturnsToBaseTest`
- `hysteresisSafetyHoldsAtWitnessEndTest`

The hysteresis fixture treats the required sigma as the output of the telemetry
controller. If low Crosslink-participating hash power, fork rollback depth, or
combined risk raises that required floor, live sigma follows immediately. If a
later window reports healthier participation or lower risk, live sigma waits for
stable confirmation windows and then steps down one ladder level instead of
jumping directly back to base.

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

$QUINT run spec/quint/CrosslinkDynamicSigmaHysteresis.qnt \
  --main=CrosslinkDynamicSigmaHysteresisModel \
  --init=Init \
  --step=Next \
  --max-steps=5 \
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

$QUINT verify spec/quint/CrosslinkDynamicSigmaHysteresis.qnt \
  --main=CrosslinkDynamicSigmaHysteresisModel \
  --max-steps=5 \
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

The bounded baseline-accountability checks report no violation for
`BaselineAccountabilitySafety`, which is the current fixed-sigma/sticky
Tenderlink safety invariant. The named witnesses show that a nil-precommit
certificate advances the round without clearing same-round `validValue` or
`lockedValue`, and that conflicting value commits without nil-unlock evidence
are attributable through amnesia evidence. The same test module now includes
faulty proposal, prevote, and nil/value precommit evidence witnesses that prove
observed faulty evidence reaches the equivocation predicates.
`BaselineFaultyInitSafety` additionally checks a tiny instance initialized with
nondeterministic faulty proposal, prevote, and precommit powersets.
`BaselineForkingFaultyInitSafety` checks the fixed-sigma/forking baseline
parameter surface with a bounded nondeterministic faulty-init domain.
`CrosslinkBaselineCounterexampleModel` adds negative tests showing that false
claims about conflicting commits, amnesia, equivocation, and agreement are
rejected by the harness.

The bounded upstream-shaped baseline checks report no violation for the
`n4_f1`, `n4_f2`, `n5_f2`, or `n7_f2` Rust-backed safety witnesses. The stable
`n4_f1` instance keeps the normal fixed-sigma decision path,
no-double-proposal witness, nil-prevote quorum path, and timeout-driven nil
vote/round-advance witnesses alive through the parameter shell. It also covers
a focused future-round catchup witness from `f + 1` future-round messages. The
forking `n4_f1` instance records that the shell derives the fresh round-1
`head - sigma` sample while the sticky baseline can still carry the stale
round-0 sample. The `f = 2` instances document that `n4_f2` cannot form even
correct-only catchup evidence, `n5_f2` can form f+1 catchup evidence but not a
2f+1 correct value quorum, and `n7_f2` retains the ordinary 2f+1 correct
decision path.

The bounded baseline BFT-height checks report no violation for
`BaselineBftHeightSafety`, which combines bounded consensus-height progression
with finalized-prefix safety. The named witnesses show consecutive BFT
decisions advancing finality, and reject skipped BFT heights or fork finality
after a prefix is final.

The bounded baseline-finality checks report no violation for `ComposedSafety`,
which combines the current fixed-sigma/sticky Tenderlink safety invariant with
the finalized-prefix invariant. The stable-stream liveness harness reaches an
`a2` decision and finality update by phase 9; the stream-change witness records
that the sticky baseline can carry `a1` into round 1 and leave finality at `g`
instead of finalizing the fresh stream value.

The bounded baseline PoW-sampling check reports no violation for
`BaselinePowSamplingSafety`, which combines the current fixed-sigma/sticky
Tenderlink safety invariant with an explicit `Stream(round) = head - sigma`
condition. The fork-switch witness records a rollback from `a4` to `b4` where
the round-0 fixed-sigma sample `a3` no longer survives, but the sticky baseline
still carries it into round 1 instead of sampling `b3`.

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
- source header timestamps derive a conservative block-interval variance input
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

The dynamic-sigma hysteresis harness reports no violation for `Safety`, which
combines:

- live sigma remains within the configured ladder
- live sigma remains at least the required floor for the current window
- higher required sigma applies immediately
- lower required sigma waits for stable windows and steps down one ladder level
  at a time

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
  path now runs the controller over a policy-guarded timed-header fixture, and
  the pure telemetry assembly boundary fails closed on missing participating-work
  evidence or inconsistent round counters. The event accumulator now gives live
  Tenderlink hooks an exact round-counter contract, the hash-work observation
  accumulator gives source producers an exact participation-numerator contract,
  the hash-work window policy bounds participation by minimum history, recent
  window size, and total observed work, the header adapter derives a
  work-weighted participation share from the current Crosslink fat-pointer
  marker, custom verifier hooks can replace that prototype marker with stricter
  production validation, the header
  observation-window assembler composes those headers with round and fork
  evidence into telemetry components, the timed header-window adapter derives
  conservative block-interval variance from adjacent header timestamps and can
  apply the same hash-work window policy in that timed path, the target-spacing
  helper derives expected spacing from the active network upgrade, and
  pure rollback-depth helpers record best-tip transitions, derive the current
  observed reorg-depth input, and produce one rollback-depth history sample per
  transition window from explicit best-tip transition evidence. The
  pure rollback-risk estimator derives a monotone ppm curve from observed
  rollback-depth windows plus a bounded margin, and the explicit window policy
  requires enough history while truncating to the most recent bounded sample
  window. The
  economic exposure policy now explicitly separates consensus-critical exposure
  from service-local risk, with proposal-evidence tests rejecting selected sigma
  below a consensus-critical economic floor. The pure evidence-selection
  helpers now construct proposal evidence from raw telemetry or assembled
  components, with optional hysteresis, and the prototype proposal path uses
  them. The pure hysteresis helper covers bounded sigma decreases for
  short-window stability, and the prototype evidence builder now applies an
  explicit hysteresis state before carrying `selected_sigma`. The proposal
  callback reuses that planned evidence for both depth selection and payload
  encoding, then advances the prototype service's in-process hysteresis state
  after successful encoding. The hysteresis policy/state now have deterministic
  Zcash serialization for a future durable or proposal-carried state source, and
  the selection helper now models disabled, durable-local, and proposal-carried
  source policies explicitly. The remaining work is replacing the fixture with
  consensus-safe or proposal-verifiable input producers and configuring the
  production hysteresis state source
- refine the split projection checks into smaller inductive lemmas if bounds
  beyond the checked depth-10 projections still need very large JVM/Z3 heaps
