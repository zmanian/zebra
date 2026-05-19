//! Dynamic sigma controller prototype.
//!
//! This module is deliberately pure. It turns a production-shaped telemetry
//! window into the same sigma floor described by the Quint dynamic-sigma
//! telemetry contract, and provides the hysteresis policy that proposal
//! construction can apply before carrying a selected sigma in evidence.

use std::io::{Read, Write};

use zebra_chain::{
    block::{FatPointerToBftBlock, Header},
    serialization::{SerializationError, ZcashDeserialize, ZcashSerialize},
};

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

/// Crosslink-participation classification for one observed PoW work unit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DynamicSigmaHashParticipation {
    /// The observed work carries objectively verified Crosslink participation.
    VerifiedParticipating,
    /// The observed work does not carry objectively verified Crosslink participation.
    NotVerifiedParticipating,
}

/// One source-side PoW work observation for dynamic-sigma hash participation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DynamicSigmaHashWorkObservation {
    /// Observed PoW work represented by this sample.
    pub hash_work: u128,
    /// Whether this observed work is verified as Crosslink-participating.
    pub participation: DynamicSigmaHashParticipation,
}

/// Aggregated PoW work participation telemetry for a measurement window.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DynamicSigmaHashWorkTelemetry {
    /// Total observed PoW work in the measurement window.
    pub total_hash_work: u128,
    /// Observed PoW work with verified Crosslink participation.
    pub crosslink_participating_hash_work: u128,
}

/// Invalid source-side PoW work telemetry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DynamicSigmaHashWorkTelemetryError {
    /// No PoW work observations were provided.
    EmptyObservationWindow,
    /// A PoW work observation carried zero work.
    ZeroHashWorkObservation,
    /// The observed PoW work total overflowed.
    HashWorkOverflow,
}

/// Invalid source-side PoW header observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DynamicSigmaHeaderObservationError {
    /// The header difficulty threshold cannot be converted into PoW work.
    InvalidDifficultyThreshold,
    /// The header difficulty threshold converted into zero PoW work.
    ZeroHeaderWork,
}

/// Tenderlink round counters collected over a dynamic-sigma telemetry window.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DynamicSigmaRoundCounters {
    /// Tenderlink rounds that started in the measurement window.
    pub started_rounds: u64,
    /// Tenderlink rounds that failed to decide and required recovery.
    pub failed_rounds: u64,
    /// Failed rounds with a nil-precommit recovery certificate.
    pub nil_precommit_rounds: u64,
    /// Failed rounds caused by a stale proposal or stream change.
    pub stale_proposal_rounds: u64,
    /// Failed rounds caused by a timeout.
    pub timeout_rounds: u64,
    /// Failed rounds caused by an invalid proposal.
    pub invalid_proposal_rounds: u64,
    /// Failed rounds caused by mixed or conflicting round evidence.
    pub mixed_evidence_rounds: u64,
    /// Tenderlink rounds that decided a value.
    pub decided_rounds: u64,
}

/// Tenderlink round telemetry event used to build durable counters.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DynamicSigmaRoundEvent {
    /// A Tenderlink round started.
    StartedRound,
    /// A Tenderlink round decided a value.
    Decided,
    /// A Tenderlink round failed through nil-precommit recovery.
    NilPrecommitRecovery,
    /// A Tenderlink round failed because its proposal became stale.
    StaleProposal,
    /// A Tenderlink round failed by timeout.
    Timeout,
    /// A Tenderlink round failed because the proposal was invalid.
    InvalidProposal,
    /// A Tenderlink round failed with mixed or conflicting evidence.
    MixedEvidence,
}

/// Production-shaped telemetry inputs before raw controller telemetry assembly.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DynamicSigmaTelemetryComponents {
    /// Total observed PoW work over the measurement window.
    pub total_hash_work: Option<u128>,
    /// Observed PoW work with objectively verifiable Crosslink participation.
    pub crosslink_participating_hash_work: Option<u128>,
    /// Tenderlink round counters for the same measurement window.
    pub round_counters: DynamicSigmaRoundCounters,
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

/// Invalid production telemetry assembly inputs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DynamicSigmaTelemetryAssemblyError {
    /// Total PoW work evidence was not provided.
    MissingTotalHashWork,
    /// Crosslink-participating PoW work evidence was not provided.
    MissingCrosslinkParticipatingHashWork,
    /// Failed rounds exceed started rounds.
    FailedRoundsExceedStarted,
    /// Decided rounds exceed started rounds.
    DecidedRoundsExceedStarted,
    /// Failed and decided rounds together exceed started rounds.
    DecidedAndFailedRoundsExceedStarted,
    /// Nil-precommit recovery rounds exceed failed rounds.
    NilPrecommitRoundsExceedFailed,
    /// Stale-proposal rounds exceed failed rounds.
    StaleProposalRoundsExceedFailed,
    /// Failure-reason counters collectively exceed failed rounds.
    FailureReasonCountersExceedFailed,
    /// Assembled raw telemetry is invalid.
    InvalidRawTelemetry(DynamicSigmaError),
}

/// A best-tip transition with its common ancestor in the previous best chain.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DynamicSigmaBestTipTransition {
    /// Previous best-tip height before the transition.
    pub previous_tip_height: u64,
    /// New best-tip height after the transition.
    pub new_tip_height: u64,
    /// Common ancestor height shared by the previous and new best tips.
    pub common_ancestor_height: u64,
}

/// Invalid rollback-depth telemetry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DynamicSigmaRollbackTelemetryError {
    /// The common ancestor is above the previous best tip.
    CommonAncestorAbovePreviousTip,
    /// The common ancestor is above the new best tip.
    CommonAncestorAboveNewTip,
}

/// Invalid rollback-risk estimator input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DynamicSigmaRollbackRiskEstimatorError {
    /// Controller parameters were invalid.
    InvalidParameters(DynamicSigmaError),
    /// No observed rollback-depth windows were supplied.
    EmptyObservationWindow,
    /// The safety margin exceeds one million parts per million.
    RiskMarginTooLarge {
        /// Risk margin supplied by the caller.
        margin_ppm: u64,
    },
}

/// Source-side observation window for building dynamic-sigma telemetry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DynamicSigmaTelemetryObservationWindow<'a> {
    /// PoW work observations used to derive hash participation.
    pub hash_work_observations: &'a [DynamicSigmaHashWorkObservation],
    /// Tenderlink round counters for this window.
    pub round_counters: DynamicSigmaRoundCounters,
    /// Best-tip transitions used to derive observed rollback depth.
    pub best_tip_transitions: &'a [DynamicSigmaBestTipTransition],
    /// Measured PoW timing variance percentage.
    pub measured_block_interval_variance_pct: u8,
    /// Rollback risk estimates across the sigma ladder.
    pub rollback_risk: RollbackRiskCurve,
    /// Economic value exposed to rollback in the window.
    pub value_at_risk_units: u128,
    /// Maximum acceptable expected loss for this window.
    pub max_acceptable_expected_loss_units: u128,
}

/// Source-side observation window whose PoW work evidence is provided as headers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DynamicSigmaHeaderObservationWindow<'a> {
    /// Validated PoW headers used to derive hash-work participation.
    pub pow_headers: &'a [Header],
    /// Tenderlink round counters for this window.
    pub round_counters: DynamicSigmaRoundCounters,
    /// Best-tip transitions used to derive observed rollback depth.
    pub best_tip_transitions: &'a [DynamicSigmaBestTipTransition],
    /// Measured PoW timing variance percentage.
    pub measured_block_interval_variance_pct: u8,
    /// Rollback risk estimates across the sigma ladder.
    pub rollback_risk: RollbackRiskCurve,
    /// Economic value exposed to rollback in the window.
    pub value_at_risk_units: u128,
    /// Maximum acceptable expected loss for this window.
    pub max_acceptable_expected_loss_units: u128,
}

/// Invalid source-side observation window.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DynamicSigmaTelemetryObservationError {
    /// Hash-work observations were invalid.
    InvalidHashWork(DynamicSigmaHashWorkTelemetryError),
    /// Tenderlink round counters were internally inconsistent.
    InvalidRoundCounters(DynamicSigmaTelemetryAssemblyError),
    /// Rollback-depth observations were invalid.
    InvalidRollbackTelemetry(DynamicSigmaRollbackTelemetryError),
}

/// Invalid source-side header observation window.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DynamicSigmaHeaderObservationWindowError {
    /// A PoW header could not be converted into hash-work telemetry.
    InvalidHeader(DynamicSigmaHeaderObservationError),
    /// The derived source telemetry window was invalid.
    InvalidTelemetry(DynamicSigmaTelemetryObservationError),
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

/// Economic exposure policy for a dynamic-sigma telemetry window.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DynamicSigmaEconomicExposurePolicy {
    /// Expected loss is part of consensus-critical proposal validity.
    ConsensusCritical {
        /// Economic value exposed to rollback in the window.
        value_at_risk_units: u128,
        /// Maximum acceptable expected loss for this window.
        max_acceptable_expected_loss_units: u128,
    },
    /// Expected loss is handled by services and does not affect consensus sigma.
    ServiceLocal,
}

/// Unit values carried into dynamic-sigma telemetry for economic checks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DynamicSigmaEconomicExposureUnits {
    /// Economic value exposed to rollback in the window.
    pub value_at_risk_units: u128,
    /// Maximum acceptable expected loss for this window.
    pub max_acceptable_expected_loss_units: u128,
}

impl DynamicSigmaEconomicExposurePolicy {
    /// Convert an exposure policy into the unit fields carried by telemetry.
    pub fn to_units(self) -> DynamicSigmaEconomicExposureUnits {
        match self {
            DynamicSigmaEconomicExposurePolicy::ConsensusCritical {
                value_at_risk_units,
                max_acceptable_expected_loss_units,
            } => DynamicSigmaEconomicExposureUnits {
                value_at_risk_units,
                max_acceptable_expected_loss_units,
            },
            DynamicSigmaEconomicExposurePolicy::ServiceLocal => DynamicSigmaEconomicExposureUnits {
                value_at_risk_units: 0,
                max_acceptable_expected_loss_units: 0,
            },
        }
    }
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

/// Hysteresis policy for applying dynamic-sigma decisions across windows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DynamicSigmaHysteresisParameters {
    /// Stable windows required before sigma may step down one ladder level.
    pub decrease_confirmation_windows: u64,
}

