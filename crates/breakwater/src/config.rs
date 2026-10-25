//! Suite configuration.

use std::ops::RangeInclusive;

use serde::{Deserialize, Serialize};

/// How authorization is handled while executing steps.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMode {
    /// `env.mock_all_auths()`: every `require_auth` passes. Use when
    /// authorization is not what is under test.
    MockAll,
    /// Build real authorization trees so custom accounts' `__check_auth`
    /// runs. Not implemented yet; lands with the `policy-bypass` pack.
    Real,
}

/// Knobs for one [`crate::Suite`] run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SuiteConfig {
    /// Seed for sequence generation. The same seed, suite and contracts
    /// reproduce the same run.
    pub seed: u64,
    /// Inclusive range of steps per generated sequence.
    pub steps: RangeInclusive<usize>,
    /// Number of sequences to generate before declaring the invariants held.
    pub cases: u32,
    /// Authorization mode.
    pub auth: AuthMode,
    /// Upper bound on shrink iterations once a failure is found.
    pub max_shrink_iters: u32,
    /// Lift the CPU and memory budget of the test `Env` so long sequences do
    /// not hit resource limits.
    pub unlimited_budget: bool,
}

impl Default for SuiteConfig {
    fn default() -> Self {
        Self {
            seed: 0,
            steps: 1..=20,
            cases: 64,
            auth: AuthMode::MockAll,
            max_shrink_iters: 2048,
            unlimited_budget: true,
        }
    }
}
