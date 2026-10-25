//! The transaction log: what a sequence did so far.

use serde::{Deserialize, Serialize};

use crate::actor::Actor;

/// How far the ledger moved before a step.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gap {
    /// Ledgers added to `sequence_number`.
    pub ledgers: u32,
    /// Seconds added to `timestamp`.
    pub seconds: u64,
}

impl Gap {
    /// A gap of `ledgers` ledgers with the timestamp moving at Stellar's
    /// approximate close time of five seconds per ledger.
    pub fn ledgers(ledgers: u32) -> Self {
        Self {
            ledgers,
            seconds: u64::from(ledgers) * crate::ledger::SECONDS_PER_LEDGER,
        }
    }
}

/// What happened when a step was executed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum StepOutcome {
    /// The call succeeded.
    Success,
    /// The contract rejected the call with a `contracterror` code.
    ContractError(u32),
    /// The host rejected the call's authorization.
    Unauthorized,
    /// The handler returned an error that is not a contract error.
    HandlerError(String),
    /// The call panicked.
    Panic(String),
}

impl StepOutcome {
    /// True for [`StepOutcome::Success`].
    pub fn is_success(&self) -> bool {
        matches!(self, StepOutcome::Success)
    }
}

/// One executed step.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepRecord {
    /// Zero-based position in the sequence.
    pub index: usize,
    /// Handler name.
    pub handler: String,
    /// Actor that performed the call.
    pub actor: Actor,
    /// Arguments as JSON.
    pub args: serde_json::Value,
    /// Ledger advance applied before the call.
    pub gap: Gap,
    /// Ledger sequence number at the time of the call.
    pub ledger_sequence: u32,
    /// Ledger timestamp at the time of the call.
    pub timestamp: u64,
    /// Result of the call.
    pub outcome: StepOutcome,
}

/// The steps executed so far in the current sequence, oldest first.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TxLog {
    steps: Vec<StepRecord>,
}

impl TxLog {
    /// All steps, oldest first.
    pub fn steps(&self) -> &[StepRecord] {
        &self.steps
    }

    /// Number of steps executed.
    pub fn len(&self) -> usize {
        self.steps.len()
    }

    /// True when no step has run yet.
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// The most recent step.
    pub fn last(&self) -> Option<&StepRecord> {
        self.steps.last()
    }

    /// Steps whose call succeeded.
    pub fn successes(&self) -> impl Iterator<Item = &StepRecord> {
        self.steps.iter().filter(|s| s.outcome.is_success())
    }

    /// Successful steps of the named handler.
    pub fn successes_of<'a>(
        &'a self,
        handler: &'a str,
    ) -> impl Iterator<Item = &'a StepRecord> + 'a {
        self.successes().filter(move |s| s.handler == handler)
    }

    /// Builds a log from existing records, for example when replaying an
    /// exported sequence or testing an invariant against a known-bad history.
    pub fn from_records(steps: Vec<StepRecord>) -> Self {
        Self { steps }
    }

    pub(crate) fn push(&mut self, record: StepRecord) {
        self.steps.push(record);
    }
}
