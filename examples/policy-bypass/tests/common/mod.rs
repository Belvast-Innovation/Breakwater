//! Shared fixture, handlers and suite builder for the policy-bypass demo.
//! Used by `suite.rs` (the fuzzing suite) and by exported reproductions.
#![allow(dead_code)]

// Breakwater suite for the demo smart account, run in `AuthMode::Real`.
//
// Setup (all in the base `Env`, with mocked auth):
//
// - `DemoAccount` with two context rules:
//   - rule 0, `Default`: the owner, a `Signer::Delegated(OwnerAccount)` whose
//     `__check_auth` verifies an Ed25519 signature;
//   - rule 1, `CallContract(usdc)`: the agent, a `Signer::External` Ed25519
//     key checked by `Ed25519Verifier`, with two `SpendingLimitPolicy`
//     instances: 50 USDC per ledger (the de-facto per-call cap) and the
//     "daily" cap of 100 USDC per **1 728 ledgers**.
//
// The daily window is the deliberate defect. OpenZeppelin's `SpendingLimit`
// counts its window in ledgers; a day at ~5 s per ledger is 17 280 ledgers.
// Someone who divides 86 400 s by 50 instead of 5 ships a 2.4-hour window and
// believes it is a day. Every single call still respects both caps, so only a
// sequence that waits out the short window reveals it.
//
// Every step goes through the real authorization path: the handlers sign and
// install `SorobanAuthorizationEntry` values, and the host calls
// `DemoAccount::__check_auth`, which runs the policies.

use std::sync::Arc;

use breakwater::auth::{contract_fn, to_scval};
use breakwater::proptest::prelude::*;
use breakwater::{
    Actor, AuthMode, CallContext, CallOutcome, Handler, HandlerError, ReplayStep, RunReport, Suite,
    SuiteConfig,
};
use breakwater_pack_policy_bypass::auth::{auth_failure, authorize, classify};
use breakwater_pack_policy_bypass::{
    amount_near_cap, window_straddling_advance, AllowlistOnly, PinnedWasm, RevokedNeverAuths,
    RollingCap, SignerKey,
};
use example_policy_bypass::helpers::{Ed25519Verifier, OwnerAccount, SpendingLimitPolicy};
use example_policy_bypass::{DemoAccount, DemoAccountClient};
use serde::{Deserialize, Serialize};
use soroban_sdk::testutils::{Address as _, EnvTestConfig, Ledger as _};
use soroban_sdk::token::{Client as TokenClient, StellarAssetClient};
use soroban_sdk::{vec, Address, BytesN, Env, IntoVal, Map};
use stellar_accounts::policies::spending_limit::SpendingLimitAccountParams;
use stellar_accounts::smart_account::ContextRuleType;

/// 1 USDC in stroops (7 decimals).
pub const USDC: i128 = 10_000_000;
/// Stellar closes a ledger roughly every 5 seconds.
pub const LEDGERS_PER_DAY: u32 = 86_400 / 5;
/// The misconfigured "daily" window: 86_400 / 50.
pub const BUGGY_DAILY_WINDOW: u32 = 86_400 / 50;
pub const PER_CALL_CAP_USDC: i64 = 50;
pub const DAILY_CAP_USDC: i64 = 100;

pub const OWNER_SEED: [u8; 32] = [0x11; 32];
pub const AGENT_SEED: [u8; 32] = [0x22; 32];

/// Everything the handlers need, as strkeys so it survives `Env` restores.
pub struct Fixture {
    pub account: String,
    pub owner_account: String,
    pub verifier: String,
    pub per_call_policy: String,
    pub daily_policy: String,
    pub token: String,
    pub merchant: String,
    pub owner_key: SignerKey,
    pub agent_key: SignerKey,
    pub owner_rule: u32,
    pub agent_rule: u32,
    pub agent_signer_id: u32,
}

pub fn addr(env: &Env, strkey: &str) -> Address {
    Address::from_str(env, strkey)
}

pub fn strkey(address: &Address) -> String {
    address.to_string().to_string()
}

pub fn params(limit_usdc: i64, period_ledgers: u32) -> SpendingLimitAccountParams {
    SpendingLimitAccountParams {
        spending_limit: i128::from(limit_usdc) * USDC,
        period_ledgers,
    }
}

