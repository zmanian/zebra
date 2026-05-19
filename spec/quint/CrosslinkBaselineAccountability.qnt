// -*- mode: Bluespec; -*-
module CrosslinkBaselineAccountabilityModel {
  /*
   Baseline Crosslink keeps the current fixed-sigma/sticky Tenderlink lock
   behavior. In this variant, a nil-precommit certificate advances the round but
   does not clear same-round `validValue` or `lockedValue` state.

   These witnesses make the baseline accountability projection explicit: actual
   baseline transitions retain same-round value locks after nil precommit, and
   conflicting value commits without unlock evidence expose Tendermint-style
   amnesia evidence.
   */

  import CrosslinkResamplingTest(
    Corr = Set("p1", "p2", "p3"),
    Faulty = Set("p4"),
    N = 4,
    T = 1,
    MaxRound = 2,
    Proposer = Map(0 -> "p1", 1 -> "p2", 2 -> "p3"),
    Stream = Map(0 -> "s0", 1 -> "s1", 2 -> "s2"),
    Snapshots = Set("s0", "s1", "s2"),
    ResampleOnNilPrecommit = false,
    ExpectedRound1Proposal = "s0",
  ).* from "./CrosslinkResampling"

  val BaselineAccountabilitySafety =
    Safety

  run baselineNilPrecommitDoesNotClearSameRoundValueLockTest = {
    Init
      .then(SeedValidNilPrecommitState("p1"))
      .then(SeedValidNilPrecommitState("p2"))
      .then(SeedFaultyNilPrecommit("p4"))
      .then(SeedSameRoundValueLock("p3"))
      .then(StartNextRoundAfterPrecommitQuorum("p3"))
      .then(all {
        assert(NilPrecommitCert(0)),
        assert(lockedValue.get("p3") == "s0"),
        assert(lockedRound.get("p3") == 0),
        assert(validValue.get("p3") == "s0"),
        assert(validRound.get("p3") == 0),
        assert(size(CorrectValueLocks(0, "s0")) == 1),
        assert(NilCertLeavesAtMostFCurrentRoundLocks),
        assert(CorrectValueLocksHavePrecommitEvidence),
        assert(BaselineAccountabilitySafety),
        unchangedAll,
      })
  }

  run baselineConflictingCommitsWithoutUnlockExposeAmnesiaTest = {
    Init
      .then(SeedCorrectPrecommitEvidence("p1", 0, "s0"))
      .then(SeedCorrectPrecommitEvidence("p2", 0, "s0"))
      .then(SeedFaultyPrecommitEvidence("p4", 0, "s0"))
      .then(SeedCorrectPrecommitEvidence("p2", 1, "s1"))
      .then(SeedCorrectPrecommitEvidence("p3", 1, "s1"))
      .then(SeedFaultyPrecommitEvidence("p4", 1, "s1"))
      .then(all {
        assert(PrecommitQuorum(0, "s0")),
        assert(PrecommitQuorum(1, "s1")),
        assert(not(NilPrecommitCert(0))),
        assert(CorrectValueSwitchWithoutUnlock("p2", 0, "s0", 1, "s1")),
        assert(AmnesiaBy("p2")),
        assert(AmnesiaBy("p4")),
        assert(size(DetectableFaults) >= THRESHOLD1),
        assert(Accountability),
        assert(ConflictHasAccountabilityEvidence(0, "s0", 1, "s1")),
        assert(ConflictingCommitsAccountable),
        unchangedAll,
      })
  }

  run baselineFaultyProposalEvidenceFeedsEquivocationTest = {
    Init
      .then(SeedFaultyProposalEvidence("p4", 0, "s0", -1))
      .then(SeedFaultyProposalEvidence("p4", 0, "s1", -1))
      .then(all {
        assert(ProposalEquivocationIn("p4", EvidencePropose)),
        assert(EquivocationBy("p4")),
        assert(DetectableFaultBy("p4")),
        assert(EvidenceCoversObservedMessages),
        assert(BaselineAccountabilitySafety),
        unchangedAll,
      })
  }

  run baselineFaultyPrevoteEvidenceFeedsEquivocationTest = {
    Init
      .then(SeedFaultyPrevoteEvidence("p4", 0, "s0"))
      .then(SeedFaultyPrevoteEvidence("p4", 0, "s1"))
      .then(all {
        assert(VoteEquivocationIn("p4", EvidencePrevote)),
        assert(EquivocationBy("p4")),
        assert(DetectableFaultBy("p4")),
        assert(EvidenceCoversObservedMessages),
        assert(BaselineAccountabilitySafety),
        unchangedAll,
      })
  }

