//! Breakwater suite for the demo smart account, run in `AuthMode::Real`.
//!
//! Setup (all in the base `Env`, with mocked auth):
//!
//! - `DemoAccount` with two context rules:
//!   - rule 0, `Default`: the owner, a `Signer::Delegated(OwnerAccount)` whose
//!     `__check_auth` verifies an Ed25519 signature;
//!   - rule 1, `CallContract(usdc)`: the agent, a `Signer::External` Ed25519
//!     key checked by `Ed25519Verifier`, with two `SpendingLimitPolicy`
//!     instances: 50 USDC per ledger (the de-facto per-call cap) and the
//!     "daily" cap of 100 USDC per **1 728 ledgers**.
//!
//! The daily window is the deliberate defect. OpenZeppelin's `SpendingLimit`
//! counts its window in ledgers; a day at ~5 s per ledger is 17 280 ledgers.
//! Someone who divides 86 400 s by 50 instead of 5 ships a 2.4-hour window and
//! believes it is a day. Every single call still respects both caps, so only a
//! sequence that waits out the short window reveals it.
//!
//! Every step goes through the real authorization path: the handlers sign and
//! install `SorobanAuthorizationEntry` values, and the host calls
//! `DemoAccount::__check_auth`, which runs the policies.

use std::sync::Arc;
use std::time::Instant;

use breakwater::auth::{contract_fn, to_scval};
use breakwater::proptest::prelude::*;
use breakwater::{
    Actor, AuthMode, CallContext, CallOutcome, Handler, HandlerError, Outcome, Suite, SuiteConfig,
    ViolationKind,
};
use breakwater_pack_policy_bypass::auth::{auth_failure, authorize, classify};
use breakwater_pack_policy_bypass::{
    amount_near_cap, window_straddling_advance, AllowlistOnly, PinnedWasm, RevokedNeverAuths,
    RollingCap, SignerKey,
};
use example_policy_bypass::helpers::{Ed25519Verifier, OwnerAccount, SpendingLimitPolicy};
use example_policy_bypass::{DemoAccount, DemoAccountClient};
use serde::Serialize;
use soroban_sdk::testutils::{Address as _, EnvTestConfig, Ledger as _};
use soroban_sdk::token::{Client as TokenClient, StellarAssetClient};
use soroban_sdk::{vec, Address, BytesN, Env, IntoVal, Map};
use stellar_accounts::policies::spending_limit::SpendingLimitAccountParams;
use stellar_accounts::smart_account::ContextRuleType;

/// 1 USDC in stroops (7 decimals).
const USDC: i128 = 10_000_000;
/// Stellar closes a ledger roughly every 5 seconds.
const LEDGERS_PER_DAY: u32 = 86_400 / 5;
/// The misconfigured "daily" window: 86_400 / 50.
const BUGGY_DAILY_WINDOW: u32 = 86_400 / 50;
const PER_CALL_CAP_USDC: i64 = 50;
const DAILY_CAP_USDC: i64 = 100;

const OWNER_SEED: [u8; 32] = [0x11; 32];
const AGENT_SEED: [u8; 32] = [0x22; 32];

/// Everything the handlers need, as strkeys so it survives `Env` restores.
struct Fixture {
    account: String,
    owner_account: String,
    verifier: String,
    per_call_policy: String,
    daily_policy: String,
    token: String,
    merchant: String,
    owner_key: SignerKey,
    agent_key: SignerKey,
    owner_rule: u32,
    agent_rule: u32,
    agent_signer_id: u32,
}

fn addr(env: &Env, strkey: &str) -> Address {
    Address::from_str(env, strkey)
}

fn strkey(address: &Address) -> String {
    address.to_string().to_string()
}

fn params(limit_usdc: i64, period_ledgers: u32) -> SpendingLimitAccountParams {
    SpendingLimitAccountParams {
        spending_limit: i128::from(limit_usdc) * USDC,
        period_ledgers,
    }
}

