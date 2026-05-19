// -*- mode: Bluespec; -*-
module CrosslinkDynamicSigma {
  /*
   A focused model of the third Crosslink variant: dynamic sigma.

   The resampling model handles a moving PoW stream by treating a 2f + 1
   PRECOMMIT nil certificate as a same-round unlock certificate. This model
   isolates one input to the dynamic-sigma controller: the percentage of total
   PoW hash power that is participating in Crosslink.

   Hash-power participation is not a Tendermint voting threshold. It is a
   coverage signal for the PoW stream that finalizers see. If Crosslink only
   observes or is respected by a small fraction of hash power, a nonparticipating
   branch has a better chance of becoming the eventual longest chain, so sigma
   must have a higher floor. Below a critical participation floor, increasing
   sigma improves conservatism but does not by itself recover the same assurance
   as broad PoW participation.
   */

  type Round_t = int
  type Sigma_t = int
  type Pct_t = int

  const MaxRound: Round_t
  const BaseSigma: Sigma_t
  const RaisedSigma: Sigma_t
  const MaxSigma: Sigma_t
  const TargetHashParticipationPct: Pct_t
  const CriticalHashParticipationPct: Pct_t
  const HashParticipationPct: Round_t -> Pct_t
  const RoundFailed: Round_t -> bool

  pure val Rounds = 0.to(MaxRound)
  pure val Percentages = 0.to(100)

  assume max_round_non_negative =
    MaxRound >= 0
  assume sigma_ladder_is_ordered =
    1 <= BaseSigma and BaseSigma < RaisedSigma and RaisedSigma < MaxSigma
  assume participation_thresholds_are_ordered =
    0 <= CriticalHashParticipationPct and
    CriticalHashParticipationPct < TargetHashParticipationPct and
    TargetHashParticipationPct <= 100
  assume observed_participation_is_percentage =
    Rounds.forall(r =>
      0 <= HashParticipationPct.get(r) and HashParticipationPct.get(r) <= 100
    )

  var round: Round_t
  var sigma: Sigma_t
  var controllerStatus: str
  var firedAction: str

  pure def MaxInt(a: int, b: int): int =
    if (a >= b) { a } else { b }

  pure def HashParticipationSigmaFloor(pct: Pct_t): Sigma_t =
    if (pct < CriticalHashParticipationPct) {
      MaxSigma
    } else if (pct < TargetHashParticipationPct) {
      RaisedSigma
    } else {
      BaseSigma
    }

  pure def EscalatedSigma(current: Sigma_t): Sigma_t =
    if (current < RaisedSigma) {
      RaisedSigma
    } else {
      MaxSigma
    }

  pure def ControllerSigma(
    previousSigma: Sigma_t,
    nextHashParticipationPct: Pct_t,
    previousRoundFailed: bool
  ): Sigma_t =
    MaxInt(
      HashParticipationSigmaFloor(nextHashParticipationPct),
      if (previousRoundFailed) { EscalatedSigma(previousSigma) } else { BaseSigma }
    )

  pure def ControllerStatusFor(pct: Pct_t): str =
    if (pct < CriticalHashParticipationPct) {
      "critical-hash-participation"
    } else if (pct < TargetHashParticipationPct) {
      "degraded-hash-participation"
    } else {
      "healthy-hash-participation"
    }

  action Init = all {
    round' = 0,
    sigma' = HashParticipationSigmaFloor(HashParticipationPct.get(0)),
    controllerStatus' = ControllerStatusFor(HashParticipationPct.get(0)),
    firedAction' = "Init",
  }

  action AdvanceRound = {
    val oldRound = round
    val nextRound = oldRound + 1
    val nextHashParticipation = HashParticipationPct.get(nextRound)
    val nextSigma = ControllerSigma(
      sigma,
      nextHashParticipation,
      RoundFailed.get(oldRound)
    )
    all {
      nextRound.in(Rounds),
      round' = nextRound,
      sigma' = nextSigma,
      controllerStatus' = ControllerStatusFor(nextHashParticipation),
      firedAction' =
        if (RoundFailed.get(oldRound)) {
          "AdvanceAfterRoundFailure"
        } else if (nextHashParticipation < TargetHashParticipationPct) {
          "AdvanceWithLowHashParticipation"
        } else {
          "AdvanceStable"
        },
    }
  }

