// -*- mode: Bluespec; -*-
module CrosslinkBaselinePowSamplingModel {
  /*
   Baseline Crosslink samples a fixed-depth PoW ancestor: `head - sigma`.

   The round machine in CrosslinkBaseline.qnt accepts `Stream(round)` as an
   input. This model pins that stream to an explicit best-tip schedule and a
   fixed sigma value, then checks the sticky baseline behavior when a fork switch
   rolls back the previously sampled ancestor.
   */

  import CrosslinkResamplingTest(
    Corr = Set("p1", "p2", "p3"),
    Faulty = Set("p4"),
    N = 4,
    T = 1,
    MaxRound = 2,
    Proposer = Map(0 -> "p1", 1 -> "p2", 2 -> "p3"),
    Stream = Map(0 -> "a3", 1 -> "b3", 2 -> "b3"),
    Snapshots = Set("g", "a1", "a2", "a3", "a4", "b3", "b4"),
    ResampleOnNilPrecommit = false,
    ExpectedRound1Proposal = "a3",
  ).* from "./CrosslinkResampling"

  type Height_t = int

  pure val FixedSigma = 1
  pure val MaxHeight = 4
  pure val Heights = 0.to(MaxHeight)

  pure val BestTipByRound = Map(
    0 -> "a4",
    1 -> "b4",
    2 -> "b4",
  )

  pure val HeightOf = Map(
    "g" -> 0,
    "a1" -> 1,
    "a2" -> 2,
    "a3" -> 3,
    "a4" -> 4,
    "b3" -> 3,
    "b4" -> 4,
  )

  pure val AncestorAt = Map(
    "g" -> Map(0 -> "g", 1 -> "None", 2 -> "None", 3 -> "None", 4 -> "None"),
    "a1" -> Map(0 -> "g", 1 -> "a1", 2 -> "None", 3 -> "None", 4 -> "None"),
    "a2" -> Map(0 -> "g", 1 -> "a1", 2 -> "a2", 3 -> "None", 4 -> "None"),
    "a3" -> Map(0 -> "g", 1 -> "a1", 2 -> "a2", 3 -> "a3", 4 -> "None"),
    "a4" -> Map(0 -> "g", 1 -> "a1", 2 -> "a2", 3 -> "a3", 4 -> "a4"),
    "b3" -> Map(0 -> "g", 1 -> "a1", 2 -> "b2", 3 -> "b3", 4 -> "None"),
    "b4" -> Map(0 -> "g", 1 -> "a1", 2 -> "b2", 3 -> "b3", 4 -> "b4"),
  )

  pure val LcaHeight = Map(
    "a4" -> Map("a4" -> 4, "b4" -> 2),
    "b4" -> Map("a4" -> 2, "b4" -> 4),
  )

  pure def BestTipAt(r: Round_t): Snapshot_t =
    BestTipByRound.get(r)

  pure def Height(v: Snapshot_t): Height_t =
    HeightOf.get(v)

  pure def AncestorAtHeight(v: Snapshot_t, h: Height_t): Snapshot_t =
    AncestorAt.get(v).get(h)

  pure def HeadMinusFixedSigma(r: Round_t): Snapshot_t =
    AncestorAtHeight(BestTipAt(r), Height(BestTipAt(r)) - FixedSigma)

  pure def RollbackDepth(previousTip: Snapshot_t, nextTip: Snapshot_t): int =
    Height(previousTip) - LcaHeight.get(previousTip).get(nextTip)

  pure def FixedSampleSurvivesRollback(previousTip: Snapshot_t, nextTip: Snapshot_t): bool =
    RollbackDepth(previousTip, nextTip) < FixedSigma

  val StreamMatchesFixedHeadMinusSigma =
    Rounds.forall(r => Stream.get(r) == HeadMinusFixedSigma(r))

  val BaselinePowSamplingSafety =
    Safety and StreamMatchesFixedHeadMinusSigma

  run fixedSigmaStreamDerivesFromBestTipAncestorsTest = all {
    assert(HeadMinusFixedSigma(0) == "a3"),
    assert(HeadMinusFixedSigma(1) == "b3"),
    assert(StreamMatchesFixedHeadMinusSigma),
  }

