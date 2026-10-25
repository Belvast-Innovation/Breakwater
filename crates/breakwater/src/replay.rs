//! Replaying a recorded sequence step by step.

use serde::{Deserialize, Serialize};

use crate::error::SuiteError;
use crate::log::{Gap, StepRecord, TxLog};

/// One step of a recorded sequence, by name rather than by index, so it can
/// be stored in a file and replayed against a freshly built suite.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayStep {
    /// Handler name.
    pub handler: String,
    /// Actor name (must be one of the handler's actors).
    pub actor: String,
    /// Arguments in their logged JSON form.
    pub args: serde_json::Value,
    /// Ledger advance before the call.
    pub gap: Gap,
}

impl ReplayStep {
    /// The replay steps of a log, in order.
    pub fn from_log(log: &TxLog) -> Vec<ReplayStep> {
        log.steps().iter().map(ReplayStep::from).collect()
    }
}

impl From<&StepRecord> for ReplayStep {
    fn from(record: &StepRecord) -> Self {
        ReplayStep {
            handler: record.handler.clone(),
            actor: record.actor.name().to_owned(),
            args: record.args.clone(),
            gap: record.gap,
        }
    }
}

/// Why a recorded step could not be mapped onto the suite.
#[derive(Debug, thiserror::Error)]
pub enum ReplayError {
    /// No handler with this name.
    #[error("step {step}: unknown handler `{handler}`")]
    UnknownHandler {
        /// Step index.
        step: usize,
        /// Handler name.
        handler: String,
    },
    /// The handler has no actor with this name.
    #[error("step {step}: handler `{handler}` has no actor `{actor}`")]
    UnknownActor {
        /// Step index.
        step: usize,
        /// Handler name.
        handler: String,
        /// Actor name.
        actor: String,
    },
    /// The arguments could not be rebuilt from JSON.
    #[error("step {step}: arguments of `{handler}` do not parse: {args}")]
    BadArgs {
        /// Step index.
        step: usize,
        /// Handler name.
        handler: String,
        /// The JSON that failed.
        args: serde_json::Value,
    },
}

impl From<ReplayError> for SuiteError {
    fn from(e: ReplayError) -> Self {
        SuiteError::Replay(e.to_string())
    }
}
