// -*- mode: Bluespec; -*-
module CrosslinkDynamicSigmaBranchCompetition {
  /*
   Composition layer between dynamic sigma and generated PoW branch competition.

   The branch-competition model derives best-tip changes from published tips and
   honest plus adversarial work. This module feeds the resulting rollback depth
   into the dynamic-sigma controller.
   */

  import CrosslinkPowBranchCompetition.* from "./CrosslinkPowBranchCompetition"
  export CrosslinkPowBranchCompetition.*

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

  pure def BranchCompetitionControllerSigma(
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

  action BranchCompetitionDynamicInit = all {
    Init,
    dynSigma' = DynHashParticipationSigmaFloor(HashParticipationPct.get(0)),
    dynStatus' = DynStatusFor(HashParticipationPct.get(0)),
    dynAction' = "BranchCompetitionDynamicInit",
  }

  action BranchCompetitionDynamicAdvanceRound = {
    val oldRound = round
    val nextRound = oldRound + 1
    val nextSigma = BranchCompetitionControllerSigma(
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
          "GeneratedAdvanceAfterRoundFailure"
        } else if (DerivedReorgDepth(nextRound) >= BaseSigma) {
          "GeneratedAdvanceAfterAdversarialForkSwitch"
        } else if (HashParticipationPct.get(nextRound) < TargetHashParticipationPct) {
          "GeneratedAdvanceWithLowHashParticipation"
        } else {
          "GeneratedAdvanceStable"
        },
    }
  }

  action BranchCompetitionDynamicStutter = all {
    Stutter,
    dynSigma' = dynSigma,
    dynStatus' = dynStatus,
    dynAction' = dynAction,
  }

  action BranchCompetitionDynamicNext =
    any {
      BranchCompetitionDynamicAdvanceRound,
      BranchCompetitionDynamicStutter,
    }

  val DynSigmaWithinBounds =
    BaseSigma <= dynSigma and dynSigma <= MaxSigma

  val DynSigmaRespectsHashParticipationFloor =
    dynSigma >= DynHashParticipationSigmaFloor(HashParticipationPct.get(round))

  val DynSigmaRespectsGeneratedReorgFloor =
    dynSigma >= DynReorgSigmaFloor(DerivedReorgDepth(round))

  val DynStatusMatchesHashParticipation =
    dynStatus == DynStatusFor(HashParticipationPct.get(round))

  val BranchCompetitionDynamicSafety =
    Safety and
    DynSigmaWithinBounds and
    DynSigmaRespectsHashParticipationFloor and
    DynSigmaRespectsGeneratedReorgFloor and
    DynStatusMatchesHashParticipation
}

module CrosslinkDynamicSigmaBranchCompetitionTest {
  import CrosslinkDynamicSigmaBranchCompetition.* from "./CrosslinkDynamicSigmaBranchCompetition"
  export CrosslinkDynamicSigmaBranchCompetition.*

  run generatedCompetitionFeedsDynamicSigmaTest = all {
    assert(IsBestTipAt(2, "b4")),
    assert(DerivedReorgDepth(2) == 2)
  }

  run generatedCompetitionForkSwitchRaisesDynamicSigmaTest = {
    BranchCompetitionDynamicInit
      .then(BranchCompetitionDynamicAdvanceRound)
      .then(all {
        assert(round == 1),
        assert(bestTip == "a4"),
        assert(DerivedReorgDepth(round) == 0),
        assert(dynSigma == BaseSigma),
        assert(dynAction == "GeneratedAdvanceStable"),
        assert(BranchCompetitionDynamicSafety),
        BranchCompetitionDynamicStutter,
      })
      .then(BranchCompetitionDynamicAdvanceRound)
      .then(all {
        assert(round == 2),
        assert(bestTip == "b4"),
        assert(bestTipWork == 5),
        assert(DerivedReorgDepth(round) == 2),
        assert(dynSigma == RaisedSigma),
        assert(dynAction == "GeneratedAdvanceAfterAdversarialForkSwitch"),
        assert(BranchCompetitionDynamicSafety),
        BranchCompetitionDynamicStutter,
      })
  }

  run generatedRaisedSigmaSurvivesAdversarialSwitchTest = all {
    assert(not(SampleSurvivesRollback(BestTipAt(1), BestTipAt(2), BaseSigma))),
    assert(SampleSurvivesRollback(BestTipAt(1), BestTipAt(2), RaisedSigma)),
  }
}

module CrosslinkDynamicSigmaBranchCompetitionModel {
  import CrosslinkDynamicSigmaBranchCompetitionTest(
    MaxRound = 2,
    MaxHeight = 4,
    MaxWork = 5,
    Sigma = 3,
    Snapshots = Set("g", "a3", "a4", "b4"),
    BestTip = Map(
      0 -> "a3",
      1 -> "a4",
      2 -> "b4",
    ),
    PublishedAt = Map(
      0 -> Map("g" -> true, "a3" -> true, "a4" -> false, "b4" -> false),
      1 -> Map("g" -> true, "a3" -> true, "a4" -> true, "b4" -> false),
      2 -> Map("g" -> true, "a3" -> true, "a4" -> true, "b4" -> true),
    ),
    HonestWorkAt = Map(
      0 -> Map("g" -> 0, "a3" -> 3, "a4" -> 0, "b4" -> 0),
      1 -> Map("g" -> 0, "a3" -> 3, "a4" -> 4, "b4" -> 0),
      2 -> Map("g" -> 0, "a3" -> 3, "a4" -> 4, "b4" -> 0),
    ),
    AdversarialWorkAt = Map(
      0 -> Map("g" -> 0, "a3" -> 0, "a4" -> 0, "b4" -> 4),
      1 -> Map("g" -> 0, "a3" -> 0, "a4" -> 0, "b4" -> 4),
      2 -> Map("g" -> 0, "a3" -> 0, "a4" -> 0, "b4" -> 5),
    ),
    TieBreakRank = Map(
      "g" -> 0,
      "a3" -> 1,
      "a4" -> 2,
      "b4" -> 3,
    ),
    HeightOf = Map(
      "g" -> 0,
      "a3" -> 3,
      "a4" -> 4,
      "b4" -> 4,
    ),
    LcaHeight = Map(
      "g" -> Map("g" -> 0, "a3" -> 0, "a4" -> 0, "b4" -> 0),
      "a3" -> Map("g" -> 0, "a3" -> 3, "a4" -> 3, "b4" -> 2),
      "a4" -> Map("g" -> 0, "a3" -> 3, "a4" -> 4, "b4" -> 2),
      "b4" -> Map("g" -> 0, "a3" -> 2, "a4" -> 2, "b4" -> 4),
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
