// -*- mode: Bluespec; -*-
module CrosslinkDynamicSigma {
  /*
   A focused model of the third Crosslink variant: dynamic sigma.

   The resampling model handles a moving PoW stream by treating a 2f + 1
   PRECOMMIT nil certificate as a same-round unlock certificate. This model
   isolates one input to the dynamic-sigma controller: the percentage of total
   PoW hash power that is participating in Crosslink.

   Hash-power participation is not a Tendermint voting threshold. It is a
   coverage signal for the PoW stream that finalizers see. If Crosslink only
   observes or is respected by a small fraction of hash power, a nonparticipating
   branch has a better chance of becoming the eventual longest chain, so sigma
   must have a higher floor. Below a critical participation floor, increasing
   sigma improves conservatism but does not by itself recover the same assurance
   as broad PoW participation.
   */

  type Round_t = int
  type Sigma_t = int
  type Pct_t = int
  type Snapshot_t = str

  const MaxRound: Round_t
  const BaseSigma: Sigma_t
  const RaisedSigma: Sigma_t
  const MaxSigma: Sigma_t
  const TargetHashParticipationPct: Pct_t
  const CriticalHashParticipationPct: Pct_t
  const HashParticipationPct: Round_t -> Pct_t
  const RoundFailed: Round_t -> bool
  const Snapshots: Set[Snapshot_t]
  const SnapshotAtSigma: Round_t -> Sigma_t -> Snapshot_t

  pure val Rounds = 0.to(MaxRound)
  pure val Percentages = 0.to(100)
  pure val SigmaLadder = Set(BaseSigma, RaisedSigma, MaxSigma)

  assume max_round_non_negative =
    MaxRound >= 0
  assume sigma_ladder_is_ordered =
    1 <= BaseSigma and BaseSigma < RaisedSigma and RaisedSigma < MaxSigma
  assume participation_thresholds_are_ordered =
    0 <= CriticalHashParticipationPct and
    CriticalHashParticipationPct < TargetHashParticipationPct and
    TargetHashParticipationPct <= 100
  assume observed_participation_is_percentage =
    Rounds.forall(r =>
      0 <= HashParticipationPct.get(r) and HashParticipationPct.get(r) <= 100
    )
  assume sampled_snapshots_are_valid =
    Rounds.forall(r =>
      SigmaLadder.forall(s =>
        Snapshots.contains(SnapshotAtSigma.get(r).get(s))
      )
    )

  var round: Round_t
  var sigma: Sigma_t
  var sampledSnapshot: Snapshot_t
  var sampleStability: str
  var controllerStatus: str
  var firedAction: str

  pure def MaxInt(a: int, b: int): int =
    if (a >= b) { a } else { b }

  pure def HashParticipationSigmaFloor(pct: Pct_t): Sigma_t =
    if (pct < CriticalHashParticipationPct) {
      MaxSigma
    } else if (pct < TargetHashParticipationPct) {
      RaisedSigma
    } else {
      BaseSigma
    }

  pure def EscalatedSigma(current: Sigma_t): Sigma_t =
    if (current < RaisedSigma) {
      RaisedSigma
    } else {
      MaxSigma
    }

  pure def ControllerSigma(
    previousSigma: Sigma_t,
    nextHashParticipationPct: Pct_t,
    previousRoundFailed: bool
  ): Sigma_t =
    MaxInt(
      HashParticipationSigmaFloor(nextHashParticipationPct),
      if (previousRoundFailed) { EscalatedSigma(previousSigma) } else { BaseSigma }
    )

  pure def ControllerStatusFor(pct: Pct_t): str =
    if (pct < CriticalHashParticipationPct) {
      "critical-hash-participation"
    } else if (pct < TargetHashParticipationPct) {
      "degraded-hash-participation"
    } else {
      "healthy-hash-participation"
    }

  pure def HeadMinusSigma(r: Round_t, s: Sigma_t): Snapshot_t =
    SnapshotAtSigma.get(r).get(s)

  pure def SampleStableAcross(previousRound: Round_t, nextRound: Round_t, s: Sigma_t): bool =
    HeadMinusSigma(previousRound, s) == HeadMinusSigma(nextRound, s)

  pure def SampleStabilityStatusFor(r: Round_t, s: Sigma_t): str =
    if (r == 0) {
      "initial-sample"
    } else if (SampleStableAcross(r - 1, r, s)) {
      "stable-under-current-sigma"
    } else {
      "unstable-under-current-sigma"
    }

  action Init = all {
    round' = 0,
    sigma' = HashParticipationSigmaFloor(HashParticipationPct.get(0)),
    sampledSnapshot' = HeadMinusSigma(0, HashParticipationSigmaFloor(HashParticipationPct.get(0))),
    sampleStability' = SampleStabilityStatusFor(0, HashParticipationSigmaFloor(HashParticipationPct.get(0))),
    controllerStatus' = ControllerStatusFor(HashParticipationPct.get(0)),
    firedAction' = "Init",
  }

  action AdvanceRound = {
    val oldRound = round
    val nextRound = oldRound + 1
    val nextHashParticipation = HashParticipationPct.get(nextRound)
    val nextSigma = ControllerSigma(
      sigma,
      nextHashParticipation,
      RoundFailed.get(oldRound)
    )
    all {
      nextRound.in(Rounds),
      round' = nextRound,
      sigma' = nextSigma,
      sampledSnapshot' = HeadMinusSigma(nextRound, nextSigma),
      sampleStability' = SampleStabilityStatusFor(nextRound, nextSigma),
      controllerStatus' = ControllerStatusFor(nextHashParticipation),
      firedAction' =
        if (RoundFailed.get(oldRound)) {
          "AdvanceAfterRoundFailure"
        } else if (nextHashParticipation < TargetHashParticipationPct) {
          "AdvanceWithLowHashParticipation"
        } else {
          "AdvanceStable"
        },
    }
  }

