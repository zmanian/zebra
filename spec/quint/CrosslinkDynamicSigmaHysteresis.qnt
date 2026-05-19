// -*- mode: Bluespec; -*-
module CrosslinkDynamicSigmaHysteresis {
  /*
   A focused model of bounded dynamic-sigma decreases.

   The dynamic-sigma controller may need to raise sigma immediately when a
   telemetry window reports worse fork, participation, or economic risk. The
   reverse direction should be slower: short windows near a threshold should not
   rapidly lower the confirmation depth. This model captures the policy shape
   implemented by the Rust helper `apply_dynamic_sigma_hysteresis`.
   */

  type Step_t = int
  type Sigma_t = int

  const MaxStep: Step_t
  const BaseSigma: Sigma_t
  const RaisedSigma: Sigma_t
  const MaxSigma: Sigma_t
  const InitialSigma: Sigma_t
  const DecreaseConfirmationWindows: int
  const RequiredSigma: Step_t -> Sigma_t

  pure val Steps = 0.to(MaxStep)
  pure val SigmaLadder = Set(BaseSigma, RaisedSigma, MaxSigma)

  var step: Step_t
  var currentSigma: Sigma_t
  var stableWindowsBelowCurrent: int

  assume max_step_non_negative =
    MaxStep >= 0
  assume sigma_ladder_is_ordered =
    1 <= BaseSigma and BaseSigma < RaisedSigma and RaisedSigma < MaxSigma
  assume initial_sigma_is_in_ladder =
    SigmaLadder.contains(InitialSigma)
  assume decrease_confirmation_windows_is_non_negative =
    DecreaseConfirmationWindows >= 0
  assume required_sigmas_are_in_ladder =
    Steps.forall(s => SigmaLadder.contains(RequiredSigma.get(s)))

  pure def MaxInt(a: int, b: int): int =
    if (a >= b) { a } else { b }

  pure def NextLowerSigma(sigma: Sigma_t): Sigma_t =
    if (sigma > RaisedSigma) { RaisedSigma } else { BaseSigma }

  pure def HysteresisSigma(
    current: Sigma_t,
    stableBelow: int,
    required: Sigma_t,
  ): Sigma_t =
    if (required >= current) {
      required
    } else if (stableBelow + 1 < DecreaseConfirmationWindows) {
      current
    } else {
      MaxInt(NextLowerSigma(current), required)
    }

  pure def HysteresisStableWindows(
    current: Sigma_t,
    stableBelow: int,
    required: Sigma_t,
  ): int =
    if (required >= current) {
      0
    } else if (stableBelow + 1 < DecreaseConfirmationWindows) {
      stableBelow + 1
    } else {
      0
    }

  action Init = all {
    step' = 0,
    currentSigma' = InitialSigma,
    stableWindowsBelowCurrent' = 0,
  }

  action Advance = {
    val nextStep = step + 1
    val required = RequiredSigma.get(nextStep)
    all {
      nextStep.in(Steps),
      step' = nextStep,
      currentSigma' = HysteresisSigma(currentSigma, stableWindowsBelowCurrent, required),
      stableWindowsBelowCurrent' =
        HysteresisStableWindows(currentSigma, stableWindowsBelowCurrent, required),
    }
  }

  action Stutter = all {
    step' = step,
    currentSigma' = currentSigma,
    stableWindowsBelowCurrent' = stableWindowsBelowCurrent,
  }

  action Next =
    any {
      Advance,
      Stutter,
    }

  val CurrentSigmaIsInLadder =
    SigmaLadder.contains(currentSigma)

  val CurrentSigmaRespectsRequiredFloor =
    currentSigma >= RequiredSigma.get(step)

  val StableCounterIsNonNegative =
    stableWindowsBelowCurrent >= 0

  val Safety =
    step.in(Steps) and
    CurrentSigmaIsInLadder and
    CurrentSigmaRespectsRequiredFloor and
    StableCounterIsNonNegative
}

module CrosslinkDynamicSigmaHysteresisTest {
  import CrosslinkDynamicSigmaHysteresis.* from "./CrosslinkDynamicSigmaHysteresis"
  export CrosslinkDynamicSigmaHysteresis.*

  run higherRequiredSigmaAppliesImmediatelyTest = {
    Init
      .then(Advance)
      .then(all {
        assert(currentSigma == MaxSigma),
        assert(stableWindowsBelowCurrent == 0),
        Stutter,
      })
  }

  run lowerRequiredSigmaWaitsForStableWindowTest = {
    Init
      .then(Advance)
      .then(Advance)
      .then(all {
        assert(currentSigma == MaxSigma),
        assert(stableWindowsBelowCurrent == 1),
        Stutter,
      })
  }

  run stableLowerRequiredSigmaStepsDownOneLevelTest = {
    Init
      .then(Advance)
      .then(Advance)
      .then(Advance)
      .then(all {
        assert(currentSigma == RaisedSigma),
        assert(stableWindowsBelowCurrent == 0),
        Stutter,
      })
  }

  run secondStableDecreaseReturnsToBaseTest = {
    Init
      .then(Advance)
      .then(Advance)
      .then(Advance)
      .then(Advance)
      .then(Advance)
      .then(all {
        assert(currentSigma == BaseSigma),
        assert(stableWindowsBelowCurrent == 0),
        Stutter,
      })
  }

  run hysteresisSafetyHoldsAtWitnessEndTest = {
    Init
      .then(Advance)
      .then(Advance)
      .then(Advance)
      .then(Advance)
      .then(Advance)
      .then(all {
        assert(Safety),
        Stutter,
      })
  }
}

module CrosslinkDynamicSigmaHysteresisModel {
  import CrosslinkDynamicSigmaHysteresisTest(
    MaxStep = 5,
    BaseSigma = 1,
    RaisedSigma = 3,
    MaxSigma = 6,
    InitialSigma = 1,
    DecreaseConfirmationWindows = 2,
    RequiredSigma = Map(
      0 -> 1,
      1 -> 6,
      2 -> 1,
      3 -> 1,
      4 -> 1,
      5 -> 1,
    ),
  ).*
}
