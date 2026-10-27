//! `budget-snapshot` attack pack.
//!
//! A `forge snapshot` for Soroban: measure the resources every handler of a
//! suite consumes, store the statistics in a file that is committed next to
//! the code, and fail CI when a later run exceeds them.
//!
//! - [`Resource`] and [`Sample`]: the twelve per-invocation measurements the
//!   host reports, and one reading of them. [`Resource::class`] sorts them
//!   into [`ResourceClass::Storage`] and [`ResourceClass::Compute`].
//! - [`HandlerStats`]: `max` and `p50` of every resource over the samples of
//!   one handler, plus the sample count.
//! - [`Snapshot`]: the `.breakwater-snapshot.json` file model, keyed by suite
//!   and handler, with the toolchain it was taken with.
//! - [`Recorder`], [`Metered`] and [`Target`]: wrap a suite's handlers so that
//!   every successful call is sampled right after it returns, and turn the
//!   samples into a [`BudgetReport`].
//! - [`digest::fnv1a64`]: the digest that identifies the generated workload.
//! - [`compare()`]: a fresh measurement against a stored suite, within a
//!   [`Tolerance`]. [`compare_tiered`] does the same with a [`Tolerances`],
//!   which can differ per [`ResourceClass`] or per resource.
//!
//! See `docs/ARCHITECTURE.md` §5.
#![deny(unsafe_code)]
#![warn(missing_docs)]

mod compare;
pub mod digest;
mod record;
mod report;
mod resource;
mod snapshot;
mod stats;

pub use compare::{
    compare, compare_tiered, Comparison, Finding, SampleChange, StatKind, Tolerance,
    ToleranceError, ToleranceSource, Tolerances, TolerancesError, Verdict,
};
pub use record::{Metered, RecordOnly, Recorder, Target};
pub use report::{BudgetError, BudgetReport, REPORT_ENV};
pub use resource::{Resource, ResourceClass, Sample};
pub use snapshot::{
    Mode, Snapshot, SnapshotError, SuiteSnapshot, Toolchain, FORMAT_VERSION, SNAPSHOT_FILE,
};
pub use stats::{HandlerStats, Stat};