  run baselineFaultyNilValuePrecommitEvidenceFeedsEquivocationTest = {
    Init
      .then(SeedFaultyNilPrecommitEvidence("p4", 0))
      .then(SeedFaultyPrecommitEvidence("p4", 0, "s0"))
      .then(all {
        assert(VoteEquivocationIn("p4", EvidencePrecommit)),
        assert(EquivocationBy("p4")),
        assert(DetectableFaultBy("p4")),
        assert(EvidenceCoversObservedMessages),
        assert(BaselineAccountabilitySafety),
        unchangedAll,
      })
  }
}

module CrosslinkBaselineFaultyInitTinyModel {
  /*
   A tiny upstream-style faulty Init harness. It keeps the nondeterministic
   faulty-message powersets small enough for the baseline proof gate while
   checking that initially injected faulty evidence is covered by the same
   baseline accountability invariant.
   */

  import CrosslinkResamplingTest(
    Corr = Set("p1", "p2", "p3"),
    Faulty = Set("p4"),
    N = 4,
    T = 1,
    MaxRound = 1,
    Proposer = Map(0 -> "p1", 1 -> "p2"),
    Stream = Map(0 -> "s0", 1 -> "s0"),
    Snapshots = Set("s0"),
    ResampleOnNilPrecommit = false,
    ExpectedRound1Proposal = "s0",
  ).* from "./CrosslinkResampling"

  val BaselineFaultyInitSafety =
    Safety

  run baselineFaultyInitCoversInjectedEvidenceTest = {
    InitWithFaultyEvidence.then(all {
      assert(EvidenceCoversObservedMessages),
      assert(BaselineFaultyInitSafety),
      unchangedAll,
    })
  }
}

