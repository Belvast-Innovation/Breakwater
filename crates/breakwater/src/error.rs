//! Error type for suite construction and execution.

use thiserror::Error;

/// Errors returned by [`crate::Suite::run`] and the exporter.
#[derive(Debug, Error)]
pub enum SuiteError {
    /// The suite has no handlers, so there is nothing to generate.
    #[error("suite `{0}` has no handlers")]
    NoHandlers(String),
    /// A handler declared no actors, so no step can be built for it.
    #[error("handler `{0}` declares no actors")]
    HandlerWithoutActors(String),
    /// The suite has no invariants, so no run could ever fail.
    #[error("suite `{0}` has no invariants")]
    NoInvariants(String),
    /// The configured step range is empty.
    #[error("step range is empty")]
    EmptySteps,
    /// A recorded step could not be mapped onto the suite's handlers.
    #[error("replay failed: {0}")]
    Replay(String),
    /// proptest aborted the run, for example after too many rejected cases.
    #[error("fuzzing aborted: {0}")]
    Aborted(String),
    /// Writing an export file failed.
    #[error("export failed: {0}")]
    Io(#[from] std::io::Error),
    /// Serialising a report failed.
    #[error("serialisation failed: {0}")]
    Json(#[from] serde_json::Error),
}
