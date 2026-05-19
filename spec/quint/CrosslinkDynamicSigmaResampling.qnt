// -*- mode: Bluespec; -*-
module CrosslinkDynamicSigmaResampling {
  /*
   Composition layer for dynamic sigma and nil-precommit resampling.

   The target witness is a fork switch that derives rollback depth, raises sigma,
   and then lets the nil-precommit recovery path resample into a fresh
   Tenderlink decision.
   */

  import CrosslinkResampling.* from "./CrosslinkResampling"
  export CrosslinkResampling.*

  const BaseSigma: int
  const RaisedSigma: int
  const MaxSigma: int
  const MaxWork: int
  const TargetHashParticipationPct: int
  const CriticalHashParticipationPct: int
  const HashParticipationPct: Round_t -> int
  const BestTip: Round_t -> Snapshot_t
  const PublishedAt: Round_t -> Snapshot_t -> bool
  const HonestWorkAt: Round_t -> Snapshot_t -> int
  const AdversarialWorkAt: Round_t -> Snapshot_t -> int
  const TieBreakRank: Snapshot_t -> int
  const HeightOf: Snapshot_t -> int
  const LcaHeight: Snapshot_t -> Snapshot_t -> int

  pure val Percentages = 0.to(100)

  assume sigma_ladder_is_ordered =
    1 <= BaseSigma and BaseSigma < RaisedSigma and RaisedSigma < MaxSigma
  assume max_work_non_negative =
    MaxWork >= 0
  assume participation_thresholds_are_ordered =
    0 <= CriticalHashParticipationPct and
    CriticalHashParticipationPct < TargetHashParticipationPct and
    TargetHashParticipationPct <= 100
  assume observed_participation_is_percentage =
    Rounds.forall(r =>
      0 <= HashParticipationPct.get(r) and HashParticipationPct.get(r) <= 100
    )
  assume best_tips_are_snapshots =
    Rounds.forall(r => Snapshots.contains(BestTip.get(r)))
  assume published_work_is_bounded =
    Rounds.forall(r =>
      Snapshots.forall(v =>
        0 <= HonestWorkAt.get(r).get(v) and
        0 <= AdversarialWorkAt.get(r).get(v) and
        TotalWorkAt(r, v) <= MaxWork
      )
    )
  assume generated_best_tips_match_work_competition =
    Rounds.forall(r => IsBestTipAt(r, BestTip.get(r)))

  var dynRound: Round_t
  var dynSigma: int
  var controllerStatus: str
  var dynamicAction: str

  pure def MaxInt(a: int, b: int): int =
    if (a >= b) { a } else { b }

  pure def TotalWorkAt(r: Round_t, v: Snapshot_t): int =
    HonestWorkAt.get(r).get(v) + AdversarialWorkAt.get(r).get(v)

  pure def CandidateAt(r: Round_t, v: Snapshot_t): bool =
    Snapshots.contains(v) and PublishedAt.get(r).get(v)

  pure def BetterOrEqualAt(r: Round_t, a: Snapshot_t, b: Snapshot_t): bool =
    TotalWorkAt(r, a) > TotalWorkAt(r, b) or
      (
        TotalWorkAt(r, a) == TotalWorkAt(r, b) and
        TieBreakRank.get(a) >= TieBreakRank.get(b)
      )

  pure def IsBestTipAt(r: Round_t, v: Snapshot_t): bool =
    CandidateAt(r, v) and
      Snapshots.forall(w =>
        not(CandidateAt(r, w)) or BetterOrEqualAt(r, v, w)
      )

  pure def CommonAncestorHeight(previousTip: Snapshot_t, nextTip: Snapshot_t): int =
    LcaHeight.get(previousTip).get(nextTip)

  pure def RollbackDepth(previousTip: Snapshot_t, nextTip: Snapshot_t): int =
    HeightOf.get(previousTip) - CommonAncestorHeight(previousTip, nextTip)

  pure def DerivedReorgDepth(r: Round_t): int =
    if (r == 0) {
      0
    } else {
      RollbackDepth(BestTip.get(r - 1), BestTip.get(r))
    }

  pure def ReorgSigmaFloor(depth: int): int =
    if (depth >= RaisedSigma) {
      MaxSigma
    } else if (depth >= BaseSigma) {
      RaisedSigma
    } else {
      BaseSigma
    }