module CrosslinkBaselineFaultyInitForkingModel {
  /*
   A larger fixed-sigma/forking baseline faulty-init harness. The baseline
   protocol parameters match the upstream-shaped N4/F1 forking shell, while the
   injected faulty evidence is bounded to a small domain so the symbolic proof
   gate remains tractable.
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

  pure val FaultyInitRounds =
    Set(0)

  pure val FaultyInitSnapshots =
    Set(TestHeadMinusSigma(0), TestHeadMinusSigma(1))

  pure val FaultyInitSnapshotsOrNil =
    Set(NilSnapshot).union(FaultyInitSnapshots)

  pure val FaultyInitValidRounds =
    Set(NilRound, 0)

  pure val FaultyInitDomainWellFormed =
    FaultyInitRounds.subseteq(Rounds) and
    FaultyInitSnapshots.subseteq(Snapshots) and
    FaultyInitValidRounds.subseteq(RoundsOrNil)

  def BoundedFaultyInitProposals(r: Round_t): Set[Propose_t] =
    tuples(Faulty, FaultyInitSnapshots, FaultyInitValidRounds)
      .map(((p, v, vr)) => {
        src: p,
        round: r,
        proposal: v,
        validRound: vr,
      })

  val AllBoundedFaultyInitProposals =
    FaultyInitRounds.map(r => BoundedFaultyInitProposals(r)).flatten()

  def BoundedFaultyInitPrevotes(r: Round_t): Set[Vote_t] =
    tuples(Faulty, FaultyInitSnapshotsOrNil)
      .map(((p, v)) => {
        src: p,
        round: r,
        id: v,
      })

  val AllBoundedFaultyInitPrevotes =
    FaultyInitRounds.map(r => BoundedFaultyInitPrevotes(r)).flatten()

  def BoundedFaultyInitPrecommits(r: Round_t): Set[Vote_t] =
    tuples(Faulty, FaultyInitSnapshotsOrNil)
      .map(((p, v)) => {
        src: p,
        round: r,
        id: v,
      })

  val AllBoundedFaultyInitPrecommits =
    FaultyInitRounds.map(r => BoundedFaultyInitPrecommits(r)).flatten()

  action InitWithBoundedForkingFaultyEvidence =
    nondet faultyProposals = AllBoundedFaultyInitProposals.powerset().oneOf()
    nondet faultyPrevotes = AllBoundedFaultyInitPrevotes.powerset().oneOf()
    nondet faultyPrecommits = AllBoundedFaultyInitPrecommits.powerset().oneOf()
    all {
      round' = Corr.mapBy(_ => 0),
      step' = Corr.mapBy(_ => "propose"),
      decision' = Corr.mapBy(_ => NilSnapshot),
      lockedValue' = Corr.mapBy(_ => NilSnapshot),
      lockedRound' = Corr.mapBy(_ => NilRound),
      validValue' = Corr.mapBy(_ => NilSnapshot),
      validRound' = Corr.mapBy(_ => NilRound),
      cachedProposal' = Corr.mapBy(_ => NilSnapshot),
      cachedProposalRound' = Corr.mapBy(_ => NilRound),
      msgsPropose' = Rounds.mapBy(r =>
        faultyProposals.filter(m => m.round == r)
      ),
      msgsPrevote' = Rounds.mapBy(r =>
        faultyPrevotes.filter(m => m.round == r)
      ),
      msgsPrecommit' = Rounds.mapBy(r =>
        faultyPrecommits.filter(m => m.round == r)
      ),
      evidencePropose' = faultyProposals,
      evidencePrevote' = faultyPrevotes,
      evidencePrecommit' = faultyPrecommits,
      firedAction' = "InitWithBoundedForkingFaultyEvidence",
    }

  val BaselineForkingFaultyInitSafety =
    Safety and TestStreamMatchesFixedSigma and FaultyInitDomainWellFormed

  run baselineForkingFaultyInitCoversEvidenceTest = {
    InitWithBoundedForkingFaultyEvidence.then(all {
      assert(TestHeadMinusSigma(0) == "a1"),
      assert(TestHeadMinusSigma(1) == "b1"),
      assert(EvidenceCoversObservedMessages),
      assert(BaselineForkingFaultyInitSafety),
      unchangedAll,
    })
  }

  run baselineForkingFaultyInitAllowsCorrectProposalTest = {
    InitWithBoundedForkingFaultyEvidence
      .then(InsertProposal("p1"))
      .then(all {
        assert(HasProposal(0, "a1")),
        assert(EvidenceCoversObservedMessages),
        assert(BaselineForkingFaultyInitSafety),
        unchangedAll,
      })
  }
}

module CrosslinkBaselineFullFaultyInitForkingModel {
  /*
   A full-domain fixed-sigma/forking faulty-init harness for the upstream-shaped
   n4/f1 baseline instance. This keeps the larger parameter surface usable in
   quick checks while leaving the bounded faulty-init harness as the symbolic
   gate.
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

  pure val FullFaultyInitDomainWellFormed =
    AllFaultyProposals.forall(m =>
      m.src.in(Faulty) and
      m.round.in(Rounds) and
      m.proposal.in(Snapshots) and
      m.validRound.in(RoundsOrNil)
    ) and
    AllFaultyPrevotes.forall(m =>
      m.src.in(Faulty) and m.round.in(Rounds) and m.id.in(SnapshotsOrNil)
    ) and
    AllFaultyPrecommits.forall(m =>
      m.src.in(Faulty) and m.round.in(Rounds) and m.id.in(SnapshotsOrNil)
    )

  pure val FullFaultyInitUsesWholeForkingDomain =
    AllFaultyProposals.exists(m =>
      m.src == "p4" and
      m.round == 4 and
      m.proposal == "b2" and
      m.validRound == 4
    ) and
    AllFaultyPrevotes.exists(m =>
      m.src == "p4" and m.round == 4 and m.id == NilSnapshot
    ) and
    AllFaultyPrecommits.exists(m =>
      m.src == "p4" and m.round == 4 and m.id == "a1"
    )

  val BaselineFullForkingFaultyInitSafety =
    Safety and
    TestStreamMatchesFixedSigma and
    FullFaultyInitDomainWellFormed

  run baselineFullForkingFaultyInitCoversEvidenceTest = {
    InitWithFaultyEvidence.then(all {
      assert(TestHeadMinusSigma(0) == "a1"),
      assert(TestHeadMinusSigma(1) == "b1"),
      assert(FullFaultyInitUsesWholeForkingDomain),
      assert(EvidenceCoversObservedMessages),
      assert(BaselineFullForkingFaultyInitSafety),
      unchangedAll,
    })
  }

  run baselineFullForkingFaultyInitAllowsCorrectProposalTest = {
    InitWithFaultyEvidence
      .then(InsertProposal("p1"))
      .then(all {
        assert(HasProposal(0, "a1")),
        assert(EvidenceCoversObservedMessages),
        assert(BaselineFullForkingFaultyInitSafety),
        unchangedAll,
      })
  }
}

module CrosslinkBaselineFullFaultyInitN7F2ForkingModel {
  /*
   A full-domain faulty-init harness for the proper f=2 boundary instance. This
   keeps arbitrary faulty proposal, prevote, and precommit injection alive for
   the n7/f2 fixed-sigma/forking surface in the Rust-backed quick gate.
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

  pure val FullN7F2FaultyInitDomainWellFormed =
    AllFaultyProposals.forall(m =>
      m.src.in(Faulty) and
      m.round.in(Rounds) and
      m.proposal.in(Snapshots) and
      m.validRound.in(RoundsOrNil)
    ) and
    AllFaultyPrevotes.forall(m =>
      m.src.in(Faulty) and m.round.in(Rounds) and m.id.in(SnapshotsOrNil)
    ) and
    AllFaultyPrecommits.forall(m =>
      m.src.in(Faulty) and m.round.in(Rounds) and m.id.in(SnapshotsOrNil)
    )

  pure val FullN7F2FaultyInitUsesWholeForkingDomain =
    AllFaultyProposals.exists(m =>
      m.src == "p7" and
      m.round == 4 and
      m.proposal == "b2" and
      m.validRound == 4
    ) and
    AllFaultyPrevotes.exists(m =>
      m.src == "p6" and m.round == 4 and m.id == NilSnapshot
    ) and
    AllFaultyPrecommits.exists(m =>
      m.src == "p7" and m.round == 4 and m.id == "a1"
    )

  val BaselineFullN7F2ForkingFaultyInitSafety =
    Safety and
    TestStreamMatchesFixedSigma and
    FullN7F2FaultyInitDomainWellFormed

  run baselineFullN7F2ForkingFaultyInitCoversEvidenceTest = {
    InitWithFaultyEvidence.then(all {
      assert(THRESHOLD1 == 3),
      assert(THRESHOLD2 == 5),
      assert(TestHeadMinusSigma(0) == "a1"),
      assert(TestHeadMinusSigma(1) == "b1"),
      assert(FullN7F2FaultyInitUsesWholeForkingDomain),
      assert(EvidenceCoversObservedMessages),
      assert(BaselineFullN7F2ForkingFaultyInitSafety),
      unchangedAll,
    })
  }

  run baselineFullN7F2ForkingFaultyInitAllowsCorrectProposalTest = {
    InitWithFaultyEvidence
      .then(InsertProposal("p1"))
      .then(all {
        assert(HasProposal(0, "a1")),
        assert(EvidenceCoversObservedMessages),
        assert(BaselineFullN7F2ForkingFaultyInitSafety),
        unchangedAll,
      })
  }
}

module CrosslinkBaselineCounterexampleModel {
  /*
   False-invariant harnesses. These are intentionally bad claims that should
   fail under seeded evidence, proving the accountability predicates are not
   vacuous in the bounded baseline model.
   */

