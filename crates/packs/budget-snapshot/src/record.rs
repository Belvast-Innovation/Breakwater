//! Sampling the resources of handler calls.
//!
//! The host meters every top-level invocation separately: it clears the CPU
//! and memory counters, the events and the storage footprint when one starts.
//! `env.cost_estimate().resources()` and the host's
//! `get_last_invocation_resources` report only the *last* one, so a reading is
//! valid only if nothing else was invoked since the call it is meant for.
//! Invariants run after the handler and often read the contract through a
//! client, which is an invocation too. That is why the reading is taken inside
//! the handler, by [`Metered`], right after the call returns.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

use breakwater::proptest::prelude::BoxedStrategy;
use breakwater::{
    Actor, CallContext, CallOutcome, Handler, HandlerError, Invariant, RunReport, SuiteConfig,
    TxLog, Violation,
};
use soroban_sdk::xdr::{ScAddress, ScSymbol};
use soroban_sdk::{Address, Env};

use crate::digest::Fnv1a64;
use crate::report::{BudgetError, BudgetReport};
use crate::resource::Sample;
use crate::snapshot::{Mode, SuiteSnapshot};
use crate::stats::HandlerStats;

/// The contract function a handler is supposed to measure.
#[derive(Clone, Debug)]
pub struct Target {
    contract: String,
    function: &'static str,
}

impl Target {
    /// The function `function` of the contract at `contract` (a strkey).
    pub fn new(contract: impl Into<String>, function: &'static str) -> Target {
        Target {
            contract: contract.into(),
            function,
        }
    }

    /// True if `invocation`, the `Debug` form of the host's description of the
    /// last top-level invocation, is a call of this contract function.
    ///
    /// `MeteringInvocation` is not reachable through `soroban-sdk`, so the
    /// comparison is on its derived `Debug` text, which is built from the same
    /// XDR types used here. A unit test pins the format for the locked
    /// `soroban-env-host`.
    fn matches(&self, env: &Env, invocation: &str) -> bool {
        let Ok(function) = ScSymbol::try_from(self.function) else {
            return false;
        };
        let contract = ScAddress::from(&Address::from_str(env, &self.contract));
        invocation == format!("InvokeContract({contract:?}, {function:?})")
    }
}

#[derive(Default)]
struct State {
    samples: BTreeMap<String, Vec<Sample>>,
    skipped: BTreeMap<String, u64>,
    unattributed: BTreeMap<String, u64>,
    digest: Fnv1a64,
}

/// Collects the samples of every [`Metered`] handler of one suite run.
///
/// Create one, wrap each handler with [`Recorder::handler`], add
/// [`Recorder::invariant`], run the suite, then call [`Recorder::finish`].
#[derive(Clone, Default)]
pub struct Recorder {
    state: Arc<Mutex<State>>,
}

impl Recorder {
    /// An empty recorder.
    pub fn new() -> Recorder {
        Recorder::default()
    }

    /// Wraps `handler` so that its calls are measured against `target`.
    pub fn handler<H: Handler>(&self, handler: H, target: Target) -> Metered<H> {
        Metered {
            inner: handler,
            target,
            recorder: self.clone(),
        }
    }

