//! Demo target for the `ttl-archival` attack pack.
//!
//! The real contract (a payment stream that writes to persistent and instance
//! storage but never calls `extend_ttl`) lands with the pack. This scaffold only
//! establishes the crate layout and the error-handling convention: contract
//! errors are declared with `contracterror` and raised with `panic_with_error!`,
//! never with a bare `panic!`, because Breakwater reports a bare panic as a bug.
#![no_std]

use soroban_sdk::{contract, contracterror, contractimpl, panic_with_error, Env};

/// Errors raised by the demo contract.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    /// The supplied value was zero.
    ZeroValue = 1,
}

/// Placeholder contract; replaced by the payment-stream demo in a later change.
#[contract]
pub struct PaymentStream;

#[contractimpl]
impl PaymentStream {
    /// Returns `value` unchanged, or fails with [`Error::ZeroValue`] when it is zero.
    pub fn ping(env: Env, value: u32) -> u32 {
        if value == 0 {
            panic_with_error!(&env, Error::ZeroValue);
        }
        value
    }
}

#[cfg(test)]
mod test;