  pure def HashParticipationSigmaFloor(pct: int): int =
    if (pct < CriticalHashParticipationPct) {
      MaxSigma
    } else if (pct < TargetHashParticipationPct) {
      RaisedSigma
    } else {
      BaseSigma
    }

  pure def ControllerStatusFor(pct: int): str =
    if (pct < CriticalHashParticipationPct) {
      "critical-hash-participation"
    } else if (pct < TargetHashParticipationPct) {
      "degraded-hash-participation"
    } else {
      "healthy-hash-participation"
    }

  def DynamicControllerSigma(nextRound: Round_t): int =
    MaxInt(
      dynSigma,
      MaxInt(
        ReorgSigmaFloor(DerivedReorgDepth(nextRound)),
        HashParticipationSigmaFloor(HashParticipationPct.get(nextRound))
      )
    )

  action DynamicStateUnchanged = all {
    dynRound' = dynRound,
    dynSigma' = dynSigma,
    controllerStatus' = controllerStatus,
    dynamicAction' = dynamicAction,
  }

  action DynamicResamplingInit = all {
    Init,
    dynRound' = 0,
    dynSigma' = HashParticipationSigmaFloor(HashParticipationPct.get(0)),
    controllerStatus' = ControllerStatusFor(HashParticipationPct.get(0)),
    dynamicAction' = "DynamicResamplingInit",
  }

  action AdvanceDerivedForkSignal = {
    val nextRound = dynRound + 1
    all {
      nextRound.in(Rounds),
      DerivedReorgDepth(nextRound) >= BaseSigma,
      round' = round,
      step' = step,
      decision' = decision,
      lockedValue' = lockedValue,
      lockedRound' = lockedRound,
      validValue' = validValue,
      validRound' = validRound,
      cachedProposal' = cachedProposal,
      cachedProposalRound' = cachedProposalRound,
      msgsPropose' = msgsPropose,
      msgsPrevote' = msgsPrevote,
      msgsPrecommit' = msgsPrecommit,
      evidencePropose' = evidencePropose,
      evidencePrevote' = evidencePrevote,
      evidencePrecommit' = evidencePrecommit,
      firedAction' = firedAction,
      dynRound' = nextRound,
      dynSigma' = DynamicControllerSigma(nextRound),
      controllerStatus' = ControllerStatusFor(HashParticipationPct.get(nextRound)),
      dynamicAction' = "DerivedForkRaiseSigma",
    }
  }

  action AdvanceHashParticipationSignal = {
    val nextRound = dynRound + 1
    all {
      nextRound.in(Rounds),
      DerivedReorgDepth(nextRound) < BaseSigma,
      HashParticipationPct.get(nextRound) < TargetHashParticipationPct,
      round' = round,
      step' = step,
      decision' = decision,
      lockedValue' = lockedValue,
      lockedRound' = lockedRound,
      validValue' = validValue,
      validRound' = validRound,
      cachedProposal' = cachedProposal,
      cachedProposalRound' = cachedProposalRound,
      msgsPropose' = msgsPropose,
      msgsPrevote' = msgsPrevote,
      msgsPrecommit' = msgsPrecommit,
      evidencePropose' = evidencePropose,
      evidencePrevote' = evidencePrevote,
      evidencePrecommit' = evidencePrecommit,
      firedAction' = firedAction,
      dynRound' = nextRound,
      dynSigma' = DynamicControllerSigma(nextRound),
      controllerStatus' = ControllerStatusFor(HashParticipationPct.get(nextRound)),
      dynamicAction' =
        if (HashParticipationPct.get(nextRound) < CriticalHashParticipationPct) {
          "CriticalHashParticipationRaiseSigma"
        } else {
          "LowHashParticipationRaiseSigma"
        },
    }
  }

  action DynamicResamplingStutter = all {
    round' = round,
    step' = step,
    decision' = decision,
    lockedValue' = lockedValue,
    lockedRound' = lockedRound,
    validValue' = validValue,
    validRound' = validRound,
    cachedProposal' = cachedProposal,
    cachedProposalRound' = cachedProposalRound,
    msgsPropose' = msgsPropose,
    msgsPrevote' = msgsPrevote,
    msgsPrecommit' = msgsPrecommit,
    evidencePropose' = evidencePropose,
    evidencePrevote' = evidencePrevote,
    evidencePrecommit' = evidencePrecommit,
    firedAction' = firedAction,
    DynamicStateUnchanged,
  }

