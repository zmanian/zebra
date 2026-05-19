// -*- mode: Bluespec; -*-
module CrosslinkPowBranchCompetition {
  /*
   A bounded PoW branch-competition model for Crosslink.

   CrosslinkPowForkSchedule constrains a supplied BestTip map. This model adds
   the missing competition layer: each round has published tips, honest work,
   and adversarial work. The selected best tip must be a published candidate
   with maximal total work, using a fixture rank only to break exact ties.
   */

  type Round_t = int
  type Height_t = int
  type Work_t = int
  type Snapshot_t = str

  const MaxRound: Round_t
  const MaxHeight: Height_t
  const MaxWork: Work_t
  const Sigma: int
  const Snapshots: Set[Snapshot_t]
  const BestTip: Round_t -> Snapshot_t
  const PublishedAt: Round_t -> Snapshot_t -> bool
  const HonestWorkAt: Round_t -> Snapshot_t -> Work_t
  const AdversarialWorkAt: Round_t -> Snapshot_t -> Work_t
  const TieBreakRank: Snapshot_t -> int
  const HeightOf: Snapshot_t -> Height_t
  const LcaHeight: Snapshot_t -> Snapshot_t -> Height_t

  pure val Rounds = 0.to(MaxRound)

  pure def Height(v: Snapshot_t): Height_t =
    HeightOf.get(v)

  pure def TotalWorkAt(r: Round_t, v: Snapshot_t): Work_t =
    HonestWorkAt.get(r).get(v) + AdversarialWorkAt.get(r).get(v)

  pure def CandidateAt(r: Round_t, v: Snapshot_t): bool =
    Snapshots.contains(v) and PublishedAt.get(r).get(v)

  pure def BetterOrEqualAt(r: Round_t, a: Snapshot_t, b: Snapshot_t): bool =
    TotalWorkAt(r, a) > TotalWorkAt(r, b) or
      (
        TotalWorkAt(r, a) == TotalWorkAt(r, b) and
        TieBreakRank.get(a) >= TieBreakRank.get(b)
      )

  pure def IsBestTipAt(r: Round_t, v: Snapshot_t): bool =
    CandidateAt(r, v) and
      Snapshots.forall(w =>
        not(CandidateAt(r, w)) or BetterOrEqualAt(r, v, w)
      )

  pure def BestTipAt(r: Round_t): Snapshot_t =
    BestTip.get(r)

  pure def CommonAncestorHeight(previousTip: Snapshot_t, nextTip: Snapshot_t): Height_t =
    LcaHeight.get(previousTip).get(nextTip)

  pure def RollbackDepth(previousTip: Snapshot_t, nextTip: Snapshot_t): int =
    Height(previousTip) - CommonAncestorHeight(previousTip, nextTip)

  pure def RoundRollbackDepth(r: Round_t): int =
    if (r == 0) {
      0
    } else {
      RollbackDepth(BestTipAt(r - 1), BestTipAt(r))
    }

  pure def SampleSurvivesRollback(previousTip: Snapshot_t, nextTip: Snapshot_t, sigma: int): bool =
    RollbackDepth(previousTip, nextTip) < sigma

  assume max_round_non_negative =
    MaxRound >= 0
  assume max_height_non_negative =
    MaxHeight >= 0
  assume max_work_non_negative =
    MaxWork >= 0
  assume sigma_is_positive =
    Sigma >= 1
  assume best_tips_are_snapshots =
    Rounds.forall(r => Snapshots.contains(BestTipAt(r)))
  assume generated_best_tips_match_work_competition =
    Rounds.forall(r => IsBestTipAt(r, BestTipAt(r)))
  assume published_work_is_bounded =
    Rounds.forall(r =>
      Snapshots.forall(v =>
        0 <= HonestWorkAt.get(r).get(v) and
        0 <= AdversarialWorkAt.get(r).get(v) and
        TotalWorkAt(r, v) <= MaxWork
      )
    )
  assume best_tip_heights_are_bounded =
    Rounds.forall(r =>
      0 <= Height(BestTipAt(r)) and Height(BestTipAt(r)) <= MaxHeight
    )
  assume adjacent_lca_heights_are_bounded =
    Rounds.forall(r =>
      if (r == 0) {
        true
      } else {
        val lca = CommonAncestorHeight(BestTipAt(r - 1), BestTipAt(r))
        all {
          0 <= lca,
          lca <= Height(BestTipAt(r - 1)),
          lca <= Height(BestTipAt(r)),
        }
      }
    )

  var round: Round_t
  var bestTip: Snapshot_t
  var bestTipWork: Work_t
  var observedRollbackDepth: int
  var firedAction: str

  action Init = all {
    round' = 0,
    bestTip' = BestTipAt(0),
    bestTipWork' = TotalWorkAt(0, BestTipAt(0)),
    observedRollbackDepth' = 0,
    firedAction' = "Init",
  }

  action AdvanceRound = {
    val nextRound = round + 1
    all {
      nextRound.in(Rounds),
      round' = nextRound,
      bestTip' = BestTipAt(nextRound),
      bestTipWork' = TotalWorkAt(nextRound, BestTipAt(nextRound)),
      observedRollbackDepth' = RoundRollbackDepth(nextRound),
      firedAction' =
        if (RoundRollbackDepth(nextRound) == 0) {
          "AdvanceSameBranch"
        } else {
          "AdvanceAdversarialForkSwitch"
        },
    }
  }

