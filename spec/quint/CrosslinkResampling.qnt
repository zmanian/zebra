// -*- mode: Bluespec; -*-
module CrosslinkResampling {
  /*
   A small Crosslink/Tenderlink model derived from the current Quint
   Tendermint examples, focused on one question:

     What changes when a 2f+1 PRECOMMIT nil certificate lets the next
     round resample Crosslink's moving PoW stream?

   This is not yet a full Tendermint accountability model. It intentionally
   isolates the Crosslink-specific moving-value part:

   - BFT values are PoW snapshots sampled from Stream(round), modelling the
     implementation's current "head - sigma" proposal.
   - A nil-precommit quorum for round r is a round-abandon certificate.
   - In the resampling variant, that certificate clears Crosslink's proposal
     cache and same-round Tendermint valid/lock state for r, allowing the next
     proposer to sample Stream(r + 1).
   - In the current/sticky variant, the proposal cache continues to carry the
     stale sample into the next round, and same-round valid/lock state blocks
     fresh voting.
   - Older Tendermint validValue/lockedValue state is preserved; the nil
     precommit certificate is only an unlock certificate for its own round.
   - The unlock rule keeps Tendermint's value-lock safety argument: retained
     locks are backed by value precommits, and a nil certificate can coexist
     with at most f correct same-round value locks, not a commit-capable quorum.
   */

  type Proc_t = str
  type Snapshot_t = str
  type Round_t = int
  type Step_t = str

  const Corr: Set[Proc_t]
  const Faulty: Set[Proc_t]
  const N: int
  const T: int
  const MaxRound: Round_t
  const Proposer: Round_t -> Proc_t
  const Stream: Round_t -> Snapshot_t
  const Snapshots: Set[Snapshot_t]
  const ResampleOnNilPrecommit: bool

  pure val Rounds = 0.to(MaxRound)
  pure val NilRound = -1
  pure val NilSnapshot = "None"
  pure val THRESHOLD2 = 2 * T + 1

  assume corr_and_faulty_disjoint = Corr.intersect(Faulty) == Set()
  assume corr_and_faulty_make_n = N == size(Corr.union(Faulty))
  assume faulty_at_threshold = size(Faulty) <= T
  assume quorum_intersection_has_correct =
    2 * THRESHOLD2 - N > T
  assume nil_is_not_snapshot = not(Snapshots.contains(NilSnapshot))
  assume streams_are_snapshots =
    Rounds.forall(r => Snapshots.contains(Stream.get(r)))

  type Propose_t = {
    src: Proc_t,
    round: Round_t,
    proposal: Snapshot_t,
    validRound: Round_t,
  }

  type Vote_t = {
    src: Proc_t,
    round: Round_t,
    id: Snapshot_t,
  }

  var round: Proc_t -> Round_t
  var step: Proc_t -> Step_t
  var decision: Proc_t -> Snapshot_t
  var lockedValue: Proc_t -> Snapshot_t
  var lockedRound: Proc_t -> Round_t
  var validValue: Proc_t -> Snapshot_t
  var validRound: Proc_t -> Round_t
  var cachedProposal: Proc_t -> Snapshot_t
  var cachedProposalRound: Proc_t -> Round_t
  var msgsPropose: Round_t -> Set[Propose_t]
  var msgsPrevote: Round_t -> Set[Vote_t]
  var msgsPrecommit: Round_t -> Set[Vote_t]
  var firedAction: str

  pure def IsValidSnapshot(v: Snapshot_t): bool =
    Snapshots.contains(v)

  pure def IsFreshForRound(r: Round_t, v: Snapshot_t): bool =
    Stream.get(r) == v

  pure def StreamChangedAfter(r: Round_t): bool =
    (r + 1).in(Rounds) and Stream.get(r + 1) != Stream.get(r)

  pure def Voters(votes: Set[Vote_t]): Set[Proc_t] =
    votes.map(m => m.src)

  def PrevoteQuorum(r: Round_t, v: Snapshot_t): bool =
    size(Voters(msgsPrevote.get(r).filter(m => m.id == v))) >= THRESHOLD2

  def PrecommitQuorum(r: Round_t, v: Snapshot_t): bool =
    size(Voters(msgsPrecommit.get(r).filter(m => m.id == v))) >= THRESHOLD2

  def NilPrecommitCert(r: Round_t): bool =
    PrecommitQuorum(r, NilSnapshot)

  def HasPrecommitFrom(p: Proc_t, r: Round_t): bool =
    msgsPrecommit.get(r).exists(m => m.src == p)

  def HasValuePrecommitFrom(p: Proc_t, r: Round_t, v: Snapshot_t): bool =
    msgsPrecommit.get(r).exists(m => m.src == p and m.id == v)

  def HasNilPrecommitFrom(p: Proc_t, r: Round_t): bool =
    msgsPrecommit.get(r).exists(m => m.src == p and m.id == NilSnapshot)

  def CorrectValueLocks(r: Round_t, v: Snapshot_t): Set[Proc_t] =
    Corr.filter(p => lockedRound.get(p) == r and lockedValue.get(p) == v)

  def HasProposal(r: Round_t, v: Snapshot_t): bool =
    msgsPropose.get(r).exists(m =>
      m.src == Proposer.get(r) and m.round == r and m.proposal == v
    )

