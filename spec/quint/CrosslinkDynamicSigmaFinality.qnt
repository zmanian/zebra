// -*- mode: Bluespec; -*-
module CrosslinkDynamicSigmaFinality {
  /*
   Full bounded composition for the current Crosslink Quint slices.

   A derived PoW fork signal raises dynamic sigma, nil-precommit resampling lets
   Tenderlink decide the fresh stream value, and Crosslink finality uses the live
   dynamic sigma as its tail-confirmation depth.
   */

  import CrosslinkDynamicSigmaResampling.* from "./CrosslinkDynamicSigmaResampling"
  export CrosslinkDynamicSigmaResampling.*

  type Height_t = int
  type Work_t = int
  type Block_t = str

  const MaxHeight: Height_t
  const MaxWork: Work_t
  const InitialFinalized: Snapshot_t
  const PublishedAt: Round_t -> Snapshot_t -> bool
  const HonestWorkAt: Round_t -> Snapshot_t -> Work_t
  const AdversarialWorkAt: Round_t -> Snapshot_t -> Work_t
  const TieBreakRank: Snapshot_t -> int
  const AncestorAt: Snapshot_t -> Height_t -> Block_t

  pure val Heights = 0.to(MaxHeight)

  assume initial_finalized_is_snapshot =
    Snapshots.contains(InitialFinalized)
  assume max_work_non_negative =
    MaxWork >= 0
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

  var latestFinal: Snapshot_t
  var finalized: Set[Snapshot_t]
  var finalityAction: str

  pure def IsSnapshot(v: Snapshot_t): bool =
    Snapshots.contains(v)

  pure def Height(v: Snapshot_t): Height_t =
    HeightOf.get(v)

  pure def BlockAt(v: Snapshot_t, h: Height_t): Block_t =
    AncestorAt.get(v).get(h)

  pure def TotalWorkAt(r: Round_t, v: Snapshot_t): Work_t =
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

  def Extends(newer: Snapshot_t, older: Snapshot_t): bool =
    IsSnapshot(newer) and
    IsSnapshot(older) and
    Height(newer) >= Height(older) and
    Heights.forall(h =>
      h > Height(older) or BlockAt(newer, h) == BlockAt(older, h)
    )

  def Agrees(a: Snapshot_t, b: Snapshot_t): bool =
    Extends(a, b) or Extends(b, a)

  def TailConfirmsAtSigma(tip: Snapshot_t, candidate: Snapshot_t, sigma: int): bool =
    IsSnapshot(tip) and
    IsSnapshot(candidate) and
    Height(tip) >= Height(candidate) + sigma and
    BlockAt(tip, Height(candidate)) == BlockAt(candidate, Height(candidate))

  def DynamicTailConfirms(tip: Snapshot_t, candidate: Snapshot_t): bool =
    TailConfirmsAtSigma(tip, candidate, dynSigma)

  def ValidFinalityCandidate(candidate: Snapshot_t, tip: Snapshot_t): bool =
    Extends(candidate, latestFinal) and
    Height(candidate) > Height(latestFinal) and
    DynamicTailConfirms(tip, candidate)

  action FullComposedInit = all {
    DynamicResamplingInit,
    latestFinal' = InitialFinalized,
    finalized' = Set(InitialFinalized),
    finalityAction' = "FullComposedInit",
  }

  action FinalityUnchanged = all {
    latestFinal' = latestFinal,
    finalized' = finalized,
    finalityAction' = finalityAction,
  }

  action DynamicAndProtocolUnchanged = all {
    DynamicResamplingStutter,
  }

  action FullSeedValidNil(p: Proc_t): bool =
    all { DynamicSeedValidNil(p), FinalityUnchanged }

  action FullSeedFaultyNil(p: Proc_t): bool =
    all { DynamicSeedFaultyNil(p), FinalityUnchanged }

  action FullSeedSameRoundLock(p: Proc_t): bool =
    all { DynamicSeedSameRoundLock(p), FinalityUnchanged }

  action FullAdvanceDerivedForkSignal =
    all { AdvanceDerivedForkSignal, FinalityUnchanged }

  action FullAdvanceHashParticipationSignal =
    all { AdvanceHashParticipationSignal, FinalityUnchanged }

  action FullStartNextRound(p: Proc_t): bool =
    all { DynamicStartNextRound(p), FinalityUnchanged }

  action FullInsertProposal(p: Proc_t): bool =
    all { DynamicInsertProposal(p), FinalityUnchanged }

  action FullPrevote(p: Proc_t, v: Snapshot_t): bool =
    all { DynamicPrevote(p, v), FinalityUnchanged }

  action FullPrecommitValue(p: Proc_t, v: Snapshot_t): bool =
    all { DynamicPrecommitValue(p, v), FinalityUnchanged }

  action FullDecide(p: Proc_t, r: Round_t, v: Snapshot_t): bool =
    all { DynamicDecide(p, r, v), FinalityUnchanged }

