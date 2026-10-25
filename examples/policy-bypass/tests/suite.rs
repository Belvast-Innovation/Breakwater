//! Breakwater suite for the demo smart account, run in `AuthMode::Real`.
//! The fixture and handlers live in `common/mod.rs`; see the notes there.
//!
//! `breakwater_suite` is the test the `breakwater` CLI drives: it honours
//! `BREAKWATER_SEED`, `BREAKWATER_STEPS` and `BREAKWATER_AUTH`, writes the
//! report to `BREAKWATER_REPORT` and, when driven that way, leaves judging the
//! result to the CLI. Under plain `cargo test` it also asserts the known shape.

mod common;

use std::sync::Arc;
use std::time::Instant;

use breakwater::{
    Actor, AuthMode, CallContext, CallOutcome, Exporter, Handler, Outcome, SuiteConfig,
    ViolationKind,
};
use common::*;
use soroban_sdk::testutils::{EnvTestConfig, Ledger as _};
use soroban_sdk::token::Client as TokenClient;
use soroban_sdk::Env;

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

#[test]
fn breakwater_suite() {
    let driven = std::env::var_os("BREAKWATER_REPORT").is_some();
    let config = SuiteConfig::from_env(default_config(7, 64));
    let started = Instant::now();
    let report = suite_with_config(config, BUGGY_DAILY_WINDOW)
        .run()
        .expect("suite runs");
    let elapsed = started.elapsed();
    Exporter::write_if_requested(&report).expect("report is written");
    if driven {
        return;
    }
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
    // Minimal shape: three payments of cap - 1, the third landing exactly one
    // mis-sized window after the first, so that the first payment has just
    // dropped out of the policy's history. How the shrinker splits the wait
    // between the second and third step is not significant.
    let steps = failure.log.steps();
    assert_eq!(steps.len(), 3, "shrunk to three steps");
    assert!(steps
        .iter()
        .all(|s| s.args["amount_usdc"] == PER_CALL_CAP_USDC - 1));
    assert_eq!(spent, 3 * (PER_CALL_CAP_USDC - 1));
    assert_eq!(
        steps[1].gap.ledgers + steps[2].gap.ledgers,
        BUGGY_DAILY_WINDOW,
        "third payment lands exactly one window after the first"
    );
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