/// Deploys and configures the account. `daily_window` is the policy window
/// in ledgers; the demo passes the buggy value, the control test the right one.
pub fn setup(daily_window: u32) -> (Env, Fixture) {
    let env = Env::new_with_config(EnvTestConfig {
        capture_snapshot_at_drop: false,
    });
    env.mock_all_auths();
    env.ledger().with_mut(|li| {
        li.sequence_number = 100_000;
        li.timestamp = 1_761_350_400;
        li.max_entry_ttl = 6_312_000;
        li.min_persistent_entry_ttl = 4_096;
        li.min_temp_entry_ttl = 16;
    });

    let admin = Address::generate(&env);
    let token = env.register_stellar_asset_contract_v2(admin).address();
    let merchant = Address::generate(&env);

    let owner_key_pub = BytesN::from_array(
        &env,
        &breakwater_pack_policy_bypass::ed25519_public_key(&OWNER_SEED),
    );
    let owner_account = env.register(OwnerAccount, (owner_key_pub,));
    let verifier = env.register(Ed25519Verifier, ());
    let per_call_policy = env.register(SpendingLimitPolicy, ());
    let daily_policy = env.register(SpendingLimitPolicy, ());
    let account = env.register(DemoAccount, ());

    let owner_key = SignerKey::delegated_ed25519(&owner_account, OWNER_SEED);
    let agent_key = SignerKey::external_ed25519(&verifier, AGENT_SEED);

    let client = DemoAccountClient::new(&env, &account);
    let owner_rule = client
        .add_context_rule(
            &ContextRuleType::Default,
            &soroban_sdk::String::from_str(&env, "owner"),
            &None,
            &vec![&env, owner_key.signer(&env)],
            &Map::new(&env),
        )
        .id;

    let mut policies = Map::new(&env);
    policies.set(
        per_call_policy.clone(),
        params(PER_CALL_CAP_USDC, 1).into_val(&env),
    );
    policies.set(
        daily_policy.clone(),
        params(DAILY_CAP_USDC, daily_window).into_val(&env),
    );
    let agent_rule = client
        .add_context_rule(
            &ContextRuleType::CallContract(token.clone()),
            &soroban_sdk::String::from_str(&env, "agent"),
            &None,
            &vec![&env, agent_key.signer(&env)],
            &policies,
        )
        .id;
    let agent_signer_id = client.get_signer_id(&agent_key.signer(&env));

    StellarAssetClient::new(&env, &token).mint(&account, &(10_000 * USDC));

    let fixture = Fixture {
        account: strkey(&account),
        owner_account: strkey(&owner_account),
        verifier: strkey(&verifier),
        per_call_policy: strkey(&per_call_policy),
        daily_policy: strkey(&daily_policy),
        token: strkey(&token),
        merchant: strkey(&merchant),
        owner_key,
        agent_key,
        owner_rule,
        agent_rule,
        agent_signer_id,
    };
    (env, fixture)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PayArgs {
    pub amount_usdc: i64,
}

/// `usdc.transfer(account, merchant, amount)` authorized by the agent key.
pub struct AgentTransfer {
    pub fx: Arc<Fixture>,
    pub actors: Vec<Actor>,
}

impl Handler for AgentTransfer {
    type Args = PayArgs;

    fn name(&self) -> &'static str {
        "transfer"
    }

    fn actors(&self) -> &[Actor] {
        &self.actors
    }

    fn strategy(&self) -> BoxedStrategy<PayArgs> {
        amount_near_cap(PER_CALL_CAP_USDC)
            .prop_map(|amount_usdc| PayArgs { amount_usdc })
            .boxed()
    }

    fn call(
        &self,
        env: &Env,
        ctx: &CallContext,
        _actor: &Actor,
        args: &PayArgs,
    ) -> Result<CallOutcome, HandlerError> {
        let account = addr(env, &self.fx.account);
        let token = addr(env, &self.fx.token);
        let merchant = addr(env, &self.fx.merchant);
        let amount = i128::from(args.amount_usdc) * USDC;

        let root = contract_fn(
            &token,
            "transfer",
            std::vec![
                to_scval(env, account.clone()).map_err(auth_failure)?,
                to_scval(env, merchant.clone()).map_err(auth_failure)?,
                to_scval(env, amount).map_err(auth_failure)?,
            ],
        )
        .map_err(auth_failure)?;
        let entries = authorize(
            env,
            &account,
            &self.fx.agent_key,
            self.fx.agent_rule,
            root,
            ctx,
        )
        .map_err(auth_failure)?;
        env.set_auths(&entries);

        match TokenClient::new(env, &token).try_transfer(&account, &merchant, &amount) {
            Ok(Ok(())) => Ok(CallOutcome::returning(())),
            Ok(Err(e)) => Err(HandlerError::Other(format!(
                "return value conversion failed: {e:?}"
            ))),
            Err(Ok(e)) => classify(e),
            Err(Err(e)) => Err(HandlerError::Host(format!("{e:?}"))),
        }
    }
}