  def StickyOrStreamProposal(p: Proc_t): Snapshot_t =
    if (validValue.get(p) == NilSnapshot) {
      if (cachedProposal.get(p) == NilSnapshot) {
        Stream.get(round.get(p))
      } else {
        cachedProposal.get(p)
      }
    } else {
      validValue.get(p)
    }

  action Init = all {
    round' = Corr.mapBy(_ => 0),
    step' = Corr.mapBy(_ => "propose"),
    decision' = Corr.mapBy(_ => NilSnapshot),
    lockedValue' = Corr.mapBy(_ => NilSnapshot),
    lockedRound' = Corr.mapBy(_ => NilRound),
    validValue' = Corr.mapBy(_ => NilSnapshot),
    validRound' = Corr.mapBy(_ => NilRound),
    cachedProposal' = Corr.mapBy(_ => NilSnapshot),
    cachedProposalRound' = Corr.mapBy(_ => NilRound),
    msgsPropose' = Rounds.mapBy(_ => Set()),
    msgsPrevote' = Rounds.mapBy(_ => Set()),
    msgsPrecommit' = Rounds.mapBy(_ => Set()),
    firedAction' = "Init",
  }

  action BroadcastProposal(p: Proc_t, r: Round_t, v: Snapshot_t, vr: Round_t): bool =
    msgsPropose' = msgsPropose.setBy(r, old => old.union(Set({
      src: p,
      round: r,
      proposal: v,
      validRound: vr,
    })))

  action BroadcastPrevote(p: Proc_t, r: Round_t, v: Snapshot_t): bool =
    msgsPrevote' = msgsPrevote.setBy(r, old => old.union(Set({
      src: p,
      round: r,
      id: v,
    })))

  action BroadcastPrecommit(p: Proc_t, r: Round_t, v: Snapshot_t): bool =
    msgsPrecommit' = msgsPrecommit.setBy(r, old => old.union(Set({
      src: p,
      round: r,
      id: v,
    })))

  action InsertProposal(p: Proc_t): bool = {
    val r = round.get(p)
    val proposal = StickyOrStreamProposal(p)
    all {
      p == Proposer.get(r),
      step.get(p) == "propose",
      msgsPropose.get(r).forall(m => m.src != p),
      BroadcastProposal(p, r, proposal, validRound.get(p)),
      firedAction' = "InsertProposal",
      round' = round,
      step' = step,
      decision' = decision,
      lockedValue' = lockedValue,
      lockedRound' = lockedRound,
      validValue' = validValue,
      validRound' = validRound,
      cachedProposal' = cachedProposal,
      cachedProposalRound' = cachedProposalRound,
      msgsPrevote' = msgsPrevote,
      msgsPrecommit' = msgsPrecommit,
    }
  }

  action UponProposalPrevote(p: Proc_t, v: Snapshot_t): bool = {
    val r = round.get(p)
    val vote = if (
      IsValidSnapshot(v) and
      IsFreshForRound(r, v) and
      (lockedRound.get(p) == NilRound or lockedValue.get(p) == v)
    ) {
      v
    } else {
      NilSnapshot
    }
    all {
      step.get(p) == "propose",
      HasProposal(r, v),
      BroadcastPrevote(p, r, vote),
      step' = step.set(p, "prevote"),
      firedAction' = "UponProposalPrevote",
      round' = round,
      decision' = decision,
      lockedValue' = lockedValue,
      lockedRound' = lockedRound,
      validValue' = validValue,
      validRound' = validRound,
      cachedProposal' = cachedProposal,
      cachedProposalRound' = cachedProposalRound,
      msgsPropose' = msgsPropose,
      msgsPrecommit' = msgsPrecommit,
    }
  }

  action UponValuePrevoteQuorum(p: Proc_t, v: Snapshot_t): bool = {
    val r = round.get(p)
    all {
      step.get(p).in(Set("prevote", "precommit")),
      HasProposal(r, v),
      IsValidSnapshot(v),
      IsFreshForRound(r, v),
      PrevoteQuorum(r, v),
      not(HasPrecommitFrom(p, r)),
      BroadcastPrecommit(p, r, v),
      step' = step.set(p, "precommit"),
      lockedValue' = lockedValue.set(p, v),
      lockedRound' = lockedRound.set(p, r),
      validValue' = validValue.set(p, v),
      validRound' = validRound.set(p, r),
      cachedProposal' = cachedProposal,
      cachedProposalRound' = cachedProposalRound,
      firedAction' = "UponValuePrevoteQuorum",
      round' = round,
      decision' = decision,
      msgsPropose' = msgsPropose,
      msgsPrevote' = msgsPrevote,
    }
  }

  action UponStreamChangePrecommitNil(p: Proc_t): bool = {
    val r = round.get(p)
    all {
      step.get(p).in(Set("prevote", "precommit")),
      StreamChangedAfter(r),
      not(HasPrecommitFrom(p, r)),
      BroadcastPrecommit(p, r, NilSnapshot),
      step' = step.set(p, "precommit"),
      firedAction' = "UponStreamChangePrecommitNil",
      round' = round,
      decision' = decision,
      lockedValue' = lockedValue,
      lockedRound' = lockedRound,
      validValue' = validValue,
      validRound' = validRound,
      cachedProposal' = cachedProposal,
      cachedProposalRound' = cachedProposalRound,
      msgsPropose' = msgsPropose,
      msgsPrevote' = msgsPrevote,
    }
  }

  action StartNextRoundAfterPrecommitQuorum(p: Proc_t): bool = {
    val oldRound = round.get(p)
    val nextRound = oldRound + 1
    val clearSameRoundCache = ResampleOnNilPrecommit and NilPrecommitCert(oldRound)
    all {
      nextRound.in(Rounds),
      size(Voters(msgsPrecommit.get(oldRound))) >= THRESHOLD2,
      round' = round.set(p, nextRound),
      step' = step.set(p, "propose"),
      validValue' =
        if (clearSameRoundCache and validRound.get(p) == oldRound) {
          validValue.set(p, NilSnapshot)
        } else {
          validValue
        },
      validRound' =
        if (clearSameRoundCache and validRound.get(p) == oldRound) {
          validRound.set(p, NilRound)
        } else {
          validRound
        },
      lockedValue' =
        if (clearSameRoundCache and lockedRound.get(p) == oldRound) {
          lockedValue.set(p, NilSnapshot)
        } else {
          lockedValue
        },
      lockedRound' =
        if (clearSameRoundCache and lockedRound.get(p) == oldRound) {
          lockedRound.set(p, NilRound)
        } else {
          lockedRound
        },
      cachedProposal' =
        if (clearSameRoundCache and cachedProposalRound.get(p) == oldRound) {
          cachedProposal.set(p, NilSnapshot)
        } else {
          cachedProposal
        },
      cachedProposalRound' =
        if (clearSameRoundCache and cachedProposalRound.get(p) == oldRound) {
          cachedProposalRound.set(p, NilRound)
        } else {
          cachedProposalRound
        },
      firedAction' = "StartNextRoundAfterPrecommitQuorum",
      decision' = decision,
      msgsPropose' = msgsPropose,
      msgsPrevote' = msgsPrevote,
      msgsPrecommit' = msgsPrecommit,
    }
  }

  action ApplyLateNilPrecommitCertificate(p: Proc_t, certRound: Round_t): bool = {
    all {
      certRound.in(Rounds),
      certRound < round.get(p),
      ResampleOnNilPrecommit,
      NilPrecommitCert(certRound),
      round' = round,
      step' = step,
      validValue' =
        if (validRound.get(p) == certRound) {
          validValue.set(p, NilSnapshot)
        } else {
          validValue
        },
      validRound' =
        if (validRound.get(p) == certRound) {
          validRound.set(p, NilRound)
        } else {
          validRound
        },
      lockedValue' =
        if (lockedRound.get(p) == certRound) {
          lockedValue.set(p, NilSnapshot)
        } else {
          lockedValue
        },
      lockedRound' =
        if (lockedRound.get(p) == certRound) {
          lockedRound.set(p, NilRound)
        } else {
          lockedRound
        },
      cachedProposal' =
        if (cachedProposalRound.get(p) == certRound) {
          cachedProposal.set(p, NilSnapshot)
        } else {
          cachedProposal
        },
      cachedProposalRound' =
        if (cachedProposalRound.get(p) == certRound) {
          cachedProposalRound.set(p, NilRound)
        } else {
          cachedProposalRound
        },
      firedAction' = "ApplyLateNilPrecommitCertificate",
      decision' = decision,
      msgsPropose' = msgsPropose,
      msgsPrevote' = msgsPrevote,
      msgsPrecommit' = msgsPrecommit,
    }
  }

  action Decide(p: Proc_t, r: Round_t, v: Snapshot_t): bool = all {
    decision.get(p) == NilSnapshot,
    HasProposal(r, v),
    IsValidSnapshot(v),
    IsFreshForRound(r, v),
    PrecommitQuorum(r, v),
    decision' = decision.set(p, v),
    step' = step.set(p, "decided"),
    firedAction' = "Decide",
    round' = round,
    lockedValue' = lockedValue,
    lockedRound' = lockedRound,
    validValue' = validValue,
    validRound' = validRound,
    cachedProposal' = cachedProposal,
    cachedProposalRound' = cachedProposalRound,
    msgsPropose' = msgsPropose,
    msgsPrevote' = msgsPrevote,
    msgsPrecommit' = msgsPrecommit,
  }

  action SeedAbandonedRoundState(p: Proc_t): bool = {
    val r = 0
    val stale = Stream.get(r)
    all {
      r.in(Rounds),
      p.in(Corr),
      round' = round.set(p, r),
      step' = step.set(p, "precommit"),
      lockedValue' = lockedValue,
      lockedRound' = lockedRound,
      validValue' = validValue,
      validRound' = validRound,
      cachedProposal' = cachedProposal.set(p, stale),
      cachedProposalRound' = cachedProposalRound.set(p, r),
      BroadcastPrecommit(p, r, NilSnapshot),
      firedAction' = "SeedAbandonedRoundState",
      decision' = decision,
      msgsPropose' = msgsPropose,
      msgsPrevote' = msgsPrevote,
    }
  }

  action SeedValidNilPrecommitState(p: Proc_t): bool = {
    val r = 0
    val stale = Stream.get(r)
    all {
      r.in(Rounds),
      p.in(Corr),
      round' = round.set(p, r),
      step' = step.set(p, "precommit"),
      lockedValue' = lockedValue,
      lockedRound' = lockedRound,
      validValue' = validValue.set(p, stale),
      validRound' = validRound.set(p, r),
      cachedProposal' = cachedProposal.set(p, stale),
      cachedProposalRound' = cachedProposalRound.set(p, r),
      BroadcastPrecommit(p, r, NilSnapshot),
      firedAction' = "SeedValidNilPrecommitState",
      decision' = decision,
      msgsPropose' = msgsPropose,
      msgsPrevote' = msgsPrevote,
    }
  }

  action SeedSameRoundValueLock(p: Proc_t): bool = {
    val r = 0
    val locked = Stream.get(r)
    all {
      r.in(Rounds),
      p.in(Corr),
      round' = round.set(p, r),
      step' = step.set(p, "precommit"),
      lockedValue' = lockedValue.set(p, locked),
      lockedRound' = lockedRound.set(p, r),
      validValue' = validValue.set(p, locked),
      validRound' = validRound.set(p, r),
      cachedProposal' = cachedProposal,
      cachedProposalRound' = cachedProposalRound,
      BroadcastPrecommit(p, r, locked),
      firedAction' = "SeedSameRoundValueLock",
      decision' = decision,
      msgsPropose' = msgsPropose,
      msgsPrevote' = msgsPrevote,
    }
  }

  action SeedTimedOutSameRoundValueLock(p: Proc_t): bool = {
    val r = 0
    val nextRound = 1
    val locked = Stream.get(r)
    all {
      r.in(Rounds),
      nextRound.in(Rounds),
      p.in(Corr),
      round' = round.set(p, nextRound),
      step' = step.set(p, "propose"),
      lockedValue' = lockedValue.set(p, locked),
      lockedRound' = lockedRound.set(p, r),
      validValue' = validValue.set(p, locked),
      validRound' = validRound.set(p, r),
      cachedProposal' = cachedProposal,
      cachedProposalRound' = cachedProposalRound,
      BroadcastPrecommit(p, r, locked),
      firedAction' = "SeedTimedOutSameRoundValueLock",
      decision' = decision,
      msgsPropose' = msgsPropose,
      msgsPrevote' = msgsPrevote,
    }
  }

  action SeedFaultyNilPrecommit(p: Proc_t): bool = {
    val r = 0
    all {
      r.in(Rounds),
      p.in(Faulty),
      round' = round,
      step' = step,
      lockedValue' = lockedValue,
      lockedRound' = lockedRound,
      validValue' = validValue,
      validRound' = validRound,
      cachedProposal' = cachedProposal,
      cachedProposalRound' = cachedProposalRound,
      BroadcastPrecommit(p, r, NilSnapshot),
      firedAction' = "SeedFaultyNilPrecommit",
      decision' = decision,
      msgsPropose' = msgsPropose,
      msgsPrevote' = msgsPrevote,
    }
  }

  action SeedOlderLockWithNilPrecommit(p: Proc_t): bool = {
    val activeRound = 1
    val lockRound = 0
    val locked = Stream.get(lockRound)
    all {
      activeRound.in(Rounds),
      lockRound.in(Rounds),
      p.in(Corr),
      round' = round.set(p, activeRound),
      step' = step.set(p, "precommit"),
      lockedValue' = lockedValue.set(p, locked),
      lockedRound' = lockedRound.set(p, lockRound),
      validValue' = validValue.set(p, locked),
      validRound' = validRound.set(p, lockRound),
      cachedProposal' = cachedProposal,
      cachedProposalRound' = cachedProposalRound,
      msgsPrecommit' = msgsPrecommit
        .setBy(lockRound, old => old.union(Set({
          src: p,
          round: lockRound,
          id: locked,
        })))
        .setBy(activeRound, old => old.union(Set({
          src: p,
          round: activeRound,
          id: NilSnapshot,
        }))),
      firedAction' = "SeedOlderLockWithNilPrecommit",
      decision' = decision,
      msgsPropose' = msgsPropose,
      msgsPrevote' = msgsPrevote,
    }
  }

  action SeedCorrectPrecommitEvidence(p: Proc_t, r: Round_t, v: Snapshot_t): bool =
    all {
      p.in(Corr),
      r.in(Rounds),
      IsValidSnapshot(v),
      BroadcastPrecommit(p, r, v),
      firedAction' = "SeedCorrectPrecommitEvidence",
      round' = round,
      step' = step,
      decision' = decision,
      lockedValue' = lockedValue,
      lockedRound' = lockedRound,
      validValue' = validValue,
      validRound' = validRound,
      cachedProposal' = cachedProposal,
      cachedProposalRound' = cachedProposalRound,
      msgsPropose' = msgsPropose,
      msgsPrevote' = msgsPrevote,
    }

  action SeedFaultyPrecommitEvidence(p: Proc_t, r: Round_t, v: Snapshot_t): bool =
    all {
      p.in(Faulty),
      r.in(Rounds),
      IsValidSnapshot(v),
      BroadcastPrecommit(p, r, v),
      firedAction' = "SeedFaultyPrecommitEvidence",
      round' = round,
      step' = step,
      decision' = decision,
      lockedValue' = lockedValue,
      lockedRound' = lockedRound,
      validValue' = validValue,
      validRound' = validRound,
      cachedProposal' = cachedProposal,
      cachedProposalRound' = cachedProposalRound,
      msgsPropose' = msgsPropose,
      msgsPrevote' = msgsPrevote,
    }

  action Next =
    nondet p = oneOf(Corr)
    any {
      InsertProposal(p),
      nondet v = oneOf(Snapshots)
      UponProposalPrevote(p, v),
      nondet v = oneOf(Snapshots)
      UponValuePrevoteQuorum(p, v),
      UponStreamChangePrecommitNil(p),
      StartNextRoundAfterPrecommitQuorum(p),
      nondet r = oneOf(Rounds)
      nondet v = oneOf(Snapshots)
      Decide(p, r, v),
    }

  val Agreement =
    tuples(Corr, Corr).forall(((p, q)) =>
      decision.get(p) == NilSnapshot or
      decision.get(q) == NilSnapshot or
      decision.get(p) == decision.get(q)
    )

  val NoCorrectPrecommitEquivocation =
    Corr.forall(p =>
      Rounds.forall(r =>
        msgsPrecommit.get(r).filter(m => m.src == p).map(m => m.id).size() <= 1
      )
    )

  val SameRoundNilAndValueCommitExcluded =
    Rounds.forall(r =>
      NilPrecommitCert(r) implies
        Snapshots.forall(v => not(PrecommitQuorum(r, v)))
    )

  val LockedValueHasMatchingValidValue =
    Corr.forall(p =>
      lockedValue.get(p) == NilSnapshot or (
        validValue.get(p) == lockedValue.get(p) and
        validRound.get(p) >= lockedRound.get(p)
      )
    )

  val CorrectValueLocksHavePrecommitEvidence =
    Corr.forall(p =>
      lockedValue.get(p) == NilSnapshot or (
        lockedRound.get(p).in(Rounds) and
        IsValidSnapshot(lockedValue.get(p)) and
        HasValuePrecommitFrom(p, lockedRound.get(p), lockedValue.get(p))
      )
    )

  val NilCertLeavesAtMostFCurrentRoundLocks =
    Rounds.forall(r =>
      NilPrecommitCert(r) implies
        Snapshots.forall(v => size(CorrectValueLocks(r, v)) <= T)
    )

  def ConflictingValueCommits(
    r1: Round_t,
    v1: Snapshot_t,
    r2: Round_t,
    v2: Snapshot_t,
  ): bool =
    IsValidSnapshot(v1) and
    IsValidSnapshot(v2) and
    v1 != v2 and
    PrecommitQuorum(r1, v1) and
    PrecommitQuorum(r2, v2)

  def CorrectSameRoundEquivocationEvidence(
    r: Round_t,
    v1: Snapshot_t,
    v2: Snapshot_t,
  ): bool =
    v1 != v2 and
    Corr.exists(p =>
      HasValuePrecommitFrom(p, r, v1) and
      HasValuePrecommitFrom(p, r, v2)
    )

  def CorrectNilValueEquivocationEvidence(r: Round_t, v: Snapshot_t): bool =
    IsValidSnapshot(v) and
    NilPrecommitCert(r) and
    PrecommitQuorum(r, v) and
    Corr.exists(p =>
      HasNilPrecommitFrom(p, r) and HasValuePrecommitFrom(p, r, v)
    )

  def CorrectValueSwitchWithoutUnlock(
    p: Proc_t,
    fromRound: Round_t,
    fromValue: Snapshot_t,
    toRound: Round_t,
    toValue: Snapshot_t,
  ): bool =
    p.in(Corr) and
    fromRound < toRound and
    IsValidSnapshot(fromValue) and
    IsValidSnapshot(toValue) and
    fromValue != toValue and
    HasValuePrecommitFrom(p, fromRound, fromValue) and
    HasValuePrecommitFrom(p, toRound, toValue) and
    not(NilPrecommitCert(fromRound))

  def ConflictHasAccountabilityEvidence(
    r1: Round_t,
    v1: Snapshot_t,
    r2: Round_t,
    v2: Snapshot_t,
  ): bool =
    ConflictingValueCommits(r1, v1, r2, v2) and
    if (r1 == r2) {
      CorrectSameRoundEquivocationEvidence(r1, v1, v2)
    } else if (r1 < r2) {
      CorrectNilValueEquivocationEvidence(r1, v1) or
      Corr.exists(p => CorrectValueSwitchWithoutUnlock(p, r1, v1, r2, v2))
    } else {
      CorrectNilValueEquivocationEvidence(r2, v2) or
      Corr.exists(p => CorrectValueSwitchWithoutUnlock(p, r2, v2, r1, v1))
    }

  val ConflictingCommitsAccountable =
    Rounds.forall(r1 =>
      Rounds.forall(r2 =>
        Snapshots.forall(v1 =>
          Snapshots.forall(v2 =>
            ConflictingValueCommits(r1, v1, r2, v2) implies
              ConflictHasAccountabilityEvidence(r1, v1, r2, v2)
          )
        )
      )
    )

  val Safety =
    Agreement and
    NoCorrectPrecommitEquivocation and
    SameRoundNilAndValueCommitExcluded and
    LockedValueHasMatchingValidValue and
    CorrectValueLocksHavePrecommitEvidence and
    NilCertLeavesAtMostFCurrentRoundLocks and
    ConflictingCommitsAccountable
}

