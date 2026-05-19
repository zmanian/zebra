//! Dynamic sigma controller prototype.
//!
//! This module is deliberately pure: it does not change proposal or validation
//! rules yet. It turns a production-shaped telemetry window into the same sigma
//! floor described by the Quint dynamic-sigma telemetry contract.

/// Parts-per-million denominator used by rollback risk estimates.
pub const PPM_DENOMINATOR: u128 = 1_000_000;

/// Controller parameters shared by validators for a dynamic-sigma variant.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DynamicSigmaParameters {
    /// Baseline confirmation depth.
    pub base_sigma: u64,
    /// Raised confirmation depth used for degraded but recoverable conditions.
    pub raised_sigma: u64,
    /// Maximum confirmation depth used for critical or unreachable conditions.
    pub max_sigma: u64,
    /// Target Crosslink-participating hash-work percentage.
    pub target_hash_participation_pct: u8,
    /// Critical Crosslink-participating hash-work percentage.
    pub critical_hash_participation_pct: u8,
    /// Maximum acceptable rollback probability in parts per million.
    pub max_acceptable_rollback_risk_ppm: u64,
    /// Weight applied to estimated coverage risk percentage.
    pub coverage_risk_weight: u64,
    /// Weight applied to estimated Tenderlink round failure rate.
    pub round_failure_risk_weight: u64,
    /// Weight applied to measured block interval variance.
    pub block_interval_variance_risk_weight: u64,
    /// Weight applied to observed rollback depth.
    pub reorg_depth_risk_weight: u64,
    /// Risk score threshold for `raised_sigma`.
    pub risk_score_raised_threshold: u64,
    /// Risk score threshold for `max_sigma`.
    pub risk_score_max_threshold: u64,
}

/// Rollback-risk estimates for the configured sigma ladder.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RollbackRiskCurve {
    /// Risk at `base_sigma`.
    pub base_sigma_ppm: u64,
    /// Risk at `raised_sigma`.
    pub raised_sigma_ppm: u64,
    /// Risk at `max_sigma`.
    pub max_sigma_ppm: u64,
}

/// A telemetry window that can be checked before selecting dynamic sigma.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DynamicSigmaTelemetryWindow {
    /// Total observed PoW work over the measurement window.
    pub total_hash_work: u128,
    /// Observed PoW work with objectively verifiable Crosslink participation.
    pub crosslink_participating_hash_work: u128,
    /// Tenderlink rounds observed in the measurement window.
    pub total_tenderlink_rounds: u64,
    /// Tenderlink rounds that failed to decide and required recovery.
    pub failed_tenderlink_rounds: u64,
    /// Conservative upper bound on non-participating or unseen work.
    pub estimated_coverage_risk_pct: u8,
    /// Conservative upper bound on failed Tenderlink rounds.
    pub estimated_round_failure_rate_pct: u8,
    /// Measured PoW timing variance percentage.
    pub measured_block_interval_variance_pct: u8,
    /// Maximum observed rollback depth in the window.
    pub measured_observed_reorg_depth: u64,
    /// Rollback risk estimates across the sigma ladder.
    pub rollback_risk: RollbackRiskCurve,
    /// Economic value exposed to rollback in the window.
    pub value_at_risk_units: u128,
    /// Maximum acceptable expected loss for this window.
    pub max_acceptable_expected_loss_units: u128,
}

/// Conservative margins added to raw telemetry-derived estimates.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TelemetryEstimateMargins {
    /// Additional coverage-risk percentage points.
    pub coverage_risk_margin_pct: u8,
    /// Additional round-failure percentage points.
    pub round_failure_margin_pct: u8,
}

/// Raw telemetry counters before conservative percentage estimates are derived.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DynamicSigmaRawTelemetry {
    /// Total observed PoW work over the measurement window.
    pub total_hash_work: u128,
    /// Observed PoW work with objectively verifiable Crosslink participation.
    pub crosslink_participating_hash_work: u128,
    /// Tenderlink rounds observed in the measurement window.
    pub total_tenderlink_rounds: u64,
    /// Tenderlink rounds that failed to decide and required recovery.
    pub failed_tenderlink_rounds: u64,
    /// Measured PoW timing variance percentage.
    pub measured_block_interval_variance_pct: u8,
    /// Maximum observed rollback depth in the window.
    pub measured_observed_reorg_depth: u64,
    /// Rollback risk estimates across the sigma ladder.
    pub rollback_risk: RollbackRiskCurve,
    /// Economic value exposed to rollback in the window.
    pub value_at_risk_units: u128,
    /// Maximum acceptable expected loss for this window.
    pub max_acceptable_expected_loss_units: u128,
}

