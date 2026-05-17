module CrosslinkComposed {
  /*
   Composition layer for the focused Crosslink models.

   CrosslinkResampling models Tenderlink's moving-value round recovery.
   This module adds the finality-prefix state and checks the end-to-end flow:
   a nil-precommit certificate triggers resampling, the next round decides a
   fresh PoW snapshot, and that decision advances Crosslink finality without
   violating the finalized-prefix rules.
   */

  import CrosslinkResampling.* from "./CrosslinkResampling"
  export CrosslinkResampling.*

  type Height_t = int
  type Block_t = str

  const MaxHeight: Height_t
  const Sigma: int
  const InitialFinalized: Snapshot_t
  const HeightOf: Snapshot_t -> Height_t
  const AncestorAt: Snapshot_t -> Height_t -> Block_t

  assume initial_finalized_is_snapshot =
    Snapshots.contains(InitialFinalized)
  assume sigma_is_positive =
    Sigma >= 1

  pure val Heights = 0.to(MaxHeight)

  var latestFinal: Snapshot_t
  var finalized: Set[Snapshot_t]
  var finalityAction: str

  pure def IsSnapshot(v: Snapshot_t): bool =
    Snapshots.contains(v)

  pure def Height(v: Snapshot_t): Height_t =
    HeightOf.get(v)

  pure def BlockAt(v: Snapshot_t, h: Height_t): Block_t =
    AncestorAt.get(v).get(h)

  def Extends(newer: Snapshot_t, older: Snapshot_t): bool =
    IsSnapshot(newer) and
    IsSnapshot(older) and
    Height(newer) >= Height(older) and
    Heights.forall(h =>
      h > Height(older) or BlockAt(newer, h) == BlockAt(older, h)
    )

  def Agrees(a: Snapshot_t, b: Snapshot_t): bool =
    Extends(a, b) or Extends(b, a)

  def TailConfirms(tip: Snapshot_t, candidate: Snapshot_t): bool =
    IsSnapshot(tip) and
    IsSnapshot(candidate) and
    Height(tip) >= Height(candidate) + Sigma and
    BlockAt(tip, Height(candidate)) == BlockAt(candidate, Height(candidate))

  def ValidFinalityCandidate(candidate: Snapshot_t, tip: Snapshot_t): bool =
    Extends(candidate, latestFinal) and
    Height(candidate) > Height(latestFinal) and
    TailConfirms(tip, candidate)

  action ComposedInit = all {
    Init,
    latestFinal' = InitialFinalized,
    finalized' = Set(InitialFinalized),
    finalityAction' = "ComposedInit",
  }

  action FinalityUnchanged = all {
    latestFinal' = latestFinal,
    finalized' = finalized,
    finalityAction' = finalityAction,
  }

  action ProtocolUnchanged = all {
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
    firedAction' = firedAction,
  }

  action SeedValidNil(p: Proc_t): bool =
    all { SeedValidNilPrecommitState(p), FinalityUnchanged }

  action SeedFaultyNil(p: Proc_t): bool =
    all { SeedFaultyNilPrecommit(p), FinalityUnchanged }

  action SeedSameRoundLock(p: Proc_t): bool =
    all { SeedSameRoundValueLock(p), FinalityUnchanged }

  action AdvanceAfterPrecommitQuorum(p: Proc_t): bool =
    all { StartNextRoundAfterPrecommitQuorum(p), FinalityUnchanged }

  action Propose(p: Proc_t): bool =
    all { InsertProposal(p), FinalityUnchanged }

  action Prevote(p: Proc_t, v: Snapshot_t): bool =
    all { UponProposalPrevote(p, v), FinalityUnchanged }

  action PrecommitValue(p: Proc_t, v: Snapshot_t): bool =
    all { UponValuePrevoteQuorum(p, v), FinalityUnchanged }

  action DecideValue(p: Proc_t, r: Round_t, v: Snapshot_t): bool =
    all { Decide(p, r, v), FinalityUnchanged }

  action FinalizeDecision(p: Proc_t, candidate: Snapshot_t, tip: Snapshot_t): bool = all {
    decision.get(p) == candidate,
    ValidFinalityCandidate(candidate, tip),
    ProtocolUnchanged,
    latestFinal' = candidate,
    finalized' = finalized.union(Set(candidate)),
    finalityAction' = "FinalizeDecision",
  }

  action ProtocolNext =
    all { Next, FinalityUnchanged }

  action ComposedStutter = all {
    ProtocolUnchanged,
    FinalityUnchanged,
  }

  action ComposedNext =
    any {
      ProtocolNext,
      nondet p = oneOf(Corr)
      nondet candidate = oneOf(Snapshots)
      nondet tip = oneOf(Snapshots)
      FinalizeDecision(p, candidate, tip),
      ComposedStutter,
    }

  val FinalizedPrefixLinear =
    tuples(finalized, finalized).forall(((a, b)) =>
      Agrees(a, b)
    )

  val LatestFinalExtendsAllFinalized =
    finalized.forall(v => Extends(latestFinal, v))

  val InitialFinalizedRemainsFinalized =
    finalized.contains(InitialFinalized)

  val FinalitySafety =
    FinalizedPrefixLinear and
    LatestFinalExtendsAllFinalized and
    InitialFinalizedRemainsFinalized

  val ComposedSafety =
    Safety and FinalitySafety
}