module CrosslinkResamplingTest {
  import CrosslinkResampling.* from "./CrosslinkResampling"
  export CrosslinkResampling.*

  const ExpectedRound1Proposal: Snapshot_t

  action unchangedAll = all {
    round' = round,
    step' = step,
    decision' = decision,
    lockedValue' = lockedValue,
    lockedRound' = lockedRound,
    validValue' = validValue,
    validRound' = validRound,
    cachedProposal' = cachedProposal,
    cachedProposalRound' = cachedProposalRound,
    msgsPropose' = msgsPropose,
    msgsPrevote' = msgsPrevote,
    msgsPrecommit' = msgsPrecommit,
    firedAction' = firedAction,
  }

  run abandonedRoundProposalTest = {
    Init
      .then(SeedAbandonedRoundState("p1"))
      .then(SeedAbandonedRoundState("p2"))
      .then(SeedAbandonedRoundState("p3"))
      .then(StartNextRoundAfterPrecommitQuorum("p2"))
      .then(InsertProposal("p2"))
      .then(all {
        assert(msgsPropose.get(1).exists(m =>
          m.src == "p2" and m.round == 1 and m.proposal == ExpectedRound1Proposal
        )),
        unchangedAll,
      })
  }
}

module CrosslinkStickyModel {
  import CrosslinkResamplingTest(
    Corr = Set("p1", "p2", "p3"),
    Faulty = Set("p4"),
    N = 4,
    T = 1,
    MaxRound = 2,
    Proposer = Map(0 -> "p1", 1 -> "p2", 2 -> "p3"),
    Stream = Map(0 -> "s0", 1 -> "s1", 2 -> "s2"),
    Snapshots = Set("s0", "s1", "s2"),
    ResampleOnNilPrecommit = false,
    ExpectedRound1Proposal = "s0",
  ).*

