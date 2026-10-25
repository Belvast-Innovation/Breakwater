//! Executes a sequence of steps against a fresh `Env` and checks invariants.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use soroban_sdk::testutils::EnvTestConfig;
use soroban_sdk::Env;

use crate::config::{AuthMode, SuiteConfig};
use crate::handler::{CallOutcome, DynHandler};
use crate::invariant::{Invariant, Violation};
use crate::ledger::LedgerAdvance;
use crate::log::{StepOutcome, StepRecord, TxLog};
use crate::step::Step;

/// Hook run on every restored `Env`, used to re-register native test
/// contracts at their original addresses.
pub(crate) type RestoreHook = Box<dyn Fn(&Env) + Send + Sync>;

/// Outcome of executing one sequence.
pub(crate) struct Execution {
    pub log: TxLog,
    /// Index of the failing step and the violation, if any.
    pub failure: Option<(usize, Violation)>,
}

pub(crate) struct Runner {
    /// The post-setup `Env`; a ledger snapshot of it is taken for every run.
    /// `soroban_sdk` does not export the snapshot type, so the `Env` is kept
    /// instead of a cached snapshot.
    base: Env,
    handlers: Arc<[Box<dyn DynHandler>]>,
    invariants: Arc<[Box<dyn Invariant>]>,
    restore_hooks: Arc<[RestoreHook]>,
    config: SuiteConfig,
}

impl Runner {
    pub(crate) fn new(
        base: Env,
        handlers: Arc<[Box<dyn DynHandler>]>,
        invariants: Arc<[Box<dyn Invariant>]>,
        restore_hooks: Arc<[RestoreHook]>,
        config: SuiteConfig,
    ) -> Self {
        Self {
            base,
            handlers,
            invariants,
            restore_hooks,
            config,
        }
    }

    /// Restores the post-setup `Env` from the snapshot.
    fn fresh_env(&self) -> Env {
        let mut env = Env::from_ledger_snapshot(self.base.to_ledger_snapshot());
        env.set_config(EnvTestConfig {
            capture_snapshot_at_drop: false,
        });
        match self.config.auth {
            AuthMode::MockAll => env.mock_all_auths(),
            // Validated away in `Suite::run`; kept explicit for when `Real` lands.
            AuthMode::Real => {}
        }
        if self.config.unlimited_budget {
            env.cost_estimate().budget().reset_unlimited();
        }
        for hook in self.restore_hooks.iter() {
            hook(&env);
        }
        env
    }

    /// Runs `steps` from a fresh `Env`, checking every invariant after every
    /// step. Stops at the first violation.
    ///
    /// `seen` is proptest-state-machine's counter of applied transitions; it
    /// must be incremented before each step so that shrinking can drop the
    /// steps that were never reached.
    pub(crate) fn execute(&self, steps: &[Step], seen: Option<&AtomicUsize>) -> Execution {
        let env = self.fresh_env();
        let mut log = TxLog::default();

        for (index, step) in steps.iter().enumerate() {
            if let Some(seen) = seen {
                seen.fetch_add(1, Ordering::SeqCst);
            }

            LedgerAdvance::apply(&env, &step.gap);
            let handler = &self.handlers[step.handler];
            let actor = handler.actors()[step.actor].clone();

            let outcome =
                match catch_unwind(AssertUnwindSafe(|| handler.call(&env, &actor, &step.args))) {
                    Ok(Ok(CallOutcome::Success)) => StepOutcome::Success,
                    Ok(Ok(CallOutcome::ContractError(code))) => StepOutcome::ContractError(code),
                    Ok(Err(err)) => StepOutcome::HandlerError(err.to_string()),
                    Err(payload) => StepOutcome::Panic(panic_message(payload.as_ref())),
                };

            log.push(StepRecord {
                index,
                handler: handler.name().to_owned(),
                actor,
                args: step.args.to_json(),
                gap: step.gap,
                ledger_sequence: env.ledger().sequence(),
                timestamp: env.ledger().timestamp(),
                outcome: outcome.clone(),
            });

            let step_failure = match outcome {
                StepOutcome::Panic(message) => Some(Violation::panic(handler.name(), message)),
                StepOutcome::HandlerError(message) => {
                    Some(Violation::handler_error(handler.name(), message))
                }
                StepOutcome::Success | StepOutcome::ContractError(_) => None,
            };
            if let Some(violation) = step_failure {
                return Execution {
                    log,
                    failure: Some((index, violation)),
                };
            }

            for invariant in self.invariants.iter() {
                if let Err(mut violation) = invariant.check(&env, &log) {
                    violation.source = invariant.name().to_owned();
                    return Execution {
                        log,
                        failure: Some((index, violation)),
                    };
                }
            }
        }

        Execution { log, failure: None }
    }
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_owned()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "panic with non-string payload".to_owned()
    }
}
