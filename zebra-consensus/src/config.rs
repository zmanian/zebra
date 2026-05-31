//! Configuration for semantic verification which is run in parallel.

use serde::{Deserialize, Serialize};

/// Configuration for parallel semantic verification:
/// <https://zebra.zfnd.org/dev/rfcs/0002-parallel-verification.html#definitions>
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(
    deny_unknown_fields,
    default,
    from = "InnerConfig",
    into = "InnerConfig"
)]
pub struct Config {
    /// Should Zebra make sure that it follows the consensus chain while syncing?
    /// This is a developer-only option.
    ///
    /// # Security
    ///
    /// Disabling this option leaves your node vulnerable to some kinds of chain-based attacks.
    /// Zebra regularly updates its checkpoints to ensure nodes are following the best chain.
    ///
    /// # Details
    ///
    /// This option is `true` by default, because it prevents some kinds of chain attacks.
    ///
    /// Disabling this option makes Zebra start full validation earlier.
    /// It is slower and less secure.
    ///
    /// Zebra requires some checkpoints to simplify validation of legacy network upgrades.
    /// Required checkpoints are always active, even when this option is `false`.
    ///
    /// # Deprecation
    ///
    /// For security reasons, this option might be deprecated or ignored in a future Zebra
    /// release.
    pub checkpoint_sync: bool,

    /// Experimental Halo2 verification acceleration settings.
    ///
    /// These settings are disabled by default. When enabled in a build that supports
    /// accelerated Pasta operations, Zebra can use them to select and gate the
    /// accelerated verifier path.
    pub halo2_accel: Halo2AccelConfig,
}

impl From<InnerConfig> for Config {
    fn from(
        InnerConfig {
            halo2_accel,
            checkpoint_sync,
            ..
        }: InnerConfig,
    ) -> Self {
        Self {
            checkpoint_sync,
            halo2_accel,
        }
    }
}

impl From<Config> for InnerConfig {
    fn from(
        Config {
            checkpoint_sync,
            halo2_accel,
        }: Config,
    ) -> Self {
        Self {
            checkpoint_sync,
            halo2_accel,
            _debug_skip_parameter_preload: false,
        }
    }
}

/// Experimental Halo2 verifier acceleration settings.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Halo2AccelConfig {
    /// Enable experimental Halo2 verifier acceleration.
    pub enabled: bool,

    /// Backend selection for accelerated Pasta operations.
    pub backend: Halo2AccelBackend,

    /// Safety mode used when comparing accelerated and CPU verification paths.
    pub mode: Halo2AccelMode,

    /// Minimum Orchard action batch size before trying acceleration.
    pub min_batch_actions: usize,

    /// Minimum MSM size forwarded to the Pasta acceleration backend.
    pub min_msm_size: usize,
}

/// Backend selection for experimental Halo2 verifier acceleration.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Halo2AccelBackend {
    /// Choose the best available backend at runtime.
    #[default]
    Auto,

    /// Use CUDA acceleration when available.
    Cuda,

    /// Use AVX512 IFMA acceleration when available.
    Avx512,
}

/// Runtime safety mode for experimental Halo2 verifier acceleration.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Halo2AccelMode {
    /// Keep the current CPU verifier behavior only.
    #[default]
    Cpu,

    /// Run accelerated and CPU paths and require matching results before accepting.
    Crosscheck,

    /// Accept accelerated results directly.
    ExperimentalAccept,
}

impl Halo2AccelConfig {
    /// Returns true when a Halo2 Orchard action batch should be considered for acceleration.
    pub fn should_try_accel(&self, action_count: usize) -> bool {
        self.enabled && self.mode != Halo2AccelMode::Cpu && action_count >= self.min_batch_actions
    }
}

impl Halo2AccelBackend {
    /// Stable label for metrics and traces.
    pub fn as_metric_label(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Cuda => "cuda",
            Self::Avx512 => "avx512",
        }
    }
}