  run currentProtocolCarriesStaleSampleTest = {
    Init
      .then(SeedAbandonedRoundState("p1"))
      .then(SeedAbandonedRoundState("p2"))
      .then(SeedAbandonedRoundState("p3"))
      .then(StartNextRoundAfterPrecommitQuorum("p2"))
      .then(InsertProposal("p2"))
      .then(all {
        assert(msgsPropose.get(1).exists(m =>
          m.src == "p2" and m.round == 1 and m.proposal == "s0"
        )),
        unchangedAll,
      })
  }

  run currentProtocolSameRoundLockBlocksFreshDecisionTest = {
    Init
      .then(SeedValidNilPrecommitState("p1"))
      .then(SeedValidNilPrecommitState("p2"))
      .then(SeedFaultyNilPrecommit("p4"))
      .then(SeedSameRoundValueLock("p3"))
      .then(StartNextRoundAfterPrecommitQuorum("p1"))
      .then(StartNextRoundAfterPrecommitQuorum("p2"))
      .then(StartNextRoundAfterPrecommitQuorum("p3"))
      .then(InsertProposal("p2"))
      .then(UponProposalPrevote("p1", "s1"))
      .fail()
  }
}

module CrosslinkNilResamplingModel {
  import CrosslinkResamplingTest(
    Corr = Set("p1", "p2", "p3"),
    Faulty = Set("p4"),
    N = 4,
    T = 1,
    MaxRound = 2,
    Proposer = Map(0 -> "p1", 1 -> "p2", 2 -> "p3"),
    Stream = Map(0 -> "s0", 1 -> "s1", 2 -> "s2"),
    Snapshots = Set("s0", "s1", "s2"),
    ResampleOnNilPrecommit = true,
    ExpectedRound1Proposal = "s1",
  ).*