  run forkSwitchRollsBackFixedSigmaSampleTest = all {
    assert(RollbackDepth(BestTipAt(0), BestTipAt(1)) == 2),
    assert(not(FixedSampleSurvivesRollback(BestTipAt(0), BestTipAt(1)))),
    assert(HeadMinusFixedSigma(0) != HeadMinusFixedSigma(1)),
  }

  run stickyBaselineCarriesRolledBackHeadMinusSigmaTest = {
    Init
      .then(SeedAbandonedRoundState("p1"))
      .then(SeedAbandonedRoundState("p2"))
      .then(SeedAbandonedRoundState("p3"))
      .then(StartNextRoundAfterPrecommitQuorum("p2"))
      .then(InsertProposal("p2"))
      .then(all {
        assert(msgsPropose.get(1).exists(m =>
          m.src == "p2" and m.round == 1 and m.proposal == HeadMinusFixedSigma(0)
        )),
        assert(HeadMinusFixedSigma(0) == "a3"),
        assert(HeadMinusFixedSigma(1) == "b3"),
        assert(not(FixedSampleSurvivesRollback(BestTipAt(0), BestTipAt(1)))),
        assert(BaselinePowSamplingSafety),
        unchangedAll,
      })
  }

  run stickyBaselineCannotPrevoteFreshForkSampleTest = {
    Init
      .then(SeedValidNilPrecommitState("p1"))
      .then(SeedValidNilPrecommitState("p2"))
      .then(SeedFaultyNilPrecommit("p4"))
      .then(SeedSameRoundValueLock("p3"))
      .then(StartNextRoundAfterPrecommitQuorum("p1"))
      .then(StartNextRoundAfterPrecommitQuorum("p2"))
      .then(StartNextRoundAfterPrecommitQuorum("p3"))
      .then(InsertProposal("p2"))
      .then(UponProposalPrevote("p1", HeadMinusFixedSigma(1)))
      .fail()
  }
}

module CrosslinkBaselinePowLongReorgModel {
  /*
   A long-reorg fixed-sigma fixture. The best tip switches from `a5` to `c5`
   with common ancestor `a2`, so the rollback depth is 3 while sigma is 2. The
   sticky baseline still carries the old `head - sigma` sample after the fork
   switch.
   */

  import CrosslinkResamplingTest(
    Corr = Set("p1", "p2", "p3"),
    Faulty = Set("p4"),
    N = 4,
    T = 1,
    MaxRound = 2,
    Proposer = Map(0 -> "p1", 1 -> "p2", 2 -> "p3"),
    Stream = Map(0 -> "a3", 1 -> "c3", 2 -> "c3"),
    Snapshots = Set("g", "a1", "a2", "a3", "a4", "a5", "c3", "c4", "c5"),
    ResampleOnNilPrecommit = false,
    ExpectedRound1Proposal = "a3",
  ).* from "./CrosslinkResampling"

  type Height_t = int

  pure val FixedSigma = 2
  pure val MaxHeight = 5
  pure val Heights = 0.to(MaxHeight)

  pure val BestTipByRound = Map(
    0 -> "a5",
    1 -> "c5",
    2 -> "c5",
  )

  pure val HeightOf = Map(
    "g" -> 0,
    "a1" -> 1,
    "a2" -> 2,
    "a3" -> 3,
    "a4" -> 4,
    "a5" -> 5,
    "c3" -> 3,
    "c4" -> 4,
    "c5" -> 5,
  )

  pure val AncestorAt = Map(
    "g" -> Map(0 -> "g", 1 -> "None", 2 -> "None", 3 -> "None", 4 -> "None", 5 -> "None"),
    "a1" -> Map(0 -> "g", 1 -> "a1", 2 -> "None", 3 -> "None", 4 -> "None", 5 -> "None"),
    "a2" -> Map(0 -> "g", 1 -> "a1", 2 -> "a2", 3 -> "None", 4 -> "None", 5 -> "None"),
    "a3" -> Map(0 -> "g", 1 -> "a1", 2 -> "a2", 3 -> "a3", 4 -> "None", 5 -> "None"),
    "a4" -> Map(0 -> "g", 1 -> "a1", 2 -> "a2", 3 -> "a3", 4 -> "a4", 5 -> "None"),
    "a5" -> Map(0 -> "g", 1 -> "a1", 2 -> "a2", 3 -> "a3", 4 -> "a4", 5 -> "a5"),
    "c3" -> Map(0 -> "g", 1 -> "a1", 2 -> "a2", 3 -> "c3", 4 -> "None", 5 -> "None"),
    "c4" -> Map(0 -> "g", 1 -> "a1", 2 -> "a2", 3 -> "c3", 4 -> "c4", 5 -> "None"),
    "c5" -> Map(0 -> "g", 1 -> "a1", 2 -> "a2", 3 -> "c3", 4 -> "c4", 5 -> "c5"),
  )