/// Hash-participation health status.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HashParticipationStatus {
    /// Participation is at or above target.
    Healthy,
    /// Participation is below target but above the critical threshold.
    Degraded,
    /// Participation is below the critical threshold.
    Critical,
}

/// Economic risk target status.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EconomicTargetStatus {
    /// The selected sigma satisfies the configured rollback-risk and loss targets.
    TargetSatisfied,
    /// The target is unreachable even at max sigma.
    TargetUnreachableAtMax,
}

/// Dynamic sigma decision and its component floors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DynamicSigmaDecision {
    /// Selected confirmation depth.
    pub sigma: u64,
    /// Floor derived from hash-work participation.
    pub hash_participation_floor: u64,
    /// Floor derived from observed rollback depth.
    pub reorg_floor: u64,
    /// Floor derived from the calibrated risk score.
    pub risk_score_floor: u64,
    /// Floor derived from rollback-risk and expected-loss targets.
    pub economic_floor: u64,
    /// Hash-participation health status.
    pub hash_participation_status: HashParticipationStatus,
    /// Economic target status at the selected sigma.
    pub economic_target_status: EconomicTargetStatus,
}

/// Invalid controller parameters or telemetry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DynamicSigmaError {
    /// Sigma ladder is not strictly increasing.
    InvalidSigmaLadder,
    /// Hash participation thresholds are not ordered percentages.
    InvalidHashParticipationThresholds,
    /// Risk thresholds are not ordered.
    InvalidRiskScoreThresholds,
    /// Telemetry has no observed PoW work.
    EmptyHashWorkWindow,
    /// Crosslink-participating work exceeds total observed work.
    ParticipatingHashWorkExceedsTotal,
    /// Telemetry has no Tenderlink rounds.
    EmptyTenderlinkRoundWindow,
    /// Failed Tenderlink rounds exceed total observed rounds.
    FailedRoundsExceedTotal,
    /// Coverage risk estimate is below the raw observed work gap.
    CoverageRiskEstimateTooLow,
    /// Round-failure estimate is below the raw observed round-failure rate.
    RoundFailureEstimateTooLow,
    /// Rollback risk estimates are not monotone as sigma increases.
    RollbackRiskCurveNotMonotone,
}

impl DynamicSigmaRawTelemetry {
    /// Convert raw observation counters into a conservative telemetry window.
    pub fn into_window(
        self,
        margins: TelemetryEstimateMargins,
    ) -> Result<DynamicSigmaTelemetryWindow, DynamicSigmaError> {
        if self.total_hash_work == 0 {
            return Err(DynamicSigmaError::EmptyHashWorkWindow);
        }

        if self.crosslink_participating_hash_work > self.total_hash_work {
            return Err(DynamicSigmaError::ParticipatingHashWorkExceedsTotal);
        }

        if self.total_tenderlink_rounds == 0 {
            return Err(DynamicSigmaError::EmptyTenderlinkRoundWindow);
        }

        if self.failed_tenderlink_rounds > self.total_tenderlink_rounds {
            return Err(DynamicSigmaError::FailedRoundsExceedTotal);
        }

        let raw_coverage_gap_pct = ceil_ratio_pct(
            self.total_hash_work - self.crosslink_participating_hash_work,
            self.total_hash_work,
        );
        let raw_round_failure_pct = ceil_ratio_pct(
            u128::from(self.failed_tenderlink_rounds),
            u128::from(self.total_tenderlink_rounds),
        );
        let window = DynamicSigmaTelemetryWindow {
            total_hash_work: self.total_hash_work,
            crosslink_participating_hash_work: self.crosslink_participating_hash_work,
            total_tenderlink_rounds: self.total_tenderlink_rounds,
            failed_tenderlink_rounds: self.failed_tenderlink_rounds,
            estimated_coverage_risk_pct: saturating_pct_add(
                raw_coverage_gap_pct,
                margins.coverage_risk_margin_pct,
            ),
            estimated_round_failure_rate_pct: saturating_pct_add(
                raw_round_failure_pct,
                margins.round_failure_margin_pct,
            ),
            measured_block_interval_variance_pct: self.measured_block_interval_variance_pct,
            measured_observed_reorg_depth: self.measured_observed_reorg_depth,
            rollback_risk: self.rollback_risk,
            value_at_risk_units: self.value_at_risk_units,
            max_acceptable_expected_loss_units: self.max_acceptable_expected_loss_units,
        };

        validate_window(window)?;

        Ok(window)
    }
}