  import CrosslinkResamplingTest(
    Corr = Set("p1", "p2", "p3"),
    Faulty = Set("p4"),
    N = 4,
    T = 1,
    MaxRound = 2,
    Proposer = Map(0 -> "p1", 1 -> "p2", 2 -> "p3"),
    Stream = Map(0 -> "s0", 1 -> "s1", 2 -> "s2"),
    Snapshots = Set("s0", "s1", "s2"),
    ResampleOnNilPrecommit = false,
    ExpectedRound1Proposal = "s0",
  ).* from "./CrosslinkResampling"

  val NoConflictingValueCommits =
    Rounds.forall(r1 =>
      Rounds.forall(r2 =>
        Snapshots.forall(v1 =>
          Snapshots.forall(v2 =>
            not(ConflictingValueCommits(r1, v1, r2, v2))
          )
        )
      )
    )

  val AgreementOrAmnesia =
    Agreement or Faulty.forall(p => AmnesiaBy(p))

  val ShowMeAmnesiaWithoutEquivocation =
    (not(Agreement) and Faulty.exists(p => not(EquivocationBy(p)))) implies
      Faulty.forall(p => not(AmnesiaBy(p)))

  val AmnesiaImpliesEquivocation =
    Faulty.exists(p => AmnesiaBy(p)) implies Faulty.exists(q => EquivocationBy(q))