  run nilPrecommitResamplesFreshStreamTest = {
    Init
      .then(SeedAbandonedRoundState("p1"))
      .then(SeedAbandonedRoundState("p2"))
      .then(SeedAbandonedRoundState("p3"))
      .then(StartNextRoundAfterPrecommitQuorum("p2"))
      .then(InsertProposal("p2"))
      .then(all {
        assert(msgsPropose.get(1).exists(m =>
          m.src == "p2" and m.round == 1 and m.proposal == "s1"
        )),
        unchangedAll,
      })
  }

  run nilPrecommitPreservesOlderTendermintValueLockTest = {
    Init
      .then(SeedOlderLockWithNilPrecommit("p1"))
      .then(SeedOlderLockWithNilPrecommit("p2"))
      .then(SeedOlderLockWithNilPrecommit("p3"))
      .then(StartNextRoundAfterPrecommitQuorum("p2"))
      .then(all {
        assert(lockedValue.get("p2") == "s0"),
        assert(lockedRound.get("p2") == 0),
        assert(validValue.get("p2") == "s0"),
        assert(validRound.get("p2") == 0),
        unchangedAll,
      })
  }

  run nilPrecommitUnlocksSameRoundTendermintStateTest = {
    Init
      .then(SeedValidNilPrecommitState("p1"))
      .then(SeedValidNilPrecommitState("p2"))
      .then(SeedFaultyNilPrecommit("p4"))
      .then(SeedSameRoundValueLock("p3"))
      .then(StartNextRoundAfterPrecommitQuorum("p2"))
      .then(StartNextRoundAfterPrecommitQuorum("p3"))
      .then(all {
        assert(validValue.get("p2") == "None"),
        assert(validRound.get("p2") == -1),
        assert(lockedValue.get("p2") == "None"),
        assert(lockedRound.get("p2") == -1),
        assert(lockedValue.get("p3") == "None"),
        assert(lockedRound.get("p3") == -1),
        assert(validValue.get("p3") == "None"),
        assert(validRound.get("p3") == -1),
        assert(NilPrecommitCert(0)),
        assert(not(PrecommitQuorum(0, "s0"))),
        unchangedAll,
      })
  }

