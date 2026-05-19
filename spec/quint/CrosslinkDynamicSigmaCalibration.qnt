// -*- mode: Bluespec; -*-
module CrosslinkDynamicSigmaCalibration {
  /*
   Calibration harness for the dynamic-sigma controller.

   The controller consumes four measured signals:

   - percentage of total PoW hash power participating in Crosslink
   - recent Tenderlink round-failure rate
   - recent PoW block-interval variance
   - observed rollback/reorg depth

   This model does not claim production economics. It captures the calibration
   contract: for a bounded measurement window, the selected weights and risk
   thresholds must classify each observed window into the expected sigma floor.
   */

  type Observation_t = int
  type Pct_t = int
  type Sigma_t = int

  const MaxObservation: Observation_t
  const BaseSigma: Sigma_t
  const RaisedSigma: Sigma_t
  const MaxSigma: Sigma_t
  const TargetHashParticipationPct: Pct_t
  const CriticalHashParticipationPct: Pct_t
  const MeasuredHashParticipationPct: Observation_t -> Pct_t
  const MeasuredRoundFailureRatePct: Observation_t -> Pct_t
  const MeasuredBlockIntervalVariancePct: Observation_t -> Pct_t
  const MeasuredObservedReorgDepth: Observation_t -> int
  const ExpectedMeasuredSigmaFloor: Observation_t -> Sigma_t
  const CoverageRiskWeight: int
  const RoundFailureRiskWeight: int
  const BlockVarianceRiskWeight: int
  const ReorgDepthRiskWeight: int
  const RiskScoreRaisedThreshold: int
  const RiskScoreMaxThreshold: int

  pure val Observations = 0.to(MaxObservation)
  pure val Percentages = 0.to(100)
  pure val SigmaLadder = Set(BaseSigma, RaisedSigma, MaxSigma)

  var observation: Observation_t

  assume max_observation_non_negative =
    MaxObservation >= 0
  assume sigma_ladder_is_ordered =
    1 <= BaseSigma and BaseSigma < RaisedSigma and RaisedSigma < MaxSigma
  assume participation_thresholds_are_ordered =
    0 <= CriticalHashParticipationPct and
    CriticalHashParticipationPct < TargetHashParticipationPct and
    TargetHashParticipationPct <= 100
  assume measured_hash_participation_is_percentage =
    Observations.forall(o =>
      0 <= MeasuredHashParticipationPct.get(o) and MeasuredHashParticipationPct.get(o) <= 100
    )
  assume measured_round_failure_rate_is_percentage =
    Observations.forall(o =>
      0 <= MeasuredRoundFailureRatePct.get(o) and MeasuredRoundFailureRatePct.get(o) <= 100
    )
  assume measured_block_variance_is_percentage =
    Observations.forall(o =>
      0 <= MeasuredBlockIntervalVariancePct.get(o) and
        MeasuredBlockIntervalVariancePct.get(o) <= 100
    )
  assume measured_reorg_depth_is_bounded =
    Observations.forall(o =>
      0 <= MeasuredObservedReorgDepth.get(o) and
        MeasuredObservedReorgDepth.get(o) <= MaxSigma
    )
  assume measured_expected_floors_are_in_ladder =
    Observations.forall(o =>
      SigmaLadder.contains(ExpectedMeasuredSigmaFloor.get(o))
    )
  assume risk_weights_are_material =
    CoverageRiskWeight > 0 and
    RoundFailureRiskWeight > 0 and
    BlockVarianceRiskWeight > 0 and
    ReorgDepthRiskWeight > 0
  assume risk_score_thresholds_are_ordered =
    0 <= RiskScoreRaisedThreshold and RiskScoreRaisedThreshold < RiskScoreMaxThreshold

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

  pure def CalibratedRiskScoreForObservation(o: Observation_t): int =
    CalibratedRiskScore(
      MeasuredHashParticipationPct.get(o),
      MeasuredRoundFailureRatePct.get(o),
      MeasuredBlockIntervalVariancePct.get(o),
      MeasuredObservedReorgDepth.get(o)
    )

  pure def CalibratedSigmaFloorForObservation(o: Observation_t): Sigma_t =
    MaxInt(
      RiskScoreSigmaFloor(CalibratedRiskScoreForObservation(o)),
      MaxInt(
        HashParticipationSigmaFloor(MeasuredHashParticipationPct.get(o)),
        ReorgSigmaFloor(MeasuredObservedReorgDepth.get(o))
      )
    )

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

  val RoundFailureWeightIsMaterial =
    CalibratedRiskScore(90, 10, 0, 0) > CalibratedRiskScore(90, 0, 0, 0)

  val BlockVarianceWeightIsMaterial =
    CalibratedRiskScore(90, 0, 10, 0) > CalibratedRiskScore(90, 0, 0, 0)

  val ReorgDepthWeightIsMaterial =
    CalibratedRiskScore(90, 0, 0, 1) > CalibratedRiskScore(90, 0, 0, 0)

  val CalibrationMatchesMeasuredSigmaLabels =
    Observations.forall(o =>
      CalibratedSigmaFloorForObservation(o) == ExpectedMeasuredSigmaFloor.get(o)
    )