module CrosslinkComposedTest {
  import CrosslinkComposed.* from "./CrosslinkComposed"
  export CrosslinkComposed.*

  action composedUnchanged = all {
    ProtocolUnchanged,
    latestFinal' = latestFinal,
    finalized' = finalized,
    finalityAction' = finalityAction,
  }

  run resamplingNilPrecommitFinalizesFreshCandidateTest = {
    ComposedInit
      .then(SeedValidNil("p1"))
      .then(SeedValidNil("p2"))
      .then(SeedFaultyNil("p4"))
      .then(SeedSameRoundLock("p3"))
      .then(AdvanceAfterPrecommitQuorum("p1"))
      .then(AdvanceAfterPrecommitQuorum("p2"))
      .then(AdvanceAfterPrecommitQuorum("p3"))
      .then(Propose("p2"))
      .then(Prevote("p1", "a2"))
      .then(Prevote("p2", "a2"))
      .then(Prevote("p3", "a2"))
      .then(PrecommitValue("p1", "a2"))
      .then(PrecommitValue("p2", "a2"))
      .then(PrecommitValue("p3", "a2"))
      .then(DecideValue("p1", 1, "a2"))
      .then(FinalizeDecision("p1", "a2", "a3"))
      .then(all {
        assert(latestFinal == "a2"),
        assert(finalized.contains("a2")),
        assert(ComposedSafety),
        composedUnchanged,
      })
  }
}

module CrosslinkComposedResamplingModel {
  import CrosslinkComposedTest(
    Corr = Set("p1", "p2", "p3"),
    Faulty = Set("p4"),
    N = 4,
    T = 1,
    MaxRound = 2,
    Proposer = Map(0 -> "p1", 1 -> "p2", 2 -> "p3"),
    Stream = Map(0 -> "a1", 1 -> "a2", 2 -> "a2"),
    Snapshots = Set("g", "a1", "a2", "a3", "b1", "b2", "b3"),
    ResampleOnNilPrecommit = true,
    MaxHeight = 3,
    Sigma = 1,
    InitialFinalized = "g",
    HeightOf = Map(
      "g" -> 0,
      "a1" -> 1,
      "a2" -> 2,
      "a3" -> 3,
      "b1" -> 1,
      "b2" -> 2,
      "b3" -> 3,
    ),
    AncestorAt = Map(
      "g" -> Map(0 -> "g", 1 -> "None", 2 -> "None", 3 -> "None"),
      "a1" -> Map(0 -> "g", 1 -> "a1", 2 -> "None", 3 -> "None"),
      "a2" -> Map(0 -> "g", 1 -> "a1", 2 -> "a2", 3 -> "None"),
      "a3" -> Map(0 -> "g", 1 -> "a1", 2 -> "a2", 3 -> "a3"),
      "b1" -> Map(0 -> "g", 1 -> "b1", 2 -> "None", 3 -> "None"),
      "b2" -> Map(0 -> "g", 1 -> "b1", 2 -> "b2", 3 -> "None"),
      "b3" -> Map(0 -> "g", 1 -> "b1", 2 -> "b2", 3 -> "b3"),
    ),
  ).*
}

