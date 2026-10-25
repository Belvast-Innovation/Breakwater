//! Demo target for the `policy-bypass` attack pack.
//!
//! `DemoAccount` is an OpenZeppelin `SmartAccount` with no changes besides a
//! constructor that installs the initial context rules (an account with no
//! rules could never authorize adding one). The bug the demo exhibits is not
//! in the contract but in how the suite configures it
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
use soroban_sdk::{
    contract, contractimpl, contracttype, Address, BytesN, Env, IntoVal, Map, String, Val, Vec,
};
use stellar_accounts::policies::spending_limit::SpendingLimitAccountParams;
use stellar_accounts::smart_account::{
    add_context_rule, do_check_auth, AuthDigestPreimage, AuthPayload, ContextRule, ContextRuleType,
    Signer, SmartAccount, SmartAccountError,
};

/// A spending-limit policy to attach to an initial rule, in a shape the
/// Stellar CLI can take as typed JSON (a `Val` parameter cannot be typed on
/// the command line).
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct SpendingLimitInstall {
    /// Policy contract.
    pub policy: Address,
    /// Maximum cumulative amount inside one window.
    pub spending_limit: i128,
    /// Window length in ledgers.
    pub period_ledgers: u32,
}

/// A context rule installed by the constructor.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct InitialRule {
    /// Scope of the rule.
    pub context_type: ContextRuleType,
    /// Human-readable name (at most 20 bytes).
    pub name: String,
    /// Optional expiry ledger.
    pub valid_until: Option<u32>,
    /// Signers allowed under this rule.
    pub signers: Vec<Signer>,
    /// Arbitrary policies with pre-encoded install parameters.
    pub policies: Map<Address, Val>,
    /// Spending-limit policies with typed parameters; merged into `policies`.
    pub spending_limits: Vec<SpendingLimitInstall>,
}

/// An OpenZeppelin smart account with no customisation beyond a constructor
/// that installs its initial context rules.
///
/// The constructor exists for on-chain deployment: `add_context_rule` requires
/// the account's own authorization, which an account with zero rules can never
/// produce. Inside the constructor the storage-level function is called
/// directly, and the policies' `install` callbacks pass because the account is
/// their direct invoker.
#[contract]
pub struct DemoAccount;

#[contractimpl]
impl DemoAccount {
    /// Installs `rules` in order; an empty list leaves the account unconfigured.
    pub fn __constructor(env: Env, rules: Vec<InitialRule>) {
        for rule in rules.iter() {
            let mut policies = rule.policies.clone();
            for install in rule.spending_limits.iter() {
                let params = SpendingLimitAccountParams {
                    spending_limit: install.spending_limit,
                    period_ledgers: install.period_ledgers,
                };
                policies.set(install.policy.clone(), params.into_val(&env));
            }
            add_context_rule(
                &env,
                &rule.context_type,
                &rule.name,
                rule.valid_until,
                &rule.signers,
                &policies,
            );
        }
    }
}

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
