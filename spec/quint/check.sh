#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: spec/quint/check.sh [quick|symbolic|all|quick-baseline|symbolic-baseline|symbolic-baseline-core|symbolic-baseline-accountability]

Modes:
  quick              typecheck all specs, run witness tests, and run Rust safety checks
  symbolic           run bounded Apalache checks from README.md
  all                run quick and symbolic
  quick-baseline     run only the baseline Crosslink quick checks
  symbolic-baseline  run only the baseline Crosslink bounded Apalache checks
  symbolic-baseline-core
                     run baseline non-accountability bounded Apalache checks
  symbolic-baseline-accountability
                     run baseline accountability bounded Apalache checks

Set QUINT to override the command, for example:
  QUINT="node /private/tmp/quint-node26-patched-validround/dist/src/cli.js" spec/quint/check.sh quick

Set APALACHE_PORT_BASE to give each symbolic check a sequential local checker
port, for example:
  APALACHE_PORT_BASE=8830 spec/quint/check.sh symbolic-baseline
USAGE
}

mode="${1:-quick}"
if [[
  "${mode}" != "quick" &&
  "${mode}" != "symbolic" &&
  "${mode}" != "all" &&
  "${mode}" != "quick-baseline" &&
  "${mode}" != "symbolic-baseline" &&
  "${mode}" != "symbolic-baseline-core" &&
  "${mode}" != "symbolic-baseline-accountability"
]]; then
  usage
  exit 2
fi

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${root}"

if [[ -n "${QUINT:-}" ]]; then
  read -r -a quint_cmd <<< "${QUINT}"
else
  quint_cmd=(quint)
fi

run_quint() {
  printf '+'
  printf ' %q' "${quint_cmd[@]}" "$@"
  printf '\n'
  "${quint_cmd[@]}" "$@"
}

typecheck_all() {
  local specs=(
    spec/quint/CrosslinkBaseline.qnt
    spec/quint/CrosslinkBaselineTenderlink.qnt
    spec/quint/CrosslinkBaselineModels.qnt
    spec/quint/CrosslinkBaselineTest.qnt
    spec/quint/CrosslinkBaselineAccountability.qnt
    spec/quint/CrosslinkBaselineBftHeights.qnt
    spec/quint/CrosslinkBaselineFinality.qnt
    spec/quint/CrosslinkBaselinePowSampling.qnt
    spec/quint/CrosslinkResampling.qnt
    spec/quint/CrosslinkForkFinality.qnt
    spec/quint/CrosslinkPowForkSchedule.qnt
    spec/quint/CrosslinkPowBranchCompetition.qnt
    spec/quint/CrosslinkComposed.qnt
    spec/quint/CrosslinkBftHeights.qnt
    spec/quint/CrosslinkDynamicSigma.qnt
    spec/quint/CrosslinkDynamicSigmaCalibration.qnt
    spec/quint/CrosslinkDynamicSigmaTelemetry.qnt
    spec/quint/CrosslinkDynamicSigmaHysteresis.qnt
    spec/quint/CrosslinkDynamicSigmaForkSchedule.qnt
    spec/quint/CrosslinkDynamicSigmaBranchCompetition.qnt
    spec/quint/CrosslinkDynamicSigmaResampling.qnt
    spec/quint/CrosslinkDynamicSigmaFinality.qnt
  )

  for spec in "${specs[@]}"; do
    run_quint typecheck "${spec}"
  done
}

typecheck_baseline() {
  local specs=(
    spec/quint/CrosslinkResampling.qnt
    spec/quint/CrosslinkBaseline.qnt
    spec/quint/CrosslinkBaselineTenderlink.qnt
    spec/quint/CrosslinkBaselineModels.qnt
    spec/quint/CrosslinkBaselineTest.qnt
    spec/quint/CrosslinkBaselineAccountability.qnt
    spec/quint/CrosslinkBaselineBftHeights.qnt
    spec/quint/CrosslinkBaselineFinality.qnt
    spec/quint/CrosslinkBaselinePowSampling.qnt
  )

  for spec in "${specs[@]}"; do
    run_quint typecheck "${spec}"
  done
}

test_model() {
  local spec="$1"
  local main="$2"
  run_quint test "${spec}" --main="${main}" --max-samples=100 --backend=rust
}

run_model() {
  local spec="$1"
  local main="$2"
  local init="$3"
  local step="$4"
  local max_steps="$5"
  local max_samples="$6"
  local invariant="$7"
  run_quint run "${spec}" \
    --main="${main}" \
    --init="${init}" \
    --step="${step}" \
    --max-steps="${max_steps}" \
    --max-samples="${max_samples}" \
    --invariant="${invariant}" \
    --backend=rust \
    --verbosity=0
}

verify_count=0

verify_model() {
  local spec="$1"
  local main="$2"
  local max_steps="$3"
  local init="$4"
  local step="$5"
  local invariant="$6"

  local args=(
    verify "${spec}"
    --main="${main}" \
    --max-steps="${max_steps}" \
    --init="${init}" \
    --step="${step}" \
    --invariant="${invariant}"
  )

  if [[ -n "${APALACHE_PORT_BASE:-}" ]]; then
    local port=$((APALACHE_PORT_BASE + verify_count))
    verify_count=$((verify_count + 1))
    args+=(--server-endpoint="localhost:${port}")
  fi

  run_quint "${args[@]}"
}

