//! Demo target for the `policy-bypass` attack pack.
//!
//! `DemoAccount` is an unmodified OpenZeppelin `SmartAccount`. The bug the
//! demo exhibits is not in the contract but in how the suite configures it
//! (see `tests/suite.rs`): the agent's daily spending limit is installed with
//! a window of 1 728 ledgers instead of 17 280. OpenZeppelin's `SpendingLimit`
//! measures its window in **ledgers**, and at Stellar's ~5 s close time one
//! day is 86 400 / 5 = 17 280 ledgers. Dividing by 50 instead of 5 is the kind
//! of unit slip an operator makes once, and it turns "100 USDC per day" into
//! "100 USDC per 2.4 hours".
//!
//! With the `helpers` feature the crate also ships the three auxiliary
//! contracts the suite deploys next to the account: an owner key account, a
//! spending-limit policy and an Ed25519 verifier.
#![no_std]

use soroban_sdk::auth::{Context, CustomAccountInterface};
use soroban_sdk::crypto::Hash;
// The `contracttrait` expansion of `SmartAccount` names these types unqualified.
use soroban_sdk::{contract, contractimpl, Address, BytesN, Env, Map, String, Val, Vec};
use stellar_accounts::smart_account::{
    do_check_auth, AuthDigestPreimage, AuthPayload, ContextRule, ContextRuleType, Signer,
    SmartAccount, SmartAccountError,
};

/// An OpenZeppelin smart account with no customisation.
#[contract]
pub struct DemoAccount;

#[contractimpl]
impl CustomAccountInterface for DemoAccount {
    type Error = SmartAccountError;
    type Signature = AuthPayload;

    #[allow(non_snake_case)]
    fn __check_auth(
        e: Env,
        signature_payload: Hash<32>,
        signatures: AuthPayload,
        auth_contexts: Vec<Context>,
    ) -> Result<(), Self::Error> {
        do_check_auth(&e, &signature_payload, &signatures, &auth_contexts)
    }
}

#[contractimpl(contracttrait)]
impl SmartAccount for DemoAccount {}

#[cfg(any(test, feature = "helpers"))]
pub mod helpers;

#[cfg(test)]
mod test;
