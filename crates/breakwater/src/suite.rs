//! The suite: declare actors, handlers and invariants; run; get a report.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use proptest::test_runner::{Config, RngAlgorithm, TestCaseError, TestError, TestRng, TestRunner};
use serde::{Deserialize, Serialize};
use soroban_sdk::Env;

use crate::config::{AuthMode, SuiteConfig};
use crate::error::SuiteError;
use crate::handler::{DynHandler, Handler};
use crate::invariant::{Invariant, Violation};
use crate::ledger::LedgerAdvance;
use crate::log::TxLog;
use crate::runner::{RestoreHook, Runner};
use crate::step::{sequence_strategy, Step};

/// A stateful property test for a set of Soroban contracts.
///
/// Build it from the `Env` in which the contracts under test have already
/// been deployed and configured. [`Suite::run`] snapshots that `Env` once and
/// restores a fresh copy for every generated sequence and every shrink
/// candidate.
///
/// Native Rust contracts registered with `env.register(..)` live in the host,
/// not in the ledger, so they do not survive a snapshot restore. Re-register
/// them at the same address from an [`Suite::on_restore`] hook.
pub struct Suite {
    name: String,
    env: Env,
    config: SuiteConfig,
    advance: LedgerAdvance,
    handlers: Vec<Box<dyn DynHandler>>,
    invariants: Vec<Box<dyn Invariant>>,
    restore_hooks: Vec<RestoreHook>,
}

impl Suite {
    /// Creates a suite named `name` over the post-setup `env`.
    pub fn new(name: impl Into<String>, env: Env) -> Self {
        Self {
            name: name.into(),
            env,
            config: SuiteConfig::default(),
            advance: LedgerAdvance::default(),
            handlers: Vec::new(),
            invariants: Vec::new(),
            restore_hooks: Vec::new(),
        }
    }

    /// Replaces the whole configuration.
    pub fn config(mut self, config: SuiteConfig) -> Self {
        self.config = config;
        self
    }

    /// Sets the generation seed.
    pub fn seed(mut self, seed: u64) -> Self {
        self.config.seed = seed;
        self
    }

    /// Sets the inclusive range of steps per sequence.
    pub fn steps(mut self, steps: std::ops::RangeInclusive<usize>) -> Self {
        self.config.steps = steps;
        self
    }

    /// Sets how many sequences to try before declaring the invariants held.
    pub fn cases(mut self, cases: u32) -> Self {
        self.config.cases = cases;
        self
    }

    /// Sets how the ledger moves between steps.
    pub fn ledger_advance(mut self, advance: LedgerAdvance) -> Self {
        self.advance = advance;
        self
    }

    /// Adds a handler.
    pub fn handler(mut self, handler: impl Handler) -> Self {
        self.handlers.push(Box::new(handler));
        self
    }

    /// Adds an invariant.
    pub fn invariant(mut self, invariant: impl Invariant) -> Self {
        self.invariants.push(Box::new(invariant));
        self
    }

    /// Registers a hook that runs on every restored `Env`, before any step.
    pub fn on_restore(mut self, hook: impl Fn(&Env) + Send + Sync + 'static) -> Self {
        self.restore_hooks.push(Box::new(hook));
        self
    }

