// -*- mode: Bluespec; -*-
module CrosslinkBaselineN4F1StableTest {
  /*
   Upstream-style smoke tests for the parameterized baseline shell. This mirrors
   the upstream Tendermint test shape while checking Crosslink's fixed
   `head - sigma` value selection.
   */

  import CrosslinkBaselineModels.* from "./CrosslinkBaselineModels"

  pure val TestRounds = 0.to(4)

  pure def TestHeadMinusSigma(r: int): str =
    AncestorByHeight
      .get(StableBestTip.get(r))
      .get(HeightBySnapshot.get(StableBestTip.get(r)) - 1)

  pure val TestStream: int -> str =
    TestRounds.mapBy(r => TestHeadMinusSigma(r))

  pure val TestStreamMatchesFixedSigma =
    TestRounds.forall(r => TestStream.get(r) == TestHeadMinusSigma(r))

  import CrosslinkResamplingTest(
    Corr = Set("p1", "p2", "p3"),
    Faulty = Set("p4"),
    N = 4,
    T = 1,
    MaxRound = 4,
    Proposer = ProposerSchedule,
    Stream = TestStream,
    Snapshots = ValidSnapshots,
    ResampleOnNilPrecommit = false,
    ExpectedRound1Proposal = TestStream.get(0),
  ).* from "./CrosslinkResampling"

  val BaselineN4F1StableSafety =
    Safety and TestStreamMatchesFixedSigma

  run fixedSigmaSampleTest = all {
    assert(TestHeadMinusSigma(0) == "a1"),
    assert(TestHeadMinusSigma(1) == "a1"),
    assert(TestStreamMatchesFixedSigma),
  }

  run decisionTest = {
    Init
      .then(InsertProposal("p1"))
      .then(UponProposalPrevote("p1", "a1"))
      .then(UponProposalPrevote("p2", "a1"))
      .then(UponProposalPrevote("p3", "a1"))
      .then(UponValuePrevoteQuorum("p1", "a1"))
      .then(UponValuePrevoteQuorum("p2", "a1"))
      .then(UponValuePrevoteQuorum("p3", "a1"))
      .then(Decide("p1", 0, "a1"))
      .then(all {
        assert(decision.get("p1") == "a1"),
        assert(BaselineN4F1StableSafety),
        unchangedAll,
      })
  }

  run noProposeTwiceTest = {
    Init
      .then(InsertProposal("p1"))
      .then(InsertProposal("p1"))
      .fail()
  }
}

module CrosslinkBaselineN4F1ForkingTest {
  /*
   A small stream-change instance for the upstream-shaped baseline shell. It
   keeps the baseline sticky behavior explicit: a nil-precommit round advance
   still carries the round-0 fixed-sigma sample.
   */

  import CrosslinkBaselineModels.* from "./CrosslinkBaselineModels"

  pure val TestRounds = 0.to(4)

  pure def TestHeadMinusSigma(r: int): str =
    AncestorByHeight
      .get(ForkingBestTip.get(r))
      .get(HeightBySnapshot.get(ForkingBestTip.get(r)) - 1)

  pure val TestStream: int -> str =
    TestRounds.mapBy(r => TestHeadMinusSigma(r))

  pure val TestStreamMatchesFixedSigma =
    TestRounds.forall(r => TestStream.get(r) == TestHeadMinusSigma(r))

  import CrosslinkResamplingTest(
    Corr = Set("p1", "p2", "p3"),
    Faulty = Set("p4"),
    N = 4,
    T = 1,
    MaxRound = 4,
    Proposer = ProposerSchedule,
    Stream = TestStream,
    Snapshots = ValidSnapshots,
    ResampleOnNilPrecommit = false,
    ExpectedRound1Proposal = TestStream.get(0),
  ).* from "./CrosslinkResampling"

  val BaselineN4F1ForkingSafety =
    Safety and TestStreamMatchesFixedSigma

  run streamChangeDerivesFreshHeadMinusSigmaTest = all {
    assert(TestHeadMinusSigma(0) == "a1"),
    assert(TestHeadMinusSigma(1) == "b1"),
    assert(TestStreamMatchesFixedSigma),
  }

  run stickyBaselineCarriesStaleFixedSigmaSampleTest = {
    Init
      .then(SeedAbandonedRoundState("p1"))
      .then(SeedAbandonedRoundState("p2"))
      .then(SeedAbandonedRoundState("p3"))
      .then(StartNextRoundAfterPrecommitQuorum("p2"))
      .then(InsertProposal("p2"))
      .then(all {
        assert(msgsPropose.get(1).exists(m =>
          m.src == "p2" and
            m.round == 1 and
            m.proposal == TestHeadMinusSigma(0)
        )),
        assert(TestHeadMinusSigma(0) != TestHeadMinusSigma(1)),
        assert(BaselineN4F1ForkingSafety),
        unchangedAll,
      })
  }
}
