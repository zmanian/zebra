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
locks, not with a commit-capable value-lock quorum.

`CrosslinkForkFinality.qnt` is a separate value-semantics model. It abstracts
PoW snapshots as a finite fork tree, then checks that Crosslink finality can skip
heights on one branch while rejecting finalization of a fork after a block is
final.

`CrosslinkComposed.qnt` connects those two pieces: a Tenderlink decision over a
resampled PoW snapshot becomes the input to Crosslink finality, which can then
advance to a tail-confirmed snapshot while preserving the finalized prefix.

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
previous BFT round did not fail.

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

Typecheck:

```sh
$QUINT typecheck spec/quint/CrosslinkResampling.qnt
$QUINT typecheck spec/quint/CrosslinkForkFinality.qnt
$QUINT typecheck spec/quint/CrosslinkComposed.qnt
$QUINT typecheck spec/quint/CrosslinkDynamicSigma.qnt
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

- two conflicting value commits across rounds expose an invalid unlock
  transition by at least one correct validator
- a bogus nil certificate that coexists with a same-round value commit exposes
  correct-validator nil/value equivocation
- a valid same-round nil certificate justifies switching away from a minority
  same-round value lock
- a later nil certificate does not justify abandoning an older value lock

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
floor.

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

$QUINT run spec/quint/CrosslinkDynamicSigma.qnt \
  --main=CrosslinkDynamicSigmaHashParticipationModel \
  --init=Init \
  --step=Next \
  --max-steps=7 \
  --max-samples=1000 \
  --invariant=Safety \
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

$QUINT verify spec/quint/CrosslinkDynamicSigma.qnt \
  --main=CrosslinkDynamicSigmaHashParticipationModel \
  --max-steps=7 \
  --init=Init \
  --step=Next \
  --invariant=Safety
```

The bounded resampling checks currently report no violation for `Safety`, which
combines:

- agreement on decided values
- no correct-validator precommit equivocation
- no same-round quorum for both nil and a concrete value
- any retained Tendermint lock keeps its matching valid value across
  nil-precommit round recovery
- any retained Tendermint lock is backed by value-precommit evidence
- a nil certificate leaves at most `f` correct same-round value locks, so
  same-round unlock is not discarding a commit-capable value-lock quorum
- every pair of conflicting value commit quorums has accountability evidence:
  either correct-validator precommit equivocation, nil/value equivocation in the
  purported unlock round, or a correct-validator value switch without a valid
  same-round nil unlock certificate

The bounded fork-finality check reports no violation for its `Safety`, which
combines:

- finalized snapshots are prefix-linear
- the latest final snapshot extends every previously finalized snapshot
- the initial final snapshot remains finalized

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

The dynamic-sigma harness checks a bounded controller invariant: live sigma
remains within the configured ladder, never falls below the floor implied by
observed hash-power participation or observed reorg depth, tracks the
`head - sigma` snapshot selected for the current round, and uses a monotone
participation floor where lower participation cannot require a lower sigma than
higher participation. This is still a controller sketch, not a calibrated
stochastic model; it does not yet derive the thresholds from measured hashrate
coverage, block interval variance, or reorg distributions.

## Next Extensions

This model is intentionally narrow. The next useful extensions are:

- replace the concrete fork and `head - sigma` fixtures with parameterized PoW
  chains and adversarial reorg schedules
- refine `CrosslinkDynamicSigma.qnt` with a calibrated stochastic controller
  that uses measured hash-power participation, round-failure rate, block
  interval variance, and observed reorg depth rather than the current three-step
  sigma ladder
- turn the observed reorg-depth schedule into generated PoW fork transitions,
  so rollback depth is derived from branch competition rather than supplied as
  a controller input
- add BFT heights so successive Tenderlink decisions update Crosslink finality
  directly
- port the full upstream Tendermint accountability evidence model into the
  composed model; the current resampling model only adds the conflict/evidence
  witnesses needed for nil-precommit unlocks