  pure val LcaHeight = Map(
    "a5" -> Map("a5" -> 5, "c5" -> 2),
    "c5" -> Map("a5" -> 2, "c5" -> 5),
  )

  pure def BestTipAt(r: Round_t): Snapshot_t =
    BestTipByRound.get(r)

  pure def Height(v: Snapshot_t): Height_t =
    HeightOf.get(v)

  pure def AncestorAtHeight(v: Snapshot_t, h: Height_t): Snapshot_t =
    AncestorAt.get(v).get(h)

  pure def HeadMinusFixedSigma(r: Round_t): Snapshot_t =
    AncestorAtHeight(BestTipAt(r), Height(BestTipAt(r)) - FixedSigma)

  pure def RollbackDepth(previousTip: Snapshot_t, nextTip: Snapshot_t): int =
    Height(previousTip) - LcaHeight.get(previousTip).get(nextTip)

  pure def FixedSampleSurvivesRollback(previousTip: Snapshot_t, nextTip: Snapshot_t): bool =
    RollbackDepth(previousTip, nextTip) < FixedSigma

  val StreamMatchesFixedHeadMinusSigma =
    Rounds.forall(r => Stream.get(r) == HeadMinusFixedSigma(r))

  val BaselinePowLongReorgSafety =
    Safety and StreamMatchesFixedHeadMinusSigma

  run longReorgExceedsFixedSigmaTest = all {
    assert(HeadMinusFixedSigma(0) == "a3"),
    assert(HeadMinusFixedSigma(1) == "c3"),
    assert(RollbackDepth(BestTipAt(0), BestTipAt(1)) == 3),
    assert(FixedSigma == 2),
    assert(not(FixedSampleSurvivesRollback(BestTipAt(0), BestTipAt(1)))),
    assert(StreamMatchesFixedHeadMinusSigma),
  }

  run stickyBaselineCarriesLongReorgHeadMinusSigmaTest = {
    Init
      .then(SeedAbandonedRoundState("p1"))
      .then(SeedAbandonedRoundState("p2"))
      .then(SeedAbandonedRoundState("p3"))
      .then(StartNextRoundAfterPrecommitQuorum("p2"))
      .then(InsertProposal("p2"))
      .then(all {
        assert(msgsPropose.get(1).exists(m =>
          m.src == "p2" and m.round == 1 and m.proposal == HeadMinusFixedSigma(0)
        )),
        assert(HeadMinusFixedSigma(0) == "a3"),
        assert(HeadMinusFixedSigma(1) == "c3"),
        assert(not(FixedSampleSurvivesRollback(BestTipAt(0), BestTipAt(1)))),
        assert(BaselinePowLongReorgSafety),
        unchangedAll,
      })
  }

  run stickyBaselineCannotPrevoteFreshLongReorgSampleTest = {
    Init
      .then(SeedValidNilPrecommitState("p1"))
      .then(SeedValidNilPrecommitState("p2"))
      .then(SeedFaultyNilPrecommit("p4"))
      .then(SeedSameRoundValueLock("p3"))
      .then(StartNextRoundAfterPrecommitQuorum("p1"))
      .then(StartNextRoundAfterPrecommitQuorum("p2"))
      .then(StartNextRoundAfterPrecommitQuorum("p3"))
      .then(InsertProposal("p2"))
      .then(UponProposalPrevote("p1", HeadMinusFixedSigma(1)))
      .fail()
  }
}

module CrosslinkBaselinePowGeneratedScheduleModel {
  /*
   A bounded generated PoW schedule for the fixed-sigma baseline. Published work
   first selects the honest `a` branch, then an adversarially released `b4`
   branch outworks it. The baseline stream is derived from the generated best
   tips instead of being treated as a standalone hand-authored map.
   */

