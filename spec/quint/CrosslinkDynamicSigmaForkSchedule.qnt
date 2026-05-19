// -*- mode: Bluespec; -*-
module CrosslinkDynamicSigmaForkSchedule {
  /*
   Composition layer between the dynamic-sigma controller and the PoW fork
   schedule. The purpose is to remove one layer of hand-wiring: dynamic sigma
   should consume rollback depth derived from best-tip changes, not a supplied
   ObservedReorgDepth map.
   */

  import CrosslinkPowForkSchedule.* from "./CrosslinkPowForkSchedule"
  export CrosslinkPowForkSchedule.*

  const BaseSigma: int
  const RaisedSigma: int
  const MaxSigma: int
  const TargetHashParticipationPct: int
  const CriticalHashParticipationPct: int
  const HashParticipationPct: Round_t -> int
  const RoundFailed: Round_t -> bool

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

  var dynSigma: int
  var dynStatus: str
  var dynAction: str

  pure def MaxInt(a: int, b: int): int =
    if (a >= b) { a } else { b }

  pure def DerivedReorgDepth(r: Round_t): int =
    RoundRollbackDepth(r)

  pure def DynHashParticipationSigmaFloor(pct: int): int =
    if (pct < CriticalHashParticipationPct) {
      MaxSigma
    } else if (pct < TargetHashParticipationPct) {
      RaisedSigma
    } else {
      BaseSigma
    }

  pure def DynReorgSigmaFloor(depth: int): int =
    if (depth >= RaisedSigma) {
      MaxSigma
    } else if (depth >= BaseSigma) {
      RaisedSigma
    } else {
      BaseSigma
    }

  pure def DynEscalatedSigma(current: int): int =
    if (current < RaisedSigma) {
      RaisedSigma
    } else {
      MaxSigma
    }

  pure def DerivedControllerSigma(
    previousSigma: int,
    nextRound: Round_t,
    previousRoundFailed: bool
  ): int =
    MaxInt(
      DynReorgSigmaFloor(DerivedReorgDepth(nextRound)),
      MaxInt(
        DynHashParticipationSigmaFloor(HashParticipationPct.get(nextRound)),
        if (previousRoundFailed) { DynEscalatedSigma(previousSigma) } else { BaseSigma }
      )
    )

  pure def DynStatusFor(pct: int): str =
    if (pct < CriticalHashParticipationPct) {
      "critical-hash-participation"
    } else if (pct < TargetHashParticipationPct) {
      "degraded-hash-participation"
    } else {
      "healthy-hash-participation"
    }

  action DerivedInit = all {
    Init,
    dynSigma' = DynHashParticipationSigmaFloor(HashParticipationPct.get(0)),
    dynStatus' = DynStatusFor(HashParticipationPct.get(0)),
    dynAction' = "DerivedInit",
  }

  action DerivedAdvanceRound = {
    val oldRound = round
    val nextRound = oldRound + 1
    val nextSigma = DerivedControllerSigma(
      dynSigma,
      nextRound,
      RoundFailed.get(oldRound)
    )
    all {
      AdvanceRound,
      dynSigma' = nextSigma,
      dynStatus' = DynStatusFor(HashParticipationPct.get(nextRound)),
      dynAction' =
        if (RoundFailed.get(oldRound)) {
          "DerivedAdvanceAfterRoundFailure"
        } else if (DerivedReorgDepth(nextRound) >= BaseSigma) {
          "DerivedAdvanceAfterForkSwitch"
        } else if (HashParticipationPct.get(nextRound) < TargetHashParticipationPct) {
          "DerivedAdvanceWithLowHashParticipation"
        } else {
          "DerivedAdvanceStable"
        },
    }
  }

  action DerivedStutter = all {
    Stutter,
    dynSigma' = dynSigma,
    dynStatus' = dynStatus,
    dynAction' = dynAction,
  }

  action DerivedNext =
    any {
      DerivedAdvanceRound,
      DerivedStutter,
    }

  val DynSigmaWithinBounds =
    BaseSigma <= dynSigma and dynSigma <= MaxSigma

  val DynSigmaRespectsHashParticipationFloor =
    dynSigma >= DynHashParticipationSigmaFloor(HashParticipationPct.get(round))

  val DynSigmaRespectsDerivedReorgFloor =
    dynSigma >= DynReorgSigmaFloor(DerivedReorgDepth(round))

  val DynStatusMatchesHashParticipation =
    dynStatus == DynStatusFor(HashParticipationPct.get(round))

  val DerivedSafety =
    Safety and
    DynSigmaWithinBounds and
    DynSigmaRespectsHashParticipationFloor and
    DynSigmaRespectsDerivedReorgFloor and
    DynStatusMatchesHashParticipation
}

module CrosslinkDynamicSigmaForkScheduleTest {
  import CrosslinkDynamicSigmaForkSchedule.* from "./CrosslinkDynamicSigmaForkSchedule"
  export CrosslinkDynamicSigmaForkSchedule.*

  run derivedReorgDepthFeedsDynamicSigmaTest = {
    assert(DerivedReorgDepth(2) == 2)
  }

  run forkScheduleDerivedReorgRaisesDynamicSigmaTest = {
    DerivedInit
      .then(DerivedAdvanceRound)
      .then(all {
        assert(round == 1),
        assert(bestTip == "a4"),
        assert(DerivedReorgDepth(round) == 0),
        assert(dynSigma == BaseSigma),
        assert(dynAction == "DerivedAdvanceStable"),
        assert(DerivedSafety),
        DerivedStutter,
      })
      .then(DerivedAdvanceRound)
      .then(all {
        assert(round == 2),
        assert(bestTip == "b4"),
        assert(DerivedReorgDepth(round) == 2),
        assert(dynSigma == RaisedSigma),
        assert(dynAction == "DerivedAdvanceAfterForkSwitch"),
        assert(DerivedSafety),
        DerivedStutter,
      })
  }

  run derivedRaisedSigmaSurvivesForkSwitchTest = all {
    assert(not(SampleSurvivesRollback("a4", "b4", BaseSigma))),
    assert(SampleSurvivesRollback("a4", "b4", RaisedSigma)),
  }
}

module CrosslinkDynamicSigmaForkScheduleModel {
  import CrosslinkDynamicSigmaForkScheduleTest(
    MaxRound = 2,
    MaxHeight = 4,
    Sigma = 3,
    Snapshots = Set("g", "a1", "a2", "a3", "a4", "b3", "b4"),
    BestTip = Map(
      0 -> "a3",
      1 -> "a4",
      2 -> "b4",
    ),
    HeightOf = Map(
      "g" -> 0,
      "a1" -> 1,
      "a2" -> 2,
      "a3" -> 3,
      "a4" -> 4,
      "b3" -> 3,
      "b4" -> 4,
    ),
    LcaHeight = Map(
      "a3" -> Map("a4" -> 3, "b4" -> 2),
      "a4" -> Map("a4" -> 4, "b4" -> 2),
      "b4" -> Map("a4" -> 2, "b4" -> 4),
    ),
    BaseSigma = 1,
    RaisedSigma = 3,
    MaxSigma = 6,
    TargetHashParticipationPct = 67,
    CriticalHashParticipationPct = 50,
    HashParticipationPct = Map(
      0 -> 90,
      1 -> 90,
      2 -> 90,
    ),
    RoundFailed = Map(
      0 -> false,
      1 -> false,
      2 -> false,
    ),
  ).*
}
