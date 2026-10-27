//! `budget-snapshot` attack pack.
//!
//! A `forge snapshot` for Soroban: measure the resources every handler of a
//! suite consumes, store the statistics in a file that is committed next to
//! the code, and fail CI when a later run exceeds them.
//!
//! - [`Resource`] and [`Sample`]: the twelve per-invocation measurements the
//!   host reports, and one reading of them.
//! - [`HandlerStats`]: `max` and `p50` of every resource over the samples of
//!   one handler, plus the sample count.
//! - [`Snapshot`]: the `.breakwater-snapshot.json` file model, keyed by suite
//!   and handler, with the toolchain it was taken with.
//! - [`fnv1a64`]: the digest that identifies the generated workload.
//!
//! See `docs/ARCHITECTURE.md` §5.
#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod digest;
mod resource;
mod snapshot;
mod stats;

pub use resource::{Resource, Sample};
pub use snapshot::{
    Mode, Snapshot, SnapshotError, SuiteSnapshot, Toolchain, FORMAT_VERSION, SNAPSHOT_FILE,
};
pub use stats::{HandlerStats, Stat};