module CrosslinkComposedLivenessModel {
  import CrosslinkComposedTest(
    Corr = Set("p1", "p2", "p3"),
    Faulty = Set("p4"),
    N = 4,
    T = 1,
    MaxRound = 2,
    Proposer = Map(0 -> "p1", 1 -> "p2", 2 -> "p3"),
    Stream = Map(0 -> "a1", 1 -> "a2", 2 -> "a2"),
    Snapshots = Set("g", "a1", "a2", "a3", "b1", "b2", "b3"),
    ResampleOnNilPrecommit = true,
    MaxHeight = 3,
    Sigma = 1,
    InitialFinalized = "g",
    HeightOf = Map(
      "g" -> 0,
      "a1" -> 1,
      "a2" -> 2,
      "a3" -> 3,
      "b1" -> 1,
      "b2" -> 2,
      "b3" -> 3,
    ),
    AncestorAt = Map(
      "g" -> Map(0 -> "g", 1 -> "None", 2 -> "None", 3 -> "None"),
      "a1" -> Map(0 -> "g", 1 -> "a1", 2 -> "None", 3 -> "None"),
      "a2" -> Map(0 -> "g", 1 -> "a1", 2 -> "a2", 3 -> "None"),
      "a3" -> Map(0 -> "g", 1 -> "a1", 2 -> "a2", 3 -> "a3"),
      "b1" -> Map(0 -> "g", 1 -> "b1", 2 -> "None", 3 -> "None"),
      "b2" -> Map(0 -> "g", 1 -> "b1", 2 -> "b2", 3 -> "None"),
      "b3" -> Map(0 -> "g", 1 -> "b1", 2 -> "b2", 3 -> "b3"),
    ),
  ).*

  var phase: int

  action LivenessInit = all {
    ComposedInit,
    phase' = 0,
  }

  action LivenessStep = any {
    all { phase == 0, SeedValidNil("p1"), phase' = 1 },
    all { phase == 1, SeedValidNil("p2"), phase' = 2 },
    all { phase == 2, SeedFaultyNil("p4"), phase' = 3 },
    all { phase == 3, SeedSameRoundLock("p3"), phase' = 4 },
    all { phase == 4, AdvanceAfterPrecommitQuorum("p1"), phase' = 5 },
    all { phase == 5, AdvanceAfterPrecommitQuorum("p2"), phase' = 6 },
    all { phase == 6, AdvanceAfterPrecommitQuorum("p3"), phase' = 7 },
    all { phase == 7, Propose("p2"), phase' = 8 },
    all { phase == 8, Prevote("p1", "a2"), phase' = 9 },
    all { phase == 9, Prevote("p2", "a2"), phase' = 10 },
    all { phase == 10, Prevote("p3", "a2"), phase' = 11 },
    all { phase == 11, PrecommitValue("p1", "a2"), phase' = 12 },
    all { phase == 12, PrecommitValue("p2", "a2"), phase' = 13 },
    all { phase == 13, PrecommitValue("p3", "a2"), phase' = 14 },
    all { phase == 14, DecideValue("p1", 1, "a2"), phase' = 15 },
    all { phase == 15, FinalizeDecision("p1", "a2", "a3"), phase' = 16 },
    all { phase == 16, ComposedStutter, phase' = 16 },
  }

  val FreshFinalityByEnd =
    phase < 16 or latestFinal == "a2"

  val LivenessSafety =
    ComposedSafety and FreshFinalityByEnd
}
