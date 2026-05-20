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

  // Shell-level safety alias used by the first-class BaselineNext gates.
  val BaselineSafety =
    BaselineTenderlinkSafety

  val BaselineFaultyInitDomainWellFormed =
    AllFaultyProposals.forall(m =>
      m.src.in(BaselineFaulty) and
      m.round.in(BaselineRounds) and
      m.proposal.in(BaselineValidSnapshots) and
      m.validRound.in(RoundsOrNil)
    ) and
    AllFaultyPrevotes.forall(m =>
      m.src.in(BaselineFaulty) and
      m.round.in(BaselineRounds) and
      m.id.in(SnapshotsOrNil)
    ) and
    AllFaultyPrecommits.forall(m =>
      m.src.in(BaselineFaulty) and
      m.round.in(BaselineRounds) and
      m.id.in(SnapshotsOrNil)
    )

  val BaselineEvidenceCoversObservedMessages =
    EvidenceCoversObservedMessages

  val BaselineFaultyInitSafety =
    BaselineTenderlinkSafety and
    BaselineFaultyInitDomainWellFormed and
    BaselineEvidenceCoversObservedMessages

  val BaselineAgreement =
    Agreement

  val BaselineValidity =
    Validity

  val BaselineAccountability =
    Accountability

  action BaselineInitWithFaultyEvidence =
    InitWithFaultyEvidence

  // BaselineNext is now a first-class transition system. It no longer
  // delegates to the focused-model `Next`. The disjunction below mirrors the
  // upstream Tendermint.qnt `Next` shape (proposer/proposal/vote/timeout/
  // catchup/decide), but every action is the baseline-prefixed shell action
  // and the Crosslink rules are preserved by construction:
  //   - Value selection via `head - sigma` (BaselineInsertProposal uses
  //     `StickyOrStreamProposal`).
  //   - Sticky nil-precommit semantics (BaselineStartNextRoundAfterPrecommit-
  //     Quorum and BaselineApplyLateNilPrecommitCertificate keep the
  //     baseline's `ResampleOnNilPrecommit = false` carryover).
  //   - Decision freshness via BaselineDecide's underlying
  //     `IsFreshForRound` precondition.
  // The precommit-quorum handlers are also exposed in evidence-set-parameter
  // shape via BaselineUponPrecommitQuorumEvidence so symbolic gates can range
  // over arbitrary precommit-evidence sets, matching the upstream
  // `UponPrecommitQuorum(p, evidence)` interface while keeping baseline
  // behavior unchanged.
  action BaselineNext = {
    nondet p = oneOf(BaselineCorr)
    any {
      BaselineInsertProposal(p),
      nondet v = oneOf(BaselineValidSnapshots)
      BaselineUponProposalInPropose(p, v),
      nondet v = oneOf(BaselineValidSnapshots)
      nondet vr = oneOf(BaselineRounds)
      BaselineUponProposalInProposeAndPrevote(p, v, vr),
      BaselineTimeoutProposePrevoteNil(p),
      nondet v = oneOf(BaselineValidSnapshots)
      BaselineUponValuePrevoteQuorum(p, v),
      BaselineTimeoutPrevotePrecommitNil(p),
      BaselineUponNilPrevoteQuorum(p),
      BaselineUponStreamChangePrecommitNil(p),
      BaselineStartNextRoundAfterPrecommitQuorum(p),
      BaselineTimeoutPrecommitStartNextRound(p),
      nondet targetRound = oneOf(BaselineRounds)
      BaselineCatchUpToRound(p, targetRound),
      nondet r = oneOf(BaselineRounds)
      nondet v = oneOf(BaselineValidSnapshots)
      BaselineDecide(p, r, v),
      // Evidence-set-shaped precommit-quorum handler: ranges over arbitrary
      // subsets of observed precommit evidence, matching upstream's
      // UponPrecommitQuorum(p, evidence) interface. Behavior is identical to
      // the focused baseline; only the interface differs.
      nondet r = oneOf(BaselineRounds)
      nondet evidence = oneOf(powerset(msgsPrecommit.get(r)))
      BaselineUponPrecommitQuorumEvidence(p, r, evidence),
    }
  }

  action BaselineStartRound(
    p: BaselineProc_t,
    targetRound: BaselineRound_t,
  ): bool =
    StartRound(p, targetRound)

  action BaselineInsertProposal(p: BaselineProc_t): bool =
    InsertProposal(p)

  action BaselineUponProposalPrevote(
    p: BaselineProc_t,
    v: BaselineSnapshot_t,
  ): bool =
    UponProposalPrevote(p, v)

  action BaselineUponProposalInPropose(
    p: BaselineProc_t,
    v: BaselineSnapshot_t,
  ): bool =
    UponProposalInPropose(p, v)

  action BaselineUponProposalInProposeAndPrevote(
    p: BaselineProc_t,
    v: BaselineSnapshot_t,
    vr: BaselineRound_t,
  ): bool =
    UponProposalInProposeAndPrevote(p, v, vr)

  action BaselineTimeoutProposePrevoteNil(p: BaselineProc_t): bool =
    TimeoutProposePrevoteNil(p)

  action BaselineUponValuePrevoteQuorum(
    p: BaselineProc_t,
    v: BaselineSnapshot_t,
  ): bool =
    UponValuePrevoteQuorum(p, v)

  action BaselineTimeoutPrevotePrecommitNil(p: BaselineProc_t): bool =
    TimeoutPrevotePrecommitNil(p)

  action BaselineUponNilPrevoteQuorum(p: BaselineProc_t): bool =
    UponNilPrevoteQuorum(p)

  action BaselineUponStreamChangePrecommitNil(p: BaselineProc_t): bool =
    UponStreamChangePrecommitNil(p)

  action BaselineStartNextRoundAfterPrecommitQuorum(
    p: BaselineProc_t,
  ): bool =
    StartNextRoundAfterPrecommitQuorum(p)

  action BaselineTimeoutPrecommitStartNextRound(p: BaselineProc_t): bool =
    TimeoutPrecommitStartNextRound(p)

  action BaselineCatchUpToRound(
    p: BaselineProc_t,
    targetRound: BaselineRound_t,
  ): bool =
    CatchUpToRound(p, targetRound)

  action BaselineApplyLateNilPrecommitCertificate(
    p: BaselineProc_t,
    certRound: BaselineRound_t,
  ): bool =
    ApplyLateNilPrecommitCertificate(p, certRound)

  action BaselineDecide(
    p: BaselineProc_t,
    r: BaselineRound_t,
    v: BaselineSnapshot_t,
  ): bool =
    Decide(p, r, v)

  // BaselineUponPrecommitQuorumEvidence(p, r, evidence) is the evidence-set
  // -parameter-shaped precommit-quorum handler. It takes an arbitrary subset
  // of observed precommit messages at round `r` and requires that subset to
  // contain precommits from at least THRESHOLD2 = 2T+1 distinct sources,
  // matching the upstream UponPrecommitQuorum(p, evidence) interface.
  //
  // Baseline behavior is preserved: the action dispatches to either
  // - BaselineDecide(p, r, v) when the evidence projects onto a single
  //   non-nil value `v` with full precommit quorum (value-commit path), or
  // - BaselineStartNextRoundAfterPrecommitQuorum(p) otherwise (sticky nil/
  //   any-precommit round advance). The sticky baseline does not clear
  //   same-round value, locked, or cached state on a nil-precommit quorum.
  action BaselineUponPrecommitQuorumEvidence(
    p: BaselineProc_t,
    r: BaselineRound_t,
    evidence: Set[Vote_t],
  ): bool = all {
    r.in(BaselineRounds),
    evidence.subseteq(msgsPrecommit.get(r)),
    size(evidence.map(m => m.src)) >= THRESHOLD2,
    any {
      // Value-commit dispatch: evidence carries a non-nil value precommit
      // quorum and the decided value is fresh for the round.
      nondet v = oneOf(BaselineValidSnapshots)
      all {
        evidence.filter(m => m.id == v).map(m => m.src).size() >= THRESHOLD2,
        Decide(p, r, v),
      },
      // Nil/any-precommit round-advance dispatch. Sticky baseline keeps
      // same-round value/locked/cached state across this transition.
      StartNextRoundAfterPrecommitQuorum(p),
    },
  }

  action BaselineUnchangedAll =
    unchangedAll

  def BaselineRoundOf(p: BaselineProc_t): BaselineRound_t =
    round.get(p)

  def BaselineStepOf(p: BaselineProc_t): str =
    step.get(p)

  def BaselineDecisionOf(p: BaselineProc_t): BaselineSnapshot_t =
    decision.get(p)

  def BaselineLockedValueOf(p: BaselineProc_t): BaselineSnapshot_t =
    lockedValue.get(p)

  def BaselineLockedRoundOf(p: BaselineProc_t): BaselineRound_t =
    lockedRound.get(p)

  def BaselineValidValueOf(p: BaselineProc_t): BaselineSnapshot_t =
    validValue.get(p)

  def BaselineValidRoundOf(p: BaselineProc_t): BaselineRound_t =
    validRound.get(p)

  def BaselineHasProposal(
    r: BaselineRound_t,
    v: BaselineSnapshot_t,
  ): bool =
    HasProposal(r, v)

  def BaselinePrevoteQuorum(
    r: BaselineRound_t,
    v: BaselineSnapshot_t,
  ): bool =
    PrevoteQuorum(r, v)

  def BaselinePrecommitQuorum(
    r: BaselineRound_t,
    v: BaselineSnapshot_t,
  ): bool =
    PrecommitQuorum(r, v)

  def BaselineNilPrecommitCert(r: BaselineRound_t): bool =
    NilPrecommitCert(r)

  def BaselineRoundCatchupEvidence(r: BaselineRound_t): bool =
    RoundCatchupEvidence(r)

  def BaselineHasPrevoteFrom(
    p: BaselineProc_t,
    r: BaselineRound_t,
  ): bool =
    HasPrevoteFrom(p, r)

  def BaselineHasPrecommitFrom(
    p: BaselineProc_t,
    r: BaselineRound_t,
  ): bool =
    HasPrecommitFrom(p, r)
}