/// Deploys and configures the account. `daily_window` is the policy window
/// in ledgers; the demo passes the buggy value, the control test the right one.
fn setup(daily_window: u32) -> (Env, Fixture) {
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

#[derive(Clone, Debug, Serialize)]
struct PayArgs {
    amount_usdc: i64,
}

/// `usdc.transfer(account, merchant, amount)` authorized by the agent key.
struct AgentTransfer {
    fx: Arc<Fixture>,
    actors: Vec<Actor>,
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
            Ok(Ok(())) => Ok(CallOutcome::Success),
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
struct AgentApprove {
    fx: Arc<Fixture>,
    actors: Vec<Actor>,
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
            Ok(Ok(())) => Ok(CallOutcome::Success),
            Ok(Err(e)) => Err(HandlerError::Other(format!(
                "return value conversion failed: {e:?}"
            ))),
            Err(Ok(e)) => classify(e),
            Err(Err(e)) => Err(HandlerError::Host(format!("{e:?}"))),
        }
    }
}

/// `account.remove_signer(agent_rule, agent_signer)` authorized by the owner.
struct OwnerRevokeAgent {
    fx: Arc<Fixture>,
    actors: Vec<Actor>,
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
            Ok(Ok(())) => Ok(CallOutcome::Success),
            Ok(Err(e)) => Err(HandlerError::Other(format!(
                "return value conversion failed: {e:?}"
            ))),
            Err(Ok(e)) => classify(e),
            Err(Err(e)) => Err(HandlerError::Host(format!("{e:?}"))),
        }
    }
}