quick_checks() {
  typecheck_all

  test_model spec/quint/CrosslinkResampling.qnt CrosslinkStickyModel
  test_model spec/quint/CrosslinkBaseline.qnt CrosslinkBaselineStableModel
  test_model spec/quint/CrosslinkBaseline.qnt CrosslinkBaselineStreamChangeModel
  test_model spec/quint/CrosslinkBaselineTest.qnt CrosslinkBaselineParameterizedShellTest
  test_model spec/quint/CrosslinkBaselineTest.qnt CrosslinkBaselineN4F1StableTest
  test_model spec/quint/CrosslinkBaselineTest.qnt CrosslinkBaselineN4F1ForkingTest
  test_model spec/quint/CrosslinkBaselineTest.qnt CrosslinkBaselineN4F2ForkingTest
  test_model spec/quint/CrosslinkBaselineTest.qnt CrosslinkBaselineN5F2ForkingTest
  test_model spec/quint/CrosslinkBaselineTest.qnt CrosslinkBaselineN7F2ForkingTest
  test_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineAccountabilityModel
  test_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineFaultyInitTinyModel
  test_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineFaultyInitForkingModel
  test_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineBoundedFaultyInitN4F2ForkingModel
  test_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineSingleFaultyInitN4F2ForkingModel
  test_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselinePairFaultyInitN4F2ForkingModel
  test_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineTripleFaultyInitN4F2ForkingModel
  test_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineBoundedFaultyInitN5F2ForkingModel
  test_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineSingleFaultyInitN5F2ForkingModel
  test_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselinePairFaultyInitN5F2ForkingModel
  test_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineTripleFaultyInitN5F2ForkingModel
  test_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineBoundedFaultyInitN7F2ForkingModel
  test_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineSingleFaultyInitN7F2ForkingModel
  test_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselinePairFaultyInitN7F2ForkingModel
  test_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineFullFaultyInitForkingModel
  test_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineFullFaultyInitN4F2ForkingModel
  test_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineFullFaultyInitN5F1ForkingModel
  test_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineFullFaultyInitN5F2ForkingModel
  test_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineFullFaultyInitN7F2ForkingModel
  test_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineCounterexampleModel
  test_model spec/quint/CrosslinkBaselineBftHeights.qnt CrosslinkBaselineBftHeightsModel
  test_model spec/quint/CrosslinkBaselineFinality.qnt CrosslinkBaselineFinalityStableModel
  test_model spec/quint/CrosslinkBaselineFinality.qnt CrosslinkBaselineFinalityStreamChangeModel
  test_model spec/quint/CrosslinkBaselinePowSampling.qnt CrosslinkBaselinePowSamplingModel
  test_model spec/quint/CrosslinkBaselinePowSampling.qnt CrosslinkBaselinePowLongReorgModel
  test_model spec/quint/CrosslinkBaselinePowSampling.qnt CrosslinkBaselinePowGeneratedScheduleModel
  test_model spec/quint/CrosslinkBaselinePowSampling.qnt CrosslinkBaselinePowRepeatedGeneratedScheduleModel
  test_model spec/quint/CrosslinkBaselinePowSampling.qnt CrosslinkBaselinePowStochasticProductionModel
  test_model spec/quint/CrosslinkResampling.qnt CrosslinkNilResamplingModel
  test_model spec/quint/CrosslinkForkFinality.qnt CrosslinkForkFinalityModel
  test_model spec/quint/CrosslinkPowForkSchedule.qnt CrosslinkPowForkScheduleModel
  test_model spec/quint/CrosslinkPowBranchCompetition.qnt CrosslinkPowBranchCompetitionModel
  test_model spec/quint/CrosslinkComposed.qnt CrosslinkComposedResamplingModel
  test_model spec/quint/CrosslinkBftHeights.qnt CrosslinkBftHeightsModel
  test_model spec/quint/CrosslinkDynamicSigma.qnt CrosslinkDynamicSigmaHashParticipationModel
  test_model spec/quint/CrosslinkDynamicSigmaCalibration.qnt CrosslinkDynamicSigmaCalibrationModel
  test_model spec/quint/CrosslinkDynamicSigmaTelemetry.qnt CrosslinkDynamicSigmaTelemetryModel
  test_model spec/quint/CrosslinkDynamicSigmaHysteresis.qnt CrosslinkDynamicSigmaHysteresisModel
  test_model spec/quint/CrosslinkDynamicSigmaForkSchedule.qnt CrosslinkDynamicSigmaForkScheduleModel
  test_model spec/quint/CrosslinkDynamicSigmaBranchCompetition.qnt CrosslinkDynamicSigmaBranchCompetitionModel
  test_model spec/quint/CrosslinkDynamicSigmaResampling.qnt CrosslinkDynamicSigmaResamplingModel
  test_model spec/quint/CrosslinkDynamicSigmaFinality.qnt CrosslinkDynamicSigmaFinalityModel

  run_model spec/quint/CrosslinkResampling.qnt CrosslinkStickyModel Init Next 10 1000 Safety
  run_model spec/quint/CrosslinkBaseline.qnt CrosslinkBaselineStableModel Init Next 10 1000 BaselineSafety
  run_model spec/quint/CrosslinkBaseline.qnt CrosslinkBaselineStreamChangeModel Init Next 10 1000 BaselineSafety
  run_model spec/quint/CrosslinkBaselineTest.qnt CrosslinkBaselineParameterizedShellTest BaselineInitWithFaultyEvidence BaselineNext 2 100 BaselineFaultyInitSafety
  run_model spec/quint/CrosslinkBaselineTest.qnt CrosslinkBaselineN4F1StableTest Init Next 10 1000 BaselineN4F1StableSafety
  run_model spec/quint/CrosslinkBaselineTest.qnt CrosslinkBaselineN4F1ForkingTest Init Next 10 1000 BaselineN4F1ForkingSafety
  run_model spec/quint/CrosslinkBaselineTest.qnt CrosslinkBaselineN4F2ForkingTest Init Next 10 1000 BaselineN4F2ForkingSafety
  run_model spec/quint/CrosslinkBaselineTest.qnt CrosslinkBaselineN5F2ForkingTest Init Next 10 1000 BaselineN5F2ForkingSafety
  run_model spec/quint/CrosslinkBaselineTest.qnt CrosslinkBaselineN7F2ForkingTest Init Next 10 1000 BaselineN7F2ForkingSafety
  run_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineAccountabilityModel Init Next 10 1000 BaselineAccountabilitySafety
  run_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineFaultyInitTinyModel InitWithFaultyEvidence Next 2 1000 BaselineFaultyInitSafety
  run_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineFaultyInitForkingModel InitWithBoundedForkingFaultyEvidence Next 2 1000 BaselineForkingFaultyInitSafety
  run_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineBoundedFaultyInitN4F2ForkingModel InitWithRepresentativeN4F2FaultyEvidence Next 2 1000 BaselineBoundedN4F2ForkingFaultyInitSafety
  run_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineSingleFaultyInitN4F2ForkingModel InitWithSingleN4F2FaultyEvidence Next 2 1000 BaselineSingleN4F2ForkingFaultyInitSafety
  run_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselinePairFaultyInitN4F2ForkingModel InitWithPairN4F2FaultyEvidence Next 2 1000 BaselinePairN4F2ForkingFaultyInitSafety
  run_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineTripleFaultyInitN4F2ForkingModel InitWithTripleN4F2FaultyEvidence Next 2 1000 BaselineTripleN4F2ForkingFaultyInitSafety
  run_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineBoundedFaultyInitN5F2ForkingModel InitWithRepresentativeN5F2FaultyEvidence Next 2 1000 BaselineBoundedN5F2ForkingFaultyInitSafety
  run_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineSingleFaultyInitN5F2ForkingModel InitWithSingleN5F2FaultyEvidence Next 2 1000 BaselineSingleN5F2ForkingFaultyInitSafety
  run_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselinePairFaultyInitN5F2ForkingModel InitWithPairN5F2FaultyEvidence Next 2 1000 BaselinePairN5F2ForkingFaultyInitSafety
  run_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineTripleFaultyInitN5F2ForkingModel InitWithTripleN5F2FaultyEvidence Next 2 1000 BaselineTripleN5F2ForkingFaultyInitSafety
  run_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineBoundedFaultyInitN7F2ForkingModel InitWithRepresentativeN7F2FaultyEvidence Next 2 1000 BaselineBoundedN7F2ForkingFaultyInitSafety
  run_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineSingleFaultyInitN7F2ForkingModel InitWithSingleN7F2FaultyEvidence Next 2 1000 BaselineSingleN7F2ForkingFaultyInitSafety
  run_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselinePairFaultyInitN7F2ForkingModel InitWithPairN7F2FaultyEvidence Next 2 1000 BaselinePairN7F2ForkingFaultyInitSafety
  run_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineFullFaultyInitForkingModel InitWithFaultyEvidence Next 2 100 BaselineFullForkingFaultyInitSafety
  run_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineFullFaultyInitN4F2ForkingModel InitWithFaultyEvidence Next 2 100 BaselineFullN4F2ForkingFaultyInitSafety
  run_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineFullFaultyInitN5F1ForkingModel InitWithFaultyEvidence Next 2 100 BaselineFullN5F1ForkingFaultyInitSafety
  run_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineFullFaultyInitN5F2ForkingModel InitWithFaultyEvidence Next 2 100 BaselineFullN5F2ForkingFaultyInitSafety
  run_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineFullFaultyInitN7F2ForkingModel InitWithFaultyEvidence Next 2 100 BaselineFullN7F2ForkingFaultyInitSafety
  run_model spec/quint/CrosslinkBaselineBftHeights.qnt CrosslinkBaselineBftHeightsModel Init Next 5 1000 BaselineBftHeightSafety
  run_model spec/quint/CrosslinkBaselineFinality.qnt CrosslinkBaselineFinalityStableModel ComposedInit ComposedNext 10 1000 ComposedSafety
  run_model spec/quint/CrosslinkBaselineFinality.qnt CrosslinkBaselineFinalityStreamChangeModel ComposedInit ComposedNext 10 1000 ComposedSafety
  run_model spec/quint/CrosslinkBaselineFinality.qnt CrosslinkBaselineFinalityLivenessModel LivenessInit LivenessStep 9 1 LivenessSafety
  run_model spec/quint/CrosslinkBaselinePowSampling.qnt CrosslinkBaselinePowSamplingModel Init Next 10 1000 BaselinePowSamplingSafety
  run_model spec/quint/CrosslinkBaselinePowSampling.qnt CrosslinkBaselinePowLongReorgModel Init Next 10 1000 BaselinePowLongReorgSafety
  run_model spec/quint/CrosslinkBaselinePowSampling.qnt CrosslinkBaselinePowGeneratedScheduleModel Init Next 10 1000 BaselinePowGeneratedScheduleSafety
  run_model spec/quint/CrosslinkBaselinePowSampling.qnt CrosslinkBaselinePowRepeatedGeneratedScheduleModel Init Next 10 1000 BaselinePowRepeatedGeneratedScheduleSafety
  run_model spec/quint/CrosslinkBaselinePowSampling.qnt CrosslinkBaselinePowStochasticProductionModel Init Next 10 1000 BaselinePowStochasticProductionSafety
  run_model spec/quint/CrosslinkResampling.qnt CrosslinkNilResamplingModel Init Next 10 1000 Safety
  run_model spec/quint/CrosslinkForkFinality.qnt CrosslinkForkFinalityModel Init Next 6 1000 Safety
  run_model spec/quint/CrosslinkPowForkSchedule.qnt CrosslinkPowForkScheduleModel Init Next 4 1000 Safety
  run_model spec/quint/CrosslinkPowBranchCompetition.qnt CrosslinkPowBranchCompetitionModel Init Next 4 1000 Safety
  run_model spec/quint/CrosslinkResampling.qnt CrosslinkNilResamplingLivenessModel LivenessInit LivenessStep 15 1 LivenessSafety
  run_model spec/quint/CrosslinkComposed.qnt CrosslinkComposedResamplingModel ComposedInit ComposedNext 10 1000 ComposedSafety
  run_model spec/quint/CrosslinkComposed.qnt CrosslinkComposedLivenessModel LivenessInit LivenessStep 16 1 LivenessSafety
  run_model spec/quint/CrosslinkBftHeights.qnt CrosslinkBftHeightsModel Init Next 5 1000 Safety
  run_model spec/quint/CrosslinkDynamicSigma.qnt CrosslinkDynamicSigmaHashParticipationModel Init Next 7 1000 Safety
  run_model spec/quint/CrosslinkDynamicSigmaCalibration.qnt CrosslinkDynamicSigmaCalibrationModel Init Next 8 1000 Safety
  run_model spec/quint/CrosslinkDynamicSigmaTelemetry.qnt CrosslinkDynamicSigmaTelemetryModel Init Next 8 1000 Safety
  run_model spec/quint/CrosslinkDynamicSigmaHysteresis.qnt CrosslinkDynamicSigmaHysteresisModel Init Next 5 1000 Safety
  run_model spec/quint/CrosslinkDynamicSigmaForkSchedule.qnt CrosslinkDynamicSigmaForkScheduleModel DerivedInit DerivedNext 4 1000 DerivedSafety
  run_model spec/quint/CrosslinkDynamicSigmaBranchCompetition.qnt CrosslinkDynamicSigmaBranchCompetitionModel BranchCompetitionDynamicInit BranchCompetitionDynamicNext 4 1000 BranchCompetitionDynamicSafety
  run_model spec/quint/CrosslinkDynamicSigmaResampling.qnt CrosslinkDynamicSigmaResamplingModel DynamicResamplingInit DynamicResamplingNext 8 1000 DynamicResamplingSafety
  run_model spec/quint/CrosslinkDynamicSigmaFinality.qnt CrosslinkDynamicSigmaFinalityModel FullComposedInit FullComposedNext 10 1000 FullComposedSafety
}

