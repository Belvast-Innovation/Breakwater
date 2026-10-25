//! Auxiliary contracts deployed next to [`crate::DemoAccount`] by the suite.
//!
//! Each would be its own crate in a real deployment; they live here behind a
//! feature so the demo stays in one package.

use soroban_sdk::auth::{Context, CustomAccountInterface};
use soroban_sdk::crypto::Hash;
use soroban_sdk::{
    contract, contracterror, contractimpl, symbol_short, Address, Bytes, BytesN, Env, Symbol, Vec,
};
use stellar_accounts::policies::spending_limit::{self, SpendingLimitAccountParams};
use stellar_accounts::policies::Policy;
use stellar_accounts::smart_account::{ContextRule, Signer};
use stellar_accounts::verifiers::{ed25519, Verifier};

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

/// The owner's hardware-key account: a minimal custom account that accepts an
/// Ed25519 signature over the host signature payload. It is the
/// `Signer::Delegated` of the demo smart account.
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

/// OpenZeppelin's spending-limit policy as a deployable contract.
#[contract]
pub struct SpendingLimitPolicy;

#[contractimpl]
impl Policy for SpendingLimitPolicy {
    type AccountParams = SpendingLimitAccountParams;

    fn enforce(
        e: &Env,
        context: Context,
        authenticated_signers: Vec<Signer>,
        context_rule: ContextRule,
        smart_account: Address,
    ) {
        spending_limit::enforce(
            e,
            &context,
            &authenticated_signers,
            &context_rule,
            &smart_account,
        )
    }

    fn install(
        e: &Env,
        install_params: SpendingLimitAccountParams,
        context_rule: ContextRule,
        smart_account: Address,
    ) {
        spending_limit::install(e, &install_params, &context_rule, &smart_account)
    }

    fn uninstall(e: &Env, context_rule: ContextRule, smart_account: Address) {
        spending_limit::uninstall(e, &context_rule, &smart_account)
    }
}

/// OpenZeppelin's Ed25519 verifier as a deployable contract.
#[contract]
pub struct Ed25519Verifier;

#[contractimpl]
impl Verifier for Ed25519Verifier {
    type KeyData = BytesN<32>;
    type SigData = BytesN<64>;

    fn verify(e: &Env, hash: Bytes, key_data: BytesN<32>, sig_data: BytesN<64>) -> bool {
        ed25519::verify(e, &hash, &key_data, &sig_data)
    }

    fn canonicalize_key(e: &Env, key_data: BytesN<32>) -> Bytes {
        ed25519::canonicalize_key(e, &key_data)
    }

    fn batch_canonicalize_key(e: &Env, key_data: Vec<BytesN<32>>) -> Vec<Bytes> {
        ed25519::batch_canonicalize_key(e, &key_data)
    }
}