impl Halo2AccelMode {
    /// Stable label for metrics and traces.
    pub fn as_metric_label(&self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Crosscheck => "crosscheck",
            Self::ExperimentalAccept => "experimental-accept",
        }
    }
}

/// Inner consensus configuration for backwards compatibility with older `zebrad.toml` files,
/// which contain fields that have been removed.
///
/// Rust API callers should use [`Config`].
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct InnerConfig {
    /// See [`Config`] for more details.
    pub checkpoint_sync: bool,

    /// See [`Config`] for more details.
    pub halo2_accel: Halo2AccelConfig,

    #[serde(skip_serializing, rename = "debug_skip_parameter_preload")]
    /// Unused config field for backwards compatibility.
    pub _debug_skip_parameter_preload: bool,
}

// we like our default configs to be explicit
#[allow(unknown_lints)]
#[allow(clippy::derivable_impls)]
impl Default for Config {
    fn default() -> Self {
        Self {
            checkpoint_sync: true,
            halo2_accel: Halo2AccelConfig::default(),
        }
    }
}

impl Default for InnerConfig {
    fn default() -> Self {
        Self {
            checkpoint_sync: Config::default().checkpoint_sync,
            halo2_accel: Config::default().halo2_accel,
            _debug_skip_parameter_preload: false,
        }
    }
}

impl Default for Halo2AccelConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            backend: Halo2AccelBackend::default(),
            mode: Halo2AccelMode::default(),
            min_batch_actions: 64,
            min_msm_size: 4096,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Config, Halo2AccelBackend, Halo2AccelMode};

    #[test]
    fn halo2_accel_defaults_are_safe() {
        let config = Config::default();

        assert!(!config.halo2_accel.enabled);
        assert_eq!(config.halo2_accel.backend, Halo2AccelBackend::Auto);
        assert_eq!(config.halo2_accel.mode, Halo2AccelMode::Cpu);
        assert_eq!(config.halo2_accel.min_batch_actions, 64);
        assert_eq!(config.halo2_accel.min_msm_size, 4096);
    }

    #[test]
    fn halo2_accel_cpu_mode_parses_from_toml() {
        let config: Config = toml::from_str(
            r#"
            [halo2_accel]
            enabled = true
            mode = "cpu"
            "#,
        )
        .expect("valid halo2 accel cpu-mode config");

        assert!(config.halo2_accel.enabled);
        assert_eq!(config.halo2_accel.mode, Halo2AccelMode::Cpu);
        assert_eq!(config.halo2_accel.mode.as_metric_label(), "cpu");
        assert!(!config.halo2_accel.should_try_accel(usize::MAX));
    }

    #[test]
    fn halo2_accel_config_parses_from_toml() {
        let config: Config = toml::from_str(
            r#"
            checkpoint_sync = false

            [halo2_accel]
            enabled = true
            backend = "avx512"
            mode = "experimental-accept"
            min_batch_actions = 128
            min_msm_size = 8192
            "#,
        )
        .expect("valid halo2 accel config");

        assert!(!config.checkpoint_sync);
        assert!(config.halo2_accel.enabled);
        assert_eq!(config.halo2_accel.backend, Halo2AccelBackend::Avx512);
        assert_eq!(config.halo2_accel.mode, Halo2AccelMode::ExperimentalAccept);
        assert_eq!(config.halo2_accel.min_batch_actions, 128);
        assert_eq!(config.halo2_accel.min_msm_size, 8192);
    }

    #[test]
    fn halo2_accel_config_gates_candidate_batches() {
        let mut accel = Config::default().halo2_accel;
        accel.enabled = true;
        accel.min_batch_actions = 64;

        assert!(!accel.should_try_accel(63));
        accel.mode = Halo2AccelMode::Crosscheck;
        assert!(accel.should_try_accel(64));
        accel.mode = Halo2AccelMode::Cpu;
        assert!(!accel.should_try_accel(usize::MAX));
        assert_eq!(accel.backend.as_metric_label(), "auto");
        assert_eq!(
            Halo2AccelMode::ExperimentalAccept.as_metric_label(),
            "experimental-accept"
        );
    }
}