baseline_quick_checks() {
  typecheck_baseline

  test_model spec/quint/CrosslinkResampling.qnt CrosslinkStickyModel
  test_model spec/quint/CrosslinkBaseline.qnt CrosslinkBaselineStableModel
  test_model spec/quint/CrosslinkBaseline.qnt CrosslinkBaselineStreamChangeModel
  test_model spec/quint/CrosslinkBaselineTest.qnt CrosslinkBaselineParameterizedShellTest
  test_model spec/quint/CrosslinkBaselineTest.qnt CrosslinkBaselineN4F1StableTest
  test_model spec/quint/CrosslinkBaselineTest.qnt CrosslinkBaselineN4F1ForkingTest
  test_model spec/quint/CrosslinkBaselineTest.qnt CrosslinkBaselineN4F2ForkingTest
  test_model spec/quint/CrosslinkBaselineTest.qnt CrosslinkBaselineN5F2ForkingTest
  test_model spec/quint/CrosslinkBaselineTest.qnt CrosslinkBaselineN7F2ForkingTest
  test_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineAccountabilityModel
  test_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineFaultyInitTinyModel
  test_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineFaultyInitForkingModel
  test_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineBoundedFaultyInitN4F2ForkingModel
  test_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineSingleFaultyInitN4F2ForkingModel
  test_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselinePairFaultyInitN4F2ForkingModel
  test_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineTripleFaultyInitN4F2ForkingModel
  test_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineBoundedFaultyInitN5F2ForkingModel
  test_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineSingleFaultyInitN5F2ForkingModel
  test_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselinePairFaultyInitN5F2ForkingModel
  test_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineTripleFaultyInitN5F2ForkingModel
  test_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineBoundedFaultyInitN7F2ForkingModel
  test_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineSingleFaultyInitN7F2ForkingModel
  test_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselinePairFaultyInitN7F2ForkingModel
  test_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineFullFaultyInitForkingModel
  test_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineFullFaultyInitN4F2ForkingModel
  test_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineFullFaultyInitN5F1ForkingModel
  test_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineFullFaultyInitN5F2ForkingModel
  test_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineFullFaultyInitN7F2ForkingModel
  test_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineCounterexampleModel
  test_model spec/quint/CrosslinkBaselineBftHeights.qnt CrosslinkBaselineBftHeightsModel
  test_model spec/quint/CrosslinkBaselineFinality.qnt CrosslinkBaselineFinalityStableModel
  test_model spec/quint/CrosslinkBaselineFinality.qnt CrosslinkBaselineFinalityStreamChangeModel
  test_model spec/quint/CrosslinkBaselinePowSampling.qnt CrosslinkBaselinePowSamplingModel
  test_model spec/quint/CrosslinkBaselinePowSampling.qnt CrosslinkBaselinePowLongReorgModel
  test_model spec/quint/CrosslinkBaselinePowSampling.qnt CrosslinkBaselinePowGeneratedScheduleModel
  test_model spec/quint/CrosslinkBaselinePowSampling.qnt CrosslinkBaselinePowRepeatedGeneratedScheduleModel
  test_model spec/quint/CrosslinkBaselinePowSampling.qnt CrosslinkBaselinePowStochasticProductionModel

  run_model spec/quint/CrosslinkResampling.qnt CrosslinkStickyModel Init Next 10 1000 Safety
  run_model spec/quint/CrosslinkBaseline.qnt CrosslinkBaselineStableModel Init Next 10 1000 BaselineSafety
  run_model spec/quint/CrosslinkBaseline.qnt CrosslinkBaselineStreamChangeModel Init Next 10 1000 BaselineSafety
  run_model spec/quint/CrosslinkBaselineTest.qnt CrosslinkBaselineParameterizedShellTest BaselineInitWithFaultyEvidence BaselineNext 2 100 BaselineFaultyInitSafety
  run_model spec/quint/CrosslinkBaselineTest.qnt CrosslinkBaselineN4F1StableTest Init Next 10 1000 BaselineN4F1StableSafety
  run_model spec/quint/CrosslinkBaselineTest.qnt CrosslinkBaselineN4F1ForkingTest Init Next 10 1000 BaselineN4F1ForkingSafety
  run_model spec/quint/CrosslinkBaselineTest.qnt CrosslinkBaselineN4F2ForkingTest Init Next 10 1000 BaselineN4F2ForkingSafety
  run_model spec/quint/CrosslinkBaselineTest.qnt CrosslinkBaselineN5F2ForkingTest Init Next 10 1000 BaselineN5F2ForkingSafety
  run_model spec/quint/CrosslinkBaselineTest.qnt CrosslinkBaselineN7F2ForkingTest Init Next 10 1000 BaselineN7F2ForkingSafety
  run_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineAccountabilityModel Init Next 10 1000 BaselineAccountabilitySafety
  run_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineFaultyInitTinyModel InitWithFaultyEvidence Next 2 1000 BaselineFaultyInitSafety
  run_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineFaultyInitForkingModel InitWithBoundedForkingFaultyEvidence Next 2 1000 BaselineForkingFaultyInitSafety
  run_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineBoundedFaultyInitN4F2ForkingModel InitWithRepresentativeN4F2FaultyEvidence Next 2 1000 BaselineBoundedN4F2ForkingFaultyInitSafety
  run_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineSingleFaultyInitN4F2ForkingModel InitWithSingleN4F2FaultyEvidence Next 2 1000 BaselineSingleN4F2ForkingFaultyInitSafety
  run_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselinePairFaultyInitN4F2ForkingModel InitWithPairN4F2FaultyEvidence Next 2 1000 BaselinePairN4F2ForkingFaultyInitSafety
  run_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineTripleFaultyInitN4F2ForkingModel InitWithTripleN4F2FaultyEvidence Next 2 1000 BaselineTripleN4F2ForkingFaultyInitSafety
  run_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineBoundedFaultyInitN5F2ForkingModel InitWithRepresentativeN5F2FaultyEvidence Next 2 1000 BaselineBoundedN5F2ForkingFaultyInitSafety
  run_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineSingleFaultyInitN5F2ForkingModel InitWithSingleN5F2FaultyEvidence Next 2 1000 BaselineSingleN5F2ForkingFaultyInitSafety
  run_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselinePairFaultyInitN5F2ForkingModel InitWithPairN5F2FaultyEvidence Next 2 1000 BaselinePairN5F2ForkingFaultyInitSafety
  run_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineTripleFaultyInitN5F2ForkingModel InitWithTripleN5F2FaultyEvidence Next 2 1000 BaselineTripleN5F2ForkingFaultyInitSafety
  run_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineBoundedFaultyInitN7F2ForkingModel InitWithRepresentativeN7F2FaultyEvidence Next 2 1000 BaselineBoundedN7F2ForkingFaultyInitSafety
  run_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineSingleFaultyInitN7F2ForkingModel InitWithSingleN7F2FaultyEvidence Next 2 1000 BaselineSingleN7F2ForkingFaultyInitSafety
  run_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselinePairFaultyInitN7F2ForkingModel InitWithPairN7F2FaultyEvidence Next 2 1000 BaselinePairN7F2ForkingFaultyInitSafety
  run_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineFullFaultyInitForkingModel InitWithFaultyEvidence Next 2 100 BaselineFullForkingFaultyInitSafety
  run_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineFullFaultyInitN4F2ForkingModel InitWithFaultyEvidence Next 2 100 BaselineFullN4F2ForkingFaultyInitSafety
  run_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineFullFaultyInitN5F1ForkingModel InitWithFaultyEvidence Next 2 100 BaselineFullN5F1ForkingFaultyInitSafety
  run_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineFullFaultyInitN5F2ForkingModel InitWithFaultyEvidence Next 2 100 BaselineFullN5F2ForkingFaultyInitSafety
  run_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineFullFaultyInitN7F2ForkingModel InitWithFaultyEvidence Next 2 100 BaselineFullN7F2ForkingFaultyInitSafety
  run_model spec/quint/CrosslinkBaselineBftHeights.qnt CrosslinkBaselineBftHeightsModel Init Next 5 1000 BaselineBftHeightSafety
  run_model spec/quint/CrosslinkBaselineFinality.qnt CrosslinkBaselineFinalityStableModel ComposedInit ComposedNext 10 1000 ComposedSafety
  run_model spec/quint/CrosslinkBaselineFinality.qnt CrosslinkBaselineFinalityStreamChangeModel ComposedInit ComposedNext 10 1000 ComposedSafety
  run_model spec/quint/CrosslinkBaselineFinality.qnt CrosslinkBaselineFinalityLivenessModel LivenessInit LivenessStep 9 1 LivenessSafety
  run_model spec/quint/CrosslinkBaselinePowSampling.qnt CrosslinkBaselinePowSamplingModel Init Next 10 1000 BaselinePowSamplingSafety
  run_model spec/quint/CrosslinkBaselinePowSampling.qnt CrosslinkBaselinePowLongReorgModel Init Next 10 1000 BaselinePowLongReorgSafety
  run_model spec/quint/CrosslinkBaselinePowSampling.qnt CrosslinkBaselinePowGeneratedScheduleModel Init Next 10 1000 BaselinePowGeneratedScheduleSafety
  run_model spec/quint/CrosslinkBaselinePowSampling.qnt CrosslinkBaselinePowRepeatedGeneratedScheduleModel Init Next 10 1000 BaselinePowRepeatedGeneratedScheduleSafety
  run_model spec/quint/CrosslinkBaselinePowSampling.qnt CrosslinkBaselinePowStochasticProductionModel Init Next 10 1000 BaselinePowStochasticProductionSafety
}