  action DynamicSeedValidNil(p: Proc_t): bool =
    all { SeedValidNilPrecommitState(p), DynamicStateUnchanged }

  action DynamicSeedFaultyNil(p: Proc_t): bool =
    all { SeedFaultyNilPrecommit(p), DynamicStateUnchanged }

  action DynamicSeedSameRoundLock(p: Proc_t): bool =
    all { SeedSameRoundValueLock(p), DynamicStateUnchanged }

  action DynamicStartNextRound(p: Proc_t): bool =
    all { StartNextRoundAfterPrecommitQuorum(p), DynamicStateUnchanged }

  action DynamicInsertProposal(p: Proc_t): bool =
    all { InsertProposal(p), DynamicStateUnchanged }

  action DynamicPrevote(p: Proc_t, v: Snapshot_t): bool =
    all { UponProposalPrevote(p, v), DynamicStateUnchanged }

  action DynamicPrecommitValue(p: Proc_t, v: Snapshot_t): bool =
    all { UponValuePrevoteQuorum(p, v), DynamicStateUnchanged }

  action DynamicDecide(p: Proc_t, r: Round_t, v: Snapshot_t): bool =
    all { Decide(p, r, v), DynamicStateUnchanged }

  action DynamicResamplingNext =
    any {
      AdvanceDerivedForkSignal,
      AdvanceHashParticipationSignal,
      nondet p = oneOf(Corr)
      DynamicStartNextRound(p),
      nondet p = oneOf(Corr)
      DynamicInsertProposal(p),
      nondet p = oneOf(Corr)
      nondet v = oneOf(Snapshots)
      DynamicPrevote(p, v),
      nondet p = oneOf(Corr)
      nondet v = oneOf(Snapshots)
      DynamicPrecommitValue(p, v),
      nondet p = oneOf(Corr)
      nondet r = oneOf(Rounds)
      nondet v = oneOf(Snapshots)
      DynamicDecide(p, r, v),
      DynamicResamplingStutter,
    }

  val DynamicSigmaWithinBounds =
    BaseSigma <= dynSigma and dynSigma <= MaxSigma

  val DynamicSigmaRespectsDerivedReorgFloor =
    dynSigma >= ReorgSigmaFloor(DerivedReorgDepth(dynRound))

  val DynamicSigmaRespectsHashParticipationFloor =
    dynSigma >= HashParticipationSigmaFloor(HashParticipationPct.get(dynRound))

  val HashParticipationFloorIsMonotone =
    Percentages.forall(lower =>
      Percentages.forall(higher =>
        (lower <= higher) implies
          HashParticipationSigmaFloor(lower) >= HashParticipationSigmaFloor(higher)
      )
    )

  val DynamicControllerStatusMatchesHashParticipation =
    controllerStatus == ControllerStatusFor(HashParticipationPct.get(dynRound))

  val CurrentDynamicBestTipMatchesGeneratedCompetition =
    IsBestTipAt(dynRound, BestTip.get(dynRound))

  val DynamicResamplingSafety =
    Safety and
    DynamicSigmaWithinBounds and
    DynamicSigmaRespectsDerivedReorgFloor and
    DynamicSigmaRespectsHashParticipationFloor and
    HashParticipationFloorIsMonotone and
    DynamicControllerStatusMatchesHashParticipation and
    CurrentDynamicBestTipMatchesGeneratedCompetition
}

module CrosslinkDynamicSigmaResamplingTest {
  import CrosslinkDynamicSigmaResampling.* from "./CrosslinkDynamicSigmaResampling"
  export CrosslinkDynamicSigmaResampling.*

  run derivedForkSignalRaisesSigmaBeforeResamplingDecisionTest = {
    DynamicResamplingInit
      .then(AdvanceDerivedForkSignal)
      .then(all {
        assert(dynSigma == RaisedSigma),
        assert(controllerStatus == "healthy-hash-participation"),
        assert(dynamicAction == "DerivedForkRaiseSigma"),
        assert(DynamicResamplingSafety),
        DynamicResamplingStutter,
      })
  }

