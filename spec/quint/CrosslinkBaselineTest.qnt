// -*- mode: Bluespec; -*-
module CrosslinkBaselineParameterizedShellTest {
  /*
   Tests for the upstream-shaped parameterized baseline shell itself. These keep
   the shell honest as it grows beyond import-only model handles.
   */

  import CrosslinkBaselineModels.* from "./CrosslinkBaselineModels"

  import CrosslinkBaselineTenderlink(
    BaselineCorr = Set("p1", "p2", "p3"),
    BaselineFaulty = Set("p4"),
    BaselineN = 4,
    BaselineT = 1,
    BaselineValidSnapshots = ValidSnapshots,
    BaselineInvalidSnapshots = InvalidSnapshots,
    BaselineMaxRound = 4,
    BaselineProposer = ProposerSchedule,
    BaselineSigma = 1,
    BaselineBestTip = ForkingBestTip,
    BaselineHeight = HeightBySnapshot,
    BaselineAncestorAt = AncestorByHeight,
  ).* from "./CrosslinkBaselineTenderlink"

  run parameterizedShellFaultyInitCoversEvidenceTest = {
    BaselineInitWithFaultyEvidence.then(all {
      assert(BaselineFaultyInitDomainWellFormed),
      assert(BaselineEvidenceCoversObservedMessages),
      assert(BaselineFaultyInitSafety),
      BaselineUnchangedAll,
    })
  }

  run parameterizedShellFaultyInitAllowsCorrectProposalTest = {
    BaselineInitWithFaultyEvidence
      .then(BaselineInsertProposal("p1"))
      .then(all {
        assert(BaselineHasProposal(0, BaselineHeadMinusSigma(0))),
        assert(BaselineEvidenceCoversObservedMessages),
        assert(BaselineFaultyInitSafety),
        BaselineUnchangedAll,
      })
  }