symbolic_checks() {
  verify_model spec/quint/CrosslinkResampling.qnt CrosslinkStickyModel 3 Init Next Safety
  verify_model spec/quint/CrosslinkBaseline.qnt CrosslinkBaselineStableModel 3 Init Next BaselineSafety
  verify_model spec/quint/CrosslinkBaseline.qnt CrosslinkBaselineStreamChangeModel 3 Init Next BaselineSafety
  verify_model spec/quint/CrosslinkBaselineTest.qnt CrosslinkBaselineN4F1StableTest 3 Init Next BaselineN4F1StableSafety
  verify_model spec/quint/CrosslinkBaselineTest.qnt CrosslinkBaselineN4F1ForkingTest 3 Init Next BaselineN4F1ForkingSafety
  verify_model spec/quint/CrosslinkBaselineTest.qnt CrosslinkBaselineN4F2ForkingTest 3 Init Next BaselineN4F2ForkingSafety
  verify_model spec/quint/CrosslinkBaselineTest.qnt CrosslinkBaselineN5F2ForkingTest 3 Init Next BaselineN5F2ForkingSafety
  verify_model spec/quint/CrosslinkBaselineTest.qnt CrosslinkBaselineN7F2ForkingTest 3 Init Next BaselineN7F2ForkingSafety
  verify_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineAccountabilityModel 3 Init Next BaselineAccountabilitySafety
  verify_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineFaultyInitTinyModel 2 InitWithFaultyEvidence Next BaselineFaultyInitSafety
  verify_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineFaultyInitForkingModel 2 InitWithBoundedForkingFaultyEvidence Next BaselineForkingFaultyInitSafety
  verify_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineBoundedFaultyInitN4F2ForkingModel 2 InitWithRepresentativeN4F2FaultyEvidence Next BaselineBoundedN4F2ForkingFaultyInitSafety
  verify_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineSingleFaultyInitN4F2ForkingModel 2 InitWithSingleN4F2FaultyEvidence Next BaselineSingleN4F2ForkingFaultyInitSafety
  verify_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselinePairFaultyInitN4F2ForkingModel 2 InitWithPairN4F2FaultyEvidence Next BaselinePairN4F2ForkingFaultyInitSafety
  verify_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineTripleFaultyInitN4F2ForkingModel 2 InitWithTripleN4F2FaultyEvidence Next BaselineTripleN4F2ForkingFaultyInitSafety
  verify_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineBoundedFaultyInitN5F2ForkingModel 2 InitWithRepresentativeN5F2FaultyEvidence Next BaselineBoundedN5F2ForkingFaultyInitSafety
  verify_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineSingleFaultyInitN5F2ForkingModel 2 InitWithSingleN5F2FaultyEvidence Next BaselineSingleN5F2ForkingFaultyInitSafety
  verify_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselinePairFaultyInitN5F2ForkingModel 2 InitWithPairN5F2FaultyEvidence Next BaselinePairN5F2ForkingFaultyInitSafety
  verify_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineTripleFaultyInitN5F2ForkingModel 2 InitWithTripleN5F2FaultyEvidence Next BaselineTripleN5F2ForkingFaultyInitSafety
  verify_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineBoundedFaultyInitN7F2ForkingModel 2 InitWithRepresentativeN7F2FaultyEvidence Next BaselineBoundedN7F2ForkingFaultyInitSafety
  verify_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineSingleFaultyInitN7F2ForkingModel 2 InitWithSingleN7F2FaultyEvidence Next BaselineSingleN7F2ForkingFaultyInitSafety
  verify_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselinePairFaultyInitN7F2ForkingModel 2 InitWithPairN7F2FaultyEvidence Next BaselinePairN7F2ForkingFaultyInitSafety
  verify_model spec/quint/CrosslinkBaselineBftHeights.qnt CrosslinkBaselineBftHeightsModel 5 Init Next BaselineBftHeightSafety
  verify_model spec/quint/CrosslinkBaselineFinality.qnt CrosslinkBaselineFinalityStableModel 5 ComposedInit ComposedNext ComposedSafety
  verify_model spec/quint/CrosslinkBaselineFinality.qnt CrosslinkBaselineFinalityStreamChangeModel 5 ComposedInit ComposedNext ComposedSafety
  verify_model spec/quint/CrosslinkBaselineFinality.qnt CrosslinkBaselineFinalityLivenessModel 9 LivenessInit LivenessStep LivenessSafety
  verify_model spec/quint/CrosslinkBaselinePowSampling.qnt CrosslinkBaselinePowSamplingModel 3 Init Next BaselinePowSamplingSafety
  verify_model spec/quint/CrosslinkBaselinePowSampling.qnt CrosslinkBaselinePowLongReorgModel 3 Init Next BaselinePowLongReorgSafety
  verify_model spec/quint/CrosslinkBaselinePowSampling.qnt CrosslinkBaselinePowGeneratedScheduleModel 3 Init Next BaselinePowGeneratedScheduleSafety
  verify_model spec/quint/CrosslinkBaselinePowSampling.qnt CrosslinkBaselinePowRepeatedGeneratedScheduleModel 3 Init Next BaselinePowRepeatedGeneratedScheduleSafety
  verify_model spec/quint/CrosslinkBaselinePowSampling.qnt CrosslinkBaselinePowStochasticProductionModel 3 Init Next BaselinePowStochasticProductionSafety
  verify_model spec/quint/CrosslinkResampling.qnt CrosslinkNilResamplingModel 3 Init Next Safety
  verify_model spec/quint/CrosslinkForkFinality.qnt CrosslinkForkFinalityModel 4 Init Next Safety
  verify_model spec/quint/CrosslinkPowForkSchedule.qnt CrosslinkPowForkScheduleModel 4 Init Next Safety
  verify_model spec/quint/CrosslinkPowBranchCompetition.qnt CrosslinkPowBranchCompetitionModel 4 Init Next Safety
  verify_model spec/quint/CrosslinkResampling.qnt CrosslinkNilResamplingLivenessModel 15 LivenessInit LivenessStep LivenessSafety
  verify_model spec/quint/CrosslinkComposed.qnt CrosslinkComposedResamplingModel 5 ComposedInit ComposedNext ComposedSafety
  verify_model spec/quint/CrosslinkComposed.qnt CrosslinkComposedLivenessModel 16 LivenessInit LivenessStep LivenessSafety
  verify_model spec/quint/CrosslinkBftHeights.qnt CrosslinkBftHeightsModel 5 Init Next Safety
  verify_model spec/quint/CrosslinkDynamicSigma.qnt CrosslinkDynamicSigmaHashParticipationModel 7 Init Next Safety
  verify_model spec/quint/CrosslinkDynamicSigmaCalibration.qnt CrosslinkDynamicSigmaCalibrationModel 8 Init Next Safety
  verify_model spec/quint/CrosslinkDynamicSigmaTelemetry.qnt CrosslinkDynamicSigmaTelemetryModel 8 Init Next Safety
  verify_model spec/quint/CrosslinkDynamicSigmaHysteresis.qnt CrosslinkDynamicSigmaHysteresisModel 5 Init Next Safety
  verify_model spec/quint/CrosslinkDynamicSigmaForkSchedule.qnt CrosslinkDynamicSigmaForkScheduleModel 4 DerivedInit DerivedNext DerivedSafety
  verify_model spec/quint/CrosslinkDynamicSigmaBranchCompetition.qnt CrosslinkDynamicSigmaBranchCompetitionModel 4 BranchCompetitionDynamicInit BranchCompetitionDynamicNext BranchCompetitionDynamicSafety
  verify_model spec/quint/CrosslinkDynamicSigmaResampling.qnt CrosslinkDynamicSigmaResamplingModel 8 DynamicResamplingInit DynamicResamplingNext DynamicResamplingSafety
  verify_model spec/quint/CrosslinkDynamicSigmaFinality.qnt CrosslinkDynamicSigmaFinalityModel 8 FullComposedInit FullComposedNext FullComposedSafety
  verify_model spec/quint/CrosslinkDynamicSigmaFinality.qnt CrosslinkDynamicSigmaFinalityModel 10 FullComposedInit FullComposedNext FullProtocolProjectionSafety
  verify_model spec/quint/CrosslinkDynamicSigmaFinality.qnt CrosslinkDynamicSigmaFinalityModel 10 FullComposedInit FullComposedNext FullFinalityProjectionSafety
  verify_model spec/quint/CrosslinkDynamicSigmaFinality.qnt CrosslinkDynamicSigmaFinalityModel 10 FullComposedInit FullComposedNext FullWorkCompetitionProjectionSafety
}

