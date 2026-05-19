// -*- mode: Bluespec; -*-
module CrosslinkBaselineBftHeightsModel {
  /*
   Baseline Crosslink finality advances at consecutive BFT consensus heights
   while using a fixed PoW confirmation depth.

   This gives the baseline variant a named heighted-finality witness alongside
   the round-level, accountability, PoW-sampling, and single-decision finality
   models.
   */

  import CrosslinkBftHeights(
    MaxConsensusHeight = 2,
    MaxPowHeight = 4,
    Sigma = 1,
    Snapshots = Set("g", "a1", "a2", "a3", "a4", "b2", "b3", "b4"),
    InitialFinalized = "g",
    ScheduledDecision = Map(
      0 -> "g",
      1 -> "a2",
      2 -> "a3",
      3 -> "a4",
    ),
    ScheduledTip = Map(
      0 -> "g",
      1 -> "a3",
      2 -> "a4",
      3 -> "a4",
    ),
    HeightOf = Map(
      "g" -> 0,
      "a1" -> 1,
      "a2" -> 2,
      "a3" -> 3,
      "a4" -> 4,
      "b2" -> 2,
      "b3" -> 3,
      "b4" -> 4,
    ),
    AncestorAt = Map(
      "g" -> Map(0 -> "g", 1 -> "None", 2 -> "None", 3 -> "None", 4 -> "None"),
      "a1" -> Map(0 -> "g", 1 -> "a1", 2 -> "None", 3 -> "None", 4 -> "None"),
      "a2" -> Map(0 -> "g", 1 -> "a1", 2 -> "a2", 3 -> "None", 4 -> "None"),
      "a3" -> Map(0 -> "g", 1 -> "a1", 2 -> "a2", 3 -> "a3", 4 -> "None"),
      "a4" -> Map(0 -> "g", 1 -> "a1", 2 -> "a2", 3 -> "a3", 4 -> "a4"),
      "b2" -> Map(0 -> "g", 1 -> "a1", 2 -> "b2", 3 -> "None", 4 -> "None"),
      "b3" -> Map(0 -> "g", 1 -> "a1", 2 -> "b2", 3 -> "b3", 4 -> "None"),
      "b4" -> Map(0 -> "g", 1 -> "a1", 2 -> "b2", 3 -> "b3", 4 -> "b4"),
    ),
  ).* from "./CrosslinkBftHeights"

  action unchangedAll = all {
    consensusHeight' = consensusHeight,
    latestFinal' = latestFinal,
    finalized' = finalized,
    heightAction' = heightAction,
  }

  val BaselineBftHeightSafety =
    Safety

  run baselineConsecutiveBftHeightsAdvanceFinalityTest = {
    Init
      .then(ApplyScheduledDecision)
      .then(all {
        assert(consensusHeight == 1),
        assert(latestFinal == "a2"),
        assert(finalized.contains("a2")),
        assert(BaselineBftHeightSafety),
        unchangedAll,
      })
      .then(ApplyScheduledDecision)
      .then(all {
        assert(consensusHeight == 2),
        assert(latestFinal == "a3"),
        assert(finalized.contains("a3")),
        assert(BaselineBftHeightSafety),
        unchangedAll,
      })
  }

  run baselineRejectsSkippedBftHeightTest = {
    Init
      .then(ApplyDecisionAt(2, "a3", "a4"))
      .fail()
  }

  run baselineRejectsForkAfterPrefixFinalityTest = {
    Init
      .then(ApplyDecisionAt(1, "a2", "a3"))
      .then(ApplyDecisionAt(2, "b3", "b4"))
      .fail()
  }
}