/// Hysteresis state for a dynamic-sigma controller.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DynamicSigmaHysteresisState {
    /// Currently applied sigma.
    pub current_sigma: u64,
    /// Consecutive windows whose required sigma was below `current_sigma`.
    pub stable_windows_below_current: u64,
}

/// Dynamic-sigma selection after applying hysteresis to the required floor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DynamicSigmaHysteresisSelection {
    /// Controller decision before hysteresis is applied.
    pub required_decision: DynamicSigmaDecision,
    /// Hysteresis state to apply to the current proposal/window.
    pub applied_state: DynamicSigmaHysteresisState,
}

/// Invalid dynamic-sigma hysteresis input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DynamicSigmaHysteresisError {
    /// Controller parameters were invalid.
    InvalidParameters(DynamicSigmaError),
    /// The current sigma is not one of the configured ladder values.
    CurrentSigmaOutsideLadder {
        /// Current applied sigma.
        current_sigma: u64,
    },
    /// The required sigma is not one of the configured ladder values.
    RequiredSigmaOutsideLadder {
        /// Sigma required by the current telemetry window.
        required_sigma: u64,
    },
}

/// Invalid dynamic-sigma selection with hysteresis.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DynamicSigmaHysteresisSelectionError {
    /// The telemetry window or controller parameters were invalid.
    InvalidTelemetry(DynamicSigmaError),
    /// The hysteresis policy or state was invalid.
    InvalidHysteresis(DynamicSigmaHysteresisError),
}

/// Proposal-carried evidence for a dynamic-sigma decision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DynamicSigmaProposalEvidence {
    /// Raw telemetry counters included with or committed by a proposal.
    pub raw_telemetry: DynamicSigmaRawTelemetry,
    /// Conservative margins applied while deriving the telemetry window.
    pub margins: TelemetryEstimateMargins,
    /// Sigma selected by the proposer.
    pub selected_sigma: u64,
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

/// Invalid proposal-carried dynamic-sigma evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DynamicSigmaEvidenceError {
    /// The underlying telemetry window is invalid.
    InvalidTelemetry(DynamicSigmaError),
    /// The selected sigma is not in the configured sigma ladder.
    SelectedSigmaOutsideLadder {
        /// Sigma selected by the proposer.
        selected: u64,
    },
    /// The selected sigma is below the controller-required floor.
    SelectedSigmaBelowRequired {
        /// Sigma selected by the proposer.
        selected: u64,
        /// Minimum sigma required by the controller.
        required: u64,
    },
}

/// Invalid proposal-evidence selection from telemetry inputs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DynamicSigmaProposalEvidenceSelectionError {
    /// Raw telemetry could not be converted into a valid controller window.
    InvalidTelemetry(DynamicSigmaError),
    /// Production-shaped telemetry components could not be assembled.
    InvalidTelemetryAssembly(DynamicSigmaTelemetryAssemblyError),
    /// Hysteresis selection failed for the supplied state or telemetry.
    InvalidHysteresisSelection(DynamicSigmaHysteresisSelectionError),
}

fn write_u64_le<W: Write>(writer: &mut W, value: u64) -> Result<(), std::io::Error> {
    writer.write_all(&value.to_le_bytes())
}

fn read_u64_le<R: Read>(reader: &mut R) -> Result<u64, std::io::Error> {
    let mut bytes = [0u8; 8];
    reader.read_exact(&mut bytes)?;
    Ok(u64::from_le_bytes(bytes))
}

fn write_u128_le<W: Write>(writer: &mut W, value: u128) -> Result<(), std::io::Error> {
    writer.write_all(&value.to_le_bytes())
}

fn read_u128_le<R: Read>(reader: &mut R) -> Result<u128, std::io::Error> {
    let mut bytes = [0u8; 16];
    reader.read_exact(&mut bytes)?;
    Ok(u128::from_le_bytes(bytes))
}

fn write_u8<W: Write>(writer: &mut W, value: u8) -> Result<(), std::io::Error> {
    writer.write_all(&[value])
}

fn read_u8<R: Read>(reader: &mut R) -> Result<u8, std::io::Error> {
    let mut bytes = [0u8; 1];
    reader.read_exact(&mut bytes)?;
    Ok(bytes[0])
}

impl ZcashSerialize for RollbackRiskCurve {
    fn zcash_serialize<W: Write>(&self, mut writer: W) -> Result<(), std::io::Error> {
        write_u64_le(&mut writer, self.base_sigma_ppm)?;
        write_u64_le(&mut writer, self.raised_sigma_ppm)?;
        write_u64_le(&mut writer, self.max_sigma_ppm)?;

        Ok(())
    }
}

impl ZcashDeserialize for RollbackRiskCurve {
    fn zcash_deserialize<R: Read>(mut reader: R) -> Result<Self, SerializationError> {
        Ok(Self {
            base_sigma_ppm: read_u64_le(&mut reader)?,
            raised_sigma_ppm: read_u64_le(&mut reader)?,
            max_sigma_ppm: read_u64_le(&mut reader)?,
        })
    }
}

impl ZcashSerialize for TelemetryEstimateMargins {
    fn zcash_serialize<W: Write>(&self, mut writer: W) -> Result<(), std::io::Error> {
        write_u8(&mut writer, self.coverage_risk_margin_pct)?;
        write_u8(&mut writer, self.round_failure_margin_pct)?;

        Ok(())
    }
}

impl ZcashDeserialize for TelemetryEstimateMargins {
    fn zcash_deserialize<R: Read>(mut reader: R) -> Result<Self, SerializationError> {
        Ok(Self {
            coverage_risk_margin_pct: read_u8(&mut reader)?,
            round_failure_margin_pct: read_u8(&mut reader)?,
        })
    }
}

impl ZcashSerialize for DynamicSigmaHysteresisParameters {
    fn zcash_serialize<W: Write>(&self, mut writer: W) -> Result<(), std::io::Error> {
        write_u64_le(&mut writer, self.decrease_confirmation_windows)?;

        Ok(())
    }
}

impl ZcashDeserialize for DynamicSigmaHysteresisParameters {
    fn zcash_deserialize<R: Read>(mut reader: R) -> Result<Self, SerializationError> {
        Ok(Self {
            decrease_confirmation_windows: read_u64_le(&mut reader)?,
        })
    }
}

impl ZcashSerialize for DynamicSigmaHysteresisState {
    fn zcash_serialize<W: Write>(&self, mut writer: W) -> Result<(), std::io::Error> {
        write_u64_le(&mut writer, self.current_sigma)?;
        write_u64_le(&mut writer, self.stable_windows_below_current)?;

        Ok(())
    }
}

impl ZcashDeserialize for DynamicSigmaHysteresisState {
    fn zcash_deserialize<R: Read>(mut reader: R) -> Result<Self, SerializationError> {
        Ok(Self {
            current_sigma: read_u64_le(&mut reader)?,
            stable_windows_below_current: read_u64_le(&mut reader)?,
        })
    }
}

impl ZcashSerialize for DynamicSigmaRawTelemetry {
    fn zcash_serialize<W: Write>(&self, mut writer: W) -> Result<(), std::io::Error> {
        write_u128_le(&mut writer, self.total_hash_work)?;
        write_u128_le(&mut writer, self.crosslink_participating_hash_work)?;
        write_u64_le(&mut writer, self.total_tenderlink_rounds)?;
        write_u64_le(&mut writer, self.failed_tenderlink_rounds)?;
        write_u8(&mut writer, self.measured_block_interval_variance_pct)?;
        write_u64_le(&mut writer, self.measured_observed_reorg_depth)?;
        self.rollback_risk.zcash_serialize(&mut writer)?;
        write_u128_le(&mut writer, self.value_at_risk_units)?;
        write_u128_le(&mut writer, self.max_acceptable_expected_loss_units)?;

        Ok(())
    }
}

impl ZcashDeserialize for DynamicSigmaRawTelemetry {
    fn zcash_deserialize<R: Read>(mut reader: R) -> Result<Self, SerializationError> {
        Ok(Self {
            total_hash_work: read_u128_le(&mut reader)?,
            crosslink_participating_hash_work: read_u128_le(&mut reader)?,
            total_tenderlink_rounds: read_u64_le(&mut reader)?,
            failed_tenderlink_rounds: read_u64_le(&mut reader)?,
            measured_block_interval_variance_pct: read_u8(&mut reader)?,
            measured_observed_reorg_depth: read_u64_le(&mut reader)?,
            rollback_risk: RollbackRiskCurve::zcash_deserialize(&mut reader)?,
            value_at_risk_units: read_u128_le(&mut reader)?,
            max_acceptable_expected_loss_units: read_u128_le(&mut reader)?,
        })
    }
}

impl ZcashSerialize for DynamicSigmaProposalEvidence {
    fn zcash_serialize<W: Write>(&self, mut writer: W) -> Result<(), std::io::Error> {
        self.raw_telemetry.zcash_serialize(&mut writer)?;
        self.margins.zcash_serialize(&mut writer)?;
        write_u64_le(&mut writer, self.selected_sigma)?;

        Ok(())
    }
}