baseline_symbolic_core_checks() {
  verify_model spec/quint/CrosslinkResampling.qnt CrosslinkStickyModel 3 Init Next Safety
  verify_model spec/quint/CrosslinkBaseline.qnt CrosslinkBaselineStableModel 3 Init Next BaselineSafety
  verify_model spec/quint/CrosslinkBaseline.qnt CrosslinkBaselineStreamChangeModel 3 Init Next BaselineSafety
  verify_model spec/quint/CrosslinkBaselineTest.qnt CrosslinkBaselineN4F1StableTest 3 Init Next BaselineN4F1StableSafety
  verify_model spec/quint/CrosslinkBaselineTest.qnt CrosslinkBaselineN4F1ForkingTest 3 Init Next BaselineN4F1ForkingSafety
  verify_model spec/quint/CrosslinkBaselineTest.qnt CrosslinkBaselineN4F2ForkingTest 3 Init Next BaselineN4F2ForkingSafety
  verify_model spec/quint/CrosslinkBaselineTest.qnt CrosslinkBaselineN5F2ForkingTest 3 Init Next BaselineN5F2ForkingSafety
  verify_model spec/quint/CrosslinkBaselineTest.qnt CrosslinkBaselineN7F2ForkingTest 3 Init Next BaselineN7F2ForkingSafety
  verify_model spec/quint/CrosslinkBaselineBftHeights.qnt CrosslinkBaselineBftHeightsModel 5 Init Next BaselineBftHeightSafety
  verify_model spec/quint/CrosslinkBaselineFinality.qnt CrosslinkBaselineFinalityStableModel 5 ComposedInit ComposedNext ComposedSafety
  verify_model spec/quint/CrosslinkBaselineFinality.qnt CrosslinkBaselineFinalityStreamChangeModel 5 ComposedInit ComposedNext ComposedSafety
  verify_model spec/quint/CrosslinkBaselineFinality.qnt CrosslinkBaselineFinalityLivenessModel 9 LivenessInit LivenessStep LivenessSafety
  verify_model spec/quint/CrosslinkBaselinePowSampling.qnt CrosslinkBaselinePowSamplingModel 3 Init Next BaselinePowSamplingSafety
  verify_model spec/quint/CrosslinkBaselinePowSampling.qnt CrosslinkBaselinePowLongReorgModel 3 Init Next BaselinePowLongReorgSafety
  verify_model spec/quint/CrosslinkBaselinePowSampling.qnt CrosslinkBaselinePowGeneratedScheduleModel 3 Init Next BaselinePowGeneratedScheduleSafety
  verify_model spec/quint/CrosslinkBaselinePowSampling.qnt CrosslinkBaselinePowRepeatedGeneratedScheduleModel 3 Init Next BaselinePowRepeatedGeneratedScheduleSafety
  verify_model spec/quint/CrosslinkBaselinePowSampling.qnt CrosslinkBaselinePowStochasticProductionModel 3 Init Next BaselinePowStochasticProductionSafety
}