fn suite(seed: u64, daily_window: u32, cases: u32) -> Suite {
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
        .config(SuiteConfig {
            seed,
            steps: 1..=10,
            cases,
            auth: AuthMode::Real,
            ..SuiteConfig::default()
        })
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
fn restore_native_contracts(env: &Env, fx: &Fixture, owner_key_pub: &[u8; 32]) {
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

/// Single calls under real authorization behave as the policies prescribe.
/// This is the ground truth the fuzzer builds on; it also documents which
/// host error a policy rejection surfaces as.
#[test]
fn single_calls_behave_as_the_policies_prescribe() {
    let (base, fx) = setup(BUGGY_DAILY_WINDOW);
    let fx = Arc::new(fx);
    let owner_key_pub = breakwater_pack_policy_bypass::ed25519_public_key(&OWNER_SEED);
    let mut env = Env::from_ledger_snapshot(base.to_ledger_snapshot());
    env.set_config(EnvTestConfig {
        capture_snapshot_at_drop: false,
    });
    env.cost_estimate().budget().reset_unlimited();
    restore_native_contracts(&env, &fx, &owner_key_pub);

    let account = addr(&env, &fx.account);
    let agent = Actor::from_address("agent", &account);
    let owner = Actor::from_address("owner", &account);
    let transfer = AgentTransfer {
        fx: fx.clone(),
        actors: vec_of(&agent),
    };
    let approve = AgentApprove {
        fx: fx.clone(),
        actors: vec_of(&agent),
    };
    let revoke = OwnerRevokeAgent {
        fx: fx.clone(),
        actors: vec_of(&owner),
    };
    let ctx = |step: usize| CallContext {
        step,
        auth: AuthMode::Real,
    };
    let bump = |env: &Env| env.ledger().with_mut(|li| li.sequence_number += 1);

    let pay = |step: usize, amount_usdc: i64| {
        transfer.call(&env, &ctx(step), &agent, &PayArgs { amount_usdc })
    };

    assert!(
        matches!(pay(0, 49), Ok(CallOutcome::Success)),
        "first 49 USDC passes both caps"
    );
    bump(&env);
    assert!(
        matches!(pay(1, 49), Ok(CallOutcome::Success)),
        "98 USDC is still under the daily cap"
    );
    bump(&env);
    assert!(
        matches!(pay(2, 49), Ok(CallOutcome::Unauthorized)),
        "147 USDC breaks the daily cap"
    );
    bump(&env);
    assert!(
        matches!(pay(3, 51), Ok(CallOutcome::Unauthorized)),
        "51 USDC breaks the per-call cap"
    );
    bump(&env);
    assert!(
        matches!(
            approve.call(&env, &ctx(4), &agent, &PayArgs { amount_usdc: 1 }),
            Ok(CallOutcome::Unauthorized)
        ),
        "approve is not allow-listed"
    );
    bump(&env);
    assert!(
        matches!(
            revoke.call(&env, &ctx(5), &owner, &()),
            Ok(CallOutcome::Success)
        ),
        "the owner revokes the agent through the nested delegated entry"
    );
    bump(&env);
    assert!(
        matches!(pay(6, 1), Ok(CallOutcome::Unauthorized)),
        "a revoked agent cannot pay"
    );

    let balance = TokenClient::new(&env, &addr(&env, &fx.token)).balance(&addr(&env, &fx.merchant));
    assert_eq!(balance, 98 * USDC);
}

fn vec_of(actor: &Actor) -> Vec<Actor> {
    std::vec![actor.clone()]
}

#[test]
fn finds_the_window_bypass_under_real_auth_and_shrinks_it() {
    let started = Instant::now();
    let report = suite(7, BUGGY_DAILY_WINDOW, 64).run().expect("suite runs");
    let elapsed = started.elapsed();
    eprintln!(
        "policy-bypass: {} cases, {} steps in {:?} ({:.2} ms/step)",
        report.cases_run,
        report.steps_run,
        elapsed,
        elapsed.as_secs_f64() * 1_000.0 / report.steps_run.max(1) as f64
    );

    let failure = report.failure().expect("the daily cap must be bypassable");
    eprintln!("shrunk sequence:");
    for step in failure.log.steps() {
        eprintln!(
            "  #{} {:<12} {:<6} +{:>5} ledgers  args={}  -> {:?}",
            step.index,
            step.handler,
            step.actor.name(),
            step.gap.ledgers,
            step.args,
            step.outcome
        );
    }
    eprintln!(
        "violation: {} ({})",
        failure.violation.message, failure.violation.source
    );

    assert_eq!(failure.violation.kind, ViolationKind::Invariant);
    assert_eq!(failure.violation.source, "rolling_cap");
    let spent: i64 = failure
        .log
        .successes_of("transfer")
        .map(|s| s.args["amount_usdc"].as_i64().unwrap_or(0))
        .sum();
    assert!(spent > DAILY_CAP_USDC, "spent {spent}");
    assert!(
        failure
            .log
            .steps()
            .iter()
            .all(|s| s.handler == "transfer" && s.outcome.is_success()),
        "the shrunk reproduction is payments only"
    );
    // Minimal shape for this seed: three payments of cap - 1, the last one
    // waiting exactly one ledger short of the mis-sized window so that the
    // first payment has just dropped out of the policy's history.
    let steps = failure.log.steps();
    assert_eq!(steps.len(), 3, "shrunk to three steps");
    assert!(steps
        .iter()
        .all(|s| s.args["amount_usdc"] == PER_CALL_CAP_USDC - 1));
    assert_eq!(spent, 3 * (PER_CALL_CAP_USDC - 1));
    assert_eq!(steps[2].gap.ledgers, BUGGY_DAILY_WINDOW - 1);
    assert_eq!(failure.failing_step, 2);
}

#[test]
fn same_seed_reproduces_the_same_sequence() {
    let a = suite(7, BUGGY_DAILY_WINDOW, 64).run().expect("suite runs");
    let b = suite(7, BUGGY_DAILY_WINDOW, 64).run().expect("suite runs");
    assert_eq!(a.failure().map(|f| &f.log), b.failure().map(|f| &f.log));
}

#[test]
fn holds_with_the_correct_daily_window() {
    let report = suite(7, LEDGERS_PER_DAY, 48).run().expect("suite runs");
    assert!(
        matches!(report.outcome, Outcome::Held),
        "{:?}",
        report.failure().map(|f| &f.violation)
    );
}