  import CrosslinkResamplingTest(
    Corr = Set("p1", "p2", "p3"),
    Faulty = Set("p4"),
    N = 4,
    T = 1,
    MaxRound = 2,
    Proposer = Map(0 -> "p1", 1 -> "p2", 2 -> "p3"),
    Stream = Map(0 -> "a2", 1 -> "a3", 2 -> "b3"),
    Snapshots = Set("g", "a1", "a2", "a3", "a4", "b3", "b4"),
    ResampleOnNilPrecommit = false,
    ExpectedRound1Proposal = "a2",
  ).* from "./CrosslinkResampling"

  type Height_t = int
  type Work_t = int

  pure val FixedSigma = 1
  pure val MaxHeight = 4
  pure val MaxWork = 5

  pure val BestTipByRound = Map(
    0 -> "a3",
    1 -> "a4",
    2 -> "b4",
  )

  pure val PublishedAt = Map(
    0 -> Map(
      "g" -> true,
      "a1" -> true,
      "a2" -> true,
      "a3" -> true,
      "a4" -> false,
      "b3" -> false,
      "b4" -> false,
    ),
    1 -> Map(
      "g" -> true,
      "a1" -> true,
      "a2" -> true,
      "a3" -> true,
      "a4" -> true,
      "b3" -> false,
      "b4" -> false,
    ),
    2 -> Map(
      "g" -> true,
      "a1" -> true,
      "a2" -> true,
      "a3" -> true,
      "a4" -> true,
      "b3" -> true,
      "b4" -> true,
    ),
  )

  pure val HonestWorkAt = Map(
    0 -> Map("g" -> 0, "a1" -> 1, "a2" -> 2, "a3" -> 3, "a4" -> 0, "b3" -> 0, "b4" -> 0),
    1 -> Map("g" -> 0, "a1" -> 1, "a2" -> 2, "a3" -> 3, "a4" -> 4, "b3" -> 0, "b4" -> 0),
    2 -> Map("g" -> 0, "a1" -> 1, "a2" -> 2, "a3" -> 3, "a4" -> 4, "b3" -> 0, "b4" -> 0),
  )

  pure val AdversarialWorkAt = Map(
    0 -> Map("g" -> 0, "a1" -> 0, "a2" -> 0, "a3" -> 0, "a4" -> 0, "b3" -> 4, "b4" -> 4),
    1 -> Map("g" -> 0, "a1" -> 0, "a2" -> 0, "a3" -> 0, "a4" -> 0, "b3" -> 4, "b4" -> 4),
    2 -> Map("g" -> 0, "a1" -> 0, "a2" -> 0, "a3" -> 0, "a4" -> 0, "b3" -> 5, "b4" -> 5),
  )

  pure val TieBreakRank = Map(
    "g" -> 0,
    "a1" -> 1,
    "a2" -> 2,
    "a3" -> 3,
    "a4" -> 4,
    "b3" -> 5,
    "b4" -> 6,
  )

  pure val HeightOf = Map(
    "g" -> 0,
    "a1" -> 1,
    "a2" -> 2,
    "a3" -> 3,
    "a4" -> 4,
    "b3" -> 3,
    "b4" -> 4,
  )

  pure val AncestorAt = Map(
    "g" -> Map(0 -> "g", 1 -> "None", 2 -> "None", 3 -> "None", 4 -> "None"),
    "a1" -> Map(0 -> "g", 1 -> "a1", 2 -> "None", 3 -> "None", 4 -> "None"),
    "a2" -> Map(0 -> "g", 1 -> "a1", 2 -> "a2", 3 -> "None", 4 -> "None"),
    "a3" -> Map(0 -> "g", 1 -> "a1", 2 -> "a2", 3 -> "a3", 4 -> "None"),
    "a4" -> Map(0 -> "g", 1 -> "a1", 2 -> "a2", 3 -> "a3", 4 -> "a4"),
    "b3" -> Map(0 -> "g", 1 -> "a1", 2 -> "a2", 3 -> "b3", 4 -> "None"),
    "b4" -> Map(0 -> "g", 1 -> "a1", 2 -> "a2", 3 -> "b3", 4 -> "b4"),
  )

  pure val LcaHeight = Map(
    "a3" -> Map("a3" -> 3, "a4" -> 3, "b4" -> 2),
    "a4" -> Map("a3" -> 3, "a4" -> 4, "b4" -> 2),
    "b4" -> Map("a3" -> 2, "a4" -> 2, "b4" -> 4),
  )