baseline_symbolic_accountability_checks() {
  verify_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineAccountabilityModel 3 Init Next BaselineAccountabilitySafety
  verify_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineFaultyInitTinyModel 2 InitWithFaultyEvidence Next BaselineFaultyInitSafety
  verify_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineFaultyInitForkingModel 2 InitWithBoundedForkingFaultyEvidence Next BaselineForkingFaultyInitSafety
  verify_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineBoundedFaultyInitN4F2ForkingModel 2 InitWithRepresentativeN4F2FaultyEvidence Next BaselineBoundedN4F2ForkingFaultyInitSafety
  verify_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineSingleFaultyInitN4F2ForkingModel 2 InitWithSingleN4F2FaultyEvidence Next BaselineSingleN4F2ForkingFaultyInitSafety
  verify_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselinePairFaultyInitN4F2ForkingModel 2 InitWithPairN4F2FaultyEvidence Next BaselinePairN4F2ForkingFaultyInitSafety
  verify_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineTripleFaultyInitN4F2ForkingModel 2 InitWithTripleN4F2FaultyEvidence Next BaselineTripleN4F2ForkingFaultyInitSafety
  verify_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineBoundedFaultyInitN5F2ForkingModel 2 InitWithRepresentativeN5F2FaultyEvidence Next BaselineBoundedN5F2ForkingFaultyInitSafety
  verify_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineSingleFaultyInitN5F2ForkingModel 2 InitWithSingleN5F2FaultyEvidence Next BaselineSingleN5F2ForkingFaultyInitSafety
  verify_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselinePairFaultyInitN5F2ForkingModel 2 InitWithPairN5F2FaultyEvidence Next BaselinePairN5F2ForkingFaultyInitSafety
  verify_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineTripleFaultyInitN5F2ForkingModel 2 InitWithTripleN5F2FaultyEvidence Next BaselineTripleN5F2ForkingFaultyInitSafety
  verify_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineBoundedFaultyInitN7F2ForkingModel 2 InitWithRepresentativeN7F2FaultyEvidence Next BaselineBoundedN7F2ForkingFaultyInitSafety
  verify_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselineSingleFaultyInitN7F2ForkingModel 2 InitWithSingleN7F2FaultyEvidence Next BaselineSingleN7F2ForkingFaultyInitSafety
  verify_model spec/quint/CrosslinkBaselineAccountability.qnt CrosslinkBaselinePairFaultyInitN7F2ForkingModel 2 InitWithPairN7F2FaultyEvidence Next BaselinePairN7F2ForkingFaultyInitSafety
}

baseline_symbolic_checks() {
  baseline_symbolic_core_checks
  baseline_symbolic_accountability_checks
}

case "${mode}" in
  quick)
    quick_checks
    ;;
  quick-baseline)
    baseline_quick_checks
    ;;
  symbolic)
    symbolic_checks
    ;;
  symbolic-baseline)
    baseline_symbolic_checks
    ;;
  symbolic-baseline-core)
    baseline_symbolic_core_checks
    ;;
  symbolic-baseline-accountability)
    baseline_symbolic_accountability_checks
    ;;
  all)
    quick_checks
    symbolic_checks
    ;;
esac