/// `usdc.approve(account, merchant, amount, ..)` by the agent: outside the
/// policy's allow-list, so it must always be rejected.
pub struct AgentApprove {
    pub fx: Arc<Fixture>,
    pub actors: Vec<Actor>,
}

impl Handler for AgentApprove {
    type Args = PayArgs;

    fn name(&self) -> &'static str {
        "approve"
    }

    fn actors(&self) -> &[Actor] {
        &self.actors
    }

    fn strategy(&self) -> BoxedStrategy<PayArgs> {
        amount_near_cap(PER_CALL_CAP_USDC)
            .prop_map(|amount_usdc| PayArgs { amount_usdc })
            .boxed()
    }

    fn call(
        &self,
        env: &Env,
        ctx: &CallContext,
        _actor: &Actor,
        args: &PayArgs,
    ) -> Result<CallOutcome, HandlerError> {
        let account = addr(env, &self.fx.account);
        let token = addr(env, &self.fx.token);
        let merchant = addr(env, &self.fx.merchant);
        let amount = i128::from(args.amount_usdc) * USDC;
        let expiration = env.ledger().sequence() + 1_000;

        let root = contract_fn(
            &token,
            "approve",
            std::vec![
                to_scval(env, account.clone()).map_err(auth_failure)?,
                to_scval(env, merchant.clone()).map_err(auth_failure)?,
                to_scval(env, amount).map_err(auth_failure)?,
                to_scval(env, expiration).map_err(auth_failure)?,
            ],
        )
        .map_err(auth_failure)?;
        let entries = authorize(
            env,
            &account,
            &self.fx.agent_key,
            self.fx.agent_rule,
            root,
            ctx,
        )
        .map_err(auth_failure)?;
        env.set_auths(&entries);

        match TokenClient::new(env, &token).try_approve(&account, &merchant, &amount, &expiration) {
            Ok(Ok(())) => Ok(CallOutcome::returning(())),
            Ok(Err(e)) => Err(HandlerError::Other(format!(
                "return value conversion failed: {e:?}"
            ))),
            Err(Ok(e)) => classify(e),
            Err(Err(e)) => Err(HandlerError::Host(format!("{e:?}"))),
        }
    }
}

/// `account.remove_signer(agent_rule, agent_signer)` authorized by the owner.
pub struct OwnerRevokeAgent {
    pub fx: Arc<Fixture>,
    pub actors: Vec<Actor>,
}

impl Handler for OwnerRevokeAgent {
    type Args = ();

    fn name(&self) -> &'static str {
        "revoke_agent"
    }

    fn actors(&self) -> &[Actor] {
        &self.actors
    }

    fn strategy(&self) -> BoxedStrategy<()> {
        Just(()).boxed()
    }

    fn call(
        &self,
        env: &Env,
        ctx: &CallContext,
        _actor: &Actor,
        _args: &(),
    ) -> Result<CallOutcome, HandlerError> {
        let account = addr(env, &self.fx.account);
        let root = contract_fn(
            &account,
            "remove_signer",
            std::vec![
                to_scval(env, self.fx.agent_rule).map_err(auth_failure)?,
                to_scval(env, self.fx.agent_signer_id).map_err(auth_failure)?,
            ],
        )
        .map_err(auth_failure)?;
        let entries = authorize(
            env,
            &account,
            &self.fx.owner_key,
            self.fx.owner_rule,
            root,
            ctx,
        )
        .map_err(auth_failure)?;
        env.set_auths(&entries);

        match DemoAccountClient::new(env, &account)
            .try_remove_signer(&self.fx.agent_rule, &self.fx.agent_signer_id)
        {
            Ok(Ok(())) => Ok(CallOutcome::returning(())),
            Ok(Err(e)) => Err(HandlerError::Other(format!(
                "return value conversion failed: {e:?}"
            ))),
            Err(Ok(e)) => classify(e),
            Err(Err(e)) => Err(HandlerError::Host(format!("{e:?}"))),
        }
    }
}