  action FullFinalizeDecision(p: Proc_t, candidate: Snapshot_t, tip: Snapshot_t): bool = all {
    decision.get(p) == candidate,
    ValidFinalityCandidate(candidate, tip),
    DynamicAndProtocolUnchanged,
    latestFinal' = candidate,
    finalized' = finalized.union(Set(candidate)),
    finalityAction' = "FullFinalizeDecision",
  }

  action FullProtocolNext =
    all { DynamicResamplingNext, FinalityUnchanged }

  action FullComposedStutter = all {
    DynamicAndProtocolUnchanged,
    FinalityUnchanged,
  }

  action FullComposedNext =
    any {
      FullProtocolNext,
      nondet p = oneOf(Corr)
      nondet candidate = oneOf(Snapshots)
      nondet tip = oneOf(Snapshots)
      FullFinalizeDecision(p, candidate, tip),
      FullComposedStutter,
    }

  val FinalizedPrefixLinear =
    tuples(finalized, finalized).forall(((a, b)) =>
      Agrees(a, b)
    )

  val LatestFinalExtendsAllFinalized =
    finalized.forall(v => Extends(latestFinal, v))

  val InitialFinalizedRemainsFinalized =
    finalized.contains(InitialFinalized)

  val CurrentDynamicBestTipMatchesGeneratedCompetition =
    IsBestTipAt(dynRound, BestTip.get(dynRound))

  val FinalitySafety =
    FinalizedPrefixLinear and
    LatestFinalExtendsAllFinalized and
    InitialFinalizedRemainsFinalized

  val FullComposedSafety =
    DynamicResamplingSafety and
    FinalitySafety and
    CurrentDynamicBestTipMatchesGeneratedCompetition
}

module CrosslinkDynamicSigmaFinalityTest {
  import CrosslinkDynamicSigmaFinality.* from "./CrosslinkDynamicSigmaFinality"
  export CrosslinkDynamicSigmaFinality.*

  run dynamicSigmaResamplingFinalizesTailConfirmedFreshCandidateTest = {
    FullComposedInit
      .then(FullSeedValidNil("p1"))
      .then(FullSeedValidNil("p2"))
      .then(FullSeedFaultyNil("p4"))
      .then(FullSeedSameRoundLock("p3"))
      .then(FullAdvanceDerivedForkSignal)
      .then(all {
        assert(dynSigma == RaisedSigma),
        assert(dynamicAction == "DerivedForkRaiseSigma"),
        assert(FullComposedSafety),
        FullComposedStutter,
      })
      .then(FullStartNextRound("p1"))
      .then(FullStartNextRound("p2"))
      .then(FullStartNextRound("p3"))
      .then(FullInsertProposal("p2"))
      .then(FullPrevote("p1", "b2"))
      .then(FullPrevote("p2", "b2"))
      .then(FullPrevote("p3", "b2"))
      .then(FullPrecommitValue("p1", "b2"))
      .then(FullPrecommitValue("p2", "b2"))
      .then(FullPrecommitValue("p3", "b2"))
      .then(FullDecide("p1", 1, "b2"))
      .then(FullFinalizeDecision("p1", "b2", "b5"))
      .then(all {
        assert(latestFinal == "b2"),
        assert(finalized.contains("b2")),
        assert(dynSigma == RaisedSigma),
        assert(FullComposedSafety),
        FullComposedStutter,
      })
  }

  run dynamicSigmaRejectsUnderconfirmedFreshCandidateTest = {
    FullComposedInit
      .then(FullSeedValidNil("p1"))
      .then(FullSeedValidNil("p2"))
      .then(FullSeedFaultyNil("p4"))
      .then(FullSeedSameRoundLock("p3"))
      .then(FullAdvanceDerivedForkSignal)
      .then(FullStartNextRound("p1"))
      .then(FullStartNextRound("p2"))
      .then(FullStartNextRound("p3"))
      .then(FullInsertProposal("p2"))
      .then(FullPrevote("p1", "b2"))
      .then(FullPrevote("p2", "b2"))
      .then(FullPrevote("p3", "b2"))
      .then(FullPrecommitValue("p1", "b2"))
      .then(FullPrecommitValue("p2", "b2"))
      .then(FullPrecommitValue("p3", "b2"))
      .then(FullDecide("p1", 1, "b2"))
      .then(FullFinalizeDecision("p1", "b2", "b4"))
      .fail()
  }

  run generatedCompetitionBacksFullCompositionForkSignalTest = all {
    assert(IsBestTipAt(0, "a3")),
    assert(not(CandidateAt(0, "b4"))),
    assert(IsBestTipAt(1, "b4")),
    assert(DerivedReorgDepth(1) == 2),
  }
}