  action Stutter = all {
    round' = round,
    sigma' = sigma,
    controllerStatus' = controllerStatus,
    firedAction' = firedAction,
  }

  action Next =
    any {
      AdvanceRound,
      Stutter,
    }

  val SigmaWithinBounds =
    BaseSigma <= sigma and sigma <= MaxSigma

  val SigmaRespectsHashParticipationFloor =
    sigma >= HashParticipationSigmaFloor(HashParticipationPct.get(round))

  val HashParticipationFloorIsMonotone =
    Percentages.forall(lower =>
      Percentages.forall(higher =>
        (lower <= higher) implies
          HashParticipationSigmaFloor(lower) >= HashParticipationSigmaFloor(higher)
      )
    )

  val ControllerStatusMatchesHashParticipation =
    controllerStatus == ControllerStatusFor(HashParticipationPct.get(round))

  val Safety =
    SigmaWithinBounds and
    SigmaRespectsHashParticipationFloor and
    HashParticipationFloorIsMonotone and
    ControllerStatusMatchesHashParticipation
}

module CrosslinkDynamicSigmaTest {
  import CrosslinkDynamicSigma.* from "./CrosslinkDynamicSigma"
  export CrosslinkDynamicSigma.*

  run highHashParticipationStartsAtBaseSigmaTest = {
    Init.then(all {
      assert(sigma == BaseSigma),
      assert(controllerStatus == "healthy-hash-participation"),
      assert(Safety),
      Stutter,
    })
  }

  run roundFailureEscalatesSigmaEvenWithHighHashParticipationTest = {
    Init
      .then(AdvanceRound)
      .then(all {
        assert(sigma == RaisedSigma),
        assert(firedAction == "AdvanceAfterRoundFailure"),
        assert(Safety),
        Stutter,
      })
  }

  run lowHashParticipationRaisesSigmaFloorWithoutRoundFailureTest = {
    assert(ControllerSigma(BaseSigma, TargetHashParticipationPct - 1, false) == RaisedSigma)
  }

  run criticalHashParticipationForcesMaxSigmaTest = {
    assert(ControllerSigma(BaseSigma, CriticalHashParticipationPct - 1, false) == MaxSigma)
  }

  run hashParticipationSigmaFloorIsMonotoneTest = {
    assert(HashParticipationFloorIsMonotone)
  }

  run observedLowHashParticipationRaisesLiveSigmaTest = {
    Init
      .then(AdvanceRound)
      .then(AdvanceRound)
      .then(all {
        assert(sigma == RaisedSigma),
        assert(firedAction == "AdvanceWithLowHashParticipation"),
        assert(controllerStatus == "degraded-hash-participation"),
        assert(Safety),
        Stutter,
      })
  }

  run observedCriticalHashParticipationForcesLiveMaxSigmaTest = {
    Init
      .then(AdvanceRound)
      .then(AdvanceRound)
      .then(AdvanceRound)
      .then(all {
        assert(sigma == MaxSigma),
        assert(firedAction == "AdvanceWithLowHashParticipation"),
        assert(controllerStatus == "critical-hash-participation"),
        assert(Safety),
        Stutter,
      })
  }
}

module CrosslinkDynamicSigmaHashParticipationModel {
  import CrosslinkDynamicSigmaTest(
    MaxRound = 3,
    BaseSigma = 1,
    RaisedSigma = 3,
    MaxSigma = 6,
    TargetHashParticipationPct = 67,
    CriticalHashParticipationPct = 50,
    HashParticipationPct = Map(
      0 -> 90,
      1 -> 90,
      2 -> 66,
      3 -> 45,
    ),
    RoundFailed = Map(
      0 -> true,
      1 -> false,
      2 -> false,
      3 -> false,
    ),
  ).*
}