impl ZcashDeserialize for DynamicSigmaProposalEvidence {
    fn zcash_deserialize<R: Read>(mut reader: R) -> Result<Self, SerializationError> {
        Ok(Self {
            raw_telemetry: DynamicSigmaRawTelemetry::zcash_deserialize(&mut reader)?,
            margins: TelemetryEstimateMargins::zcash_deserialize(&mut reader)?,
            selected_sigma: read_u64_le(&mut reader)?,
        })
    }
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

/// Aggregate observed PoW work into Crosslink-participation telemetry.
///
/// Work without objectively verified Crosslink participation contributes to
/// the denominator but not the numerator.
pub fn observed_hash_work_participation(
    observations: &[DynamicSigmaHashWorkObservation],
) -> Result<DynamicSigmaHashWorkTelemetry, DynamicSigmaHashWorkTelemetryError> {
    if observations.is_empty() {
        return Err(DynamicSigmaHashWorkTelemetryError::EmptyObservationWindow);
    }

    let mut total_hash_work = 0u128;
    let mut crosslink_participating_hash_work = 0u128;

    for observation in observations {
        if observation.hash_work == 0 {
            return Err(DynamicSigmaHashWorkTelemetryError::ZeroHashWorkObservation);
        }

        total_hash_work = total_hash_work
            .checked_add(observation.hash_work)
            .ok_or(DynamicSigmaHashWorkTelemetryError::HashWorkOverflow)?;

        if observation.participation == DynamicSigmaHashParticipation::VerifiedParticipating {
            crosslink_participating_hash_work = crosslink_participating_hash_work
                .checked_add(observation.hash_work)
                .ok_or(DynamicSigmaHashWorkTelemetryError::HashWorkOverflow)?;
        }
    }

    Ok(DynamicSigmaHashWorkTelemetry {
        total_hash_work,
        crosslink_participating_hash_work,
    })
}

/// Convert a PoW header into a dynamic-sigma hash-work observation.
///
/// The current source marker is the consensus-visible Crosslink fat pointer in
/// the header. A non-null marker counts the header work as participating, while
/// a null marker still contributes to the total observed hash-work denominator.
pub fn hash_work_observation_from_header(
    header: &Header,
) -> Result<DynamicSigmaHashWorkObservation, DynamicSigmaHeaderObservationError> {
    hash_work_observation_from_header_with_verifier(
        header,
        default_header_participation_marker_verifier,
    )
}

/// Convert a PoW header into a hash-work observation with custom marker validation.
///
/// The verifier is only consulted for non-null Crosslink fat pointers. Returning
/// `true` means the non-null marker is accepted as verified Crosslink
/// participation for this source window.
pub fn hash_work_observation_from_header_with_verifier<F>(
    header: &Header,
    marker_verifier: F,
) -> Result<DynamicSigmaHashWorkObservation, DynamicSigmaHeaderObservationError>
where
    F: FnOnce(&FatPointerToBftBlock) -> bool,
{
    let hash_work = header
        .difficulty_threshold
        .to_work()
        .ok_or(DynamicSigmaHeaderObservationError::InvalidDifficultyThreshold)?
        .as_u128();

    if hash_work == 0 {
        return Err(DynamicSigmaHeaderObservationError::ZeroHeaderWork);
    }

    let fat_pointer = &header.fat_pointer_to_bft_block;
    let participation = if default_header_participation_marker_verifier(fat_pointer)
        && marker_verifier(fat_pointer)
    {
        DynamicSigmaHashParticipation::VerifiedParticipating
    } else {
        DynamicSigmaHashParticipation::NotVerifiedParticipating
    };

    Ok(DynamicSigmaHashWorkObservation {
        hash_work,
        participation,
    })
}

/// Convert PoW headers into dynamic-sigma hash-work observations.
pub fn hash_work_observations_from_headers(
    headers: &[Header],
) -> Result<Vec<DynamicSigmaHashWorkObservation>, DynamicSigmaHeaderObservationError> {
    hash_work_observations_from_headers_with_verifier(
        headers,
        default_header_participation_marker_verifier,
    )
}

/// Convert PoW headers into hash-work observations with custom marker validation.
pub fn hash_work_observations_from_headers_with_verifier<F>(
    headers: &[Header],
    marker_verifier: F,
) -> Result<Vec<DynamicSigmaHashWorkObservation>, DynamicSigmaHeaderObservationError>
where
    F: Fn(&FatPointerToBftBlock) -> bool,
{
    headers
        .iter()
        .map(|header| hash_work_observation_from_header_with_verifier(header, &marker_verifier))
        .collect()
}

fn default_header_participation_marker_verifier(fat_pointer: &FatPointerToBftBlock) -> bool {
    *fat_pointer != FatPointerToBftBlock::null()
}

impl DynamicSigmaTelemetryComponents {
    /// Assemble production-shaped telemetry into raw controller telemetry.
    ///
    /// This requires the hash-participation numerator to be explicit. Unknown
    /// participation is not treated as healthy participation.
    pub fn try_into_raw_telemetry(
        self,
    ) -> Result<DynamicSigmaRawTelemetry, DynamicSigmaTelemetryAssemblyError> {
        self.round_counters.validate()?;

        let total_hash_work = self
            .total_hash_work
            .ok_or(DynamicSigmaTelemetryAssemblyError::MissingTotalHashWork)?;
        let crosslink_participating_hash_work = self
            .crosslink_participating_hash_work
            .ok_or(DynamicSigmaTelemetryAssemblyError::MissingCrosslinkParticipatingHashWork)?;

        let raw_telemetry = DynamicSigmaRawTelemetry {
            total_hash_work,
            crosslink_participating_hash_work,
            total_tenderlink_rounds: self.round_counters.started_rounds,
            failed_tenderlink_rounds: self.round_counters.failed_rounds,
            measured_block_interval_variance_pct: self.measured_block_interval_variance_pct,
            measured_observed_reorg_depth: self.measured_observed_reorg_depth,
            rollback_risk: self.rollback_risk,
            value_at_risk_units: self.value_at_risk_units,
            max_acceptable_expected_loss_units: self.max_acceptable_expected_loss_units,
        };

        raw_telemetry
            .into_window(TelemetryEstimateMargins::default())
            .map_err(DynamicSigmaTelemetryAssemblyError::InvalidRawTelemetry)?;

        Ok(raw_telemetry)
    }
}

impl DynamicSigmaRoundCounters {
    /// Record one Tenderlink telemetry event into this counter window.
    pub fn record_event(&mut self, event: DynamicSigmaRoundEvent) {
        match event {
            DynamicSigmaRoundEvent::StartedRound => increment_counter(&mut self.started_rounds),
            DynamicSigmaRoundEvent::Decided => increment_counter(&mut self.decided_rounds),
            DynamicSigmaRoundEvent::NilPrecommitRecovery => {
                increment_counter(&mut self.failed_rounds);
                increment_counter(&mut self.nil_precommit_rounds);
            }
            DynamicSigmaRoundEvent::StaleProposal => {
                increment_counter(&mut self.failed_rounds);
                increment_counter(&mut self.stale_proposal_rounds);
            }
            DynamicSigmaRoundEvent::Timeout => {
                increment_counter(&mut self.failed_rounds);
                increment_counter(&mut self.timeout_rounds);
            }
            DynamicSigmaRoundEvent::InvalidProposal => {
                increment_counter(&mut self.failed_rounds);
                increment_counter(&mut self.invalid_proposal_rounds);
            }
            DynamicSigmaRoundEvent::MixedEvidence => {
                increment_counter(&mut self.failed_rounds);
                increment_counter(&mut self.mixed_evidence_rounds);
            }
        }
    }