  run lateNilPrecommitCertificateUnlocksAbandonedRoundTest = {
    Init
      .then(SeedValidNilPrecommitState("p1"))
      .then(SeedValidNilPrecommitState("p2"))
      .then(SeedFaultyNilPrecommit("p4"))
      .then(SeedTimedOutSameRoundValueLock("p3"))
      .then(ApplyLateNilPrecommitCertificate("p3", 0))
      .then(all {
        assert(round.get("p3") == 1),
        assert(validValue.get("p3") == "None"),
        assert(validRound.get("p3") == -1),
        assert(lockedValue.get("p3") == "None"),
        assert(lockedRound.get("p3") == -1),
        assert(NilPrecommitCert(0)),
        assert(not(PrecommitQuorum(0, "s0"))),
        unchangedAll,
      })
  }

  run nilPrecommitUnlockResamplesAndDecidesFreshValueTest = {
    Init
      .then(SeedValidNilPrecommitState("p1"))
      .then(SeedValidNilPrecommitState("p2"))
      .then(SeedFaultyNilPrecommit("p4"))
      .then(SeedSameRoundValueLock("p3"))
      .then(StartNextRoundAfterPrecommitQuorum("p1"))
      .then(StartNextRoundAfterPrecommitQuorum("p2"))
      .then(StartNextRoundAfterPrecommitQuorum("p3"))
      .then(InsertProposal("p2"))
      .then(UponProposalPrevote("p1", "s1"))
      .then(UponProposalPrevote("p2", "s1"))
      .then(UponProposalPrevote("p3", "s1"))
      .then(UponValuePrevoteQuorum("p1", "s1"))
      .then(UponValuePrevoteQuorum("p2", "s1"))
      .then(UponValuePrevoteQuorum("p3", "s1"))
      .then(Decide("p1", 1, "s1"))
      .then(all {
        assert(decision.get("p1") == "s1"),
        unchangedAll,
      })
  }

