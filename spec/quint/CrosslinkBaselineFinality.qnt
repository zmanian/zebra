// -*- mode: Bluespec; -*-
module CrosslinkBaselineFinalityStableModel {
  /*
   Baseline Crosslink finality composes the current fixed-sigma/sticky
   Tenderlink behavior with Crosslink's finalized-prefix rule.

   In this stable-stream model, the sampled PoW snapshot does not change across
   rounds, so ordinary Tendermint voting can decide the sampled value and the
   Crosslink finality rule can finalize it once the selected tail is confirmed.
   */

  import CrosslinkComposed(
    Corr = Set("p1", "p2", "p3"),
    Faulty = Set("p4"),
    N = 4,
    T = 1,
    MaxRound = 2,
    Proposer = Map(0 -> "p1", 1 -> "p2", 2 -> "p3"),
    Stream = Map(0 -> "a2", 1 -> "a2", 2 -> "a2"),
    Snapshots = Set("g", "a1", "a2", "a3", "b1", "b2", "b3"),
    ResampleOnNilPrecommit = false,
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
  ).* from "./CrosslinkComposed"

  action composedUnchanged = all {
    ProtocolUnchanged,
    latestFinal' = latestFinal,
    finalized' = finalized,
    finalityAction' = finalityAction,
  }

  run baselineStableStreamFinalizesSampledCandidateTest = {
    ComposedInit
      .then(Propose("p1"))
      .then(Prevote("p1", "a2"))
      .then(Prevote("p2", "a2"))
      .then(Prevote("p3", "a2"))
      .then(PrecommitValue("p1", "a2"))
      .then(PrecommitValue("p2", "a2"))
      .then(PrecommitValue("p3", "a2"))
      .then(DecideValue("p1", 0, "a2"))
      .then(FinalizeDecision("p1", "a2", "a3"))
      .then(all {
        assert(decision.get("p1") == "a2"),
        assert(latestFinal == "a2"),
        assert(finalized.contains("a2")),
        assert(ComposedSafety),
        composedUnchanged,
      })
  }
}

module CrosslinkBaselineFinalityStreamChangeModel {
  /*
   This model keeps the same fixed-sigma/sticky baseline but lets the PoW stream
   change after the abandoned round. A nil-precommit quorum advances the round,
   but it does not resample or unlock same-round Tendermint state, so the
   protocol can carry the stale sample while Crosslink finality remains stuck at
   the prior prefix.
   */

  import CrosslinkComposed(
    Corr = Set("p1", "p2", "p3"),
    Faulty = Set("p4"),
    N = 4,
    T = 1,
    MaxRound = 2,
    Proposer = Map(0 -> "p1", 1 -> "p2", 2 -> "p3"),
    Stream = Map(0 -> "a1", 1 -> "a2", 2 -> "a2"),
    Snapshots = Set("g", "a1", "a2", "a3", "b1", "b2", "b3"),
    ResampleOnNilPrecommit = false,
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
  ).* from "./CrosslinkComposed"

  action composedUnchanged = all {
    ProtocolUnchanged,
    latestFinal' = latestFinal,
    finalized' = finalized,
    finalityAction' = finalityAction,
  }

  run baselineStreamChangeCarriesStaleSampleWithFinalityUnchangedTest = {
    ComposedInit
      .then(SeedValidNil("p1"))
      .then(SeedValidNil("p2"))
      .then(SeedFaultyNil("p4"))
      .then(SeedSameRoundLock("p3"))
      .then(AdvanceAfterPrecommitQuorum("p1"))
      .then(AdvanceAfterPrecommitQuorum("p2"))
      .then(AdvanceAfterPrecommitQuorum("p3"))
      .then(Propose("p2"))
      .then(all {
        assert(msgsPropose.get(1).exists(m =>
          m.src == "p2" and m.round == 1 and m.proposal == "a1"
        )),
        assert(latestFinal == "g"),
        assert(finalized == Set("g")),
        assert(ComposedSafety),
        composedUnchanged,
      })
  }

  run baselineStreamChangeBlocksFreshFinalityTest = {
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
      .fail()
  }
}

module CrosslinkBaselineFinalityLivenessModel {
  import CrosslinkComposed(
    Corr = Set("p1", "p2", "p3"),
    Faulty = Set("p4"),
    N = 4,
    T = 1,
    MaxRound = 2,
    Proposer = Map(0 -> "p1", 1 -> "p2", 2 -> "p3"),
    Stream = Map(0 -> "a2", 1 -> "a2", 2 -> "a2"),
    Snapshots = Set("g", "a1", "a2", "a3", "b1", "b2", "b3"),
    ResampleOnNilPrecommit = false,
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
  ).* from "./CrosslinkComposed"

  var phase: int

  action LivenessInit = all {
    ComposedInit,
    phase' = 0,
  }

  action LivenessStep = any {
    all { phase == 0, Propose("p1"), phase' = 1 },
    all { phase == 1, Prevote("p1", "a2"), phase' = 2 },
    all { phase == 2, Prevote("p2", "a2"), phase' = 3 },
    all { phase == 3, Prevote("p3", "a2"), phase' = 4 },
    all { phase == 4, PrecommitValue("p1", "a2"), phase' = 5 },
    all { phase == 5, PrecommitValue("p2", "a2"), phase' = 6 },
    all { phase == 6, PrecommitValue("p3", "a2"), phase' = 7 },
    all { phase == 7, DecideValue("p1", 0, "a2"), phase' = 8 },
    all { phase == 8, FinalizeDecision("p1", "a2", "a3"), phase' = 9 },
    all { phase == 9, ComposedStutter, phase' = 9 },
  }

  val StableFinalityByEnd =
    phase < 9 or latestFinal == "a2"

  val LivenessSafety =
    ComposedSafety and StableFinalityByEnd
}
