//! OpenZeppelin's spending-limit policy as a deployable contract.
//!
//! Its own crate so it can be built and deployed independently of the demo
//! smart account (a cdylib exports one contract).
#![no_std]

use soroban_sdk::auth::Context;
use soroban_sdk::{contract, contractimpl, Address, Env, Vec};
use stellar_accounts::policies::spending_limit::{self, SpendingLimitAccountParams};
use stellar_accounts::policies::Policy;
use stellar_accounts::smart_account::{ContextRule, Signer};

/// Spending-limit policy: `spending_limit` per `period_ledgers`, per context rule.
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
