// -*- mode: Bluespec; -*-
module CrosslinkDynamicSigmaTelemetry {
  /*
   Production-shaped telemetry contract for the dynamic-sigma controller.

   The calibration harness classifies bounded measurement windows from supplied
   percentages. This model adds the contract a deployed controller should
   satisfy before those percentages are trusted:

   - hash-power participation is derived from Crosslink-participating work over
     total observed PoW work
   - rate estimates conservatively upper-bound raw coverage and round-failure
     samples
   - selected sigma meets explicit rollback-probability and expected-loss
     targets whenever the bounded sigma ladder can meet them
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
  const TotalHashWork: Observation_t -> int
  const CrosslinkParticipatingHashWork: Observation_t -> int
  const TotalTenderlinkRounds: Observation_t -> int
  const FailedTenderlinkRounds: Observation_t -> int
  const EstimatedCoverageRiskPct: Observation_t -> Pct_t
  const EstimatedRoundFailureRatePct: Observation_t -> Pct_t
  const MeasuredBlockIntervalVariancePct: Observation_t -> Pct_t
  const MeasuredObservedReorgDepth: Observation_t -> int
  const RollbackRiskPpmAtSigma: Observation_t -> Sigma_t -> int
  const MaxAcceptableRollbackRiskPpm: int
  const ValueAtRiskUnits: Observation_t -> int
  const MaxAcceptableExpectedLossUnits: Observation_t -> int
  const ExpectedTelemetrySigmaFloor: Observation_t -> Sigma_t
  const CoverageRiskWeight: int
  const RoundFailureRiskWeight: int
  const BlockVarianceRiskWeight: int
  const ReorgDepthRiskWeight: int
  const RiskScoreRaisedThreshold: int
  const RiskScoreMaxThreshold: int

  pure val Observations = 0.to(MaxObservation)
  pure val Percentages = 0.to(100)
  pure val SigmaLadder = Set(BaseSigma, RaisedSigma, MaxSigma)
  pure val PpmDenominator = 1000000

  var observation: Observation_t

  assume max_observation_non_negative =
    MaxObservation >= 0
  assume sigma_ladder_is_ordered =
    1 <= BaseSigma and BaseSigma < RaisedSigma and RaisedSigma < MaxSigma
  assume participation_thresholds_are_ordered =
    0 <= CriticalHashParticipationPct and
    CriticalHashParticipationPct < TargetHashParticipationPct and
    TargetHashParticipationPct <= 100
  assume work_samples_are_well_formed =
    Observations.forall(o =>
      TotalHashWork.get(o) > 0 and
        0 <= CrosslinkParticipatingHashWork.get(o) and
        CrosslinkParticipatingHashWork.get(o) <= TotalHashWork.get(o)
    )
  assume round_samples_are_well_formed =
    Observations.forall(o =>
      TotalTenderlinkRounds.get(o) > 0 and
        0 <= FailedTenderlinkRounds.get(o) and
        FailedTenderlinkRounds.get(o) <= TotalTenderlinkRounds.get(o)
    )
  assume estimated_percentages_are_percentages =
    Observations.forall(o =>
      0 <= EstimatedCoverageRiskPct.get(o) and EstimatedCoverageRiskPct.get(o) <= 100 and
        0 <= EstimatedRoundFailureRatePct.get(o) and EstimatedRoundFailureRatePct.get(o) <= 100 and
        0 <= MeasuredBlockIntervalVariancePct.get(o) and
        MeasuredBlockIntervalVariancePct.get(o) <= 100
    )
  assume measured_reorg_depth_is_bounded =
    Observations.forall(o =>
      0 <= MeasuredObservedReorgDepth.get(o) and
        MeasuredObservedReorgDepth.get(o) <= MaxSigma
    )
  assume rollback_risk_ppm_is_non_negative =
    Observations.forall(o =>
      SigmaLadder.forall(s => RollbackRiskPpmAtSigma.get(o).get(s) >= 0)
    )
  assume max_acceptable_risk_is_non_negative =
    MaxAcceptableRollbackRiskPpm >= 0
  assume economic_exposure_is_well_formed =
    Observations.forall(o =>
      ValueAtRiskUnits.get(o) >= 0 and MaxAcceptableExpectedLossUnits.get(o) >= 0
    )
  assume expected_floors_are_in_ladder =
    Observations.forall(o =>
      SigmaLadder.contains(ExpectedTelemetrySigmaFloor.get(o))
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

  pure def WorkCoverageAtLeast(o: Observation_t, pct: Pct_t): bool =
    CrosslinkParticipatingHashWork.get(o) * 100 >= TotalHashWork.get(o) * pct

  pure def HashParticipationSigmaFloorForObservation(o: Observation_t): Sigma_t =
    if (not(WorkCoverageAtLeast(o, CriticalHashParticipationPct))) {
      MaxSigma
    } else if (not(WorkCoverageAtLeast(o, TargetHashParticipationPct))) {
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

  pure def CalibratedRiskScoreForObservation(o: Observation_t): int =
    CoverageRiskWeight * EstimatedCoverageRiskPct.get(o) +
      RoundFailureRiskWeight * EstimatedRoundFailureRatePct.get(o) +
      BlockVarianceRiskWeight * MeasuredBlockIntervalVariancePct.get(o) +
      ReorgDepthRiskWeight * MeasuredObservedReorgDepth.get(o)

  pure def RiskScoreSigmaFloor(score: int): Sigma_t =
    if (score >= RiskScoreMaxThreshold) {
      MaxSigma
    } else if (score >= RiskScoreRaisedThreshold) {
      RaisedSigma
    } else {
      BaseSigma
    }

  pure def EconomicTargetSatisfiedAtSigma(o: Observation_t, s: Sigma_t): bool =
    RollbackRiskPpmAtSigma.get(o).get(s) <= MaxAcceptableRollbackRiskPpm and
      RollbackRiskPpmAtSigma.get(o).get(s) * ValueAtRiskUnits.get(o) <=
        MaxAcceptableExpectedLossUnits.get(o) * PpmDenominator

  pure def EconomicSigmaFloorForObservation(o: Observation_t): Sigma_t =
    if (EconomicTargetSatisfiedAtSigma(o, BaseSigma)) {
      BaseSigma
    } else if (EconomicTargetSatisfiedAtSigma(o, RaisedSigma)) {
      RaisedSigma
    } else {
      MaxSigma
    }

  pure def TelemetrySigmaFloorForObservation(o: Observation_t): Sigma_t =
    MaxInt(
      EconomicSigmaFloorForObservation(o),
      MaxInt(
        RiskScoreSigmaFloor(CalibratedRiskScoreForObservation(o)),
        MaxInt(
          HashParticipationSigmaFloorForObservation(o),
          ReorgSigmaFloor(MeasuredObservedReorgDepth.get(o))
        )
      )
    )

  pure def EconomicTargetStatusForObservation(o: Observation_t): str =
    if (EconomicTargetSatisfiedAtSigma(o, TelemetrySigmaFloorForObservation(o))) {
      "target-satisfied"
    } else {
      "target-unreachable-at-max"
    }

  val CoverageEstimateUpperBoundsRawWorkGap =
    Observations.forall(o =>
      TotalHashWork.get(o) * EstimatedCoverageRiskPct.get(o) >=
        (TotalHashWork.get(o) - CrosslinkParticipatingHashWork.get(o)) * 100
    )

  val RoundFailureEstimateUpperBoundsRawFailures =
    Observations.forall(o =>
      TotalTenderlinkRounds.get(o) * EstimatedRoundFailureRatePct.get(o) >=
        FailedTenderlinkRounds.get(o) * 100
    )

  val RollbackRiskCurveIsMonotoneAcrossSigma =
    Observations.forall(o =>
      RollbackRiskPpmAtSigma.get(o).get(BaseSigma) >=
        RollbackRiskPpmAtSigma.get(o).get(RaisedSigma) and
        RollbackRiskPpmAtSigma.get(o).get(RaisedSigma) >=
        RollbackRiskPpmAtSigma.get(o).get(MaxSigma)
    )

  val EconomicTargetIsSatisfiedWhenReachable =
    Observations.forall(o =>
      if (EconomicTargetSatisfiedAtSigma(o, MaxSigma)) {
        EconomicTargetSatisfiedAtSigma(o, TelemetrySigmaFloorForObservation(o))
      } else {
        TelemetrySigmaFloorForObservation(o) == MaxSigma and
          EconomicTargetStatusForObservation(o) == "target-unreachable-at-max"
      }
    )

  val HashParticipationFloorIsMonotoneInWorkCoverage =
    Observations.forall(o =>
      if (WorkCoverageAtLeast(o, TargetHashParticipationPct)) {
        HashParticipationSigmaFloorForObservation(o) == BaseSigma
      } else if (WorkCoverageAtLeast(o, CriticalHashParticipationPct)) {
        HashParticipationSigmaFloorForObservation(o) == RaisedSigma
      } else {
        HashParticipationSigmaFloorForObservation(o) == MaxSigma
      }
    )

  val TelemetryMatchesExpectedSigmaLabels =
    Observations.forall(o =>
      TelemetrySigmaFloorForObservation(o) == ExpectedTelemetrySigmaFloor.get(o)
    )

  val TelemetryCalibrationSafety =
    CoverageEstimateUpperBoundsRawWorkGap and
    RoundFailureEstimateUpperBoundsRawFailures and
    RollbackRiskCurveIsMonotoneAcrossSigma and
    EconomicTargetIsSatisfiedWhenReachable and
    HashParticipationFloorIsMonotoneInWorkCoverage and
    TelemetryMatchesExpectedSigmaLabels

  val CurrentObservationMatchesTelemetry =
    TelemetrySigmaFloorForObservation(observation) == ExpectedTelemetrySigmaFloor.get(observation)

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
    TelemetryCalibrationSafety and
    CurrentObservationMatchesTelemetry
}

module CrosslinkDynamicSigmaTelemetryTest {
  import CrosslinkDynamicSigmaTelemetry.* from "./CrosslinkDynamicSigmaTelemetry"
  export CrosslinkDynamicSigmaTelemetry.*

  run healthyTelemetryWindowKeepsBaseSigmaTest = all {
    assert(TelemetrySigmaFloorForObservation(0) == BaseSigma),
    assert(EconomicTargetStatusForObservation(0) == "target-satisfied"),
  }

  run hashWorkParticipationRaisesSigmaTest = all {
    assert(WorkCoverageAtLeast(1, CriticalHashParticipationPct)),
    assert(not(WorkCoverageAtLeast(1, TargetHashParticipationPct))),
    assert(HashParticipationSigmaFloorForObservation(1) == RaisedSigma),
    assert(TelemetrySigmaFloorForObservation(1) == RaisedSigma),
  }

  run combinedTelemetryRiskRaisesSigmaTest = all {
    assert(WorkCoverageAtLeast(2, TargetHashParticipationPct)),
    assert(CalibratedRiskScoreForObservation(2) >= RiskScoreRaisedThreshold),
    assert(CalibratedRiskScoreForObservation(2) < RiskScoreMaxThreshold),
    assert(TelemetrySigmaFloorForObservation(2) == RaisedSigma),
  }

  run economicTargetRaisesSigmaAboveSignalFloorTest = all {
    assert(HashParticipationSigmaFloorForObservation(3) == BaseSigma),
    assert(RiskScoreSigmaFloor(CalibratedRiskScoreForObservation(3)) == BaseSigma),
    assert(EconomicSigmaFloorForObservation(3) == RaisedSigma),
    assert(TelemetrySigmaFloorForObservation(3) == RaisedSigma),
  }

  run criticalParticipationForcesMaxSigmaTest = all {
    assert(not(WorkCoverageAtLeast(4, CriticalHashParticipationPct))),
    assert(TelemetrySigmaFloorForObservation(4) == MaxSigma),
  }

  run economicTargetCanForceMaxSigmaTest = all {
    assert(EconomicSigmaFloorForObservation(5) == MaxSigma),
    assert(EconomicTargetStatusForObservation(5) == "target-satisfied"),
    assert(TelemetrySigmaFloorForObservation(5) == MaxSigma),
  }

  run unreachableEconomicTargetFallsBackToMaxSigmaTest = all {
    assert(EconomicSigmaFloorForObservation(6) == MaxSigma),
    assert(EconomicTargetStatusForObservation(6) == "target-unreachable-at-max"),
    assert(TelemetrySigmaFloorForObservation(6) == MaxSigma),
  }

  run deepReorgTelemetryWindowForcesMaxSigmaTest = all {
    assert(MeasuredObservedReorgDepth.get(7) >= RaisedSigma),
    assert(TelemetrySigmaFloorForObservation(7) == MaxSigma),
  }

  run expectedLossBudgetRaisesSigmaEvenWithinPpmTargetTest = all {
    assert(RollbackRiskPpmAtSigma.get(8).get(BaseSigma) <= MaxAcceptableRollbackRiskPpm),
    assert(not(EconomicTargetSatisfiedAtSigma(8, BaseSigma))),
    assert(EconomicTargetSatisfiedAtSigma(8, RaisedSigma)),
    assert(EconomicSigmaFloorForObservation(8) == RaisedSigma),
    assert(TelemetrySigmaFloorForObservation(8) == RaisedSigma),
  }

  run telemetryMatchesAllExpectedWindowsTest = {
    assert(TelemetryCalibrationSafety)
  }
}

module CrosslinkDynamicSigmaTelemetryModel {
  import CrosslinkDynamicSigmaTelemetryTest(
    MaxObservation = 8,
    BaseSigma = 1,
    RaisedSigma = 3,
    MaxSigma = 6,
    TargetHashParticipationPct = 67,
    CriticalHashParticipationPct = 50,
    TotalHashWork = Map(
      0 -> 100,
      1 -> 100,
      2 -> 100,
      3 -> 100,
      4 -> 100,
      5 -> 100,
      6 -> 100,
      7 -> 100,
      8 -> 100,
    ),
    CrosslinkParticipatingHashWork = Map(
      0 -> 90,
      1 -> 63,
      2 -> 75,
      3 -> 90,
      4 -> 45,
      5 -> 90,
      6 -> 90,
      7 -> 90,
      8 -> 90,
    ),
    TotalTenderlinkRounds = Map(
      0 -> 100,
      1 -> 100,
      2 -> 100,
      3 -> 100,
      4 -> 100,
      5 -> 100,
      6 -> 100,
      7 -> 100,
      8 -> 100,
    ),
    FailedTenderlinkRounds = Map(
      0 -> 0,
      1 -> 0,
      2 -> 15,
      3 -> 0,
      4 -> 0,
      5 -> 0,
      6 -> 0,
      7 -> 0,
      8 -> 0,
    ),
    EstimatedCoverageRiskPct = Map(
      0 -> 10,
      1 -> 37,
      2 -> 25,
      3 -> 10,
      4 -> 55,
      5 -> 10,
      6 -> 10,
      7 -> 10,
      8 -> 10,
    ),
    EstimatedRoundFailureRatePct = Map(
      0 -> 0,
      1 -> 0,
      2 -> 15,
      3 -> 0,
      4 -> 0,
      5 -> 0,
      6 -> 0,
      7 -> 0,
      8 -> 0,
    ),
    MeasuredBlockIntervalVariancePct = Map(
      0 -> 5,
      1 -> 10,
      2 -> 15,
      3 -> 5,
      4 -> 0,
      5 -> 5,
      6 -> 5,
      7 -> 5,
      8 -> 5,
    ),
    MeasuredObservedReorgDepth = Map(
      0 -> 0,
      1 -> 0,
      2 -> 0,
      3 -> 0,
      4 -> 0,
      5 -> 0,
      6 -> 0,
      7 -> 3,
      8 -> 0,
    ),
    RollbackRiskPpmAtSigma = Map(
      0 -> Map(1 -> 40, 3 -> 10, 6 -> 1),
      1 -> Map(1 -> 80, 3 -> 20, 6 -> 2),
      2 -> Map(1 -> 80, 3 -> 25, 6 -> 3),
      3 -> Map(1 -> 250, 3 -> 80, 6 -> 10),
      4 -> Map(1 -> 500, 3 -> 250, 6 -> 70),
      5 -> Map(1 -> 500, 3 -> 180, 6 -> 70),
      6 -> Map(1 -> 500, 3 -> 300, 6 -> 120),
      7 -> Map(1 -> 400, 3 -> 180, 6 -> 60),
      8 -> Map(1 -> 80, 3 -> 5, 6 -> 1),
    ),
    MaxAcceptableRollbackRiskPpm = 100,
    ValueAtRiskUnits = Map(
      0 -> 1000,
      1 -> 1000,
      2 -> 1000,
      3 -> 1000,
      4 -> 1000,
      5 -> 1000,
      6 -> 1000,
      7 -> 1000,
      8 -> 100000,
    ),
    MaxAcceptableExpectedLossUnits = Map(
      0 -> 100,
      1 -> 100,
      2 -> 100,
      3 -> 100,
      4 -> 100,
      5 -> 100,
      6 -> 100,
      7 -> 100,
      8 -> 1,
    ),
    ExpectedTelemetrySigmaFloor = Map(
      0 -> 1,
      1 -> 3,
      2 -> 3,
      3 -> 3,
      4 -> 6,
      5 -> 6,
      6 -> 6,
      7 -> 6,
      8 -> 3,
    ),
    CoverageRiskWeight = 2,
    RoundFailureRiskWeight = 3,
    BlockVarianceRiskWeight = 2,
    ReorgDepthRiskWeight = 60,
    RiskScoreRaisedThreshold = 100,
    RiskScoreMaxThreshold = 220,
  ).*
}
