// -*- mode: Bluespec; -*-
module CrosslinkBaselineInductiveFinality {
  /*
   Inductive multi-height finality lemmas for the baseline variant.

   The bounded BFT-height model in `CrosslinkBaselineBftHeights.qnt` checks a
   monolithic safety invariant over a fixed schedule. This module reshapes that
   single invariant into a set of named lemmas whose conjunction is the
   would-be inductive invariant for multi-height prefix finality.

   Each lemma is a standalone bounded `val` that holds at the model's depth.
   Their composition expresses the consecutive-pair preservation argument:
   if at every step the height successor extends the predecessor's finalized
   prefix, is the consecutive successor height, and does not finalize a
   competing fork, then by induction the linear finalized-prefix property at
   the model's bound generalizes to any depth that preserves the same
   transition shape.

   This is not a machine-checked induction. Apalache does not close the
   induction natively. The decomposition is the reviewable argument shape,
   and each named lemma is individually checked as a bounded invariant.
   */

  import CrosslinkBftHeights(
    MaxConsensusHeight = 3,
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

  // Track the per-height decision history so the consecutive-pair lemmas can
  // refer to (h, h+1) decisions without reconstructing them from the set.
  var decisionAt: ConsensusHeight_t -> Snapshot_t
  var priorFinal: Snapshot_t
  var priorHeight: ConsensusHeight_t

  action InductiveInit = all {
    Init,
    decisionAt' = Map(0 -> InitialFinalized),
    priorFinal' = InitialFinalized,
    priorHeight' = 0,
  }

  action InductiveApplyScheduledDecision = all {
    ApplyScheduledDecision,
    decisionAt' = decisionAt.put(consensusHeight + 1, ScheduledDecision.get(consensusHeight + 1)),
    priorFinal' = latestFinal,
    priorHeight' = consensusHeight,
  }

  action InductiveStutter = all {
    Stutter,
    decisionAt' = decisionAt,
    priorFinal' = priorFinal,
    priorHeight' = priorHeight,
  }

  action InductiveNext =
    any {
      InductiveApplyScheduledDecision,
      InductiveStutter,
    }

  // ---------------------------------------------------------------------------
  // Inductive invariant clauses
  // ---------------------------------------------------------------------------

  // Clause (a): the height-(h+1) decision extends the height-h finalized
  // prefix. Stated as: after every non-trivial transition, latestFinal extends
  // priorFinal.
  val BaselineHeightSuccessorExtendsPrefix =
    Extends(latestFinal, priorFinal)

  // Clause (b): no Tenderlink agreement at the successor height finalizes a
  // value outside the predecessor's ancestor chain. Stated as: latestFinal
  // agrees with every previously finalized value.
  val BaselineNoForkFinalityAtSuccessorHeight =
    finalized.forall(v => Agrees(latestFinal, v))

  // Clause (c): the sticky baseline at h+1 cannot finalize a fork that
  // competes with height h. Stated as: if latestFinal differs from priorFinal,
  // it strictly extends it (not a sibling).
  val BaselineStickyBaselineDoesNotFinalizeForkAtSuccessor =
    (latestFinal == priorFinal) or
    (Extends(latestFinal, priorFinal) and Height(latestFinal) > Height(priorFinal))

  // Clause (d): successor height is exactly predecessor + 1 (no skipped
  // heights).
  val BaselineSuccessorHeightIsConsecutive =
    consensusHeight == priorHeight or
    consensusHeight == priorHeight + 1

  // Clause (e): every recorded decision in the history is a snapshot and is
  // an ancestor of latestFinal.
  val BaselineRecordedDecisionsAreAncestors =
    decisionAt.keys().forall(h =>
      IsSnapshot(decisionAt.get(h)) and Extends(latestFinal, decisionAt.get(h))
    )

  // Compound inductive safety: the conjunction of the consecutive-pair
  // clauses plus the monolithic bounded safety from the base model.
  val BaselineInductiveFinalitySafety =
    Safety and
    BaselineHeightSuccessorExtendsPrefix and
    BaselineNoForkFinalityAtSuccessorHeight and
    BaselineStickyBaselineDoesNotFinalizeForkAtSuccessor and
    BaselineSuccessorHeightIsConsecutive and
    BaselineRecordedDecisionsAreAncestors

  // ---------------------------------------------------------------------------
  // Positive witnesses
  // ---------------------------------------------------------------------------

  run inductiveConsecutiveHeightsExtendPrefixTest = {
    InductiveInit
      .then(InductiveApplyScheduledDecision)
      .then(all {
        assert(consensusHeight == 1),
        assert(latestFinal == "a2"),
        assert(priorFinal == "g"),
        assert(BaselineHeightSuccessorExtendsPrefix),
        assert(BaselineNoForkFinalityAtSuccessorHeight),
        assert(BaselineStickyBaselineDoesNotFinalizeForkAtSuccessor),
        assert(BaselineSuccessorHeightIsConsecutive),
        assert(BaselineRecordedDecisionsAreAncestors),
        assert(BaselineInductiveFinalitySafety),
        InductiveStutter,
      })
      .then(InductiveApplyScheduledDecision)
      .then(all {
        assert(consensusHeight == 2),
        assert(latestFinal == "a3"),
        assert(priorFinal == "a2"),
        assert(BaselineHeightSuccessorExtendsPrefix),
        assert(BaselineNoForkFinalityAtSuccessorHeight),
        assert(BaselineStickyBaselineDoesNotFinalizeForkAtSuccessor),
        assert(BaselineSuccessorHeightIsConsecutive),
        assert(BaselineRecordedDecisionsAreAncestors),
        assert(BaselineInductiveFinalitySafety),
        InductiveStutter,
      })
  }

  // ---------------------------------------------------------------------------
  // Negative witnesses: each lemma rejects the corresponding bad shape.
  // ---------------------------------------------------------------------------

  // Each `.fail()` witness drives the system into a bad shape and then
  // asserts the lemma still holds. The lemma is violated, so the assertion
  // fails as expected, demonstrating that the lemma rejects the bad shape.

  // A fork-finality attempt at the successor height violates the
  // "successor extends predecessor" lemma.
  run falseHeightSuccessorExtendsPrefixWitnessTest = {
    InductiveInit
      .then(InductiveApplyScheduledDecision)
      .then(all {
        // Pretend the next decision is b3 (a fork) instead of a3.
        assert(latestFinal == "a2"),
        latestFinal' = "b3",
        priorFinal' = latestFinal,
        priorHeight' = consensusHeight,
        consensusHeight' = consensusHeight + 1,
        finalized' = finalized.union(Set("b3")),
        decisionAt' = decisionAt.put(consensusHeight + 1, "b3"),
        heightAction' = "ForkAttempt",
      })
      .then(all {
        BaselineHeightSuccessorExtendsPrefix,
        InductiveStutter,
      })
      .fail()
  }

  // A fork-finality attempt also violates the "no fork at successor" lemma
  // because the new finalized value disagrees with already-finalized "a2".
  run falseNoForkFinalityAtSuccessorHeightWitnessTest = {
    InductiveInit
      .then(InductiveApplyScheduledDecision)
      .then(all {
        assert(latestFinal == "a2"),
        latestFinal' = "b3",
        priorFinal' = latestFinal,
        priorHeight' = consensusHeight,
        consensusHeight' = consensusHeight + 1,
        finalized' = finalized.union(Set("b3")),
        decisionAt' = decisionAt.put(consensusHeight + 1, "b3"),
        heightAction' = "ForkAttempt",
      })
      .then(all {
        BaselineNoForkFinalityAtSuccessorHeight,
        InductiveStutter,
      })
      .fail()
  }

  // A fork-finality attempt violates the sticky-baseline-no-fork lemma:
  // the new latestFinal differs from priorFinal but does not extend it.
  run falseStickyBaselineDoesNotFinalizeForkAtSuccessorWitnessTest = {
    InductiveInit
      .then(InductiveApplyScheduledDecision)
      .then(all {
        assert(latestFinal == "a2"),
        latestFinal' = "b3",
        priorFinal' = latestFinal,
        priorHeight' = consensusHeight,
        consensusHeight' = consensusHeight + 1,
        finalized' = finalized.union(Set("b3")),
        decisionAt' = decisionAt.put(consensusHeight + 1, "b3"),
        heightAction' = "ForkAttempt",
      })
      .then(all {
        BaselineStickyBaselineDoesNotFinalizeForkAtSuccessor,
        InductiveStutter,
      })
      .fail()
  }

  // Skipping a height violates the "successor is consecutive" lemma.
  run falseSuccessorHeightIsConsecutiveWitnessTest = {
    InductiveInit
      .then(all {
        assert(consensusHeight == 0),
        consensusHeight' = 2,
        latestFinal' = "a3",
        priorFinal' = latestFinal,
        priorHeight' = consensusHeight,
        finalized' = finalized.union(Set("a3")),
        decisionAt' = decisionAt.put(2, "a3"),
        heightAction' = "SkipHeight",
      })
      .then(all {
        BaselineSuccessorHeightIsConsecutive,
        InductiveStutter,
      })
      .fail()
  }
}

module CrosslinkBaselineInductiveFinalityModel {
  import CrosslinkBaselineInductiveFinality.* from "./CrosslinkBaselineInductiveFinality"
  export CrosslinkBaselineInductiveFinality.*
}