  run derivedForkSignalThenNilResamplingDecidesFreshValueTest = {
    DynamicResamplingInit
      .then(DynamicSeedValidNil("p1"))
      .then(DynamicSeedValidNil("p2"))
      .then(DynamicSeedFaultyNil("p4"))
      .then(DynamicSeedSameRoundLock("p3"))
      .then(AdvanceDerivedForkSignal)
      .then(all {
        assert(dynSigma == RaisedSigma),
        assert(controllerStatus == "healthy-hash-participation"),
        assert(dynamicAction == "DerivedForkRaiseSigma"),
        assert(DynamicResamplingSafety),
        DynamicResamplingStutter,
      })
      .then(DynamicStartNextRound("p1"))
      .then(DynamicStartNextRound("p2"))
      .then(DynamicStartNextRound("p3"))
      .then(DynamicInsertProposal("p2"))
      .then(DynamicPrevote("p1", "s1"))
      .then(DynamicPrevote("p2", "s1"))
      .then(DynamicPrevote("p3", "s1"))
      .then(DynamicPrecommitValue("p1", "s1"))
      .then(DynamicPrecommitValue("p2", "s1"))
      .then(DynamicPrecommitValue("p3", "s1"))
      .then(DynamicDecide("p1", 1, "s1"))
      .then(all {
        assert(decision.get("p1") == "s1"),
        assert(dynSigma == RaisedSigma),
        assert(DynamicResamplingSafety),
        DynamicResamplingStutter,
      })
  }

  run criticalHashParticipationRaisesSigmaWithoutNewForkSwitchTest = {
    DynamicResamplingInit
      .then(AdvanceDerivedForkSignal)
      .then(AdvanceHashParticipationSignal)
      .then(all {
        assert(dynRound == 2),
        assert(DerivedReorgDepth(dynRound) == 0),
        assert(dynSigma == MaxSigma),
        assert(controllerStatus == "critical-hash-participation"),
        assert(dynamicAction == "CriticalHashParticipationRaiseSigma"),
        assert(DynamicResamplingSafety),
        DynamicResamplingStutter,
      })
  }

  run generatedCompetitionBacksResamplingForkSignalTest = all {
    assert(IsBestTipAt(0, "s0")),
    assert(not(CandidateAt(0, "s1"))),
    assert(IsBestTipAt(1, "s1")),
    assert(DerivedReorgDepth(1) == 1),
  }
}

module CrosslinkDynamicSigmaResamplingModel {
  import CrosslinkDynamicSigmaResamplingTest(
    Corr = Set("p1", "p2", "p3"),
    Faulty = Set("p4"),
    N = 4,
    T = 1,
    MaxRound = 2,
    Proposer = Map(0 -> "p1", 1 -> "p2", 2 -> "p3"),
    Stream = Map(0 -> "s0", 1 -> "s1", 2 -> "s2"),
    Snapshots = Set("s0", "s1", "s2"),
    ResampleOnNilPrecommit = true,
    BaseSigma = 1,
    RaisedSigma = 3,
    MaxSigma = 6,
    MaxWork = 4,
    TargetHashParticipationPct = 67,
    CriticalHashParticipationPct = 50,
    HashParticipationPct = Map(
      0 -> 90,
      1 -> 90,
      2 -> 45,
    ),
    BestTip = Map(
      0 -> "s0",
      1 -> "s1",
      2 -> "s2",
    ),
    PublishedAt = Map(
      0 -> Map("s0" -> true, "s1" -> false, "s2" -> false),
      1 -> Map("s0" -> true, "s1" -> true, "s2" -> false),
      2 -> Map("s0" -> true, "s1" -> true, "s2" -> true),
    ),
    HonestWorkAt = Map(
      0 -> Map("s0" -> 1, "s1" -> 0, "s2" -> 0),
      1 -> Map("s0" -> 1, "s1" -> 0, "s2" -> 0),
      2 -> Map("s0" -> 1, "s1" -> 0, "s2" -> 0),
    ),
    AdversarialWorkAt = Map(
      0 -> Map("s0" -> 0, "s1" -> 3, "s2" -> 0),
      1 -> Map("s0" -> 0, "s1" -> 3, "s2" -> 0),
      2 -> Map("s0" -> 0, "s1" -> 3, "s2" -> 4),
    ),
    TieBreakRank = Map(
      "s0" -> 0,
      "s1" -> 1,
      "s2" -> 2,
    ),
    HeightOf = Map(
      "s0" -> 1,
      "s1" -> 2,
      "s2" -> 3,
    ),
    LcaHeight = Map(
      "s0" -> Map("s1" -> 0, "s2" -> 0),
      "s1" -> Map("s1" -> 2, "s2" -> 2),
      "s2" -> Map("s1" -> 2, "s2" -> 3),
    ),
  ).*
}
