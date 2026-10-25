//! Actors: named signers that handlers may call as.

use serde::{Deserialize, Serialize};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, Env};

/// A named signer with a Soroban address.
///
/// Soroban [`Address`] values are bound to the [`Env`] that created them. A
/// suite restores a fresh `Env` from a ledger snapshot for every generated
/// sequence, so an actor stores the address as a strkey string and
/// materialises an `Address` for whichever `Env` is current via
/// [`Actor::address`].
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Actor {
    name: String,
    strkey: String,
}

impl Actor {
    /// Creates an actor with a freshly generated address.
    pub fn generate(env: &Env, name: impl Into<String>) -> Self {
        Self::from_address(name, &Address::generate(env))
    }

    /// Wraps an existing address, for example a smart-account signer.
    pub fn from_address(name: impl Into<String>, address: &Address) -> Self {
        Self {
            name: name.into(),
            strkey: address.to_string().to_string(),
        }
    }

    /// Human-readable name used in logs and exports.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The address as a strkey string (`G...` or `C...`).
    pub fn strkey(&self) -> &str {
        &self.strkey
    }

    /// The address bound to `env`.
    pub fn address(&self, env: &Env) -> Address {
        Address::from_str(env, &self.strkey)
    }
}
