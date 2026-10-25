//! Demo target for the `policy-bypass` attack pack.
//!
//! The real contract (an OpenZeppelin smart account with an agent signer and a
//! deliberately weak spending policy) lands with the pack. This scaffold only
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

/// Placeholder contract; replaced by the smart-account demo in a later change.
#[contract]
pub struct DemoWallet;

#[contractimpl]
impl DemoWallet {
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