module CrosslinkDynamicSigmaFinalityModel {
  import CrosslinkDynamicSigmaFinalityTest(
    Corr = Set("p1", "p2", "p3"),
    Faulty = Set("p4"),
    N = 4,
    T = 1,
    MaxRound = 2,
    Proposer = Map(0 -> "p1", 1 -> "p2", 2 -> "p3"),
    Stream = Map(0 -> "a2", 1 -> "b2", 2 -> "b2"),
    Snapshots = Set("g", "a1", "a2", "a3", "b2", "b3", "b4", "b5"),
    ResampleOnNilPrecommit = true,
    BaseSigma = 1,
    RaisedSigma = 3,
    MaxSigma = 6,
    MaxWork = 6,
    TargetHashParticipationPct = 67,
    CriticalHashParticipationPct = 50,
    HashParticipationPct = Map(
      0 -> 90,
      1 -> 90,
      2 -> 90,
    ),
    BestTip = Map(
      0 -> "a3",
      1 -> "b4",
      2 -> "b5",
    ),
    PublishedAt = Map(
      0 -> Map(
        "g" -> true,
        "a1" -> true,
        "a2" -> true,
        "a3" -> true,
        "b2" -> false,
        "b3" -> false,
        "b4" -> false,
        "b5" -> false,
      ),
      1 -> Map(
        "g" -> true,
        "a1" -> true,
        "a2" -> true,
        "a3" -> true,
        "b2" -> true,
        "b3" -> true,
        "b4" -> true,
        "b5" -> false,
      ),
      2 -> Map(
        "g" -> true,
        "a1" -> true,
        "a2" -> true,
        "a3" -> true,
        "b2" -> true,
        "b3" -> true,
        "b4" -> true,
        "b5" -> true,
      ),
    ),
    HonestWorkAt = Map(
      0 -> Map("g" -> 0, "a1" -> 1, "a2" -> 2, "a3" -> 3, "b2" -> 0, "b3" -> 0, "b4" -> 0, "b5" -> 0),
      1 -> Map("g" -> 0, "a1" -> 1, "a2" -> 2, "a3" -> 3, "b2" -> 0, "b3" -> 0, "b4" -> 0, "b5" -> 0),
      2 -> Map("g" -> 0, "a1" -> 1, "a2" -> 2, "a3" -> 3, "b2" -> 0, "b3" -> 0, "b4" -> 0, "b5" -> 0),
    ),
    AdversarialWorkAt = Map(
      0 -> Map("g" -> 0, "a1" -> 0, "a2" -> 0, "a3" -> 0, "b2" -> 2, "b3" -> 3, "b4" -> 5, "b5" -> 0),
      1 -> Map("g" -> 0, "a1" -> 0, "a2" -> 0, "a3" -> 0, "b2" -> 2, "b3" -> 3, "b4" -> 5, "b5" -> 0),
      2 -> Map("g" -> 0, "a1" -> 0, "a2" -> 0, "a3" -> 0, "b2" -> 2, "b3" -> 3, "b4" -> 5, "b5" -> 6),
    ),
    TieBreakRank = Map(
      "g" -> 0,
      "a1" -> 1,
      "a2" -> 2,
      "a3" -> 3,
      "b2" -> 4,
      "b3" -> 5,
      "b4" -> 6,
      "b5" -> 7,
    ),
    MaxHeight = 5,
    InitialFinalized = "g",
    HeightOf = Map(
      "g" -> 0,
      "a1" -> 1,
      "a2" -> 2,
      "a3" -> 3,
      "b2" -> 2,
      "b3" -> 3,
      "b4" -> 4,
      "b5" -> 5,
    ),
    AncestorAt = Map(
      "g" -> Map(0 -> "g", 1 -> "None", 2 -> "None", 3 -> "None", 4 -> "None", 5 -> "None"),
      "a1" -> Map(0 -> "g", 1 -> "a1", 2 -> "None", 3 -> "None", 4 -> "None", 5 -> "None"),
      "a2" -> Map(0 -> "g", 1 -> "a1", 2 -> "a2", 3 -> "None", 4 -> "None", 5 -> "None"),
      "a3" -> Map(0 -> "g", 1 -> "a1", 2 -> "a2", 3 -> "a3", 4 -> "None", 5 -> "None"),
      "b2" -> Map(0 -> "g", 1 -> "a1", 2 -> "b2", 3 -> "None", 4 -> "None", 5 -> "None"),
      "b3" -> Map(0 -> "g", 1 -> "a1", 2 -> "b2", 3 -> "b3", 4 -> "None", 5 -> "None"),
      "b4" -> Map(0 -> "g", 1 -> "a1", 2 -> "b2", 3 -> "b3", 4 -> "b4", 5 -> "None"),
      "b5" -> Map(0 -> "g", 1 -> "a1", 2 -> "b2", 3 -> "b3", 4 -> "b4", 5 -> "b5"),
    ),
    LcaHeight = Map(
      "a3" -> Map("b4" -> 1, "b5" -> 1),
      "b4" -> Map("b4" -> 4, "b5" -> 4),
      "b5" -> Map("b4" -> 4, "b5" -> 5),
    ),
  ).*
}