/// Default configuration of the demo suite: real authorization, short
/// sequences, enough cases to hit the window boundary.
pub fn default_config(seed: u64, cases: u32) -> SuiteConfig {
    SuiteConfig {
        seed,
        steps: 1..=10,
        cases,
        auth: AuthMode::Real,
        ..SuiteConfig::default()
    }
}

pub fn suite(seed: u64, daily_window: u32, cases: u32) -> Suite {
    suite_with_config(default_config(seed, cases), daily_window)
}

/// Replays exported steps against a freshly built demo (buggy window) and
/// returns the report. Used by generated `repro_*.rs` tests.
pub fn replay_json(json: &str) -> RunReport {
    let steps: Vec<ReplayStep> = serde_json::from_str(json).expect("replay steps are valid JSON");
    suite_with_config(default_config(0, 1), BUGGY_DAILY_WINDOW)
        .replay(&steps)
        .expect("replay runs")
}

pub fn suite_with_config(config: SuiteConfig, daily_window: u32) -> Suite {
    let (env, fx) = setup(daily_window);
    let fx = Arc::new(fx);
    let account = addr(&env, &fx.account);
    let agent = Actor::from_address("agent", &account);
    let owner = Actor::from_address("owner", &account);

    let pinned = PinnedWasm::capture(
        &env,
        &[
            ("account", &account),
            ("per_call_policy", &addr(&env, &fx.per_call_policy)),
            ("daily_policy", &addr(&env, &fx.daily_policy)),
            ("verifier", &addr(&env, &fx.verifier)),
        ],
    );

    let restore = fx.clone();
    let owner_key_pub = BytesN::from_array(
        &env,
        &breakwater_pack_policy_bypass::ed25519_public_key(&OWNER_SEED),
    );
    let owner_key_pub: [u8; 32] = owner_key_pub.to_array();

    Suite::new("policy-bypass", env)
        .config(config)
        .ledger_advance(window_straddling_advance(BUGGY_DAILY_WINDOW))
        .handler(AgentTransfer {
            fx: fx.clone(),
            actors: vec_of(&agent),
        })
        .handler(AgentApprove {
            fx: fx.clone(),
            actors: vec_of(&agent),
        })
        .handler(OwnerRevokeAgent {
            fx: fx.clone(),
            actors: vec_of(&owner),
        })
        .invariant(RollingCap {
            actor: "agent".into(),
            handler: "transfer".into(),
            amount_field: "amount_usdc".into(),
            limit: DAILY_CAP_USDC,
            window_ledgers: LEDGERS_PER_DAY,
        })
        .invariant(AllowlistOnly {
            actor: "agent".into(),
            allowed: std::vec!["transfer".into()],
        })
        .invariant(RevokedNeverAuths {
            actor: "agent".into(),
            revoke_handler: "revoke_agent".into(),
        })
        .invariant(pinned)
        .on_restore(move |env| restore_native_contracts(env, &restore, &owner_key_pub))
}

/// Native test contracts live in the host, not the ledger: re-register them at
/// their addresses on every restored `Env`; their storage survives the snapshot.
pub fn restore_native_contracts(env: &Env, fx: &Fixture, owner_key_pub: &[u8; 32]) {
    env.register_at(&addr(env, &fx.account), DemoAccount, ());
    env.register_at(
        &addr(env, &fx.owner_account),
        OwnerAccount,
        (BytesN::from_array(env, owner_key_pub),),
    );
    env.register_at(&addr(env, &fx.verifier), Ed25519Verifier, ());
    env.register_at(&addr(env, &fx.per_call_policy), SpendingLimitPolicy, ());
    env.register_at(&addr(env, &fx.daily_policy), SpendingLimitPolicy, ());
}

pub fn vec_of(actor: &Actor) -> Vec<Actor> {
    std::vec![actor.clone()]
}
