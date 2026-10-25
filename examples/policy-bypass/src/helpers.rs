//! Auxiliary contracts deployed next to [`crate::DemoAccount`] by the suite.
//!
//! They live in their own crates under `contracts/` because one cdylib cannot
//! export two `__check_auth` functions; this module only re-exports them for
//! the tests.

pub use example_ed25519_verifier::{Ed25519Verifier, Ed25519VerifierClient};
pub use example_owner_account::{OwnerAccount, OwnerAccountClient, OwnerAccountError};
pub use example_spending_limit_policy::{SpendingLimitPolicy, SpendingLimitPolicyClient};