  run parameterizedShellFaultyInitNextPreservesSafetyTest = {
    BaselineInitWithFaultyEvidence
      .then(BaselineNext)
      .then(all {
        assert(BaselineFaultyInitSafety),
        BaselineUnchangedAll,
      })
  }
}

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

  run nilPrevoteQuorumPrecommitsNilTest = {
    Init
      .then(SeedCorrectNilPrevoteState("p1", 0))
      .then(SeedCorrectNilPrevoteState("p2", 0))
      .then(SeedCorrectNilPrevoteState("p3", 0))
      .then(UponNilPrevoteQuorum("p1"))
      .then(UponNilPrevoteQuorum("p2"))
      .then(UponNilPrevoteQuorum("p3"))
      .then(all {
        assert(PrevoteQuorum(0, NilSnapshot)),
        assert(NilPrecommitCert(0)),
        assert(BaselineN4F1StableSafety),
        unchangedAll,
      })
  }

  run timeoutPrevotePathFormsNilPrecommitCertTest = {
    Init
      .then(TimeoutProposePrevoteNil("p1"))
      .then(TimeoutProposePrevoteNil("p2"))
      .then(TimeoutProposePrevoteNil("p3"))
      .then(TimeoutPrevotePrecommitNil("p1"))
      .then(TimeoutPrevotePrecommitNil("p2"))
      .then(TimeoutPrevotePrecommitNil("p3"))
      .then(all {
        assert(PrevoteQuorum(0, NilSnapshot)),
        assert(NilPrecommitCert(0)),
        assert(BaselineN4F1StableSafety),
        unchangedAll,
      })
  }

  run timeoutPrecommitAdvancesWithoutPrecommitQuorumTest = {
    Init
      .then(TimeoutProposePrevoteNil("p1"))
      .then(TimeoutPrevotePrecommitNil("p1"))
      .then(TimeoutPrecommitStartNextRound("p1"))
      .then(all {
        assert(round.get("p1") == 1),
        assert(step.get("p1") == "propose"),
        assert(not(NilPrecommitCert(0))),
        assert(BaselineN4F1StableSafety),
        unchangedAll,
      })
  }

  run roundCatchupStartsFutureRoundTest = {
    Init
      .then(SeedCorrectNilPrevoteState("p2", 2))
      .then(SeedCorrectNilPrevoteState("p3", 2))
      .then(CatchUpToRound("p1", 2))
      .then(all {
        assert(RoundCatchupEvidence(2)),
        assert(round.get("p1") == 2),
        assert(step.get("p1") == "propose"),
        assert(BaselineN4F1StableSafety),
        unchangedAll,
      })
  }

  run validRoundProposalWithoutPrevoteQuorumIsRejectedTest = {
    Init
      .then(SeedValidNilPrecommitState("p1"))
      .then(SeedValidNilPrecommitState("p2"))
      .then(SeedValidNilPrecommitState("p3"))
      .then(StartNextRoundAfterPrecommitQuorum("p1"))
      .then(StartNextRoundAfterPrecommitQuorum("p2"))
      .then(StartNextRoundAfterPrecommitQuorum("p3"))
      .then(InsertProposal("p2"))
      .then(UponProposalPrevote("p1", "a1"))
      .fail()
  }

  run validRoundProposalWithPrevoteQuorumIsAcceptedTest = {
    Init
      .then(InsertProposal("p1"))
      .then(UponProposalPrevote("p1", "a1"))
      .then(UponProposalPrevote("p2", "a1"))
      .then(UponProposalPrevote("p3", "a1"))
      .then(UponValuePrevoteQuorum("p2", "a1"))
      .then(TimeoutPrevotePrecommitNil("p1"))
      .then(TimeoutPrecommitStartNextRound("p1"))
      .then(TimeoutPrecommitStartNextRound("p2"))
      .then(InsertProposal("p2"))
      .then(UponProposalPrevote("p1", "a1"))
      .then(all {
        assert(PrevoteQuorum(0, "a1")),
        assert(msgsPropose.get(1).exists(m =>
          m.src == "p2" and
            m.round == 1 and
            m.proposal == "a1" and
            m.validRound == 0
        )),
        assert(msgsPrevote.get(1).exists(m =>
          m.src == "p1" and m.round == 1 and m.id == "a1"
        )),
        assert(BaselineN4F1StableSafety),
        unchangedAll,
      })
  }

  run correctValuePrevotesRequireJustifiedProposalTest = {
    Init
      .then(InsertProposal("p1"))
      .then(UponProposalPrevote("p1", "a1"))
      .then(all {
        assert(CorrectValuePrevotesHaveJustifiedProposal),
        assert(BaselineN4F1StableSafety),
        unchangedAll,
      })
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

  val NoStaleFixedSigmaProposal =
    Rounds.forall(r =>
      msgsPropose.get(r).forall(m => m.proposal == TestHeadMinusSigma(r))
    )

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

  run falseNoStaleFixedSigmaProposalInvariantFailsTest = {
    Init
      .then(SeedAbandonedRoundState("p1"))
      .then(SeedAbandonedRoundState("p2"))
      .then(SeedAbandonedRoundState("p3"))
      .then(StartNextRoundAfterPrecommitQuorum("p2"))
      .then(InsertProposal("p2"))
      .then(all {
        NoStaleFixedSigmaProposal,
        unchangedAll,
      })
      .fail()
  }
}

