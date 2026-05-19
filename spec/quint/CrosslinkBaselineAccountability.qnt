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
}