/// Select dynamic sigma from a validated telemetry window.
pub fn select_dynamic_sigma(
    params: DynamicSigmaParameters,
    window: DynamicSigmaTelemetryWindow,
) -> Result<DynamicSigmaDecision, DynamicSigmaError> {
    validate_params(params)?;
    validate_window(window)?;

    let (hash_participation_floor, hash_participation_status) =
        hash_participation_floor(params, window);
    let reorg_floor = reorg_floor(params, window.measured_observed_reorg_depth);
    let risk_score_floor = risk_score_floor(params, calibrated_risk_score(params, window));
    let economic_floor = economic_floor(params, window);
    let sigma = max_floor([
        hash_participation_floor,
        reorg_floor,
        risk_score_floor,
        economic_floor,
    ]);
    let economic_target_status = if economic_target_satisfied_at_sigma(params, window, sigma) {
        EconomicTargetStatus::TargetSatisfied
    } else {
        EconomicTargetStatus::TargetUnreachableAtMax
    };

    Ok(DynamicSigmaDecision {
        sigma,
        hash_participation_floor,
        reorg_floor,
        risk_score_floor,
        economic_floor,
        hash_participation_status,
        economic_target_status,
    })
}

fn validate_params(params: DynamicSigmaParameters) -> Result<(), DynamicSigmaError> {
    if !(1 <= params.base_sigma
        && params.base_sigma < params.raised_sigma
        && params.raised_sigma < params.max_sigma)
    {
        return Err(DynamicSigmaError::InvalidSigmaLadder);
    }

    if !(params.critical_hash_participation_pct < params.target_hash_participation_pct
        && params.target_hash_participation_pct <= 100)
    {
        return Err(DynamicSigmaError::InvalidHashParticipationThresholds);
    }

    if params.risk_score_raised_threshold >= params.risk_score_max_threshold {
        return Err(DynamicSigmaError::InvalidRiskScoreThresholds);
    }

    Ok(())
}

fn validate_window(window: DynamicSigmaTelemetryWindow) -> Result<(), DynamicSigmaError> {
    if window.total_hash_work == 0 {
        return Err(DynamicSigmaError::EmptyHashWorkWindow);
    }

    if window.crosslink_participating_hash_work > window.total_hash_work {
        return Err(DynamicSigmaError::ParticipatingHashWorkExceedsTotal);
    }

    if window.total_tenderlink_rounds == 0 {
        return Err(DynamicSigmaError::EmptyTenderlinkRoundWindow);
    }

    if window.failed_tenderlink_rounds > window.total_tenderlink_rounds {
        return Err(DynamicSigmaError::FailedRoundsExceedTotal);
    }

    let raw_coverage_gap_pct = ceil_ratio_pct(
        window.total_hash_work - window.crosslink_participating_hash_work,
        window.total_hash_work,
    );
    if window.estimated_coverage_risk_pct < raw_coverage_gap_pct {
        return Err(DynamicSigmaError::CoverageRiskEstimateTooLow);
    }

    let raw_round_failure_pct = ceil_ratio_pct(
        u128::from(window.failed_tenderlink_rounds),
        u128::from(window.total_tenderlink_rounds),
    );
    if window.estimated_round_failure_rate_pct < raw_round_failure_pct {
        return Err(DynamicSigmaError::RoundFailureEstimateTooLow);
    }

    if !(window.rollback_risk.base_sigma_ppm >= window.rollback_risk.raised_sigma_ppm
        && window.rollback_risk.raised_sigma_ppm >= window.rollback_risk.max_sigma_ppm)
    {
        return Err(DynamicSigmaError::RollbackRiskCurveNotMonotone);
    }

    Ok(())
}

