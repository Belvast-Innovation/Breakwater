//! Breakwater core library.
//!
//! Stateful property testing for Soroban contracts. A suite declares *actors*
//! (who may call), *handlers* (what may be called) and *invariants* (what must
//! always hold). Breakwater generates randomized call sequences against an
//! in-process [`soroban_sdk::Env`], checks every invariant after every step,
//! shrinks any failing sequence to a minimal reproduction and exports it.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod actor;
mod config;
mod error;
mod handler;
mod invariant;
mod ledger;
mod log;
mod runner;
mod step;
mod suite;

pub use actor::Actor;
pub use config::{AuthMode, SuiteConfig};
pub use error::SuiteError;
pub use handler::{CallOutcome, Handler, HandlerError, StepArgs};
pub use invariant::{Invariant, Violation, ViolationKind};
pub use ledger::LedgerAdvance;
pub use log::{Gap, StepOutcome, StepRecord, TxLog};
pub use step::Step;
pub use suite::{Failure, Outcome, RunReport, Suite};

/// Re-export so suites can name proptest strategies without a direct dependency.
pub use proptest;