  action Stutter = all {
    round' = round,
    sigma' = sigma,
    sampledSnapshot' = sampledSnapshot,
    sampleStability' = sampleStability,
    controllerStatus' = controllerStatus,
    firedAction' = firedAction,
  }

  action Next =
    any {
      AdvanceRound,
      Stutter,
    }

  val SigmaWithinBounds =
    BaseSigma <= sigma and sigma <= MaxSigma

  val SigmaRespectsHashParticipationFloor =
    sigma >= HashParticipationSigmaFloor(HashParticipationPct.get(round))

  val SampledSnapshotIsValid =
    Snapshots.contains(sampledSnapshot)

  val SampledSnapshotMatchesSigma =
    sampledSnapshot == HeadMinusSigma(round, sigma)

  val SampleStabilityMatchesSigma =
    sampleStability == SampleStabilityStatusFor(round, sigma)

  val HashParticipationFloorIsMonotone =
    Percentages.forall(lower =>
      Percentages.forall(higher =>
        (lower <= higher) implies
          HashParticipationSigmaFloor(lower) >= HashParticipationSigmaFloor(higher)
      )
    )

  val ControllerStatusMatchesHashParticipation =
    controllerStatus == ControllerStatusFor(HashParticipationPct.get(round))

  val Safety =
    SigmaWithinBounds and
    SigmaRespectsHashParticipationFloor and
    SampledSnapshotIsValid and
    SampledSnapshotMatchesSigma and
    SampleStabilityMatchesSigma and
    HashParticipationFloorIsMonotone and
    ControllerStatusMatchesHashParticipation
}

module CrosslinkDynamicSigmaTest {
  import CrosslinkDynamicSigma.* from "./CrosslinkDynamicSigma"
  export CrosslinkDynamicSigma.*

  run highHashParticipationStartsAtBaseSigmaTest = {
    Init.then(all {
      assert(sigma == BaseSigma),
      assert(controllerStatus == "healthy-hash-participation"),
      assert(Safety),
      Stutter,
    })
  }

  run roundFailureEscalatesSigmaEvenWithHighHashParticipationTest = {
    Init
      .then(AdvanceRound)
      .then(all {
        assert(sigma == RaisedSigma),
        assert(firedAction == "AdvanceAfterRoundFailure"),
        assert(sampledSnapshot == "deep-a"),
        assert(sampleStability == "stable-under-current-sigma"),
        assert(Safety),
        Stutter,
      })
  }

  run lowHashParticipationRaisesSigmaFloorWithoutRoundFailureTest = {
    assert(ControllerSigma(BaseSigma, TargetHashParticipationPct - 1, false) == RaisedSigma)
  }

  run criticalHashParticipationForcesMaxSigmaTest = {
    assert(ControllerSigma(BaseSigma, CriticalHashParticipationPct - 1, false) == MaxSigma)
  }

  run hashParticipationSigmaFloorIsMonotoneTest = {
    assert(HashParticipationFloorIsMonotone)
  }

  run raisedSigmaCanStabilizeMovingPowSampleTest = all {
    assert(not(SampleStableAcross(0, 1, BaseSigma))),
    assert(SampleStableAcross(0, 1, RaisedSigma)),
  }

  run observedLowHashParticipationRaisesLiveSigmaTest = {
    Init
      .then(AdvanceRound)
      .then(AdvanceRound)
      .then(all {
        assert(sigma == RaisedSigma),
        assert(firedAction == "AdvanceWithLowHashParticipation"),
        assert(controllerStatus == "degraded-hash-participation"),
        assert(sampledSnapshot == "deep-a"),
        assert(sampleStability == "stable-under-current-sigma"),
        assert(Safety),
        Stutter,
      })
  }

  run observedCriticalHashParticipationForcesLiveMaxSigmaTest = {
    Init
      .then(AdvanceRound)
      .then(AdvanceRound)
      .then(AdvanceRound)
      .then(all {
        assert(sigma == MaxSigma),
        assert(firedAction == "AdvanceWithLowHashParticipation"),
        assert(controllerStatus == "critical-hash-participation"),
        assert(sampledSnapshot == "final-a"),
        assert(sampleStability == "stable-under-current-sigma"),
        assert(Safety),
        Stutter,
      })
  }
}

module CrosslinkDynamicSigmaHashParticipationModel {
  import CrosslinkDynamicSigmaTest(
    MaxRound = 3,
    BaseSigma = 1,
    RaisedSigma = 3,
    MaxSigma = 6,
    TargetHashParticipationPct = 67,
    CriticalHashParticipationPct = 50,
    HashParticipationPct = Map(
      0 -> 90,
      1 -> 90,
      2 -> 66,
      3 -> 45,
    ),
    RoundFailed = Map(
      0 -> true,
      1 -> false,
      2 -> false,
      3 -> false,
    ),
    Snapshots = Set("tip-a0", "tip-b1", "tip-c2", "tip-d3", "deep-a", "final-a"),
    SnapshotAtSigma = Map(
      0 -> Map(
        1 -> "tip-a0",
        3 -> "deep-a",
        6 -> "final-a",
      ),
      1 -> Map(
        1 -> "tip-b1",
        3 -> "deep-a",
        6 -> "final-a",
      ),
      2 -> Map(
        1 -> "tip-c2",
        3 -> "deep-a",
        6 -> "final-a",
      ),
      3 -> Map(
        1 -> "tip-d3",
        3 -> "deep-a",
        6 -> "final-a",
      ),
    ),
  ).*
}
