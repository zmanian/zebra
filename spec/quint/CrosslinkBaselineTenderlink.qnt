// -*- mode: Bluespec; -*-
module CrosslinkBaselineTenderlink {
  /*
   Upstream-shaped baseline Crosslink/Tenderlink interface.

   This module keeps the current fixed-sigma/sticky semantics from
   CrosslinkResampling with ResampleOnNilPrecommit = false, but gives the
   baseline a Tendermint-like parameter shell. The Crosslink-specific value rule
   is:

     BaselineStream(round) =
       ancestor(bestTip(round), height(bestTip(round)) - sigma)

   The full upstream faulty-message injection and transition surface are still
   tracked in baseline-completeness-audit.md; this module is the first
   parameterized baseline shell for that work.
   */

  type BaselineProc_t = str
  type BaselineSnapshot_t = str
  type BaselineRound_t = int
  type BaselineHeight_t = int

  const BaselineCorr: Set[BaselineProc_t]
  const BaselineFaulty: Set[BaselineProc_t]
  const BaselineN: int
  const BaselineT: int
  const BaselineValidSnapshots: Set[BaselineSnapshot_t]
  const BaselineInvalidSnapshots: Set[BaselineSnapshot_t]
  const BaselineMaxRound: BaselineRound_t
  const BaselineProposer: BaselineRound_t -> BaselineProc_t
  const BaselineSigma: int
  const BaselineBestTip: BaselineRound_t -> BaselineSnapshot_t
  const BaselineHeight: BaselineSnapshot_t -> BaselineHeight_t
  const BaselineAncestorAt: BaselineSnapshot_t -> (BaselineHeight_t -> BaselineSnapshot_t)

  pure val BaselineRounds = 0.to(BaselineMaxRound)
  pure val BaselineAllSnapshots =
    BaselineValidSnapshots.union(BaselineInvalidSnapshots)
  pure val BaselineNilSnapshot = "None"

  assume baseline_corr_and_faulty_disjoint =
    BaselineCorr.intersect(BaselineFaulty) == Set()

  assume baseline_corr_and_faulty_make_n =
    BaselineN == size(BaselineCorr.union(BaselineFaulty))

  assume baseline_faulty_at_threshold =
    size(BaselineFaulty) <= BaselineT

  assume baseline_nil_is_not_valid_snapshot =
    not(BaselineValidSnapshots.contains(BaselineNilSnapshot))

  assume baseline_invalid_snapshots_are_not_valid =
    BaselineValidSnapshots.intersect(BaselineInvalidSnapshots) == Set()

  pure def BaselineBestTipAt(r: BaselineRound_t): BaselineSnapshot_t =
    BaselineBestTip.get(r)

  pure def BaselineAncestorAtHeight(
    v: BaselineSnapshot_t,
    h: BaselineHeight_t,
  ): BaselineSnapshot_t =
    BaselineAncestorAt.get(v).get(h)

  pure def BaselineHeadMinusSigma(r: BaselineRound_t): BaselineSnapshot_t =
    BaselineAncestorAtHeight(
      BaselineBestTipAt(r),
      BaselineHeight.get(BaselineBestTipAt(r)) - BaselineSigma
    )

  pure val BaselineStream: BaselineRound_t -> BaselineSnapshot_t =
    BaselineRounds.mapBy(r => BaselineHeadMinusSigma(r))

  pure val BaselineStreamMatchesFixedSigma =
    BaselineRounds.forall(r =>
      BaselineStream.get(r) == BaselineHeadMinusSigma(r)
    )

  assume baseline_sampled_values_are_valid =
    BaselineRounds.forall(r =>
      BaselineValidSnapshots.contains(BaselineStream.get(r))
    )

  import CrosslinkResamplingTest(
    Corr = BaselineCorr,
    Faulty = BaselineFaulty,
    N = BaselineN,
    T = BaselineT,
    MaxRound = BaselineMaxRound,
    Proposer = BaselineProposer,
    Stream = BaselineStream,
    Snapshots = BaselineValidSnapshots,
    ResampleOnNilPrecommit = false,
    ExpectedRound1Proposal = BaselineStream.get(0),
  ).* from "./CrosslinkResampling"

  val BaselineTenderlinkSafety =
    Safety and BaselineStreamMatchesFixedSigma

  val BaselineAgreement =
    Agreement

  val BaselineValidity =
    Validity

  val BaselineAccountability =
    Accountability
}
