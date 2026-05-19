// -*- mode: Bluespec; -*-
module CrosslinkBaselineStableModel {
  /*
   Baseline Crosslink fixes the current/sticky Tenderlink behavior:

   - sigma is fixed outside this focused model;
   - proposals sample the current PoW stream;
   - a nil-precommit quorum advances the round but does not resample or unlock
     same-round Tendermint state.

   This module gives the positive baseline witness: when the sampled PoW stream
   is stable long enough, the normal three-step Tendermint flow decides the
   sampled value.
   */

  import CrosslinkResamplingTest(
    Corr = Set("p1", "p2", "p3"),
    Faulty = Set("p4"),
    N = 4,
    T = 1,
    MaxRound = 2,
    Proposer = Map(0 -> "p1", 1 -> "p2", 2 -> "p3"),
    Stream = Map(0 -> "s0", 1 -> "s0", 2 -> "s0"),
    Snapshots = Set("s0"),
    ResampleOnNilPrecommit = false,
    ExpectedRound1Proposal = "s0",
  ).* from "./CrosslinkResampling"

  val BaselineSafety =
    Safety

  run baselineStableStreamDecidesSampledSnapshotTest = {
    Init
      .then(InsertProposal("p1"))
      .then(UponProposalPrevote("p1", "s0"))
      .then(UponProposalPrevote("p2", "s0"))
      .then(UponProposalPrevote("p3", "s0"))
      .then(UponValuePrevoteQuorum("p1", "s0"))
      .then(UponValuePrevoteQuorum("p2", "s0"))
      .then(UponValuePrevoteQuorum("p3", "s0"))
      .then(Decide("p1", 0, "s0"))
      .then(all {
        assert(decision.get("p1") == "s0"),
        assert(BaselineSafety),
        unchangedAll,
      })
  }
}

module CrosslinkBaselineStreamChangeModel {
  /*
   This module gives the negative baseline witnesses that motivate
   nil-precommit resampling: after a stream change, the current/sticky
   behavior can keep carrying the stale sample and same-round lock state.
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

  val BaselineSafety =
    Safety

  run baselineCarriesStaleSampleAfterStreamChangeTest = {
    Init
      .then(SeedAbandonedRoundState("p1"))
      .then(SeedAbandonedRoundState("p2"))
      .then(SeedAbandonedRoundState("p3"))
      .then(StartNextRoundAfterPrecommitQuorum("p2"))
      .then(InsertProposal("p2"))
      .then(all {
        assert(msgsPropose.get(1).exists(m =>
          m.src == "p2" and m.round == 1 and m.proposal == "s0"
        )),
        assert(BaselineSafety),
        unchangedAll,
      })
  }

  run baselineSameRoundLockBlocksFreshDecisionAfterStreamChangeTest = {
    Init
      .then(SeedValidNilPrecommitState("p1"))
      .then(SeedValidNilPrecommitState("p2"))
      .then(SeedFaultyNilPrecommit("p4"))
      .then(SeedSameRoundValueLock("p3"))
      .then(StartNextRoundAfterPrecommitQuorum("p1"))
      .then(StartNextRoundAfterPrecommitQuorum("p2"))
      .then(StartNextRoundAfterPrecommitQuorum("p3"))
      .then(InsertProposal("p2"))
      .then(UponProposalPrevote("p1", "s1"))
      .fail()
  }
}
