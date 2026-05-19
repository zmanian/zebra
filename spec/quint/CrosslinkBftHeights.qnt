// -*- mode: Bluespec; -*-
module CrosslinkBftHeights {
  /*
   A bounded model for Crosslink finality across successive BFT heights.

   Earlier composed witnesses finalize a single Tenderlink decision. This model
   adds the height dimension: consecutive BFT decisions can update Crosslink
   finality directly, while the finalized PoW prefix remains linear.
   */

  type ConsensusHeight_t = int
  type PowHeight_t = int
  type Snapshot_t = str
  type Block_t = str

  const MaxConsensusHeight: ConsensusHeight_t
  const MaxPowHeight: PowHeight_t
  const Sigma: int
  const Snapshots: Set[Snapshot_t]
  const InitialFinalized: Snapshot_t
  const ScheduledDecision: ConsensusHeight_t -> Snapshot_t
  const ScheduledTip: ConsensusHeight_t -> Snapshot_t
  const HeightOf: Snapshot_t -> PowHeight_t
  const AncestorAt: Snapshot_t -> PowHeight_t -> Block_t

  pure val ConsensusHeights = 0.to(MaxConsensusHeight)
  pure val PowHeights = 0.to(MaxPowHeight)

  assume max_consensus_height_non_negative =
    MaxConsensusHeight >= 0
  assume max_pow_height_non_negative =
    MaxPowHeight >= 0
  assume sigma_is_positive =
    Sigma >= 1
  assume initial_finalized_is_snapshot =
    Snapshots.contains(InitialFinalized)
  assume scheduled_values_are_snapshots =
    ConsensusHeights.forall(h =>
      h == 0 or
        (
          Snapshots.contains(ScheduledDecision.get(h)) and
          Snapshots.contains(ScheduledTip.get(h))
        )
    )

  var consensusHeight: ConsensusHeight_t
  var latestFinal: Snapshot_t
  var finalized: Set[Snapshot_t]
  var heightAction: str

  pure def IsSnapshot(v: Snapshot_t): bool =
    Snapshots.contains(v)

  pure def Height(v: Snapshot_t): PowHeight_t =
    HeightOf.get(v)

  pure def BlockAt(v: Snapshot_t, h: PowHeight_t): Block_t =
    AncestorAt.get(v).get(h)

  def Extends(newer: Snapshot_t, older: Snapshot_t): bool =
    IsSnapshot(newer) and
    IsSnapshot(older) and
    Height(newer) >= Height(older) and
    PowHeights.forall(h =>
      h > Height(older) or BlockAt(newer, h) == BlockAt(older, h)
    )

  def Agrees(a: Snapshot_t, b: Snapshot_t): bool =
    Extends(a, b) or Extends(b, a)

  def TailConfirms(tip: Snapshot_t, candidate: Snapshot_t): bool =
    IsSnapshot(tip) and
    IsSnapshot(candidate) and
    Height(tip) >= Height(candidate) + Sigma and
    BlockAt(tip, Height(candidate)) == BlockAt(candidate, Height(candidate))

  def ValidHeightDecision(candidate: Snapshot_t, tip: Snapshot_t): bool =
    Extends(candidate, latestFinal) and
    Height(candidate) > Height(latestFinal) and
    TailConfirms(tip, candidate)

  action Init = all {
    consensusHeight' = 0,
    latestFinal' = InitialFinalized,
    finalized' = Set(InitialFinalized),
    heightAction' = "Init",
  }

  action ApplyDecisionAt(
    nextHeight: ConsensusHeight_t,
    candidate: Snapshot_t,
    tip: Snapshot_t
  ): bool = all {
    nextHeight == consensusHeight + 1,
    nextHeight.in(ConsensusHeights),
    ValidHeightDecision(candidate, tip),
    consensusHeight' = nextHeight,
    latestFinal' = candidate,
    finalized' = finalized.union(Set(candidate)),
    heightAction' = "ApplyDecisionAt",
  }

  action ApplyScheduledDecision = {
    val nextHeight = consensusHeight + 1
    ApplyDecisionAt(
      nextHeight,
      ScheduledDecision.get(nextHeight),
      ScheduledTip.get(nextHeight)
    )
  }

  action Stutter = all {
    consensusHeight' = consensusHeight,
    latestFinal' = latestFinal,
    finalized' = finalized,
    heightAction' = heightAction,
  }

  action Next =
    any {
      ApplyScheduledDecision,
      Stutter,
    }

  val ConsensusHeightIsBounded =
    0 <= consensusHeight and consensusHeight <= MaxConsensusHeight

  val FinalizedPrefixLinear =
    tuples(finalized, finalized).forall(((a, b)) =>
      Agrees(a, b)
    )

  val LatestFinalExtendsAllFinalized =
    finalized.forall(v => Extends(latestFinal, v))

  val InitialFinalizedRemainsFinalized =
    finalized.contains(InitialFinalized)

  val Safety =
    ConsensusHeightIsBounded and
    FinalizedPrefixLinear and
    LatestFinalExtendsAllFinalized and
    InitialFinalizedRemainsFinalized
}

module CrosslinkBftHeightsTest {
  import CrosslinkBftHeights.* from "./CrosslinkBftHeights"
  export CrosslinkBftHeights.*

  action unchangedAll = all {
    consensusHeight' = consensusHeight,
    latestFinal' = latestFinal,
    finalized' = finalized,
    heightAction' = heightAction,
  }

  run successiveBftDecisionsAdvanceCrosslinkFinalityTest = {
    Init
      .then(ApplyScheduledDecision)
      .then(all {
        assert(consensusHeight == 1),
        assert(latestFinal == "a2"),
        assert(finalized.contains("a2")),
        assert(Safety),
        Stutter,
      })
      .then(ApplyScheduledDecision)
      .then(all {
        assert(consensusHeight == 2),
        assert(latestFinal == "a3"),
        assert(finalized.contains("a3")),
        assert(Safety),
        Stutter,
      })
  }

  run rejectsForkDecisionAfterPrefixFinalityTest = {
    Init
      .then(ApplyDecisionAt(1, "a2", "a3"))
      .then(ApplyDecisionAt(2, "b3", "b4"))
      .fail()
  }

  run rejectsSkippingConsensusHeightTest = {
    Init
      .then(ApplyDecisionAt(2, "a3", "a4"))
      .fail()
  }
}

module CrosslinkBftHeightsModel {
  import CrosslinkBftHeightsTest(
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
  ).*
}
