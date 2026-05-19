// -*- mode: Bluespec; -*-
module CrosslinkBaselineModels {
  /*
   Small model instances mirroring the upstream Tendermint examples. These are
   import-only handles for debugging and for keeping the baseline parameter
   surface honest across different validator/fault layouts.
   */

  pure val ValidSnapshots = Set("g", "a1", "a2", "b1", "b2")
  pure val InvalidSnapshots = Set("bad")

  pure val HeightBySnapshot = Map(
    "g" -> 0,
    "a1" -> 1,
    "a2" -> 2,
    "b1" -> 1,
    "b2" -> 2,
    "bad" -> 0,
  )

  pure val AncestorByHeight = Map(
    "g" -> Map(0 -> "g", 1 -> "None", 2 -> "None"),
    "a1" -> Map(0 -> "g", 1 -> "a1", 2 -> "None"),
    "a2" -> Map(0 -> "g", 1 -> "a1", 2 -> "a2"),
    "b1" -> Map(0 -> "g", 1 -> "b1", 2 -> "None"),
    "b2" -> Map(0 -> "g", 1 -> "b1", 2 -> "b2"),
    "bad" -> Map(0 -> "bad", 1 -> "bad", 2 -> "bad"),
  )

  pure val StableBestTip = Map(
    0 -> "a2",
    1 -> "a2",
    2 -> "a2",
    3 -> "a2",
    4 -> "a2",
  )

  pure val ForkingBestTip = Map(
    0 -> "a2",
    1 -> "b2",
    2 -> "b2",
    3 -> "b2",
    4 -> "b2",
  )

  pure val ProposerSchedule = Map(
    0 -> "p1",
    1 -> "p2",
    2 -> "p3",
    3 -> "p4",
    4 -> "p1",
  )

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
    BaselineBestTip = StableBestTip,
    BaselineHeight = HeightBySnapshot,
    BaselineAncestorAt = AncestorByHeight,
  ) as n4_f1_stable from "./CrosslinkBaselineTenderlink"

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
  ) as n4_f1_forking from "./CrosslinkBaselineTenderlink"

  import CrosslinkBaselineTenderlink(
    BaselineCorr = Set("p1", "p2", "p3", "p4"),
    BaselineFaulty = Set("p5"),
    BaselineN = 5,
    BaselineT = 1,
    BaselineValidSnapshots = ValidSnapshots,
    BaselineInvalidSnapshots = InvalidSnapshots,
    BaselineMaxRound = 4,
    BaselineProposer = ProposerSchedule,
    BaselineSigma = 1,
    BaselineBestTip = ForkingBestTip,
    BaselineHeight = HeightBySnapshot,
    BaselineAncestorAt = AncestorByHeight,
  ) as n5_f1_forking from "./CrosslinkBaselineTenderlink"
}
