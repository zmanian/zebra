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

   The calibrated controller sketch adds a bounded risk score over the signals
   a deployed controller would estimate: hash-power coverage, recent round
   failure rate, block-interval variance, and observed rollback depth. The
   thresholds here are fixtures, not production economics, but the model makes
   the intended monotone shape explicit.
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
  const RoundFailureRatePct: Round_t -> Pct_t
  const BlockIntervalVariancePct: Round_t -> Pct_t
  const ObservedReorgDepth: Round_t -> int
  const CoverageRiskWeight: int
  const RoundFailureRiskWeight: int
  const BlockVarianceRiskWeight: int
  const ReorgDepthRiskWeight: int
  const RiskScoreRaisedThreshold: int
  const RiskScoreMaxThreshold: int
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
  assume observed_round_failure_rate_is_percentage =
    Rounds.forall(r =>
      0 <= RoundFailureRatePct.get(r) and RoundFailureRatePct.get(r) <= 100
    )
  assume observed_block_variance_is_percentage =
    Rounds.forall(r =>
      0 <= BlockIntervalVariancePct.get(r) and BlockIntervalVariancePct.get(r) <= 100
    )
  assume observed_reorg_depth_is_bounded =
    Rounds.forall(r =>
      0 <= ObservedReorgDepth.get(r) and ObservedReorgDepth.get(r) <= MaxSigma
    )
  assume risk_weights_are_non_negative =
    CoverageRiskWeight >= 0 and
    RoundFailureRiskWeight >= 0 and
    BlockVarianceRiskWeight >= 0 and
    ReorgDepthRiskWeight >= 0
  assume risk_score_thresholds_are_ordered =
    0 <= RiskScoreRaisedThreshold and RiskScoreRaisedThreshold < RiskScoreMaxThreshold
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
  var riskScore: int
  var riskStatus: str
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

  pure def ReorgSigmaFloor(depth: int): Sigma_t =
    if (depth >= RaisedSigma) {
      MaxSigma
    } else if (depth >= BaseSigma) {
      RaisedSigma
    } else {
      BaseSigma
    }

  pure def CoverageRiskPct(hashParticipationPct: Pct_t): int =
    100 - hashParticipationPct

  pure def CalibratedRiskScore(
    hashParticipationPct: Pct_t,
    roundFailureRatePct: Pct_t,
    blockIntervalVariancePct: Pct_t,
    observedReorgDepth: int
  ): int =
    CoverageRiskWeight * CoverageRiskPct(hashParticipationPct) +
      RoundFailureRiskWeight * roundFailureRatePct +
      BlockVarianceRiskWeight * blockIntervalVariancePct +
      ReorgDepthRiskWeight * observedReorgDepth

  pure def RiskScoreSigmaFloor(score: int): Sigma_t =
    if (score >= RiskScoreMaxThreshold) {
      MaxSigma
    } else if (score >= RiskScoreRaisedThreshold) {
      RaisedSigma
    } else {
      BaseSigma
    }

  pure def CalibratedRiskScoreForRound(r: Round_t): int =
    CalibratedRiskScore(
      HashParticipationPct.get(r),
      RoundFailureRatePct.get(r),
      BlockIntervalVariancePct.get(r),
      ObservedReorgDepth.get(r)
    )

  pure def CalibratedSigmaFloorForRound(r: Round_t): Sigma_t =
    MaxInt(
      RiskScoreSigmaFloor(CalibratedRiskScoreForRound(r)),
      MaxInt(
        HashParticipationSigmaFloor(HashParticipationPct.get(r)),
        ReorgSigmaFloor(ObservedReorgDepth.get(r))
      )
    )

  pure def EscalatedSigma(current: Sigma_t): Sigma_t =
    if (current < RaisedSigma) {
      RaisedSigma
    } else {
      MaxSigma
    }

  pure def ControllerSigmaWithReorg(
    previousSigma: Sigma_t,
    nextHashParticipationPct: Pct_t,
    previousRoundFailed: bool,
    nextObservedReorgDepth: int
  ): Sigma_t =
    MaxInt(
      ReorgSigmaFloor(nextObservedReorgDepth),
      MaxInt(
        HashParticipationSigmaFloor(nextHashParticipationPct),
        if (previousRoundFailed) { EscalatedSigma(previousSigma) } else { BaseSigma }
      )
    )

  pure def ControllerSigma(
    previousSigma: Sigma_t,
    nextHashParticipationPct: Pct_t,
    previousRoundFailed: bool
  ): Sigma_t =
    ControllerSigmaWithReorg(previousSigma, nextHashParticipationPct, previousRoundFailed, 0)

  pure def CalibratedControllerSigma(
    previousSigma: Sigma_t,
    nextRound: Round_t,
    previousRoundFailed: bool
  ): Sigma_t =
    MaxInt(
      CalibratedSigmaFloorForRound(nextRound),
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

  pure def RiskStatusFor(score: int): str =
    if (score >= RiskScoreMaxThreshold) {
      "critical-stochastic-risk"
    } else if (score >= RiskScoreRaisedThreshold) {
      "elevated-stochastic-risk"
    } else {
      "low-stochastic-risk"
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

  action Init = {
    val initialRiskScore = CalibratedRiskScoreForRound(0)
    val initialSigma = CalibratedSigmaFloorForRound(0)
    all {
      round' = 0,
      sigma' = initialSigma,
      sampledSnapshot' = HeadMinusSigma(0, initialSigma),
      sampleStability' = SampleStabilityStatusFor(0, initialSigma),
      controllerStatus' = ControllerStatusFor(HashParticipationPct.get(0)),
      riskScore' = initialRiskScore,
      riskStatus' = RiskStatusFor(initialRiskScore),
      firedAction' = "Init",
    }
  }

  action AdvanceRound = {
    val oldRound = round
    val nextRound = oldRound + 1
    val nextHashParticipation = HashParticipationPct.get(nextRound)
    val nextReorgDepth = ObservedReorgDepth.get(nextRound)
    val nextRiskScore = CalibratedRiskScoreForRound(nextRound)
    val nextSigma = CalibratedControllerSigma(sigma, nextRound, RoundFailed.get(oldRound))
    all {
      nextRound.in(Rounds),
      round' = nextRound,
      sigma' = nextSigma,
      sampledSnapshot' = HeadMinusSigma(nextRound, nextSigma),
      sampleStability' = SampleStabilityStatusFor(nextRound, nextSigma),
      controllerStatus' = ControllerStatusFor(nextHashParticipation),
      riskScore' = nextRiskScore,
      riskStatus' = RiskStatusFor(nextRiskScore),
      firedAction' =
        if (RoundFailed.get(oldRound)) {
          "AdvanceAfterRoundFailure"
        } else if (nextReorgDepth >= BaseSigma) {
          "AdvanceAfterObservedReorg"
        } else if (nextHashParticipation < TargetHashParticipationPct) {
          "AdvanceWithLowHashParticipation"
        } else if (RiskScoreSigmaFloor(nextRiskScore) > BaseSigma) {
          "AdvanceWithElevatedStochasticRisk"
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
    riskScore' = riskScore,
    riskStatus' = riskStatus,
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

  val SigmaRespectsObservedReorgFloor =
    sigma >= ReorgSigmaFloor(ObservedReorgDepth.get(round))

  val SigmaRespectsCalibratedRiskFloor =
    sigma >= RiskScoreSigmaFloor(CalibratedRiskScoreForRound(round))

  val RiskScoreMatchesObservedInputs =
    riskScore == CalibratedRiskScoreForRound(round)

  val RiskStatusMatchesObservedInputs =
    riskStatus == RiskStatusFor(CalibratedRiskScoreForRound(round))

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

  val CalibratedRiskScoreIsMonotoneInParticipation =
    Percentages.forall(lower =>
      Percentages.forall(higher =>
        (lower <= higher) implies
          CalibratedRiskScore(lower, 0, 0, 0) >= CalibratedRiskScore(higher, 0, 0, 0)
      )
    )

  val ControllerStatusMatchesHashParticipation =
    controllerStatus == ControllerStatusFor(HashParticipationPct.get(round))

  val Safety =
    SigmaWithinBounds and
    SigmaRespectsHashParticipationFloor and
    SigmaRespectsObservedReorgFloor and
    SigmaRespectsCalibratedRiskFloor and
    RiskScoreMatchesObservedInputs and
    RiskStatusMatchesObservedInputs and
    SampledSnapshotIsValid and
    SampledSnapshotMatchesSigma and
    SampleStabilityMatchesSigma and
    HashParticipationFloorIsMonotone and
    CalibratedRiskScoreIsMonotoneInParticipation and
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

  run adversarialReorgDepthRaisesSigmaFloorTest = all {
    assert(ReorgSigmaFloor(BaseSigma) == RaisedSigma),
    assert(ReorgSigmaFloor(RaisedSigma) == MaxSigma),
  }

  run hashParticipationSigmaFloorIsMonotoneTest = {
    assert(HashParticipationFloorIsMonotone)
  }

  run calibratedRiskScoreIsMonotoneInParticipationTest = {
    assert(CalibratedRiskScoreIsMonotoneInParticipation)
  }

  run combinedStochasticRiskRaisesSigmaWithoutSingleHardSignalTest = {
    assert(RiskScoreSigmaFloor(CalibratedRiskScore(75, 30, 30, 0)) == RaisedSigma)
  }

  run criticalStochasticRiskForcesMaxSigmaTest = {
    assert(RiskScoreSigmaFloor(CalibratedRiskScore(60, 60, 60, 1)) == MaxSigma)
  }

  run blockIntervalVarianceCanRaiseSigmaTest = {
    assert(RiskScoreSigmaFloor(CalibratedRiskScore(90, 0, 90, 0)) == RaisedSigma)
  }

  run raisedSigmaCanStabilizeMovingPowSampleTest = all {
    assert(not(SampleStableAcross(0, 1, BaseSigma))),
    assert(SampleStableAcross(0, 1, RaisedSigma)),
  }

  run observedReorgRaisesLiveSigmaTest = {
    Init
      .then(AdvanceRound)
      .then(AdvanceRound)
      .then(all {
        assert(sigma == RaisedSigma),
        assert(firedAction == "AdvanceAfterObservedReorg"),
        assert(controllerStatus == "healthy-hash-participation"),
        assert(riskStatus == "low-stochastic-risk"),
        assert(sampledSnapshot == "deep-a"),
        assert(sampleStability == "stable-under-current-sigma"),
        assert(Safety),
        Stutter,
      })
  }

  run observedLowHashParticipationRaisesLiveSigmaTest = {
    Init
      .then(AdvanceRound)
      .then(AdvanceRound)
      .then(AdvanceRound)
      .then(all {
        assert(sigma == RaisedSigma),
        assert(firedAction == "AdvanceWithLowHashParticipation"),
        assert(controllerStatus == "degraded-hash-participation"),
        assert(riskStatus == "low-stochastic-risk"),
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
      .then(AdvanceRound)
      .then(all {
        assert(sigma == MaxSigma),
        assert(firedAction == "AdvanceWithLowHashParticipation"),
        assert(controllerStatus == "critical-hash-participation"),
        assert(riskStatus == "low-stochastic-risk"),
        assert(sampledSnapshot == "final-a"),
        assert(sampleStability == "stable-under-current-sigma"),
        assert(Safety),
        Stutter,
      })
  }
}

module CrosslinkDynamicSigmaHashParticipationModel {
  import CrosslinkDynamicSigmaTest(
    MaxRound = 4,
    BaseSigma = 1,
    RaisedSigma = 3,
    MaxSigma = 6,
    TargetHashParticipationPct = 67,
    CriticalHashParticipationPct = 50,
    HashParticipationPct = Map(
      0 -> 90,
      1 -> 90,
      2 -> 90,
      3 -> 66,
      4 -> 45,
    ),
    RoundFailed = Map(
      0 -> true,
      1 -> false,
      2 -> false,
      3 -> false,
      4 -> false,
    ),
    RoundFailureRatePct = Map(
      0 -> 0,
      1 -> 0,
      2 -> 0,
      3 -> 0,
      4 -> 0,
    ),
    BlockIntervalVariancePct = Map(
      0 -> 0,
      1 -> 0,
      2 -> 0,
      3 -> 0,
      4 -> 0,
    ),
    ObservedReorgDepth = Map(
      0 -> 0,
      1 -> 0,
      2 -> 1,
      3 -> 0,
      4 -> 0,
    ),
    CoverageRiskWeight = 1,
    RoundFailureRiskWeight = 1,
    BlockVarianceRiskWeight = 1,
    ReorgDepthRiskWeight = 40,
    RiskScoreRaisedThreshold = 80,
    RiskScoreMaxThreshold = 160,
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
      4 -> Map(
        1 -> "tip-d3",
        3 -> "deep-a",
        6 -> "final-a",
      ),
    ),
  ).*
}