  action Stutter = all {
    round' = round,
    bestTip' = bestTip,
    bestTipWork' = bestTipWork,
    observedRollbackDepth' = observedRollbackDepth,
    firedAction' = firedAction,
  }

  action Next =
    any {
      AdvanceRound,
      Stutter,
    }

  val CurrentTipMatchesGeneratedCompetition =
    bestTip == BestTipAt(round) and IsBestTipAt(round, bestTip)

  val CurrentWorkMatchesGeneratedCompetition =
    bestTipWork == TotalWorkAt(round, bestTip)

  val CurrentRollbackDepthMatchesGeneratedCompetition =
    observedRollbackDepth == RoundRollbackDepth(round)

  val CurrentRollbackDepthIsBounded =
    0 <= observedRollbackDepth and observedRollbackDepth <= MaxHeight

  val Safety =
    CurrentTipMatchesGeneratedCompetition and
    CurrentWorkMatchesGeneratedCompetition and
    CurrentRollbackDepthMatchesGeneratedCompetition and
    CurrentRollbackDepthIsBounded
}

module CrosslinkPowBranchCompetitionTest {
  import CrosslinkPowBranchCompetition.* from "./CrosslinkPowBranchCompetition"
  export CrosslinkPowBranchCompetition.*

  run hiddenAdversarialWorkDoesNotWinUntilPublishedTest = all {
    assert(not(CandidateAt(1, "b4"))),
    assert(IsBestTipAt(1, "a4")),
    assert(CandidateAt(2, "b4")),
    assert(IsBestTipAt(2, "b4")),
  }

  run releasedAdversarialBranchOutworksHonestTipTest = {
    assert(TotalWorkAt(2, "b4") > TotalWorkAt(2, "a4"))
  }

  run generatedCompetitionDerivesRollbackDepthTest = {
    assert(RollbackDepth(BestTipAt(1), BestTipAt(2)) == 2)
  }

  run raisedSigmaSurvivesGeneratedAdversarialSwitchTest = all {
    assert(not(SampleSurvivesRollback(BestTipAt(1), BestTipAt(2), 1))),
    assert(SampleSurvivesRollback(BestTipAt(1), BestTipAt(2), Sigma)),
  }

  run adversarialCatchupProducesForkSwitchTest = {
    Init
      .then(AdvanceRound)
      .then(all {
        assert(bestTip == "a4"),
        assert(bestTipWork == 4),
        assert(observedRollbackDepth == 0),
        assert(firedAction == "AdvanceSameBranch"),
        assert(Safety),
        Stutter,
      })
      .then(AdvanceRound)
      .then(all {
        assert(bestTip == "b4"),
        assert(bestTipWork == 5),
        assert(observedRollbackDepth == 2),
        assert(firedAction == "AdvanceAdversarialForkSwitch"),
        assert(Safety),
        Stutter,
      })
  }
}

module CrosslinkPowBranchCompetitionModel {
  import CrosslinkPowBranchCompetitionTest(
    MaxRound = 2,
    MaxHeight = 4,
    MaxWork = 5,
    Sigma = 3,
    Snapshots = Set("g", "a3", "a4", "b4"),
    BestTip = Map(
      0 -> "a3",
      1 -> "a4",
      2 -> "b4",
    ),
    PublishedAt = Map(
      0 -> Map("g" -> true, "a3" -> true, "a4" -> false, "b4" -> false),
      1 -> Map("g" -> true, "a3" -> true, "a4" -> true, "b4" -> false),
      2 -> Map("g" -> true, "a3" -> true, "a4" -> true, "b4" -> true),
    ),
    HonestWorkAt = Map(
      0 -> Map("g" -> 0, "a3" -> 3, "a4" -> 0, "b4" -> 0),
      1 -> Map("g" -> 0, "a3" -> 3, "a4" -> 4, "b4" -> 0),
      2 -> Map("g" -> 0, "a3" -> 3, "a4" -> 4, "b4" -> 0),
    ),
    AdversarialWorkAt = Map(
      0 -> Map("g" -> 0, "a3" -> 0, "a4" -> 0, "b4" -> 4),
      1 -> Map("g" -> 0, "a3" -> 0, "a4" -> 0, "b4" -> 4),
      2 -> Map("g" -> 0, "a3" -> 0, "a4" -> 0, "b4" -> 5),
    ),
    TieBreakRank = Map(
      "g" -> 0,
      "a3" -> 1,
      "a4" -> 2,
      "b4" -> 3,
    ),
    HeightOf = Map(
      "g" -> 0,
      "a3" -> 3,
      "a4" -> 4,
      "b4" -> 4,
    ),
    LcaHeight = Map(
      "g" -> Map("g" -> 0, "a3" -> 0, "a4" -> 0, "b4" -> 0),
      "a3" -> Map("g" -> 0, "a3" -> 3, "a4" -> 3, "b4" -> 2),
      "a4" -> Map("g" -> 0, "a3" -> 3, "a4" -> 4, "b4" -> 2),
      "b4" -> Map("g" -> 0, "a3" -> 2, "a4" -> 2, "b4" -> 4),
    ),
  ).*
}