  run conflictingCommitsExposeInvalidUnlockEvidenceTest = {
    Init
      .then(SeedCorrectPrecommitEvidence("p1", 0, "s0"))
      .then(SeedCorrectPrecommitEvidence("p2", 0, "s0"))
      .then(SeedFaultyPrecommitEvidence("p4", 0, "s0"))
      .then(SeedCorrectPrecommitEvidence("p2", 1, "s1"))
      .then(SeedCorrectPrecommitEvidence("p3", 1, "s1"))
      .then(SeedFaultyPrecommitEvidence("p4", 1, "s1"))
      .then(all {
        assert(PrecommitQuorum(0, "s0")),
        assert(PrecommitQuorum(1, "s1")),
        assert(ConflictHasAccountabilityEvidence(0, "s0", 1, "s1")),
        assert(ConflictingCommitsAccountable),
        unchangedAll,
      })
  }

  run conflictWithBogusNilUnlockExposesEquivocationEvidenceTest = {
    Init
      .then(SeedValidNilPrecommitState("p2"))
      .then(SeedValidNilPrecommitState("p3"))
      .then(SeedFaultyNilPrecommit("p4"))
      .then(SeedCorrectPrecommitEvidence("p1", 0, "s0"))
      .then(SeedCorrectPrecommitEvidence("p2", 0, "s0"))
      .then(SeedFaultyPrecommitEvidence("p4", 0, "s0"))
      .then(SeedCorrectPrecommitEvidence("p2", 1, "s1"))
      .then(SeedCorrectPrecommitEvidence("p3", 1, "s1"))
      .then(SeedFaultyPrecommitEvidence("p4", 1, "s1"))
      .then(all {
        assert(NilPrecommitCert(0)),
        assert(PrecommitQuorum(0, "s0")),
        assert(PrecommitQuorum(1, "s1")),
        assert(CorrectNilValueEquivocationEvidence(0, "s0")),
        assert(ConflictHasAccountabilityEvidence(0, "s0", 1, "s1")),
        assert(ConflictingCommitsAccountable),
        unchangedAll,
      })
  }

