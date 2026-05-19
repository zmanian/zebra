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
