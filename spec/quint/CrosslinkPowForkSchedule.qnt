// -*- mode: Bluespec; -*-
module CrosslinkPowForkSchedule {
  /*
   A focused PoW fork-schedule model for dynamic sigma.

   CrosslinkDynamicSigma currently accepts an observed reorg-depth schedule as a
   controller input. This model derives that depth from a bounded sequence of PoW
   best-tip changes, so the next composition step can feed a generated rollback
   signal into the dynamic-sigma controller.
   */

  type Round_t = int
  type Height_t = int
  type Snapshot_t = str

  const MaxRound: Round_t
  const MaxHeight: Height_t
  const Sigma: int
  const Snapshots: Set[Snapshot_t]
  const BestTip: Round_t -> Snapshot_t
  const HeightOf: Snapshot_t -> Height_t
  const LcaHeight: Snapshot_t -> Snapshot_t -> Height_t

  pure val Rounds = 0.to(MaxRound)

  assume max_round_non_negative =
    MaxRound >= 0
  assume max_height_non_negative =
    MaxHeight >= 0
  assume sigma_is_positive =
    Sigma >= 1
  assume best_tips_are_snapshots =
    Rounds.forall(r => Snapshots.contains(BestTip.get(r)))
  assume best_tip_heights_are_bounded =
    Rounds.forall(r =>
      0 <= HeightOf.get(BestTip.get(r)) and
      HeightOf.get(BestTip.get(r)) <= MaxHeight
    )
  assume adjacent_lca_heights_are_bounded =
    Rounds.forall(r =>
      if (r == 0) {
        true
      } else {
        val lca = LcaHeight.get(BestTip.get(r - 1)).get(BestTip.get(r))
        all {
          0 <= lca,
          lca <= HeightOf.get(BestTip.get(r - 1)),
          lca <= HeightOf.get(BestTip.get(r)),
        }
      }
    )

  var round: Round_t
  var bestTip: Snapshot_t
  var observedRollbackDepth: int
  var firedAction: str

  pure def Height(v: Snapshot_t): Height_t =
    HeightOf.get(v)

  pure def CommonAncestorHeight(previousTip: Snapshot_t, nextTip: Snapshot_t): Height_t =
    LcaHeight.get(previousTip).get(nextTip)

  pure def RollbackDepth(previousTip: Snapshot_t, nextTip: Snapshot_t): int =
    Height(previousTip) - CommonAncestorHeight(previousTip, nextTip)

  pure def RoundRollbackDepth(r: Round_t): int =
    if (r == 0) {
      0
    } else {
      RollbackDepth(BestTip.get(r - 1), BestTip.get(r))
    }

  pure def SampleSurvivesRollback(previousTip: Snapshot_t, nextTip: Snapshot_t, sigma: int): bool =
    RollbackDepth(previousTip, nextTip) < sigma

  action Init = all {
    round' = 0,
    bestTip' = BestTip.get(0),
    observedRollbackDepth' = 0,
    firedAction' = "Init",
  }

  action AdvanceRound = {
    val nextRound = round + 1
    all {
      nextRound.in(Rounds),
      round' = nextRound,
      bestTip' = BestTip.get(nextRound),
      observedRollbackDepth' = RoundRollbackDepth(nextRound),
      firedAction' =
        if (RoundRollbackDepth(nextRound) == 0) {
          "AdvanceSameBranch"
        } else {
          "AdvanceForkSwitch"
        },
    }
  }

  action Stutter = all {
    round' = round,
    bestTip' = bestTip,
    observedRollbackDepth' = observedRollbackDepth,
    firedAction' = firedAction,
  }

  action Next =
    any {
      AdvanceRound,
      Stutter,
    }

  val CurrentTipMatchesSchedule =
    bestTip == BestTip.get(round)

  val CurrentRollbackDepthMatchesSchedule =
    observedRollbackDepth == RoundRollbackDepth(round)

  val CurrentRollbackDepthIsBounded =
    0 <= observedRollbackDepth and observedRollbackDepth <= MaxHeight

  val Safety =
    CurrentTipMatchesSchedule and
    CurrentRollbackDepthMatchesSchedule and
    CurrentRollbackDepthIsBounded
}

module CrosslinkPowForkScheduleTest {
  import CrosslinkPowForkSchedule.* from "./CrosslinkPowForkSchedule"
  export CrosslinkPowForkSchedule.*

  run forkSwitchDerivesRollbackDepthTest = {
    assert(RollbackDepth("a4", "b4") == 2)
  }

  run sameBranchExtensionHasZeroRollbackDepthTest = {
    assert(RollbackDepth("a3", "a4") == 0)
  }

  run raisedSigmaSurvivesForkSwitchThatBaseSigmaDoesNotTest = all {
    assert(not(SampleSurvivesRollback("a4", "b4", 1))),
    assert(SampleSurvivesRollback("a4", "b4", Sigma)),
  }

  run scheduleDerivesRollbackDepthAcrossRoundsTest = {
    Init
      .then(AdvanceRound)
      .then(all {
        assert(bestTip == "a4"),
        assert(observedRollbackDepth == 0),
        assert(firedAction == "AdvanceSameBranch"),
        assert(Safety),
        Stutter,
      })
      .then(AdvanceRound)
      .then(all {
        assert(bestTip == "b4"),
        assert(observedRollbackDepth == 2),
        assert(firedAction == "AdvanceForkSwitch"),
        assert(Safety),
        Stutter,
      })
  }
}

module CrosslinkPowForkScheduleModel {
  import CrosslinkPowForkScheduleTest(
    MaxRound = 2,
    MaxHeight = 4,
    Sigma = 3,
    Snapshots = Set("g", "a1", "a2", "a3", "a4", "b3", "b4"),
    BestTip = Map(
      0 -> "a3",
      1 -> "a4",
      2 -> "b4",
    ),
    HeightOf = Map(
      "g" -> 0,
      "a1" -> 1,
      "a2" -> 2,
      "a3" -> 3,
      "a4" -> 4,
      "b3" -> 3,
      "b4" -> 4,
    ),
    LcaHeight = Map(
      "a3" -> Map("a4" -> 3, "b4" -> 2),
      "a4" -> Map("a4" -> 4, "b4" -> 2),
      "b4" -> Map("a4" -> 2, "b4" -> 4),
    ),
  ).*
}
