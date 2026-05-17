module CrosslinkForkFinality {
  /*
   A small value-semantics model for Crosslink finality.

   This module is intentionally independent of the Tenderlink round machine.
   It models what a BFT decision is allowed to finalize: a tail-confirmed PoW
   snapshot that extends the current finalized prefix. This lets the protocol
   skip PoW heights on a single branch, while rejecting rollback to a fork after
   a block is final.
   */

  type Snapshot_t = str
  type Block_t = str
  type Height_t = int

  const Snapshots: Set[Snapshot_t]
  const MaxHeight: Height_t
  const Sigma: int
  const InitialFinalized: Snapshot_t
  const HeightOf: Snapshot_t -> Height_t
  const AncestorAt: Snapshot_t -> Height_t -> Block_t

  assume initial_is_snapshot = Snapshots.contains(InitialFinalized)
  assume sigma_is_positive = Sigma >= 1

  pure val Heights = 0.to(MaxHeight)

  var latestFinal: Snapshot_t
  var finalized: Set[Snapshot_t]
  var firedAction: str

  pure def IsSnapshot(v: Snapshot_t): bool =
    Snapshots.contains(v)

  pure def Height(v: Snapshot_t): Height_t =
    HeightOf.get(v)

  pure def BlockAt(v: Snapshot_t, h: Height_t): Block_t =
    AncestorAt.get(v).get(h)

  def Extends(newer: Snapshot_t, older: Snapshot_t): bool =
    IsSnapshot(newer) and
    IsSnapshot(older) and
    Height(newer) >= Height(older) and
    Heights.forall(h =>
      h > Height(older) or BlockAt(newer, h) == BlockAt(older, h)
    )

  def Agrees(a: Snapshot_t, b: Snapshot_t): bool =
    Extends(a, b) or Extends(b, a)

  def TailConfirms(tip: Snapshot_t, candidate: Snapshot_t): bool =
    IsSnapshot(tip) and
    IsSnapshot(candidate) and
    Height(tip) >= Height(candidate) + Sigma and
    BlockAt(tip, Height(candidate)) == BlockAt(candidate, Height(candidate))

  def ValidFinalityCandidate(candidate: Snapshot_t, tip: Snapshot_t, prevFinal: Snapshot_t): bool =
    Extends(candidate, prevFinal) and
    Height(candidate) > Height(prevFinal) and
    TailConfirms(tip, candidate)

  action Init = all {
    latestFinal' = InitialFinalized,
    finalized' = Set(InitialFinalized),
    firedAction' = "Init",
  }

  action FinalizeCandidate(candidate: Snapshot_t, tip: Snapshot_t): bool = all {
    ValidFinalityCandidate(candidate, tip, latestFinal),
    latestFinal' = candidate,
    finalized' = finalized.union(Set(candidate)),
    firedAction' = "FinalizeCandidate",
  }

  action Stutter = all {
    latestFinal' = latestFinal,
    finalized' = finalized,
    firedAction' = "Stutter",
  }

  action Next =
    any {
      nondet candidate = oneOf(Snapshots)
      nondet tip = oneOf(Snapshots)
      FinalizeCandidate(candidate, tip),
      Stutter,
    }

  val FinalizedPrefixLinear =
    tuples(finalized, finalized).forall(((a, b)) =>
      Agrees(a, b)
    )

  val LatestFinalExtendsAllFinalized =
    finalized.forall(v => Extends(latestFinal, v))

  val InitialFinalizedRemainsFinalized =
    finalized.contains(InitialFinalized)

  val Safety =
    FinalizedPrefixLinear and
    LatestFinalExtendsAllFinalized and
    InitialFinalizedRemainsFinalized
}

module CrosslinkForkFinalityTest {
  import CrosslinkForkFinality.* from "./CrosslinkForkFinality"
  export CrosslinkForkFinality.*

  action unchangedAll = all {
    latestFinal' = latestFinal,
    finalized' = finalized,
    firedAction' = firedAction,
  }

  run canSkipHeightsOnSamePowBranchTest = {
    Init
      .then(FinalizeCandidate("a2", "a3"))
      .then(all {
        assert(latestFinal == "a2"),
        assert(finalized.contains("a2")),
        unchangedAll,
      })
  }

  run extendsFinalizedPrefixTest = {
    Init
      .then(FinalizeCandidate("a1", "a2"))
      .then(FinalizeCandidate("a2", "a3"))
      .then(all {
        assert(latestFinal == "a2"),
        assert(FinalizedPrefixLinear),
        unchangedAll,
      })
  }

  run rejectsFinalizingForkAfterFinalBlockTest = {
    Init
      .then(FinalizeCandidate("a1", "a2"))
      .then(FinalizeCandidate("b2", "b3"))
      .fail()
  }

  run rejectsUnconfirmedTailTest = {
    Init
      .then(FinalizeCandidate("a2", "a2"))
      .fail()
  }
}

module CrosslinkForkFinalityModel {
  import CrosslinkForkFinalityTest(
    Snapshots = Set("g", "a1", "a2", "a3", "b1", "b2", "b3"),
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
  ).*
}
