//! `policy-bypass` attack pack.
//!
//! Hunts for multi-step sequences that slip past OpenZeppelin smart-account
//! policies (spending limits, context rules, signer scopes). It provides:
//!
//! - [`SignerKey`] and [`auth::authorize`]: real authorization entries for an
//!   OpenZeppelin `SmartAccount`, so a suite can run in
//!   [`breakwater::AuthMode::Real`] and the account's `__check_auth` and its
//!   policies genuinely execute.
//! - Invariants: [`RollingCap`], [`AllowlistOnly`], [`RevokedNeverAuths`],
//!   [`PinnedWasm`].
//! - Generators biased toward known bypass shapes: many payments just under a
//!   cap and ledger gaps that straddle a policy window.
//!
//! See `docs/ARCHITECTURE.md` §5.
#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod auth;
mod generators;
mod invariants;
mod signer;

pub use generators::{amount_near_cap, window_straddling_advance};
pub use invariants::{AllowlistOnly, PinnedWasm, RevokedNeverAuths, RollingCap};
pub use signer::{ed25519_public_key, SignerKey};