fn max_floor(floors: [u64; 4]) -> u64 {
    floors
        .into_iter()
        .max()
        .expect("fixed-size array is nonempty")
}

fn hash_participation_floor(
    params: DynamicSigmaParameters,
    window: DynamicSigmaTelemetryWindow,
) -> (u64, HashParticipationStatus) {
    if !work_coverage_at_least(window, params.critical_hash_participation_pct) {
        (params.max_sigma, HashParticipationStatus::Critical)
    } else if !work_coverage_at_least(window, params.target_hash_participation_pct) {
        (params.raised_sigma, HashParticipationStatus::Degraded)
    } else {
        (params.base_sigma, HashParticipationStatus::Healthy)
    }
}

fn work_coverage_at_least(window: DynamicSigmaTelemetryWindow, pct: u8) -> bool {
    let required_work = ceil_mul_div(window.total_hash_work, u128::from(pct), 100);
    window.crosslink_participating_hash_work >= required_work
}

fn reorg_floor(params: DynamicSigmaParameters, depth: u64) -> u64 {
    if depth >= params.raised_sigma {
        params.max_sigma
    } else if depth >= params.base_sigma {
        params.raised_sigma
    } else {
        params.base_sigma
    }
}

fn calibrated_risk_score(
    params: DynamicSigmaParameters,
    window: DynamicSigmaTelemetryWindow,
) -> u128 {
    u128::from(params.coverage_risk_weight) * u128::from(window.estimated_coverage_risk_pct)
        + u128::from(params.round_failure_risk_weight)
            * u128::from(window.estimated_round_failure_rate_pct)
        + u128::from(params.block_interval_variance_risk_weight)
            * u128::from(window.measured_block_interval_variance_pct)
        + u128::from(params.reorg_depth_risk_weight)
            * u128::from(window.measured_observed_reorg_depth)
}

fn risk_score_floor(params: DynamicSigmaParameters, score: u128) -> u64 {
    if score >= u128::from(params.risk_score_max_threshold) {
        params.max_sigma
    } else if score >= u128::from(params.risk_score_raised_threshold) {
        params.raised_sigma
    } else {
        params.base_sigma
    }
}

fn economic_floor(params: DynamicSigmaParameters, window: DynamicSigmaTelemetryWindow) -> u64 {
    if economic_target_satisfied_at_sigma(params, window, params.base_sigma) {
        params.base_sigma
    } else if economic_target_satisfied_at_sigma(params, window, params.raised_sigma) {
        params.raised_sigma
    } else {
        params.max_sigma
    }
}

fn economic_target_satisfied_at_sigma(
    params: DynamicSigmaParameters,
    window: DynamicSigmaTelemetryWindow,
    sigma: u64,
) -> bool {
    let risk_ppm = rollback_risk_ppm_at_sigma(params, window, sigma);
    risk_ppm <= params.max_acceptable_rollback_risk_ppm
        && expected_loss_within_budget(
            u128::from(risk_ppm),
            window.value_at_risk_units,
            window.max_acceptable_expected_loss_units,
        )
}

fn rollback_risk_ppm_at_sigma(
    params: DynamicSigmaParameters,
    window: DynamicSigmaTelemetryWindow,
    sigma: u64,
) -> u64 {
    if sigma <= params.base_sigma {
        window.rollback_risk.base_sigma_ppm
    } else if sigma <= params.raised_sigma {
        window.rollback_risk.raised_sigma_ppm
    } else {
        window.rollback_risk.max_sigma_ppm
    }
}

fn expected_loss_within_budget(
    risk_ppm: u128,
    value_at_risk_units: u128,
    max_acceptable_expected_loss_units: u128,
) -> bool {
    let Some(expected_loss_numerator) = risk_ppm.checked_mul(value_at_risk_units) else {
        return false;
    };
    let Some(loss_budget_numerator) =
        max_acceptable_expected_loss_units.checked_mul(PPM_DENOMINATOR)
    else {
        return true;
    };

    expected_loss_numerator <= loss_budget_numerator
}