  val NeverUndecidedInMaxRound =
    Corr.forall(p => round.get(p) == MaxRound) implies
      Corr.forall(p => decision.get(p) != NilSnapshot)

  action SeedConflictingDecisions: bool =
    all {
      decision' = decision.set("p1", "s0").set("p2", "s1"),
      firedAction' = "SeedConflictingDecisions",
      round' = round,
      step' = step,
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
    }

  action SeedAllCorrectAtMaxRoundWithoutDecision: bool =
    all {
      round' = Corr.mapBy(_ => MaxRound),
      step' = Corr.mapBy(_ => "propose"),
      decision' = Corr.mapBy(_ => NilSnapshot),
      firedAction' = "SeedAllCorrectAtMaxRoundWithoutDecision",
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
    }

  run falseNoConflictingCommitsInvariantFailsTest = {
    Init
      .then(SeedCorrectPrecommitEvidence("p1", 0, "s0"))
      .then(SeedCorrectPrecommitEvidence("p2", 0, "s0"))
      .then(SeedFaultyPrecommitEvidence("p4", 0, "s0"))
      .then(SeedCorrectPrecommitEvidence("p2", 1, "s1"))
      .then(SeedCorrectPrecommitEvidence("p3", 1, "s1"))
      .then(SeedFaultyPrecommitEvidence("p4", 1, "s1"))
      .then(all {
        NoConflictingValueCommits,
        unchangedAll,
      })
      .fail()
  }

  run falseNoAmnesiaEvidenceInvariantFailsTest = {
    Init
      .then(SeedCorrectPrecommitEvidence("p1", 0, "s0"))
      .then(SeedCorrectPrecommitEvidence("p2", 0, "s0"))
      .then(SeedFaultyPrecommitEvidence("p4", 0, "s0"))
      .then(SeedCorrectPrecommitEvidence("p2", 1, "s1"))
      .then(SeedCorrectPrecommitEvidence("p3", 1, "s1"))
      .then(SeedFaultyPrecommitEvidence("p4", 1, "s1"))
      .then(all {
        not(AmnesiaBy("p2")),
        unchangedAll,
      })
      .fail()
  }

  run falseNoEquivocationEvidenceInvariantFailsTest = {
    Init
      .then(SeedFaultyPrevoteEvidence("p4", 0, "s0"))
      .then(SeedFaultyPrevoteEvidence("p4", 0, "s1"))
      .then(all {
        not(EquivocationBy("p4")),
        unchangedAll,
      })
      .fail()
  }

  run falseAgreementInvariantFailsTest = {
    Init
      .then(SeedConflictingDecisions)
      .then(all {
        Agreement,
        unchangedAll,
      })
      .fail()
  }

  run falseAgreementOrAmnesiaInvariantFailsTest = {
    Init
      .then(SeedConflictingDecisions)
      .then(SeedFaultyProposalEvidence("p4", 0, "s0", -1))
      .then(SeedFaultyProposalEvidence("p4", 0, "s1", -1))
      .then(all {
        AgreementOrAmnesia,
        unchangedAll,
      })
      .fail()
  }

  run falseAmnesiaImpliesEquivocationInvariantFailsTest = {
    Init
      .then(SeedFaultyPrecommitEvidence("p4", 0, "s0"))
      .then(SeedFaultyPrecommitEvidence("p4", 1, "s1"))
      .then(all {
        AmnesiaImpliesEquivocation,
        unchangedAll,
      })
      .fail()
  }

  run falseShowMeAmnesiaWithoutEquivocationInvariantFailsTest = {
    Init
      .then(SeedConflictingDecisions)
      .then(SeedFaultyPrecommitEvidence("p4", 0, "s0"))
      .then(SeedFaultyPrecommitEvidence("p4", 1, "s1"))
      .then(all {
        ShowMeAmnesiaWithoutEquivocation,
        unchangedAll,
      })
      .fail()
  }

  run falseNeverUndecidedInMaxRoundInvariantFailsTest = {
    Init
      .then(SeedAllCorrectAtMaxRoundWithoutDecision)
      .then(all {
        NeverUndecidedInMaxRound,
        unchangedAll,
      })
      .fail()
  }
}
