//! Signer keys for OpenZeppelin smart accounts, usable across restored `Env`s.

use ed25519_dalek::{Signer as _, SigningKey};
use serde::{Deserialize, Serialize};
use soroban_sdk::{Address, Bytes, Env};
use stellar_accounts::smart_account::Signer;

/// Ed25519 public key for a 32-byte seed.
pub fn ed25519_public_key(seed: &[u8; 32]) -> [u8; 32] {
    SigningKey::from_bytes(seed).verifying_key().to_bytes()
}

pub(crate) fn sign(seed: &[u8; 32], message: &[u8]) -> [u8; 64] {
    SigningKey::from_bytes(seed).sign(message).to_bytes()
}

/// How an actor signs for a smart account.
///
/// Addresses are kept as strkeys because every run uses a freshly restored
/// `Env`; see [`breakwater::Actor`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignerKey {
    /// `Signer::Delegated(delegate)`: another contract account authorizes the
    /// account's `__check_auth(preimage)` invocation. With `seed` set, the
    /// delegate expects an Ed25519 signature over its own signature payload;
    /// with `None`, the delegate approves anything (`ScVal::Void`).
    Delegated {
        /// Strkey of the delegate contract.
        delegate: String,
        /// Ed25519 seed the delegate verifies against, if any.
        seed: Option<[u8; 32]>,
    },
    /// `Signer::External(verifier, public_key)`: an Ed25519 key whose
    /// signature over the account's `AuthDigestPreimage` digest is checked by
    /// `verifier`.
    External {
        /// Strkey of the verifier contract.
        verifier: String,
        /// Ed25519 seed.
        seed: [u8; 32],
    },
}

impl SignerKey {
    /// A delegated signer that verifies an Ed25519 signature.
    pub fn delegated_ed25519(delegate: &Address, seed: [u8; 32]) -> Self {
        SignerKey::Delegated {
            delegate: delegate.to_string().to_string(),
            seed: Some(seed),
        }
    }

    /// A delegated signer that approves any payload.
    pub fn delegated_approving(delegate: &Address) -> Self {
        SignerKey::Delegated {
            delegate: delegate.to_string().to_string(),
            seed: None,
        }
    }

    /// An external Ed25519 signer checked by `verifier`.
    pub fn external_ed25519(verifier: &Address, seed: [u8; 32]) -> Self {
        SignerKey::External {
            verifier: verifier.to_string().to_string(),
            seed,
        }
    }

    /// The public key, for external signers.
    pub fn public_key(&self) -> Option<[u8; 32]> {
        match self {
            SignerKey::External { seed, .. } => Some(ed25519_public_key(seed)),
            SignerKey::Delegated { seed, .. } => seed.as_ref().map(ed25519_public_key),
        }
    }

    /// The OpenZeppelin `Signer` value bound to `env`.
    pub fn signer(&self, env: &Env) -> Signer {
        match self {
            SignerKey::Delegated { delegate, .. } => {
                Signer::Delegated(Address::from_str(env, delegate))
            }
            SignerKey::External { verifier, seed } => Signer::External(
                Address::from_str(env, verifier),
                Bytes::from_array(env, &ed25519_public_key(seed)),
            ),
        }
    }
}