    /// Validate that the counter window is internally consistent.
    pub fn validate(self) -> Result<(), DynamicSigmaTelemetryAssemblyError> {
        if self.failed_rounds > self.started_rounds {
            return Err(DynamicSigmaTelemetryAssemblyError::FailedRoundsExceedStarted);
        }

        if self.decided_rounds > self.started_rounds {
            return Err(DynamicSigmaTelemetryAssemblyError::DecidedRoundsExceedStarted);
        }

        if self
            .failed_rounds
            .checked_add(self.decided_rounds)
            .is_none_or(|observed_rounds| observed_rounds > self.started_rounds)
        {
            return Err(DynamicSigmaTelemetryAssemblyError::DecidedAndFailedRoundsExceedStarted);
        }

        if self.nil_precommit_rounds > self.failed_rounds {
            return Err(DynamicSigmaTelemetryAssemblyError::NilPrecommitRoundsExceedFailed);
        }

        if self.stale_proposal_rounds > self.failed_rounds {
            return Err(DynamicSigmaTelemetryAssemblyError::StaleProposalRoundsExceedFailed);
        }

        if failure_reason_rounds(self)
            .is_none_or(|reason_rounds| reason_rounds > self.failed_rounds)
        {
            return Err(DynamicSigmaTelemetryAssemblyError::FailureReasonCountersExceedFailed);
        }

        Ok(())
    }
}

fn increment_counter(counter: &mut u64) {
    *counter = counter
        .checked_add(1)
        .expect("telemetry window counters should fit in u64");
}

fn failure_reason_rounds(counters: DynamicSigmaRoundCounters) -> Option<u64> {
    counters
        .nil_precommit_rounds
        .checked_add(counters.stale_proposal_rounds)?
        .checked_add(counters.timeout_rounds)?
        .checked_add(counters.invalid_proposal_rounds)?
        .checked_add(counters.mixed_evidence_rounds)
}

impl DynamicSigmaBestTipTransition {
    /// Return the number of previous-best-chain blocks replaced by this transition.
    pub fn rollback_depth(self) -> Result<u64, DynamicSigmaRollbackTelemetryError> {
        if self.common_ancestor_height > self.previous_tip_height {
            return Err(DynamicSigmaRollbackTelemetryError::CommonAncestorAbovePreviousTip);
        }

        if self.common_ancestor_height > self.new_tip_height {
            return Err(DynamicSigmaRollbackTelemetryError::CommonAncestorAboveNewTip);
        }

        Ok(self.previous_tip_height - self.common_ancestor_height)
    }
}

/// Return the maximum rollback depth observed over best-tip transitions.
pub fn max_observed_rollback_depth(
    transitions: &[DynamicSigmaBestTipTransition],
) -> Result<u64, DynamicSigmaRollbackTelemetryError> {
    transitions.iter().try_fold(0, |max_depth, transition| {
        let rollback_depth = transition.rollback_depth()?;
        Ok(max_depth.max(rollback_depth))
    })
}

/// Estimate rollback risk for each sigma from observed rollback-depth windows.
///
/// Each supplied depth should be the maximum rollback depth observed in one
/// measurement window. The resulting risk for a sigma is the empirical
/// frequency of windows whose rollback depth reached that sigma, rounded up to
/// parts-per-million and increased by `risk_margin_ppm`.
pub fn rollback_risk_curve_from_observed_rollback_depths(
    params: DynamicSigmaParameters,
    observed_rollback_depths: &[u64],
    risk_margin_ppm: u64,
) -> Result<RollbackRiskCurve, DynamicSigmaRollbackRiskEstimatorError> {
    validate_params(params).map_err(DynamicSigmaRollbackRiskEstimatorError::InvalidParameters)?;

    if observed_rollback_depths.is_empty() {
        return Err(DynamicSigmaRollbackRiskEstimatorError::EmptyObservationWindow);
    }

    if u128::from(risk_margin_ppm) > PPM_DENOMINATOR {
        return Err(DynamicSigmaRollbackRiskEstimatorError::RiskMarginTooLarge {
            margin_ppm: risk_margin_ppm,
        });
    }

    Ok(RollbackRiskCurve {
        base_sigma_ppm: rollback_depth_exceedance_risk_ppm(
            observed_rollback_depths,
            params.base_sigma,
            risk_margin_ppm,
        ),
        raised_sigma_ppm: rollback_depth_exceedance_risk_ppm(
            observed_rollback_depths,
            params.raised_sigma,
            risk_margin_ppm,
        ),
        max_sigma_ppm: rollback_depth_exceedance_risk_ppm(
            observed_rollback_depths,
            params.max_sigma,
            risk_margin_ppm,
        ),
    })
}

/// Assemble source-side observations into production-shaped telemetry components.
pub fn telemetry_components_from_observation_window(
    window: DynamicSigmaTelemetryObservationWindow<'_>,
) -> Result<DynamicSigmaTelemetryComponents, DynamicSigmaTelemetryObservationError> {
    let hash_work = observed_hash_work_participation(window.hash_work_observations)
        .map_err(DynamicSigmaTelemetryObservationError::InvalidHashWork)?;
    window
        .round_counters
        .validate()
        .map_err(DynamicSigmaTelemetryObservationError::InvalidRoundCounters)?;
    let measured_observed_reorg_depth = max_observed_rollback_depth(window.best_tip_transitions)
        .map_err(DynamicSigmaTelemetryObservationError::InvalidRollbackTelemetry)?;

    Ok(DynamicSigmaTelemetryComponents {
        total_hash_work: Some(hash_work.total_hash_work),
        crosslink_participating_hash_work: Some(hash_work.crosslink_participating_hash_work),
        round_counters: window.round_counters,
        measured_block_interval_variance_pct: window.measured_block_interval_variance_pct,
        measured_observed_reorg_depth,
        rollback_risk: window.rollback_risk,
        value_at_risk_units: window.value_at_risk_units,
        max_acceptable_expected_loss_units: window.max_acceptable_expected_loss_units,
    })
}

/// Assemble header-derived source observations into telemetry components.
pub fn telemetry_components_from_header_observation_window(
    window: DynamicSigmaHeaderObservationWindow<'_>,
) -> Result<DynamicSigmaTelemetryComponents, DynamicSigmaHeaderObservationWindowError> {
    telemetry_components_from_header_observation_window_with_verifier(
        window,
        default_header_participation_marker_verifier,
    )
}

/// Assemble header-derived source observations with custom marker validation.
pub fn telemetry_components_from_header_observation_window_with_verifier<F>(
    window: DynamicSigmaHeaderObservationWindow<'_>,
    marker_verifier: F,
) -> Result<DynamicSigmaTelemetryComponents, DynamicSigmaHeaderObservationWindowError>
where
    F: Fn(&FatPointerToBftBlock) -> bool,
{
    let hash_work_observations =
        hash_work_observations_from_headers_with_verifier(window.pow_headers, marker_verifier)
            .map_err(DynamicSigmaHeaderObservationWindowError::InvalidHeader)?;

    telemetry_components_from_observation_window(DynamicSigmaTelemetryObservationWindow {
        hash_work_observations: &hash_work_observations,
        round_counters: window.round_counters,
        best_tip_transitions: window.best_tip_transitions,
        measured_block_interval_variance_pct: window.measured_block_interval_variance_pct,
        rollback_risk: window.rollback_risk,
        value_at_risk_units: window.value_at_risk_units,
        max_acceptable_expected_loss_units: window.max_acceptable_expected_loss_units,
    })
    .map_err(DynamicSigmaHeaderObservationWindowError::InvalidTelemetry)
}

/// Select proposal-carried dynamic-sigma evidence from raw telemetry.
pub fn select_dynamic_sigma_proposal_evidence(
    params: DynamicSigmaParameters,
    raw_telemetry: DynamicSigmaRawTelemetry,
    margins: TelemetryEstimateMargins,
) -> Result<DynamicSigmaProposalEvidence, DynamicSigmaProposalEvidenceSelectionError> {
    let window = raw_telemetry
        .into_window(margins)
        .map_err(DynamicSigmaProposalEvidenceSelectionError::InvalidTelemetry)?;
    let decision = select_dynamic_sigma(params, window)
        .map_err(DynamicSigmaProposalEvidenceSelectionError::InvalidTelemetry)?;

    Ok(DynamicSigmaProposalEvidence {
        raw_telemetry,
        margins,
        selected_sigma: decision.sigma,
    })
}

/// Select proposal-carried dynamic-sigma evidence from raw telemetry and hysteresis state.
pub fn select_dynamic_sigma_proposal_evidence_with_hysteresis(
    params: DynamicSigmaParameters,
    raw_telemetry: DynamicSigmaRawTelemetry,
    margins: TelemetryEstimateMargins,
    hysteresis_policy: DynamicSigmaHysteresisParameters,
    hysteresis_state: DynamicSigmaHysteresisState,
) -> Result<
    (DynamicSigmaProposalEvidence, DynamicSigmaHysteresisState),
    DynamicSigmaProposalEvidenceSelectionError,
> {
    let window = raw_telemetry
        .into_window(margins)
        .map_err(DynamicSigmaProposalEvidenceSelectionError::InvalidTelemetry)?;
    let selection =
        select_dynamic_sigma_with_hysteresis(params, window, hysteresis_policy, hysteresis_state)
            .map_err(DynamicSigmaProposalEvidenceSelectionError::InvalidHysteresisSelection)?;
    let next_hysteresis_state = selection.applied_state;

    Ok((
        DynamicSigmaProposalEvidence {
            raw_telemetry,
            margins,
            selected_sigma: next_hysteresis_state.current_sigma,
        },
        next_hysteresis_state,
    ))
}

/// Select proposal-carried dynamic-sigma evidence from production-shaped components.
pub fn select_dynamic_sigma_proposal_evidence_from_components(
    params: DynamicSigmaParameters,
    telemetry_components: DynamicSigmaTelemetryComponents,
    margins: TelemetryEstimateMargins,
) -> Result<DynamicSigmaProposalEvidence, DynamicSigmaProposalEvidenceSelectionError> {
    let raw_telemetry = telemetry_components
        .try_into_raw_telemetry()
        .map_err(DynamicSigmaProposalEvidenceSelectionError::InvalidTelemetryAssembly)?;

    select_dynamic_sigma_proposal_evidence(params, raw_telemetry, margins)
}

/// Select proposal-carried dynamic-sigma evidence from components and hysteresis state.
pub fn select_dynamic_sigma_proposal_evidence_from_components_with_hysteresis(
    params: DynamicSigmaParameters,
    telemetry_components: DynamicSigmaTelemetryComponents,
    margins: TelemetryEstimateMargins,
    hysteresis_policy: DynamicSigmaHysteresisParameters,
    hysteresis_state: DynamicSigmaHysteresisState,
) -> Result<
    (DynamicSigmaProposalEvidence, DynamicSigmaHysteresisState),
    DynamicSigmaProposalEvidenceSelectionError,
> {
    let raw_telemetry = telemetry_components
        .try_into_raw_telemetry()
        .map_err(DynamicSigmaProposalEvidenceSelectionError::InvalidTelemetryAssembly)?;

    select_dynamic_sigma_proposal_evidence_with_hysteresis(
        params,
        raw_telemetry,
        margins,
        hysteresis_policy,
        hysteresis_state,
    )
}

/// Validate proposal-carried dynamic-sigma evidence.
pub fn validate_dynamic_sigma_evidence(
    params: DynamicSigmaParameters,
    evidence: DynamicSigmaProposalEvidence,
) -> Result<DynamicSigmaDecision, DynamicSigmaEvidenceError> {
    validate_params(params).map_err(DynamicSigmaEvidenceError::InvalidTelemetry)?;

    if !sigma_is_in_ladder(params, evidence.selected_sigma) {
        return Err(DynamicSigmaEvidenceError::SelectedSigmaOutsideLadder {
            selected: evidence.selected_sigma,
        });
    }

    let window = evidence
        .raw_telemetry
        .into_window(evidence.margins)
        .map_err(DynamicSigmaEvidenceError::InvalidTelemetry)?;
    let decision = select_dynamic_sigma(params, window)
        .map_err(DynamicSigmaEvidenceError::InvalidTelemetry)?;

    if evidence.selected_sigma < decision.sigma {
        return Err(DynamicSigmaEvidenceError::SelectedSigmaBelowRequired {
            selected: evidence.selected_sigma,
            required: decision.sigma,
        });
    }

    Ok(decision)
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

/// Select dynamic sigma and apply hysteresis to the required floor.
///
/// Validators can still validate proposals by checking that the selected sigma
/// is at least the required floor. Hysteresis is a proposal-selection policy:
/// it may keep sigma higher than required while lower-risk windows stabilize.
pub fn select_dynamic_sigma_with_hysteresis(
    params: DynamicSigmaParameters,
    window: DynamicSigmaTelemetryWindow,
    policy: DynamicSigmaHysteresisParameters,
    state: DynamicSigmaHysteresisState,
) -> Result<DynamicSigmaHysteresisSelection, DynamicSigmaHysteresisSelectionError> {
    let required_decision = select_dynamic_sigma(params, window)
        .map_err(DynamicSigmaHysteresisSelectionError::InvalidTelemetry)?;
    let applied_state =
        apply_dynamic_sigma_hysteresis(params, policy, state, required_decision.sigma)
            .map_err(DynamicSigmaHysteresisSelectionError::InvalidHysteresis)?;

    Ok(DynamicSigmaHysteresisSelection {
        required_decision,
        applied_state,
    })
}

/// Apply hysteresis to a required dynamic-sigma floor.
///
/// Worse telemetry raises sigma immediately. Better telemetry must remain
/// stable for `decrease_confirmation_windows` before sigma steps down one
/// ladder level, preventing short-window oscillation from rapidly lowering the
/// confirmation depth.
pub fn apply_dynamic_sigma_hysteresis(
    params: DynamicSigmaParameters,
    policy: DynamicSigmaHysteresisParameters,
    state: DynamicSigmaHysteresisState,
    required_sigma: u64,
) -> Result<DynamicSigmaHysteresisState, DynamicSigmaHysteresisError> {
    validate_params(params).map_err(DynamicSigmaHysteresisError::InvalidParameters)?;

    if !sigma_is_in_ladder(params, state.current_sigma) {
        return Err(DynamicSigmaHysteresisError::CurrentSigmaOutsideLadder {
            current_sigma: state.current_sigma,
        });
    }

    if !sigma_is_in_ladder(params, required_sigma) {
        return Err(DynamicSigmaHysteresisError::RequiredSigmaOutsideLadder { required_sigma });
    }

    if required_sigma >= state.current_sigma {
        return Ok(DynamicSigmaHysteresisState {
            current_sigma: required_sigma,
            stable_windows_below_current: 0,
        });
    }

    let stable_windows_below_current = state.stable_windows_below_current.saturating_add(1);
    if stable_windows_below_current < policy.decrease_confirmation_windows {
        return Ok(DynamicSigmaHysteresisState {
            current_sigma: state.current_sigma,
            stable_windows_below_current,
        });
    }

    Ok(DynamicSigmaHysteresisState {
        current_sigma: next_lower_sigma(params, state.current_sigma).max(required_sigma),
        stable_windows_below_current: 0,
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

fn sigma_is_in_ladder(params: DynamicSigmaParameters, sigma: u64) -> bool {
    sigma == params.base_sigma || sigma == params.raised_sigma || sigma == params.max_sigma
}

fn next_lower_sigma(params: DynamicSigmaParameters, sigma: u64) -> u64 {
    if sigma > params.raised_sigma {
        params.raised_sigma
    } else {
        params.base_sigma
    }
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

fn rollback_depth_exceedance_risk_ppm(
    observed_rollback_depths: &[u64],
    sigma: u64,
    risk_margin_ppm: u64,
) -> u64 {
    let exceedance_count = observed_rollback_depths
        .iter()
        .filter(|rollback_depth| **rollback_depth >= sigma)
        .count();
    let raw_risk_ppm = ceil_mul_div(
        exceedance_count as u128,
        PPM_DENOMINATOR,
        observed_rollback_depths.len() as u128,
    );

    raw_risk_ppm
        .saturating_add(u128::from(risk_margin_ppm))
        .min(PPM_DENOMINATOR)
        .try_into()
        .expect("PPM risk is capped at the denominator")
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
    use chrono::Utc;
    use zebra_chain::serialization::{ZcashDeserialize, ZcashSerialize};
    use zebra_chain::{
        block::{merkle::Root, FatPointerToBftBlock, Header},
        fmt::HexDebug,
        work::{
            difficulty::{CompactDifficulty, INVALID_COMPACT_DIFFICULTY},
            equihash::Solution,
        },
    };

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

    fn hysteresis_policy() -> DynamicSigmaHysteresisParameters {
        DynamicSigmaHysteresisParameters {
            decrease_confirmation_windows: 2,
        }
    }

    fn hysteresis_state(
        current_sigma: u64,
        stable_windows_below_current: u64,
    ) -> DynamicSigmaHysteresisState {
        DynamicSigmaHysteresisState {
            current_sigma,
            stable_windows_below_current,
        }
    }

    fn evidence(
        participating_hash_work: u128,
        selected_sigma: u64,
    ) -> DynamicSigmaProposalEvidence {
        DynamicSigmaProposalEvidence {
            raw_telemetry: raw_telemetry(participating_hash_work, 0),
            margins: TelemetryEstimateMargins::default(),
            selected_sigma,
        }
    }

    fn economic_exposure_evidence(
        policy: DynamicSigmaEconomicExposurePolicy,
        selected_sigma: u64,
    ) -> DynamicSigmaProposalEvidence {
        let exposure = policy.to_units();
        let mut raw_telemetry = raw_telemetry(90, 0);
        raw_telemetry.rollback_risk = RollbackRiskCurve {
            base_sigma_ppm: 80,
            raised_sigma_ppm: 5,
            max_sigma_ppm: 1,
        };
        raw_telemetry.value_at_risk_units = exposure.value_at_risk_units;
        raw_telemetry.max_acceptable_expected_loss_units =
            exposure.max_acceptable_expected_loss_units;

        DynamicSigmaProposalEvidence {
            raw_telemetry,
            margins: TelemetryEstimateMargins::default(),
            selected_sigma,
        }
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

    fn low_risk_curve() -> RollbackRiskCurve {
        RollbackRiskCurve {
            base_sigma_ppm: 1,
            raised_sigma_ppm: 1,
            max_sigma_ppm: 1,
        }
    }

    fn production_telemetry_components() -> DynamicSigmaTelemetryComponents {
        DynamicSigmaTelemetryComponents {
            total_hash_work: Some(100),
            crosslink_participating_hash_work: Some(63),
            round_counters: DynamicSigmaRoundCounters {
                started_rounds: 10,
                failed_rounds: 2,
                nil_precommit_rounds: 1,
                stale_proposal_rounds: 1,
                timeout_rounds: 0,
                invalid_proposal_rounds: 0,
                mixed_evidence_rounds: 0,
                decided_rounds: 8,
            },
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

    fn decided_round_counters(rounds: u64) -> DynamicSigmaRoundCounters {
        DynamicSigmaRoundCounters {
            started_rounds: rounds,
            decided_rounds: rounds,
            ..DynamicSigmaRoundCounters::default()
        }
    }

    fn low_risk_components_from_hash_work(
        hash_work: DynamicSigmaHashWorkTelemetry,
    ) -> DynamicSigmaTelemetryComponents {
        DynamicSigmaTelemetryComponents {
            total_hash_work: Some(hash_work.total_hash_work),
            crosslink_participating_hash_work: Some(hash_work.crosslink_participating_hash_work),
            round_counters: decided_round_counters(10),
            measured_block_interval_variance_pct: 0,
            measured_observed_reorg_depth: 0,
            rollback_risk: RollbackRiskCurve {
                base_sigma_ppm: 1,
                raised_sigma_ppm: 1,
                max_sigma_ppm: 1,
            },
            value_at_risk_units: 1000,
            max_acceptable_expected_loss_units: 100,
        }
    }

    fn sigma_from_hash_work_observations(observations: &[DynamicSigmaHashWorkObservation]) -> u64 {
        let hash_work = observed_hash_work_participation(observations)
            .expect("hash-work observations should assemble");
        let raw = low_risk_components_from_hash_work(hash_work)
            .try_into_raw_telemetry()
            .expect("hash-work components should assemble into raw telemetry");
        let window = raw
            .into_window(TelemetryEstimateMargins::default())
            .expect("hash-work raw telemetry should build a window");

        select_dynamic_sigma(params(), window)
            .expect("hash-work telemetry should feed the controller")
            .sigma
    }

    fn telemetry_observation_window<'a>(
        hash_work_observations: &'a [DynamicSigmaHashWorkObservation],
        round_counters: DynamicSigmaRoundCounters,
        best_tip_transitions: &'a [DynamicSigmaBestTipTransition],
    ) -> DynamicSigmaTelemetryObservationWindow<'a> {
        DynamicSigmaTelemetryObservationWindow {
            hash_work_observations,
            round_counters,
            best_tip_transitions,
            measured_block_interval_variance_pct: 0,
            rollback_risk: RollbackRiskCurve {
                base_sigma_ppm: 1,
                raised_sigma_ppm: 1,
                max_sigma_ppm: 1,
            },
            value_at_risk_units: 1000,
            max_acceptable_expected_loss_units: 100,
        }
    }

    fn valid_header_difficulty() -> CompactDifficulty {
        CompactDifficulty::from_bytes_in_display_order(&0x207f_ffffu32.to_be_bytes())
            .expect("fixture difficulty should be valid")
    }

    fn participating_fat_pointer() -> FatPointerToBftBlock {
        let mut vote_for_block_without_finalizer_public_key = [0u8; 76 - 32];
        vote_for_block_without_finalizer_public_key[0] = 1;

        FatPointerToBftBlock {
            vote_for_block_without_finalizer_public_key,
            signatures: Vec::new(),
        }
    }

    fn header_with_fat_pointer(fat_pointer_to_bft_block: FatPointerToBftBlock) -> Header {
        Header {
            version: 4,
            previous_block_hash: zebra_chain::block::Hash([0; 32]),
            merkle_root: Root([0; 32]),
            commitment_bytes: HexDebug([0; 32]),
            time: Utc::now(),
            difficulty_threshold: valid_header_difficulty(),
            nonce: HexDebug([0; 32]),
            solution: Solution::for_proposal(),
            fat_pointer_to_bft_block,
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
    fn hysteresis_raises_sigma_immediately() {
        let next = apply_dynamic_sigma_hysteresis(
            params(),
            hysteresis_policy(),
            hysteresis_state(params().base_sigma, 7),
            params().max_sigma,
        )
        .expect("higher required sigma should update hysteresis state");

        assert_eq!(
            next,
            DynamicSigmaHysteresisState {
                current_sigma: params().max_sigma,
                stable_windows_below_current: 0,
            }
        );
    }

    #[test]
    fn hysteresis_lowers_sigma_one_step_after_stable_windows() {
        let held = apply_dynamic_sigma_hysteresis(
            params(),
            hysteresis_policy(),
            hysteresis_state(params().max_sigma, 0),
            params().base_sigma,
        )
        .expect("lower required sigma should be delayed");
        assert_eq!(
            held,
            DynamicSigmaHysteresisState {
                current_sigma: params().max_sigma,
                stable_windows_below_current: 1,
            }
        );

        let stepped_to_raised = apply_dynamic_sigma_hysteresis(
            params(),
            hysteresis_policy(),
            held,
            params().base_sigma,
        )
        .expect("stable lower evidence should step down by one ladder level");
        assert_eq!(
            stepped_to_raised,
            DynamicSigmaHysteresisState {
                current_sigma: params().raised_sigma,
                stable_windows_below_current: 0,
            }
        );

        let held_again = apply_dynamic_sigma_hysteresis(
            params(),
            hysteresis_policy(),
            stepped_to_raised,
            params().base_sigma,
        )
        .expect("next lower step should be delayed again");
        assert_eq!(
            held_again,
            DynamicSigmaHysteresisState {
                current_sigma: params().raised_sigma,
                stable_windows_below_current: 1,
            }
        );

        let stepped_to_base = apply_dynamic_sigma_hysteresis(
            params(),
            hysteresis_policy(),
            held_again,
            params().base_sigma,
        )
        .expect("second stable lower window should return to base");
        assert_eq!(
            stepped_to_base,
            DynamicSigmaHysteresisState {
                current_sigma: params().base_sigma,
                stable_windows_below_current: 0,
            }
        );
    }

    #[test]
    fn hysteresis_rejects_non_ladder_sigma_values() {
        assert_eq!(
            apply_dynamic_sigma_hysteresis(
                params(),
                hysteresis_policy(),
                hysteresis_state(2, 0),
                params().base_sigma,
            ),
            Err(DynamicSigmaHysteresisError::CurrentSigmaOutsideLadder { current_sigma: 2 }),
        );

        assert_eq!(
            apply_dynamic_sigma_hysteresis(
                params(),
                hysteresis_policy(),
                hysteresis_state(params().base_sigma, 0),
                2,
            ),
            Err(DynamicSigmaHysteresisError::RequiredSigmaOutsideLadder { required_sigma: 2 }),
        );
    }

    #[test]
    fn hysteresis_policy_zcash_serialization_round_trips() {
        let policy = DynamicSigmaHysteresisParameters {
            decrease_confirmation_windows: 42,
        };

        let encoded = policy
            .zcash_serialize_to_vec()
            .expect("hysteresis policy serialization should succeed");
        let decoded = DynamicSigmaHysteresisParameters::zcash_deserialize(encoded.as_slice())
            .expect("hysteresis policy deserialization should succeed");

        assert_eq!(encoded.len(), 8);
        assert_eq!(decoded, policy);
    }

    #[test]
    fn hysteresis_state_zcash_serialization_round_trips() {
        let state = DynamicSigmaHysteresisState {
            current_sigma: 6,
            stable_windows_below_current: 9,
        };

        let encoded = state
            .zcash_serialize_to_vec()
            .expect("hysteresis state serialization should succeed");
        let decoded = DynamicSigmaHysteresisState::zcash_deserialize(encoded.as_slice())
            .expect("hysteresis state deserialization should succeed");

        assert_eq!(encoded.len(), 16);
        assert_eq!(decoded, state);
    }

    #[test]
    fn hysteresis_selection_raises_immediately_on_low_participation() {
        let selection = select_dynamic_sigma_with_hysteresis(
            params(),
            window(45, 0, 55, 0, 0, 0, low_risk_curve(), 1000, 100),
            hysteresis_policy(),
            hysteresis_state(params().base_sigma, 3),
        )
        .expect("critical participation should produce a valid hysteresis selection");

        assert_eq!(selection.required_decision.sigma, params().max_sigma);
        assert_eq!(
            selection.applied_state,
            DynamicSigmaHysteresisState {
                current_sigma: params().max_sigma,
                stable_windows_below_current: 0,
            }
        );
    }

    #[test]
    fn hysteresis_selection_delays_lowering_selected_sigma() {
        let selection = select_dynamic_sigma_with_hysteresis(
            params(),
            window(100, 0, 0, 0, 0, 0, low_risk_curve(), 1000, 100),
            hysteresis_policy(),
            hysteresis_state(params().max_sigma, 0),
        )
        .expect("healthy participation should produce a valid hysteresis selection");

        assert_eq!(selection.required_decision.sigma, params().base_sigma);
        assert_eq!(
            selection.applied_state,
            DynamicSigmaHysteresisState {
                current_sigma: params().max_sigma,
                stable_windows_below_current: 1,
            }
        );
    }

    #[test]
    fn hash_work_observations_derive_participation_share() {
        let hash_work = observed_hash_work_participation(&[
            DynamicSigmaHashWorkObservation {
                hash_work: 40,
                participation: DynamicSigmaHashParticipation::VerifiedParticipating,
            },
            DynamicSigmaHashWorkObservation {
                hash_work: 20,
                participation: DynamicSigmaHashParticipation::NotVerifiedParticipating,
            },
            DynamicSigmaHashWorkObservation {
                hash_work: 30,
                participation: DynamicSigmaHashParticipation::VerifiedParticipating,
            },
            DynamicSigmaHashWorkObservation {
                hash_work: 10,
                participation: DynamicSigmaHashParticipation::NotVerifiedParticipating,
            },
        ])
        .expect("hash-work observations should assemble");

        assert_eq!(
            hash_work,
            DynamicSigmaHashWorkTelemetry {
                total_hash_work: 100,
                crosslink_participating_hash_work: 70,
            }
        );
    }

    #[test]
    fn hash_work_observations_feed_participation_sigma_floor() {
        let healthy_sigma = sigma_from_hash_work_observations(&[
            DynamicSigmaHashWorkObservation {
                hash_work: 70,
                participation: DynamicSigmaHashParticipation::VerifiedParticipating,
            },
            DynamicSigmaHashWorkObservation {
                hash_work: 30,
                participation: DynamicSigmaHashParticipation::NotVerifiedParticipating,
            },
        ]);
        let degraded_sigma = sigma_from_hash_work_observations(&[
            DynamicSigmaHashWorkObservation {
                hash_work: 60,
                participation: DynamicSigmaHashParticipation::VerifiedParticipating,
            },
            DynamicSigmaHashWorkObservation {
                hash_work: 40,
                participation: DynamicSigmaHashParticipation::NotVerifiedParticipating,
            },
        ]);
        let critical_sigma = sigma_from_hash_work_observations(&[
            DynamicSigmaHashWorkObservation {
                hash_work: 40,
                participation: DynamicSigmaHashParticipation::VerifiedParticipating,
            },
            DynamicSigmaHashWorkObservation {
                hash_work: 60,
                participation: DynamicSigmaHashParticipation::NotVerifiedParticipating,
            },
        ]);

        assert_eq!(healthy_sigma, params().base_sigma);
        assert_eq!(degraded_sigma, params().raised_sigma);
        assert_eq!(critical_sigma, params().max_sigma);
    }

    #[test]
    fn headers_derive_hash_work_participation_share() {
        let participating_header = header_with_fat_pointer(participating_fat_pointer());
        let non_participating_header = header_with_fat_pointer(FatPointerToBftBlock::null());
        let expected_header_work = valid_header_difficulty()
            .to_work()
            .expect("fixture difficulty should produce work")
            .as_u128();

        let observations =
            hash_work_observations_from_headers(&[participating_header, non_participating_header])
                .expect("valid headers should derive hash-work observations");
        let hash_work = observed_hash_work_participation(&observations)
            .expect("header observations should assemble");

        assert_eq!(hash_work.total_hash_work, expected_header_work * 2);
        assert_eq!(
            hash_work.crosslink_participating_hash_work,
            expected_header_work
        );
    }

    #[test]
    fn header_observation_rejects_invalid_difficulty() {
        let mut header = header_with_fat_pointer(participating_fat_pointer());
        header.difficulty_threshold = INVALID_COMPACT_DIFFICULTY;

        assert_eq!(
            hash_work_observation_from_header(&header),
            Err(DynamicSigmaHeaderObservationError::InvalidDifficultyThreshold),
        );
    }

    #[test]
    fn header_observation_uses_custom_participation_verifier() {
        let participating_header = header_with_fat_pointer(participating_fat_pointer());
        let null_header = header_with_fat_pointer(FatPointerToBftBlock::null());

        let rejected_observation = hash_work_observation_from_header_with_verifier(
            &participating_header,
            |_fat_pointer| false,
        )
        .expect("valid header work should assemble even when marker verification fails");
        let accepted_observation = hash_work_observation_from_header_with_verifier(
            &participating_header,
            |_fat_pointer| true,
        )
        .expect("valid header work should assemble when marker verification succeeds");
        let null_observation =
            hash_work_observation_from_header_with_verifier(&null_header, |_fat_pointer| true)
                .expect("valid null-marker header work should assemble");

        assert_eq!(
            rejected_observation.participation,
            DynamicSigmaHashParticipation::NotVerifiedParticipating,
        );
        assert_eq!(
            accepted_observation.participation,
            DynamicSigmaHashParticipation::VerifiedParticipating,
        );
        assert_eq!(
            null_observation.participation,
            DynamicSigmaHashParticipation::NotVerifiedParticipating,
        );
    }

    #[test]
    fn headers_feed_dynamic_sigma_hash_participation_floor() {
        let observations = hash_work_observations_from_headers(&[
            header_with_fat_pointer(participating_fat_pointer()),
            header_with_fat_pointer(FatPointerToBftBlock::null()),
            header_with_fat_pointer(FatPointerToBftBlock::null()),
        ])
        .expect("valid headers should derive hash-work observations");

        assert_eq!(
            sigma_from_hash_work_observations(&observations),
            params().max_sigma
        );
    }

    #[test]
    fn empty_hash_work_observations_are_rejected() {
        assert_eq!(
            observed_hash_work_participation(&[]),
            Err(DynamicSigmaHashWorkTelemetryError::EmptyObservationWindow),
        );
    }

    #[test]
    fn telemetry_observation_window_assembles_components_from_source_inputs() {
        let hash_work_observations = [
            DynamicSigmaHashWorkObservation {
                hash_work: 60,
                participation: DynamicSigmaHashParticipation::VerifiedParticipating,
            },
            DynamicSigmaHashWorkObservation {
                hash_work: 40,
                participation: DynamicSigmaHashParticipation::NotVerifiedParticipating,
            },
        ];
        let best_tip_transitions = [DynamicSigmaBestTipTransition {
            previous_tip_height: 105,
            new_tip_height: 107,
            common_ancestor_height: 103,
        }];

        let components =
            telemetry_components_from_observation_window(telemetry_observation_window(
                &hash_work_observations,
                decided_round_counters(10),
                &best_tip_transitions,
            ))
            .expect("source observations should assemble into telemetry components");

        assert_eq!(components.total_hash_work, Some(100));
        assert_eq!(components.crosslink_participating_hash_work, Some(60));
        assert_eq!(components.measured_observed_reorg_depth, 2);

        let raw = components
            .try_into_raw_telemetry()
            .expect("assembled source components should build raw telemetry");
        let decision = select_dynamic_sigma(
            params(),
            raw.into_window(TelemetryEstimateMargins::default())
                .expect("source raw telemetry should build a window"),
        )
        .expect("source telemetry should feed the controller");

        assert_eq!(decision.hash_participation_floor, params().raised_sigma);
        assert_eq!(decision.reorg_floor, params().raised_sigma);
        assert_eq!(decision.sigma, params().raised_sigma);
    }

    #[test]
    fn telemetry_observation_window_rejects_invalid_round_counters() {
        let hash_work_observations = [DynamicSigmaHashWorkObservation {
            hash_work: 100,
            participation: DynamicSigmaHashParticipation::VerifiedParticipating,
        }];
        let mut round_counters = decided_round_counters(2);
        round_counters.failed_rounds = 1;

        assert_eq!(
            telemetry_components_from_observation_window(telemetry_observation_window(
                &hash_work_observations,
                round_counters,
                &[],
            )),
            Err(DynamicSigmaTelemetryObservationError::InvalidRoundCounters(
                DynamicSigmaTelemetryAssemblyError::DecidedAndFailedRoundsExceedStarted,
            )),
        );
    }

    #[test]
    fn telemetry_observation_window_rejects_invalid_best_tip_transition() {
        let hash_work_observations = [DynamicSigmaHashWorkObservation {
            hash_work: 100,
            participation: DynamicSigmaHashParticipation::VerifiedParticipating,
        }];
        let best_tip_transitions = [DynamicSigmaBestTipTransition {
            previous_tip_height: 105,
            new_tip_height: 107,
            common_ancestor_height: 108,
        }];

        assert_eq!(
            telemetry_components_from_observation_window(telemetry_observation_window(
                &hash_work_observations,
                decided_round_counters(10),
                &best_tip_transitions,
            )),
            Err(
                DynamicSigmaTelemetryObservationError::InvalidRollbackTelemetry(
                    DynamicSigmaRollbackTelemetryError::CommonAncestorAbovePreviousTip,
                )
            ),
        );
    }

    #[test]
    fn header_observation_window_assembles_controller_components() {
        let headers = [
            header_with_fat_pointer(participating_fat_pointer()),
            header_with_fat_pointer(FatPointerToBftBlock::null()),
            header_with_fat_pointer(FatPointerToBftBlock::null()),
        ];
        let best_tip_transitions = [DynamicSigmaBestTipTransition {
            previous_tip_height: 10,
            new_tip_height: 12,
            common_ancestor_height: 9,
        }];

        let components = telemetry_components_from_header_observation_window(
            DynamicSigmaHeaderObservationWindow {
                pow_headers: &headers,
                round_counters: decided_round_counters(10),
                best_tip_transitions: &best_tip_transitions,
                measured_block_interval_variance_pct: 0,
                rollback_risk: low_risk_curve(),
                value_at_risk_units: 1000,
                max_acceptable_expected_loss_units: 100,
            },
        )
        .expect("valid header window should assemble telemetry components");
        let raw = components
            .try_into_raw_telemetry()
            .expect("header-derived components should build raw telemetry");
        let decision = select_dynamic_sigma(
            params(),
            raw.into_window(TelemetryEstimateMargins::default())
                .expect("header-derived raw telemetry should build a window"),
        )
        .expect("header-derived telemetry should feed the controller");

        assert_eq!(decision.hash_participation_floor, params().max_sigma);
        assert_eq!(decision.reorg_floor, params().raised_sigma);
        assert_eq!(decision.sigma, params().max_sigma);
    }

    #[test]
    fn header_observation_window_uses_custom_participation_verifier() {
        let headers = [
            header_with_fat_pointer(participating_fat_pointer()),
            header_with_fat_pointer(participating_fat_pointer()),
        ];

        let components = telemetry_components_from_header_observation_window_with_verifier(
            DynamicSigmaHeaderObservationWindow {
                pow_headers: &headers,
                round_counters: decided_round_counters(10),
                best_tip_transitions: &[],
                measured_block_interval_variance_pct: 0,
                rollback_risk: low_risk_curve(),
                value_at_risk_units: 1000,
                max_acceptable_expected_loss_units: 100,
            },
            |_fat_pointer| false,
        )
        .expect("valid header window should assemble even when verification rejects markers");

        assert_eq!(components.total_hash_work, Some(4));
        assert_eq!(components.crosslink_participating_hash_work, Some(0));
    }

    #[test]
    fn header_observation_window_rejects_invalid_header_difficulty() {
        let mut invalid_header = header_with_fat_pointer(participating_fat_pointer());
        invalid_header.difficulty_threshold = INVALID_COMPACT_DIFFICULTY;
        let headers = [invalid_header];

        assert_eq!(
            telemetry_components_from_header_observation_window(
                DynamicSigmaHeaderObservationWindow {
                    pow_headers: &headers,
                    round_counters: decided_round_counters(10),
                    best_tip_transitions: &[],
                    measured_block_interval_variance_pct: 0,
                    rollback_risk: low_risk_curve(),
                    value_at_risk_units: 1000,
                    max_acceptable_expected_loss_units: 100,
                },
            ),
            Err(DynamicSigmaHeaderObservationWindowError::InvalidHeader(
                DynamicSigmaHeaderObservationError::InvalidDifficultyThreshold,
            )),
        );
    }

    #[test]
    fn production_telemetry_requires_crosslink_participating_hash_work() {
        let mut components = production_telemetry_components();
        components.crosslink_participating_hash_work = None;

        assert_eq!(
            components.try_into_raw_telemetry(),
            Err(DynamicSigmaTelemetryAssemblyError::MissingCrosslinkParticipatingHashWork),
        );
    }

    #[test]
    fn production_telemetry_uses_round_counters_as_raw_failure_window() {
        let raw = production_telemetry_components()
            .try_into_raw_telemetry()
            .expect("complete production telemetry components should assemble");

        assert_eq!(raw.total_tenderlink_rounds, 10);
        assert_eq!(raw.failed_tenderlink_rounds, 2);
        assert_eq!(raw.crosslink_participating_hash_work, 63);

        let decision = select_dynamic_sigma(
            params(),
            raw.into_window(TelemetryEstimateMargins::default())
                .expect("assembled raw telemetry should build a window"),
        )
        .expect("assembled production telemetry should feed the controller");

        assert_eq!(decision.hash_participation_floor, 3);
        assert_eq!(decision.sigma, 3);
    }

    #[test]
    fn production_telemetry_rejects_inconsistent_round_counters() {
        let mut components = production_telemetry_components();
        components.round_counters.failed_rounds = 3;
        components.round_counters.decided_rounds = 8;

        assert_eq!(
            components.try_into_raw_telemetry(),
            Err(DynamicSigmaTelemetryAssemblyError::DecidedAndFailedRoundsExceedStarted),
        );
    }

    #[test]
    fn round_counter_events_derive_failed_reason_counters() {
        let mut counters = DynamicSigmaRoundCounters::default();

        counters.record_event(DynamicSigmaRoundEvent::StartedRound);
        counters.record_event(DynamicSigmaRoundEvent::StaleProposal);
        counters.record_event(DynamicSigmaRoundEvent::StartedRound);
        counters.record_event(DynamicSigmaRoundEvent::NilPrecommitRecovery);
        counters.record_event(DynamicSigmaRoundEvent::StartedRound);
        counters.record_event(DynamicSigmaRoundEvent::Timeout);
        counters.record_event(DynamicSigmaRoundEvent::StartedRound);
        counters.record_event(DynamicSigmaRoundEvent::Decided);

        assert_eq!(
            counters,
            DynamicSigmaRoundCounters {
                started_rounds: 4,
                failed_rounds: 3,
                nil_precommit_rounds: 1,
                stale_proposal_rounds: 1,
                timeout_rounds: 1,
                invalid_proposal_rounds: 0,
                mixed_evidence_rounds: 0,
                decided_rounds: 1,
            }
        );
        assert_eq!(counters.validate(), Ok(()));
    }

    #[test]
    fn production_telemetry_rejects_failed_reason_overcount() {
        let mut components = production_telemetry_components();
        components.round_counters.failed_rounds = 1;
        components.round_counters.nil_precommit_rounds = 1;
        components.round_counters.stale_proposal_rounds = 1;

        assert_eq!(
            components.try_into_raw_telemetry(),
            Err(DynamicSigmaTelemetryAssemblyError::FailureReasonCountersExceedFailed),
        );
    }

    #[test]
    fn best_tip_transitions_report_max_observed_rollback_depth() {
        let transitions = [
            DynamicSigmaBestTipTransition {
                previous_tip_height: 105,
                new_tip_height: 106,
                common_ancestor_height: 103,
            },
            DynamicSigmaBestTipTransition {
                previous_tip_height: 106,
                new_tip_height: 109,
                common_ancestor_height: 106,
            },
        ];

        assert_eq!(max_observed_rollback_depth(&transitions), Ok(2));
    }

    #[test]
    fn best_tip_rollback_depth_feeds_dynamic_sigma_reorg_floor() {
        let reorg_depth = max_observed_rollback_depth(&[DynamicSigmaBestTipTransition {
            previous_tip_height: 105,
            new_tip_height: 106,
            common_ancestor_height: 103,
        }])
        .expect("valid best-tip transition should derive rollback depth");

        let decision = decide(window(
            90,
            0,
            10,
            0,
            0,
            reorg_depth,
            RollbackRiskCurve {
                base_sigma_ppm: 80,
                raised_sigma_ppm: 20,
                max_sigma_ppm: 2,
            },
            1000,
            100,
        ));

        assert_eq!(decision.reorg_floor, 3);
        assert_eq!(decision.sigma, 3);
    }

    #[test]
    fn rollback_risk_estimator_derives_monotone_curve_from_depth_windows() {
        let curve = rollback_risk_curve_from_observed_rollback_depths(params(), &[0, 1, 4, 6], 0)
            .expect("rollback-depth windows should produce a risk curve");

        assert_eq!(
            curve,
            RollbackRiskCurve {
                base_sigma_ppm: 750_000,
                raised_sigma_ppm: 500_000,
                max_sigma_ppm: 250_000,
            }
        );
    }

    #[test]
    fn rollback_risk_estimator_adds_conservative_margin() {
        let curve = rollback_risk_curve_from_observed_rollback_depths(params(), &[0, 0, 0, 0], 25)
            .expect("rollback-depth windows should produce a risk curve");

        assert_eq!(
            curve,
            RollbackRiskCurve {
                base_sigma_ppm: 25,
                raised_sigma_ppm: 25,
                max_sigma_ppm: 25,
            }
        );
    }

    #[test]
    fn rollback_risk_estimator_rejects_empty_windows() {
        assert_eq!(
            rollback_risk_curve_from_observed_rollback_depths(params(), &[], 0),
            Err(DynamicSigmaRollbackRiskEstimatorError::EmptyObservationWindow),
        );
    }

    #[test]
    fn rollback_risk_estimator_rejects_impossible_margin() {
        assert_eq!(
            rollback_risk_curve_from_observed_rollback_depths(params(), &[0], 1_000_001),
            Err(DynamicSigmaRollbackRiskEstimatorError::RiskMarginTooLarge {
                margin_ppm: 1_000_001,
            }),
        );
    }

    #[test]
    fn rollback_risk_estimator_feeds_dynamic_sigma_economic_floor() {
        let curve = rollback_risk_curve_from_observed_rollback_depths(params(), &[0, 1, 4, 6], 0)
            .expect("rollback-depth windows should produce a risk curve");
        let decision = decide(window(90, 0, 10, 0, 0, 0, curve, 1_000, 100));

        assert_eq!(decision.economic_floor, params().max_sigma);
        assert_eq!(decision.sigma, params().max_sigma);
        assert_eq!(
            decision.economic_target_status,
            EconomicTargetStatus::TargetUnreachableAtMax,
        );
    }

    #[test]
    fn best_tip_transition_rejects_impossible_common_ancestor() {
        let transition = DynamicSigmaBestTipTransition {
            previous_tip_height: 105,
            new_tip_height: 106,
            common_ancestor_height: 107,
        };

        assert_eq!(
            transition.rollback_depth(),
            Err(DynamicSigmaRollbackTelemetryError::CommonAncestorAbovePreviousTip),
        );
    }

    #[test]
    fn proposal_evidence_accepts_required_sigma() {
        let decision = validate_dynamic_sigma_evidence(params(), evidence(63, 3))
            .expect("selected sigma should satisfy evidence");

        assert_eq!(decision.sigma, 3);
        assert_eq!(decision.hash_participation_floor, 3);
    }

    #[test]
    fn proposal_evidence_accepts_more_conservative_ladder_sigma() {
        let decision = validate_dynamic_sigma_evidence(params(), evidence(63, 6))
            .expect("higher ladder sigma should satisfy evidence");

        assert_eq!(decision.sigma, 3);
    }

    #[test]
    fn proposal_evidence_is_deterministic_for_identical_inputs() {
        let proposal_evidence = evidence(63, 3);

        assert_eq!(
            validate_dynamic_sigma_evidence(params(), proposal_evidence),
            validate_dynamic_sigma_evidence(params(), proposal_evidence),
        );
    }

    #[test]
    fn proposal_evidence_rejects_sigma_below_required_floor() {
        assert_eq!(
            validate_dynamic_sigma_evidence(params(), evidence(63, 1)),
            Err(DynamicSigmaEvidenceError::SelectedSigmaBelowRequired {
                selected: 1,
                required: 3,
            })
        );
    }

    #[test]
    fn proposal_evidence_accepts_service_local_economic_exposure_at_base_sigma() {
        let decision = validate_dynamic_sigma_evidence(
            params(),
            economic_exposure_evidence(DynamicSigmaEconomicExposurePolicy::ServiceLocal, 1),
        )
        .expect("service-local exposure should not raise consensus sigma");

        assert_eq!(decision.economic_floor, 1);
        assert_eq!(decision.sigma, 1);
    }

    #[test]
    fn proposal_evidence_rejects_sigma_below_consensus_critical_economic_floor() {
        assert_eq!(
            validate_dynamic_sigma_evidence(
                params(),
                economic_exposure_evidence(
                    DynamicSigmaEconomicExposurePolicy::ConsensusCritical {
                        value_at_risk_units: 100_000,
                        max_acceptable_expected_loss_units: 1,
                    },
                    1,
                ),
            ),
            Err(DynamicSigmaEvidenceError::SelectedSigmaBelowRequired {
                selected: 1,
                required: 3,
            })
        );
    }

    #[test]
    fn proposal_evidence_accepts_consensus_critical_economic_floor() {
        let decision = validate_dynamic_sigma_evidence(
            params(),
            economic_exposure_evidence(
                DynamicSigmaEconomicExposurePolicy::ConsensusCritical {
                    value_at_risk_units: 100_000,
                    max_acceptable_expected_loss_units: 1,
                },
                3,
            ),
        )
        .expect("selected sigma should satisfy the consensus-critical economic floor");

        assert_eq!(decision.economic_floor, 3);
        assert_eq!(decision.sigma, 3);
    }

    #[test]
    fn proposal_evidence_selection_from_raw_telemetry_selects_required_sigma() {
        let proposal_evidence = select_dynamic_sigma_proposal_evidence(
            params(),
            raw_telemetry(63, 0),
            TelemetryEstimateMargins::default(),
        )
        .expect("raw telemetry should select proposal evidence");

        assert_eq!(proposal_evidence.selected_sigma, params().raised_sigma);
    }

    #[test]
    fn proposal_evidence_selection_from_components_rejects_missing_participation() {
        let mut components = production_telemetry_components();
        components.crosslink_participating_hash_work = None;

        assert_eq!(
            select_dynamic_sigma_proposal_evidence_from_components(
                params(),
                components,
                TelemetryEstimateMargins::default(),
            ),
            Err(
                DynamicSigmaProposalEvidenceSelectionError::InvalidTelemetryAssembly(
                    DynamicSigmaTelemetryAssemblyError::MissingCrosslinkParticipatingHashWork,
                )
            ),
        );
    }

    #[test]
    fn proposal_evidence_selection_with_hysteresis_returns_applied_state() {
        let (proposal_evidence, next_state) =
            select_dynamic_sigma_proposal_evidence_with_hysteresis(
                params(),
                raw_telemetry(90, 0),
                TelemetryEstimateMargins::default(),
                hysteresis_policy(),
                DynamicSigmaHysteresisState {
                    current_sigma: params().max_sigma,
                    stable_windows_below_current: 0,
                },
            )
            .expect("hysteresis should select proposal evidence");

        assert_eq!(proposal_evidence.selected_sigma, params().max_sigma);
        assert_eq!(
            next_state,
            DynamicSigmaHysteresisState {
                current_sigma: params().max_sigma,
                stable_windows_below_current: 1,
            }
        );
    }

    #[test]
    fn proposal_evidence_rejects_sigma_outside_ladder() {
        assert_eq!(
            validate_dynamic_sigma_evidence(params(), evidence(63, 4)),
            Err(DynamicSigmaEvidenceError::SelectedSigmaOutsideLadder { selected: 4 })
        );
    }

    #[test]
    fn proposal_evidence_zcash_serialization_round_trips() {
        let proposal_evidence = DynamicSigmaProposalEvidence {
            raw_telemetry: DynamicSigmaRawTelemetry {
                total_hash_work: u128::from(u64::MAX) + 1,
                crosslink_participating_hash_work: 12_345,
                total_tenderlink_rounds: 987,
                failed_tenderlink_rounds: 65,
                measured_block_interval_variance_pct: 7,
                measured_observed_reorg_depth: 4,
                rollback_risk: RollbackRiskCurve {
                    base_sigma_ppm: 1_000,
                    raised_sigma_ppm: 100,
                    max_sigma_ppm: 10,
                },
                value_at_risk_units: 1_000_000_000_000,
                max_acceptable_expected_loss_units: 42,
            },
            margins: TelemetryEstimateMargins {
                coverage_risk_margin_pct: 3,
                round_failure_margin_pct: 5,
            },
            selected_sigma: 6,
        };

        let encoded = proposal_evidence
            .zcash_serialize_to_vec()
            .expect("evidence serialization should succeed");
        let decoded = DynamicSigmaProposalEvidence::zcash_deserialize(encoded.as_slice())
            .expect("evidence deserialization should succeed");

        assert_eq!(encoded.len(), 123);
        assert_eq!(decoded, proposal_evidence);
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
    fn service_local_economic_exposure_does_not_raise_consensus_sigma() {
        let exposure = DynamicSigmaEconomicExposurePolicy::ServiceLocal.to_units();
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
            exposure.value_at_risk_units,
            exposure.max_acceptable_expected_loss_units,
        ));

        assert_eq!(decision.economic_floor, 1);
        assert_eq!(decision.sigma, 1);
        assert_eq!(
            decision.economic_target_status,
            EconomicTargetStatus::TargetSatisfied
        );
    }

    #[test]
    fn consensus_critical_economic_exposure_can_raise_consensus_sigma() {
        let exposure = DynamicSigmaEconomicExposurePolicy::ConsensusCritical {
            value_at_risk_units: 100_000,
            max_acceptable_expected_loss_units: 1,
        }
        .to_units();
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
            exposure.value_at_risk_units,
            exposure.max_acceptable_expected_loss_units,
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