  pure def Height(v: Snapshot_t): Height_t =
    HeightOf.get(v)

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

  pure def BestTipAt(r: Round_t): Snapshot_t =
    BestTipByRound.get(r)

  pure def AncestorAtHeight(v: Snapshot_t, h: Height_t): Snapshot_t =
    AncestorAt.get(v).get(h)

  pure def HeadMinusFixedSigma(r: Round_t): Snapshot_t =
    AncestorAtHeight(BestTipAt(r), Height(BestTipAt(r)) - FixedSigma)

  pure def RollbackDepth(previousTip: Snapshot_t, nextTip: Snapshot_t): int =
    Height(previousTip) - LcaHeight.get(previousTip).get(nextTip)

  pure def SampleSurvivesRollback(previousTip: Snapshot_t, nextTip: Snapshot_t): bool =
    RollbackDepth(previousTip, nextTip) < FixedSigma

  val GeneratedBestTipsMatchCompetition =
    Rounds.forall(r => IsBestTipAt(r, BestTipAt(r)))

  val StreamMatchesGeneratedHeadMinusSigma =
    Rounds.forall(r => Stream.get(r) == HeadMinusFixedSigma(r))

  val BaselinePowGeneratedScheduleSafety =
    Safety and
    GeneratedBestTipsMatchCompetition and
    StreamMatchesGeneratedHeadMinusSigma

  action SeedGeneratedRound1Abandonment(p: Proc_t): bool = {
    val r = 1
    val stale = Stream.get(r)
    all {
      r.in(Rounds),
      p.in(Corr),
      round' = round.set(p, r),
      step' = step.set(p, "precommit"),
      lockedValue' = lockedValue,
      lockedRound' = lockedRound,
      validValue' = validValue,
      validRound' = validRound,
      cachedProposal' = cachedProposal.set(p, stale),
      cachedProposalRound' = cachedProposalRound.set(p, r),
      BroadcastPrecommit(p, r, NilSnapshot),
      firedAction' = "SeedGeneratedRound1Abandonment",
      decision' = decision,
      msgsPropose' = msgsPropose,
      msgsPrevote' = msgsPrevote,
      evidencePropose' = evidencePropose,
      evidencePrevote' = evidencePrevote,
    }
  }

  run generatedScheduleSelectsHonestThenAdversarialTipsTest = all {
    assert(IsBestTipAt(0, "a3")),
    assert(IsBestTipAt(1, "a4")),
    assert(not(CandidateAt(1, "b4"))),
    assert(IsBestTipAt(2, "b4")),
    assert(TotalWorkAt(2, "b4") > TotalWorkAt(2, "a4")),
    assert(RollbackDepth(BestTipAt(1), BestTipAt(2)) == 2),
    assert(not(SampleSurvivesRollback(BestTipAt(1), BestTipAt(2)))),
    assert(GeneratedBestTipsMatchCompetition),
  }

  run generatedScheduleDerivesBaselineStreamTest = all {
    assert(HeadMinusFixedSigma(0) == "a2"),
    assert(HeadMinusFixedSigma(1) == "a3"),
    assert(HeadMinusFixedSigma(2) == "b3"),
    assert(StreamMatchesGeneratedHeadMinusSigma),
  }

  run stickyBaselineCarriesGeneratedAdversarialForkSampleTest = {
    Init
      .then(SeedGeneratedRound1Abandonment("p1"))
      .then(SeedGeneratedRound1Abandonment("p2"))
      .then(SeedGeneratedRound1Abandonment("p3"))
      .then(StartNextRoundAfterPrecommitQuorum("p3"))
      .then(InsertProposal("p3"))
      .then(all {
        assert(msgsPropose.get(2).exists(m =>
          m.src == "p3" and m.round == 2 and m.proposal == HeadMinusFixedSigma(1)
        )),
        assert(HeadMinusFixedSigma(1) == "a3"),
        assert(HeadMinusFixedSigma(2) == "b3"),
        assert(not(SampleSurvivesRollback(BestTipAt(1), BestTipAt(2)))),
        assert(BaselinePowGeneratedScheduleSafety),
        unchangedAll,
      })
  }
}