  run nilPrecommitCertificateJustifiesSameRoundSwitchTest = {
    Init
      .then(SeedValidNilPrecommitState("p1"))
      .then(SeedValidNilPrecommitState("p2"))
      .then(SeedFaultyNilPrecommit("p4"))
      .then(SeedSameRoundValueLock("p3"))
      .then(StartNextRoundAfterPrecommitQuorum("p3"))
      .then(SeedCorrectPrecommitEvidence("p1", 1, "s1"))
      .then(SeedCorrectPrecommitEvidence("p2", 1, "s1"))
      .then(SeedCorrectPrecommitEvidence("p3", 1, "s1"))
      .then(all {
        assert(NilPrecommitCert(0)),
        assert(not(PrecommitQuorum(0, "s0"))),
        assert(not(CorrectValueSwitchWithoutUnlock("p3", 0, "s0", 1, "s1"))),
        assert(ConflictingCommitsAccountable),
        unchangedAll,
      })
  }

  run laterNilCertificateDoesNotUnlockOlderValueLockTest = {
    Init
      .then(SeedOlderLockWithNilPrecommit("p1"))
      .then(SeedOlderLockWithNilPrecommit("p2"))
      .then(SeedOlderLockWithNilPrecommit("p3"))
      .then(SeedCorrectPrecommitEvidence("p2", 2, "s2"))
      .then(SeedCorrectPrecommitEvidence("p3", 2, "s2"))
      .then(SeedFaultyPrecommitEvidence("p4", 2, "s2"))
      .then(all {
        assert(PrecommitQuorum(0, "s0")),
        assert(NilPrecommitCert(1)),
        assert(PrecommitQuorum(2, "s2")),
        assert(CorrectValueSwitchWithoutUnlock("p2", 0, "s0", 2, "s2")),
        assert(ConflictHasAccountabilityEvidence(0, "s0", 2, "s2")),
        assert(ConflictingCommitsAccountable),
        unchangedAll,
      })
  }
}

module CrosslinkNilResamplingLivenessModel {
  import CrosslinkResamplingTest(
    Corr = Set("p1", "p2", "p3"),
    Faulty = Set("p4"),
    N = 4,
    T = 1,
    MaxRound = 2,
    Proposer = Map(0 -> "p1", 1 -> "p2", 2 -> "p3"),
    Stream = Map(0 -> "s0", 1 -> "s1", 2 -> "s2"),
    Snapshots = Set("s0", "s1", "s2"),
    ResampleOnNilPrecommit = true,
    ExpectedRound1Proposal = "s1",
  ).*

  var phase: int

  action LivenessInit = all {
    Init,
    phase' = 0,
  }

  action LivenessStep = any {
    all { phase == 0, SeedValidNilPrecommitState("p1"), phase' = 1 },
    all { phase == 1, SeedValidNilPrecommitState("p2"), phase' = 2 },
    all { phase == 2, SeedFaultyNilPrecommit("p4"), phase' = 3 },
    all { phase == 3, SeedSameRoundValueLock("p3"), phase' = 4 },
    all { phase == 4, StartNextRoundAfterPrecommitQuorum("p1"), phase' = 5 },
    all { phase == 5, StartNextRoundAfterPrecommitQuorum("p2"), phase' = 6 },
    all { phase == 6, StartNextRoundAfterPrecommitQuorum("p3"), phase' = 7 },
    all { phase == 7, InsertProposal("p2"), phase' = 8 },
    all { phase == 8, UponProposalPrevote("p1", "s1"), phase' = 9 },
    all { phase == 9, UponProposalPrevote("p2", "s1"), phase' = 10 },
    all { phase == 10, UponProposalPrevote("p3", "s1"), phase' = 11 },
    all { phase == 11, UponValuePrevoteQuorum("p1", "s1"), phase' = 12 },
    all { phase == 12, UponValuePrevoteQuorum("p2", "s1"), phase' = 13 },
    all { phase == 13, UponValuePrevoteQuorum("p3", "s1"), phase' = 14 },
    all { phase == 14, Decide("p1", 1, "s1"), phase' = 15 },
    all { phase == 15, unchangedAll, phase' = 15 },
  }

  val FreshDecisionByEnd =
    phase < 15 or decision.get("p1") == "s1"

  val LivenessSafety =
    Safety and FreshDecisionByEnd
}

module CrosslinkResamplingModels {
  import CrosslinkResamplingTest(
    Corr = Set("p1", "p2", "p3"),
    Faulty = Set("p4"),
    N = 4,
    T = 1,
    MaxRound = 2,
    Proposer = Map(0 -> "p1", 1 -> "p2", 2 -> "p3"),
    Stream = Map(0 -> "s0", 1 -> "s1", 2 -> "s2"),
    Snapshots = Set("s0", "s1", "s2"),
    ResampleOnNilPrecommit = false,
    ExpectedRound1Proposal = "s0",
  ) as sticky

  import CrosslinkResamplingTest(
    Corr = Set("p1", "p2", "p3"),
    Faulty = Set("p4"),
    N = 4,
    T = 1,
    MaxRound = 2,
    Proposer = Map(0 -> "p1", 1 -> "p2", 2 -> "p3"),
    Stream = Map(0 -> "s0", 1 -> "s1", 2 -> "s2"),
    Snapshots = Set("s0", "s1", "s2"),
    ResampleOnNilPrecommit = true,
    ExpectedRound1Proposal = "s1",
  ) as resampling
}