module CrosslinkBaselineN4F2ForkingTest {
  /*
   An above-live-boundary f=2 instance. With N=4 and T=2, the quorum size is
   larger than the validator set, so this witness records the boundary instead
   of pretending the instance has a normal decision path.
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

  import CrosslinkResampling(
    Corr = Set("p1", "p2"),
    Faulty = Set("p3", "p4"),
    N = 4,
    T = 2,
    MaxRound = 4,
    Proposer = ProposerSchedule,
    Stream = TestStream,
    Snapshots = ValidSnapshots,
    ResampleOnNilPrecommit = false,
  ).* from "./CrosslinkResampling"

  action unchangedAll = all {
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
  }

  val CorrectCanMakeCatchupEvidence =
    size(Corr) >= THRESHOLD1

  val CorrectCanMakeValueQuorum =
    size(Corr) >= THRESHOLD2

  val BaselineN4F2ForkingSafety =
    Safety and TestStreamMatchesFixedSigma

  run n4F2DocumentsAboveLiveFaultBoundaryTest = {
    Init.then(all {
      assert(THRESHOLD1 == 3),
      assert(THRESHOLD2 == 5),
      assert(not(CorrectCanMakeCatchupEvidence)),
      assert(not(CorrectCanMakeValueQuorum)),
      assert(BaselineN4F2ForkingSafety),
      unchangedAll,
    })
  }
}

module CrosslinkBaselineN5F2ForkingTest {
  /*
   A second above-live-boundary f=2 instance. Correct validators can produce
   f+1 future-round activity, but they still cannot produce a 2f+1 value quorum.
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

  import CrosslinkResampling(
    Corr = Set("p1", "p2", "p3"),
    Faulty = Set("p4", "p5"),
    N = 5,
    T = 2,
    MaxRound = 4,
    Proposer = ProposerSchedule,
    Stream = TestStream,
    Snapshots = ValidSnapshots,
    ResampleOnNilPrecommit = false,
  ).* from "./CrosslinkResampling"

  action unchangedAll = all {
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
  }

  val CorrectCanMakeCatchupEvidence =
    size(Corr) >= THRESHOLD1

  val CorrectCanMakeValueQuorum =
    size(Corr) >= THRESHOLD2

  val BaselineN5F2ForkingSafety =
    Safety and TestStreamMatchesFixedSigma

  run n5F2CatchupEvidenceButNoCorrectValueQuorumTest = {
    Init
      .then(SeedCorrectNilPrevoteState("p1", 2))
      .then(SeedCorrectNilPrevoteState("p2", 2))
      .then(SeedCorrectNilPrevoteState("p3", 2))
      .then(all {
        assert(THRESHOLD1 == 3),
        assert(THRESHOLD2 == 5),
        assert(CorrectCanMakeCatchupEvidence),
        assert(RoundCatchupEvidence(2)),
        assert(not(CorrectCanMakeValueQuorum)),
        assert(BaselineN5F2ForkingSafety),
        unchangedAll,
      })
  }
}

module CrosslinkBaselineN7F2ForkingTest {
  /*
   A proper f=2 BFT-boundary instance. With N=7 and five correct validators,
   the baseline shell still has the ordinary fixed-sigma decision path.
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

  import CrosslinkResampling(
    Corr = Set("p1", "p2", "p3", "p4", "p5"),
    Faulty = Set("p6", "p7"),
    N = 7,
    T = 2,
    MaxRound = 4,
    Proposer = ProposerSchedule,
    Stream = TestStream,
    Snapshots = ValidSnapshots,
    ResampleOnNilPrecommit = false,
  ).* from "./CrosslinkResampling"

  action unchangedAll = all {
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
  }

  val CorrectCanMakeCatchupEvidence =
    size(Corr) >= THRESHOLD1

  val CorrectCanMakeValueQuorum =
    size(Corr) >= THRESHOLD2

  val BaselineN7F2ForkingSafety =
    Safety and TestStreamMatchesFixedSigma

  run n7F2DecisionPathTest = {
    Init
      .then(InsertProposal("p1"))
      .then(UponProposalPrevote("p1", "a1"))
      .then(UponProposalPrevote("p2", "a1"))
      .then(UponProposalPrevote("p3", "a1"))
      .then(UponProposalPrevote("p4", "a1"))
      .then(UponProposalPrevote("p5", "a1"))
      .then(UponValuePrevoteQuorum("p1", "a1"))
      .then(UponValuePrevoteQuorum("p2", "a1"))
      .then(UponValuePrevoteQuorum("p3", "a1"))
      .then(UponValuePrevoteQuorum("p4", "a1"))
      .then(UponValuePrevoteQuorum("p5", "a1"))
      .then(Decide("p1", 0, "a1"))
      .then(all {
        assert(THRESHOLD1 == 3),
        assert(THRESHOLD2 == 5),
        assert(CorrectCanMakeCatchupEvidence),
        assert(CorrectCanMakeValueQuorum),
        assert(decision.get("p1") == "a1"),
        assert(BaselineN7F2ForkingSafety),
        unchangedAll,
      })
  }
}