fn ceil_ratio_pct(numerator: u128, denominator: u128) -> u8 {
    debug_assert!(denominator > 0);
    debug_assert!(numerator <= denominator);

    ceil_mul_div(numerator, 100, denominator)
        .try_into()
        .expect("percentage is bounded by 100")
}

fn ceil_mul_div(value: u128, multiplier: u128, divisor: u128) -> u128 {
    debug_assert!(divisor > 0);

    let quotient = value / divisor;
    let remainder = value % divisor;
    let quotient_product = quotient
        .checked_mul(multiplier)
        .expect("bounded quotient multiplication should not overflow");
    let remainder_product = remainder
        .checked_mul(multiplier)
        .expect("bounded remainder multiplication should not overflow");

    quotient_product + remainder_product.div_ceil(divisor)
}

fn saturating_pct_add(raw_pct: u8, margin_pct: u8) -> u8 {
    raw_pct.saturating_add(margin_pct).min(100)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> DynamicSigmaParameters {
        DynamicSigmaParameters {
            base_sigma: 1,
            raised_sigma: 3,
            max_sigma: 6,
            target_hash_participation_pct: 67,
            critical_hash_participation_pct: 50,
            max_acceptable_rollback_risk_ppm: 100,
            coverage_risk_weight: 2,
            round_failure_risk_weight: 3,
            block_interval_variance_risk_weight: 2,
            reorg_depth_risk_weight: 60,
            risk_score_raised_threshold: 100,
            risk_score_max_threshold: 220,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn window(
        participating_hash_work: u128,
        failed_rounds: u64,
        coverage_risk_pct: u8,
        round_failure_rate_pct: u8,
        block_interval_variance_pct: u8,
        reorg_depth: u64,
        rollback_risk: RollbackRiskCurve,
        value_at_risk_units: u128,
        max_acceptable_expected_loss_units: u128,
    ) -> DynamicSigmaTelemetryWindow {
        DynamicSigmaTelemetryWindow {
            total_hash_work: 100,
            crosslink_participating_hash_work: participating_hash_work,
            total_tenderlink_rounds: 100,
            failed_tenderlink_rounds: failed_rounds,
            estimated_coverage_risk_pct: coverage_risk_pct,
            estimated_round_failure_rate_pct: round_failure_rate_pct,
            measured_block_interval_variance_pct: block_interval_variance_pct,
            measured_observed_reorg_depth: reorg_depth,
            rollback_risk,
            value_at_risk_units,
            max_acceptable_expected_loss_units,
        }
    }

    fn decide(window: DynamicSigmaTelemetryWindow) -> DynamicSigmaDecision {
        select_dynamic_sigma(params(), window).expect("fixture telemetry should be valid")
    }

    fn raw_telemetry(
        participating_hash_work: u128,
        failed_rounds: u64,
    ) -> DynamicSigmaRawTelemetry {
        DynamicSigmaRawTelemetry {
            total_hash_work: 100,
            crosslink_participating_hash_work: participating_hash_work,
            total_tenderlink_rounds: 100,
            failed_tenderlink_rounds: failed_rounds,
            measured_block_interval_variance_pct: 10,
            measured_observed_reorg_depth: 0,
            rollback_risk: RollbackRiskCurve {
                base_sigma_ppm: 80,
                raised_sigma_ppm: 20,
                max_sigma_ppm: 2,
            },
            value_at_risk_units: 1000,
            max_acceptable_expected_loss_units: 100,
        }
    }

    #[test]
    fn raw_observation_counters_build_conservative_window() {
        let telemetry = raw_telemetry(63, 15)
            .into_window(TelemetryEstimateMargins {
                coverage_risk_margin_pct: 2,
                round_failure_margin_pct: 3,
            })
            .expect("raw telemetry should build a window");

        assert_eq!(telemetry.estimated_coverage_risk_pct, 39);
        assert_eq!(telemetry.estimated_round_failure_rate_pct, 18);
        assert_eq!(telemetry.crosslink_participating_hash_work, 63);
        assert_eq!(telemetry.failed_tenderlink_rounds, 15);
    }

    #[test]
    fn raw_observation_estimates_saturate_at_one_hundred_percent() {
        let telemetry = raw_telemetry(0, 100)
            .into_window(TelemetryEstimateMargins {
                coverage_risk_margin_pct: 5,
                round_failure_margin_pct: 5,
            })
            .expect("raw telemetry should build a saturated window");

        assert_eq!(telemetry.estimated_coverage_risk_pct, 100);
        assert_eq!(telemetry.estimated_round_failure_rate_pct, 100);
    }

    #[test]
    fn raw_observation_window_feeds_dynamic_sigma_controller() {
        let telemetry = raw_telemetry(63, 0)
            .into_window(TelemetryEstimateMargins::default())
            .expect("raw telemetry should build a window");
        let decision = decide(telemetry);

        assert_eq!(decision.hash_participation_floor, 3);
        assert_eq!(decision.sigma, 3);
    }

    #[test]
    fn matches_quint_telemetry_fixture_windows() {
        let fixtures = [
            (
                window(
                    90,
                    0,
                    10,
                    0,
                    5,
                    0,
                    RollbackRiskCurve {
                        base_sigma_ppm: 40,
                        raised_sigma_ppm: 10,
                        max_sigma_ppm: 1,
                    },
                    1000,
                    100,
                ),
                1,
                HashParticipationStatus::Healthy,
                EconomicTargetStatus::TargetSatisfied,
            ),
            (
                window(
                    63,
                    0,
                    37,
                    0,
                    10,
                    0,
                    RollbackRiskCurve {
                        base_sigma_ppm: 80,
                        raised_sigma_ppm: 20,
                        max_sigma_ppm: 2,
                    },
                    1000,
                    100,
                ),
                3,
                HashParticipationStatus::Degraded,
                EconomicTargetStatus::TargetSatisfied,
            ),
            (
                window(
                    75,
                    15,
                    25,
                    15,
                    15,
                    0,
                    RollbackRiskCurve {
                        base_sigma_ppm: 80,
                        raised_sigma_ppm: 25,
                        max_sigma_ppm: 3,
                    },
                    1000,
                    100,
                ),
                3,
                HashParticipationStatus::Healthy,
                EconomicTargetStatus::TargetSatisfied,
            ),
            (
                window(
                    90,
                    0,
                    10,
                    0,
                    5,
                    0,
                    RollbackRiskCurve {
                        base_sigma_ppm: 250,
                        raised_sigma_ppm: 80,
                        max_sigma_ppm: 10,
                    },
                    1000,
                    100,
                ),
                3,
                HashParticipationStatus::Healthy,
                EconomicTargetStatus::TargetSatisfied,
            ),
            (
                window(
                    45,
                    0,
                    55,
                    0,
                    0,
                    0,
                    RollbackRiskCurve {
                        base_sigma_ppm: 500,
                        raised_sigma_ppm: 250,
                        max_sigma_ppm: 70,
                    },
                    1000,
                    100,
                ),
                6,
                HashParticipationStatus::Critical,
                EconomicTargetStatus::TargetSatisfied,
            ),
            (
                window(
                    90,
                    0,
                    10,
                    0,
                    5,
                    0,
                    RollbackRiskCurve {
                        base_sigma_ppm: 500,
                        raised_sigma_ppm: 180,
                        max_sigma_ppm: 70,
                    },
                    1000,
                    100,
                ),
                6,
                HashParticipationStatus::Healthy,
                EconomicTargetStatus::TargetSatisfied,
            ),
            (
                window(
                    90,
                    0,
                    10,
                    0,
                    5,
                    0,
                    RollbackRiskCurve {
                        base_sigma_ppm: 500,
                        raised_sigma_ppm: 300,
                        max_sigma_ppm: 120,
                    },
                    1000,
                    100,
                ),
                6,
                HashParticipationStatus::Healthy,
                EconomicTargetStatus::TargetUnreachableAtMax,
            ),
            (
                window(
                    90,
                    0,
                    10,
                    0,
                    5,
                    3,
                    RollbackRiskCurve {
                        base_sigma_ppm: 400,
                        raised_sigma_ppm: 180,
                        max_sigma_ppm: 60,
                    },
                    1000,
                    100,
                ),
                6,
                HashParticipationStatus::Healthy,
                EconomicTargetStatus::TargetSatisfied,
            ),
            (
                window(
                    90,
                    0,
                    10,
                    0,
                    5,
                    0,
                    RollbackRiskCurve {
                        base_sigma_ppm: 80,
                        raised_sigma_ppm: 5,
                        max_sigma_ppm: 1,
                    },
                    100000,
                    1,
                ),
                3,
                HashParticipationStatus::Healthy,
                EconomicTargetStatus::TargetSatisfied,
            ),
        ];

        for (index, (telemetry, expected_sigma, expected_status, expected_economic_status)) in
            fixtures.into_iter().enumerate()
        {
            let decision = decide(telemetry);

            assert_eq!(decision.sigma, expected_sigma, "fixture window {index}");
            assert_eq!(
                decision.hash_participation_status, expected_status,
                "fixture window {index}"
            );
            assert_eq!(
                decision.economic_target_status, expected_economic_status,
                "fixture window {index}"
            );
        }
    }

    #[test]
    fn hash_work_participation_raises_sigma() {
        let decision = decide(window(
            63,
            0,
            37,
            0,
            10,
            0,
            RollbackRiskCurve {
                base_sigma_ppm: 80,
                raised_sigma_ppm: 20,
                max_sigma_ppm: 2,
            },
            1000,
            100,
        ));

        assert_eq!(decision.hash_participation_floor, 3);
        assert_eq!(decision.sigma, 3);
        assert_eq!(
            decision.hash_participation_status,
            HashParticipationStatus::Degraded
        );
    }

    #[test]
    fn critical_hash_work_participation_forces_max_sigma() {
        let decision = decide(window(
            45,
            0,
            55,
            0,
            0,
            0,
            RollbackRiskCurve {
                base_sigma_ppm: 500,
                raised_sigma_ppm: 250,
                max_sigma_ppm: 70,
            },
            1000,
            100,
        ));

        assert_eq!(decision.hash_participation_floor, 6);
        assert_eq!(decision.sigma, 6);
        assert_eq!(
            decision.hash_participation_status,
            HashParticipationStatus::Critical
        );
    }

    #[test]
    fn expected_loss_budget_raises_sigma_even_when_ppm_target_is_met() {
        let decision = decide(window(
            90,
            0,
            10,
            0,
            5,
            0,
            RollbackRiskCurve {
                base_sigma_ppm: 80,
                raised_sigma_ppm: 5,
                max_sigma_ppm: 1,
            },
            100000,
            1,
        ));

        assert_eq!(decision.economic_floor, 3);
        assert_eq!(decision.sigma, 3);
        assert_eq!(
            decision.economic_target_status,
            EconomicTargetStatus::TargetSatisfied
        );
    }

    #[test]
    fn rejects_nonconservative_coverage_risk_estimate() {
        let mut telemetry = window(
            63,
            0,
            36,
            0,
            10,
            0,
            RollbackRiskCurve {
                base_sigma_ppm: 80,
                raised_sigma_ppm: 20,
                max_sigma_ppm: 2,
            },
            1000,
            100,
        );
        telemetry.estimated_coverage_risk_pct = 36;

        assert_eq!(
            select_dynamic_sigma(params(), telemetry),
            Err(DynamicSigmaError::CoverageRiskEstimateTooLow)
        );
    }

    #[test]
    fn rejects_nonconservative_round_failure_estimate() {
        let telemetry = window(
            75,
            15,
            25,
            14,
            15,
            0,
            RollbackRiskCurve {
                base_sigma_ppm: 80,
                raised_sigma_ppm: 25,
                max_sigma_ppm: 3,
            },
            1000,
            100,
        );

        assert_eq!(
            select_dynamic_sigma(params(), telemetry),
            Err(DynamicSigmaError::RoundFailureEstimateTooLow)
        );
    }

    #[test]
    fn rejects_nonmonotone_rollback_risk_curve() {
        let telemetry = window(
            90,
            0,
            10,
            0,
            5,
            0,
            RollbackRiskCurve {
                base_sigma_ppm: 40,
                raised_sigma_ppm: 50,
                max_sigma_ppm: 1,
            },
            1000,
            100,
        );

        assert_eq!(
            select_dynamic_sigma(params(), telemetry),
            Err(DynamicSigmaError::RollbackRiskCurveNotMonotone)
        );
    }
}