  val CalibrationSafety =
    HashParticipationFloorIsMonotone and
    CalibratedRiskScoreIsMonotoneInParticipation and
    RoundFailureWeightIsMaterial and
    BlockVarianceWeightIsMaterial and
    ReorgDepthWeightIsMaterial and
    CalibrationMatchesMeasuredSigmaLabels

  val CurrentObservationMatchesCalibration =
    CalibratedSigmaFloorForObservation(observation) == ExpectedMeasuredSigmaFloor.get(observation)

  action Init = all {
    observation' = 0,
  }

  action AdvanceObservation = {
    val nextObservation = observation + 1
    all {
      nextObservation.in(Observations),
      observation' = nextObservation,
    }
  }

  action Stutter = all {
    observation' = observation,
  }

  action Next =
    any {
      AdvanceObservation,
      Stutter,
    }

  val Safety =
    observation.in(Observations) and
    CalibrationSafety and
    CurrentObservationMatchesCalibration
}

module CrosslinkDynamicSigmaCalibrationTest {
  import CrosslinkDynamicSigmaCalibration.* from "./CrosslinkDynamicSigmaCalibration"
  export CrosslinkDynamicSigmaCalibration.*

  run healthyMeasuredWindowKeepsBaseSigmaTest = all {
    assert(CalibratedRiskScoreForObservation(0) < RiskScoreRaisedThreshold),
    assert(CalibratedSigmaFloorForObservation(0) == BaseSigma),
  }

  run marginalHashParticipationRaisesSigmaTest = all {
    assert(MeasuredHashParticipationPct.get(1) < TargetHashParticipationPct),
    assert(MeasuredHashParticipationPct.get(1) >= CriticalHashParticipationPct),
    assert(CalibratedSigmaFloorForObservation(1) == RaisedSigma),
  }

  run combinedMeasuredRiskRaisesSigmaTest = all {
    assert(MeasuredHashParticipationPct.get(2) >= TargetHashParticipationPct),
    assert(MeasuredObservedReorgDepth.get(2) < BaseSigma),
    assert(CalibratedRiskScoreForObservation(2) >= RiskScoreRaisedThreshold),
    assert(CalibratedRiskScoreForObservation(2) < RiskScoreMaxThreshold),
    assert(CalibratedSigmaFloorForObservation(2) == RaisedSigma),
  }

  run deepReorgMeasuredWindowForcesMaxSigmaTest = all {
    assert(MeasuredObservedReorgDepth.get(3) >= RaisedSigma),
    assert(CalibratedSigmaFloorForObservation(3) == MaxSigma),
  }

  run criticalParticipationMeasuredWindowForcesMaxSigmaTest = all {
    assert(MeasuredHashParticipationPct.get(4) < CriticalHashParticipationPct),
    assert(CalibratedSigmaFloorForObservation(4) == MaxSigma),
  }

  run criticalCombinedRiskForcesMaxSigmaTest = all {
    assert(MeasuredHashParticipationPct.get(5) >= CriticalHashParticipationPct),
    assert(MeasuredObservedReorgDepth.get(5) < RaisedSigma),
    assert(CalibratedRiskScoreForObservation(5) >= RiskScoreMaxThreshold),
    assert(CalibratedSigmaFloorForObservation(5) == MaxSigma),
  }

  run calibrationMatchesAllMeasuredWindowsTest = {
    assert(CalibrationSafety)
  }
}

module CrosslinkDynamicSigmaCalibrationModel {
  import CrosslinkDynamicSigmaCalibrationTest(
    MaxObservation = 5,
    BaseSigma = 1,
    RaisedSigma = 3,
    MaxSigma = 6,
    TargetHashParticipationPct = 67,
    CriticalHashParticipationPct = 50,
    MeasuredHashParticipationPct = Map(
      0 -> 92,
      1 -> 63,
      2 -> 75,
      3 -> 90,
      4 -> 45,
      5 -> 60,
    ),
    MeasuredRoundFailureRatePct = Map(
      0 -> 0,
      1 -> 0,
      2 -> 15,
      3 -> 0,
      4 -> 0,
      5 -> 30,
    ),
    MeasuredBlockIntervalVariancePct = Map(
      0 -> 5,
      1 -> 10,
      2 -> 15,
      3 -> 5,
      4 -> 0,
      5 -> 30,
    ),
    MeasuredObservedReorgDepth = Map(
      0 -> 0,
      1 -> 0,
      2 -> 0,
      3 -> 3,
      4 -> 0,
      5 -> 0,
    ),
    ExpectedMeasuredSigmaFloor = Map(
      0 -> 1,
      1 -> 3,
      2 -> 3,
      3 -> 6,
      4 -> 6,
      5 -> 6,
    ),
    CoverageRiskWeight = 2,
    RoundFailureRiskWeight = 3,
    BlockVarianceRiskWeight = 2,
    ReorgDepthRiskWeight = 60,
    RiskScoreRaisedThreshold = 100,
    RiskScoreMaxThreshold = 220,
  ).*
}