    /// Generates sequences, checks invariants after every step, and shrinks
    /// the first failing sequence to a minimal reproduction.
    pub fn run(self) -> Result<RunReport, SuiteError> {
        self.validate()?;

        let Suite {
            name,
            env,
            config,
            advance,
            handlers,
            invariants,
            restore_hooks,
        } = self;

        let handlers: Arc<[Box<dyn DynHandler>]> = handlers.into();
        let invariants: Arc<[Box<dyn Invariant>]> = invariants.into();
        let restore_hooks: Arc<[RestoreHook]> = restore_hooks.into();

        let runner = Runner::new(
            env,
            handlers.clone(),
            invariants,
            restore_hooks,
            config.clone(),
        );

        let strategy = sequence_strategy(handlers, advance, config.steps.clone());
        let proptest_config = Config {
            cases: config.cases,
            max_shrink_iters: config.max_shrink_iters,
            failure_persistence: None,
            source_file: None,
            ..Config::default()
        };
        let rng = TestRng::from_seed(RngAlgorithm::ChaCha, &seed_bytes(config.seed));
        let mut test_runner = TestRunner::new_with_rng(proptest_config, rng);

        let cases_run = AtomicU32::new(0);
        let result = test_runner.run(&strategy, |(_, steps, seen)| {
            cases_run.fetch_add(1, Ordering::Relaxed);
            let execution = runner.execute(&steps, seen.as_deref());
            match execution.failure {
                None => Ok(()),
                Some((index, violation)) => Err(TestCaseError::fail(format!(
                    "step {index}: {} ({})",
                    violation.message, violation.source
                ))),
            }
        });

        let outcome = match result {
            Ok(()) => Outcome::Held,
            Err(TestError::Fail(_, (_, steps, _))) => {
                let execution = runner.execute(&steps, None);
                let (failing_step, violation) = execution.failure.unwrap_or_else(|| {
                    (
                        steps.len().saturating_sub(1),
                        Violation::new("shrunk sequence did not reproduce the failure on replay"),
                    )
                });
                Outcome::Violated(Failure {
                    violation,
                    failing_step,
                    log: execution.log,
                    steps,
                })
            }
            Err(TestError::Abort(reason)) => return Err(SuiteError::Aborted(reason.to_string())),
        };

        Ok(RunReport {
            suite: name,
            seed: config.seed,
            cases_run: cases_run.into_inner(),
            outcome,
        })
    }

    fn validate(&self) -> Result<(), SuiteError> {
        if self.handlers.is_empty() {
            return Err(SuiteError::NoHandlers(self.name.clone()));
        }
        if let Some(h) = self.handlers.iter().find(|h| h.actors().is_empty()) {
            return Err(SuiteError::HandlerWithoutActors(h.name().to_owned()));
        }
        if self.invariants.is_empty() {
            return Err(SuiteError::NoInvariants(self.name.clone()));
        }
        if self.config.steps.is_empty() {
            return Err(SuiteError::EmptySteps);
        }
        if self.config.auth != AuthMode::MockAll {
            return Err(SuiteError::UnsupportedAuthMode(self.config.auth));
        }
        Ok(())
    }
}

/// Expands a `u64` seed into the 32 bytes the ChaCha generator needs.
fn seed_bytes(seed: u64) -> [u8; 32] {
    let mut out = [0u8; 32];
    for i in 0..4u64 {
        let start = (i as usize) * 8;
        out[start..start + 8].copy_from_slice(&seed.wrapping_add(i).to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_bytes_is_deterministic_and_distinct() {
        assert_eq!(seed_bytes(42), seed_bytes(42));
        assert_ne!(seed_bytes(42), seed_bytes(43));
        assert_eq!(&seed_bytes(1)[..8], &1u64.to_le_bytes());
    }
}

/// Result of [`Suite::run`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunReport {
    /// Suite name.
    pub suite: String,
    /// Seed that produced this run; pass it back to reproduce.
    pub seed: u64,
    /// Sequences executed, including shrink candidates.
    pub cases_run: u32,
    /// Whether the invariants held.
    pub outcome: Outcome,
}

impl RunReport {
    /// The failure, if the run found one.
    pub fn failure(&self) -> Option<&Failure> {
        match &self.outcome {
            Outcome::Held => None,
            Outcome::Violated(f) => Some(f),
        }
    }
}

/// Whether all invariants held.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Outcome {
    /// Every generated sequence satisfied every invariant.
    Held,
    /// A sequence violated an invariant; the enclosed failure is shrunk.
    Violated(Failure),
}

/// A shrunk failing sequence.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Failure {
    /// What was violated.
    pub violation: Violation,
    /// Zero-based index of the step after which the violation was detected.
    pub failing_step: usize,
    /// The executed steps, with outcomes.
    pub log: TxLog,
    /// The generated steps, for in-process replay.
    #[serde(skip)]
    pub steps: Vec<Step>,
}
