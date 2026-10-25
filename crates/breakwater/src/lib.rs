//! Breakwater core library.
//!
//! Stateful property testing for Soroban contracts. A suite declares *actors*
//! (who may call), *handlers* (what may be called) and *invariants* (what must
//! always hold). Breakwater generates randomized call sequences against an
//! in-process [`soroban_sdk::Env`], checks every invariant after every step,
//! shrinks any failing sequence and exports it as a reproducible unit test.
//!
//! This crate currently contains only the workspace scaffold. The `Suite`,
//! `Handler`, `Invariant` and `Violation` types follow in the core API change.
#![forbid(unsafe_code)]
#![warn(missing_docs)]
