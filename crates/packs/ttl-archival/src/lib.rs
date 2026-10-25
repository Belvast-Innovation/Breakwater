//! `ttl-archival` attack pack.
//!
//! Finds contract state that expires under Soroban's TTL-based state
//! archival while the business logic still depends on it. It provides:
//!
//! - [`ttl`]: side-effect-free TTL probes. The `testutils` `get_ttl` getters
//!   (and any contract read) auto-restore an archived persistent or instance
//!   entry in `soroban-sdk` 28, which would hide the very condition under
//!   test; these probes read `live_until` from the ledger entries instead.
//! - [`StreamModel`]: derives the set of live streams from the `TxLog`, so
//!   invariants never have to read contract state.
//! - Invariants: [`NoMidLifecycleExpiry`], [`InstanceAlive`],
//!   [`RestoreRestoresFunction`], [`NoUnboundedTtlExtension`],
//!   [`RestoreCostWithinBudget`].
//! - The usage profile that makes the pack bite lives in the core:
//!   `LedgerAdvance::daily_then_silence`.
//!
//! See `docs/ARCHITECTURE.md` §5.
#![deny(unsafe_code)]
#![warn(missing_docs)]

mod invariants;
mod streams;
pub mod ttl;

pub use invariants::{
    InstanceAlive, NoMidLifecycleExpiry, NoUnboundedTtlExtension, ResourceBudget,
    RestoreCostWithinBudget, RestoreHook, RestoreRestoresFunction, StreamKey, StreamProbe,
};
pub use streams::{LiveStream, StreamModel};
