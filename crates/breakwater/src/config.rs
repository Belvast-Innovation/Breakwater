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
    /// No mocking: handlers install real authorization entries with
    /// `env.set_auths(..)`, so custom accounts' `__check_auth` and their
    /// policies really run. See [`crate::auth`].
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

impl SuiteConfig {
    /// Overrides `default` with the environment the CLI sets:
    /// `BREAKWATER_SEED` (u64), `BREAKWATER_STEPS` (`A..=B`) and
    /// `BREAKWATER_AUTH` (`mock` | `real`). Unset or malformed variables leave
    /// the default in place, so suites behave normally under plain `cargo test`.
    pub fn from_env(default: SuiteConfig) -> SuiteConfig {
        let mut config = default;
        if let Some(seed) = std::env::var("BREAKWATER_SEED")
            .ok()
            .and_then(|v| v.trim().parse().ok())
        {
            config.seed = seed;
        }
        if let Some(steps) = std::env::var("BREAKWATER_STEPS")
            .ok()
            .and_then(|v| parse_steps(&v))
        {
            config.steps = steps;
        }
        if let Ok(auth) = std::env::var("BREAKWATER_AUTH") {
            match auth.trim() {
                "mock" => config.auth = AuthMode::MockAll,
                "real" => config.auth = AuthMode::Real,
                _ => {}
            }
        }
        config
    }
}

/// Parses `A..=B` (or a single `N`, meaning `N..=N`).
pub fn parse_steps(text: &str) -> Option<RangeInclusive<usize>> {
    let text = text.trim();
    if let Some((a, b)) = text.split_once("..=") {
        let a: usize = a.trim().parse().ok()?;
        let b: usize = b.trim().parse().ok()?;
        (a <= b && a > 0).then_some(a..=b)
    } else {
        let n: usize = text.parse().ok()?;
        (n > 0).then_some(n..=n)
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_step_ranges() {
        assert_eq!(parse_steps("1..=20"), Some(1..=20));
        assert_eq!(parse_steps(" 5 "), Some(5..=5));
        assert_eq!(parse_steps("0..=3"), None);
        assert_eq!(parse_steps("4..=2"), None);
        assert_eq!(parse_steps("abc"), None);
    }
}