    /// The invariant that lets a suite consist of measuring handlers only.
    pub fn invariant(&self) -> RecordOnly {
        RecordOnly
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        match self.state.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    /// Turns what was recorded into a report.
    ///
    /// Fails if the run ended on a violation, if any measured call was not
    /// the handler's last top-level invocation, or if nothing was measured.
    /// `config` must be the configuration the suite ran with.
    pub fn finish(
        &self,
        config: &SuiteConfig,
        report: &RunReport,
    ) -> Result<BudgetReport, BudgetError> {
        if let Some(failure) = report.failure() {
            return Err(BudgetError::Violated {
                source_name: failure.violation.source.clone(),
                message: failure.violation.message.clone(),
            });
        }
        let state = self.lock();
        if !state.unattributed.is_empty() {
            let handlers = state
                .unattributed
                .iter()
                .map(|(name, n)| format!("`{name}` ({n})"))
                .collect::<Vec<_>>()
                .join(", ");
            return Err(BudgetError::Unattributed { handlers });
        }
        let handlers: BTreeMap<String, HandlerStats> = state
            .samples
            .iter()
            .filter_map(|(name, samples)| {
                HandlerStats::from_samples(samples).map(|stats| (name.clone(), stats))
            })
            .collect();
        if handlers.is_empty() {
            return Err(BudgetError::NoSamples);
        }
        Ok(BudgetReport {
            suite: report.suite.clone(),
            snapshot: SuiteSnapshot {
                seed: config.seed,
                cases: config.cases,
                steps: format!("{}..={}", config.steps.start(), config.steps.end()),
                auth: match config.auth {
                    breakwater::AuthMode::MockAll => "mock",
                    breakwater::AuthMode::Real => "real",
                }
                .to_owned(),
                mode: Mode::Native,
                workload_digest: state.digest.to_tagged_hex(),
                handlers,
            },
            skipped: state.skipped.clone(),
        })
    }

    fn observe(
        &self,
        env: &Env,
        target: &Target,
        handler: &str,
        actor: &Actor,
        args: &str,
        outcome: &CallOutcome,
    ) {
        let mut state = self.lock();
        // The workload digest covers every call, whatever its outcome, so it
        // identifies what was generated and not what the contract made of it.
        state.digest.field(handler.as_bytes());
        state.digest.field(actor.name().as_bytes());
        state.digest.field(args.as_bytes());
        state
            .digest
            .field(env.ledger().sequence().to_string().as_bytes());

        if !outcome.is_success() {
            *state.skipped.entry(handler.to_owned()).or_default() += 1;
            return;
        }
        let host = env.host();
        let attributed = host
            .get_detailed_last_invocation_resources()
            .is_some_and(|last| target.matches(env, &format!("{:?}", last.invocation)));
        let Some(res) = host.get_last_invocation_resources().filter(|_| attributed) else {
            *state.unattributed.entry(handler.to_owned()).or_default() += 1;
            return;
        };
        let sample = Sample {
            instructions: res.instructions,
            mem_bytes: res.mem_bytes,
            disk_read_entries: i64::from(res.disk_read_entries),
            memory_read_entries: i64::from(res.memory_read_entries),
            write_entries: i64::from(res.write_entries),
            disk_read_bytes: i64::from(res.disk_read_bytes),
            write_bytes: i64::from(res.write_bytes),
            contract_events_size_bytes: i64::from(res.contract_events_size_bytes),
            persistent_rent_ledger_bytes: res.persistent_rent_ledger_bytes,
            persistent_entry_rent_bumps: i64::from(res.persistent_entry_rent_bumps),
            temporary_rent_ledger_bytes: res.temporary_rent_ledger_bytes,
            temporary_entry_rent_bumps: i64::from(res.temporary_entry_rent_bumps),
        };
        state
            .samples
            .entry(handler.to_owned())
            .or_default()
            .push(sample);
    }
}

/// A handler whose calls are measured. Create it with [`Recorder::handler`].
///
/// It behaves exactly like the handler it wraps. After the wrapped `call`
/// returns it reads the host's metering of the last top-level invocation and,
/// if the call succeeded and that invocation is the declared [`Target`],
/// records it. The target call must therefore be the last top-level call the
/// handler makes: set-up calls such as minting go before it.
pub struct Metered<H> {
    inner: H,
    target: Target,
    recorder: Recorder,
}

impl<H: Handler> Handler for Metered<H> {
    type Args = H::Args;

    fn name(&self) -> &'static str {
        self.inner.name()
    }

    fn actors(&self) -> &[Actor] {
        self.inner.actors()
    }

    fn strategy(&self) -> BoxedStrategy<Self::Args> {
        self.inner.strategy()
    }

    fn args_from_json(&self, value: &serde_json::Value) -> Option<Self::Args> {
        self.inner.args_from_json(value)
    }

    fn call(
        &self,
        env: &Env,
        ctx: &CallContext,
        actor: &Actor,
        args: &Self::Args,
    ) -> Result<CallOutcome, HandlerError> {
        let outcome = self.inner.call(env, ctx, actor, args)?;
        let args_json = serde_json::to_string(args).unwrap_or_else(|_| format!("{args:?}"));
        self.recorder.observe(
            env,
            &self.target,
            self.inner.name(),
            actor,
            &args_json,
            &outcome,
        );
        Ok(outcome)
    }
}

/// An invariant that never fires.
///
/// A suite needs at least one invariant, and a measuring suite has nothing to
/// assert: the numbers are the result.
pub struct RecordOnly;

impl Invariant for RecordOnly {
    fn name(&self) -> &'static str {
        "budget_snapshot_recording"
    }

    fn check(&self, _env: &Env, _log: &TxLog) -> Result<(), Violation> {
        Ok(())
    }
}
