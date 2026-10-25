//! Invariants: properties that must hold after every step.

use serde::{Deserialize, Serialize};
use soroban_sdk::Env;

use crate::log::TxLog;

/// A property checked after every step of every generated sequence.
///
/// Invariants read contract state through the normal contract client or
/// `env.as_contract(..)`, and may consult the [`TxLog`] of steps executed so
/// far. They run outside any contract frame.
pub trait Invariant: Send + Sync + 'static {
    /// Name used in reports; should be stable because it appears in exports.
    fn name(&self) -> &'static str;

    /// Returns `Err` when the property is violated.
    fn check(&self, env: &Env, log: &TxLog) -> Result<(), Violation>;
}

/// Why a run failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViolationKind {
    /// An [`Invariant`] returned `Err`.
    Invariant,
    /// A handler call panicked; contracts must use `panic_with_error!`.
    Panic,
    /// A handler returned a [`crate::HandlerError`].
    HandlerError,
}

/// A violated invariant, or a step that failed in a way that counts as a bug.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Violation {
    /// What kind of failure this is.
    pub kind: ViolationKind,
    /// Name of the invariant (or handler, for panics and handler errors).
    pub source: String,
    /// Human-readable description.
    pub message: String,
    /// Optional structured detail, for example the offending value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<serde_json::Value>,
}

impl Violation {
    /// Creates an invariant violation. The suite fills in `source` with the
    /// invariant's name.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            kind: ViolationKind::Invariant,
            source: String::new(),
            message: message.into(),
            detail: None,
        }
    }

    /// Attaches a serialisable detail value.
    pub fn with_detail(mut self, detail: impl Serialize) -> Self {
        self.detail = serde_json::to_value(detail).ok();
        self
    }

    pub(crate) fn panic(handler: &str, message: String) -> Self {
        Self {
            kind: ViolationKind::Panic,
            source: handler.to_owned(),
            message,
            detail: None,
        }
    }

    pub(crate) fn handler_error(handler: &str, message: String) -> Self {
        Self {
            kind: ViolationKind::HandlerError,
            source: handler.to_owned(),
            message,
            detail: None,
        }
    }
}
