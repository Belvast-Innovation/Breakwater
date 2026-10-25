//! The owner's hardware-key account for the policy-bypass demo: a minimal
//! custom account that accepts one Ed25519 signature over the host signature
//! payload. It is the `Signer::Delegated` of the demo smart account.
//!
//! Its own crate because a cdylib can export only one `__check_auth`.
#![no_std]

use soroban_sdk::auth::{Context, CustomAccountInterface};
use soroban_sdk::crypto::Hash;
use soroban_sdk::{
    contract, contracterror, contractimpl, symbol_short, Bytes, BytesN, Env, Symbol, Vec,
};

/// Instance storage key of the owner's public key.
const OWNER_KEY: Symbol = symbol_short!("OWNER_PK");

/// Errors of [`OwnerAccount`].
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum OwnerAccountError {
    /// The account was deployed without a public key.
    MissingKey = 1,
}

/// A custom account that verifies one Ed25519 signature.
#[contract]
pub struct OwnerAccount;

#[contractimpl]
impl OwnerAccount {
    /// Stores the Ed25519 public key that may authorize for this account.
    pub fn __constructor(env: Env, public_key: BytesN<32>) {
        env.storage().instance().set(&OWNER_KEY, &public_key);
    }
}

#[contractimpl]
impl CustomAccountInterface for OwnerAccount {
    type Error = OwnerAccountError;
    type Signature = BytesN<64>;

    #[allow(non_snake_case)]
    fn __check_auth(
        env: Env,
        signature_payload: Hash<32>,
        signature: BytesN<64>,
        _auth_contexts: Vec<Context>,
    ) -> Result<(), Self::Error> {
        let public_key: BytesN<32> = env
            .storage()
            .instance()
            .get(&OWNER_KEY)
            .ok_or(OwnerAccountError::MissingKey)?;
        let message: Bytes = signature_payload.to_bytes().into();
        env.crypto()
            .ed25519_verify(&public_key, &message, &signature);
        Ok(())
    }
}
