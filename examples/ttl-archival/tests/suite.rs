//! Breakwater suite for the payment-stream demo, under `MockAll` auth with a
//! usage profile: one call a day for thirty days, then ninety days of silence.
//!
//! The test `Env` sets the network's minimum persistent TTL to 30 days. That
//! is a demo setting: the real value is a network parameter (see the network
//! limits on Stellar Lab) and tends to be raised over time. The point is the
//! shape, not the number: `PaymentStream` never extends a TTL, so a one-year
//! stream keeps exactly the minimum TTL it was born with and is archived with
//! most of its lifetime ahead. `PaymentStreamFixed` extends every entry it
//! touches to the stream's remaining duration and survives the same profile.

use std::sync::Arc;
use std::time::Instant;

use breakwater::proptest::prelude::*;
use breakwater::{
    Actor, AuthMode, CallContext, CallOutcome, Handler, HandlerError, LedgerAdvance, Outcome,
    Suite, SuiteConfig, ViolationKind, LEDGERS_PER_DAY,
};
use breakwater_pack_ttl_archival::{
    InstanceAlive, NoMidLifecycleExpiry, RestoreRestoresFunction, StreamModel,
};
use example_ttl_archival::fixed::{PaymentStreamFixed, PaymentStreamFixedClient};
use example_ttl_archival::{DataKey, PaymentStream, PaymentStreamClient, StreamError};
use serde::Serialize;
use soroban_sdk::testutils::{Address as _, EnvTestConfig, Ledger as _};
use soroban_sdk::token::StellarAssetClient;
use soroban_sdk::xdr::ScErrorType;
use soroban_sdk::{Address, Env, IntoVal};

/// 1 USDC in stroops.
const USDC: i128 = 10_000_000;
/// Demo network setting: new persistent entries live 30 days.
const MIN_PERSISTENT_TTL_DAYS: u32 = 30;
const YEAR: u32 = 365 * LEDGERS_PER_DAY;

/// Which contract the suite drives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Variant {
    Buggy,
    Fixed,
}

struct Fixture {
    variant: Variant,
    contract: String,
    token: String,
    sender: String,
    recipient: String,
}

fn addr(env: &Env, strkey: &str) -> Address {
    Address::from_str(env, strkey)
}

fn setup(variant: Variant) -> (Env, Fixture) {
    let env = Env::new_with_config(EnvTestConfig {
        capture_snapshot_at_drop: false,
    });
    env.mock_all_auths();
    env.ledger().with_mut(|li| {
        li.sequence_number = 1_000_000;
        li.timestamp = 1_761_350_400;
        li.min_persistent_entry_ttl = MIN_PERSISTENT_TTL_DAYS * LEDGERS_PER_DAY;
    });
    let admin = Address::generate(&env);
    let token = env.register_stellar_asset_contract_v2(admin).address();
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    StellarAssetClient::new(&env, &token).mint(&sender, &(1_000_000 * USDC));
    let contract = match variant {
        Variant::Buggy => env.register(PaymentStream, (token.clone(),)),
        Variant::Fixed => env.register(PaymentStreamFixed, (token.clone(),)),
    };
    let fixture = Fixture {
        variant,
        contract: contract.to_string().to_string(),
        token: token.to_string().to_string(),
        sender: sender.to_string().to_string(),
        recipient: recipient.to_string().to_string(),
    };
    (env, fixture)
}

/// Maps a failed `try_` call: contract errors are expected, the rest is a bug.
fn classify(err: soroban_sdk::Error) -> Result<CallOutcome, HandlerError> {
    if err.is_type(ScErrorType::Contract) {
        Ok(CallOutcome::ContractError(err.get_code()))
    } else {
        Err(HandlerError::Host(format!("{err:?}")))
    }
}

/// The two contract variants share an interface; dispatch by fixture.
macro_rules! with_client {
    ($fx:expr, $env:expr, |$client:ident| $body:expr) => {{
        let id = addr($env, &$fx.contract);
        match $fx.variant {
            Variant::Buggy => {
                let $client = PaymentStreamClient::new($env, &id);
                $body
            }
            Variant::Fixed => {
                let $client = PaymentStreamFixedClient::new($env, &id);
                $body
            }
        }
    }};
}

#[derive(Clone, Debug, Serialize)]
struct CreateArgs {
    amount_usdc: i64,
    duration_ledgers: u32,
}

struct CreateStream {
    fx: Arc<Fixture>,
    actors: Vec<Actor>,
}

impl Handler for CreateStream {
    type Args = CreateArgs;

    fn name(&self) -> &'static str {
        "create_stream"
    }

    fn actors(&self) -> &[Actor] {
        &self.actors
    }

    fn strategy(&self) -> BoxedStrategy<CreateArgs> {
        (1_000i64..=100_000, Just(YEAR))
            .prop_map(|(amount_usdc, duration_ledgers)| CreateArgs {
                amount_usdc,
                duration_ledgers,
            })
            .boxed()
    }

    fn call(
        &self,
        env: &Env,
        _ctx: &CallContext,
        _actor: &Actor,
        args: &CreateArgs,
    ) -> Result<CallOutcome, HandlerError> {
        let sender = addr(env, &self.fx.sender);
        let recipient = addr(env, &self.fx.recipient);
        let amount = i128::from(args.amount_usdc) * USDC;
        let result = with_client!(self.fx, env, |c| c.try_create_stream(
            &sender,
            &recipient,
            &amount,
            &args.duration_ledgers
        ));
        match result {
            Ok(Ok(_)) => Ok(CallOutcome::Success),
            Ok(Err(e)) => Err(HandlerError::Other(format!("conversion: {e:?}"))),
            Err(Ok(e)) => classify(e),
            Err(Err(e)) => Err(HandlerError::Host(format!("{e:?}"))),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
struct StreamArgs {
    stream: u64,
}

fn stream_args() -> BoxedStrategy<StreamArgs> {
    (0u64..3).prop_map(|stream| StreamArgs { stream }).boxed()
}

struct Withdraw {
    fx: Arc<Fixture>,
    actors: Vec<Actor>,
}

impl Handler for Withdraw {
    type Args = StreamArgs;

    fn name(&self) -> &'static str {
        "withdraw"
    }

    fn actors(&self) -> &[Actor] {
        &self.actors
    }

    fn strategy(&self) -> BoxedStrategy<StreamArgs> {
        stream_args()
    }

    fn call(
        &self,
        env: &Env,
        _ctx: &CallContext,
        _actor: &Actor,
        args: &StreamArgs,
    ) -> Result<CallOutcome, HandlerError> {
        let result = with_client!(self.fx, env, |c| c.try_withdraw(&args.stream));
        match result {
            Ok(Ok(_)) => Ok(CallOutcome::Success),
            Ok(Err(e)) => Err(HandlerError::Other(format!("conversion: {e:?}"))),
            Err(Ok(e)) => classify(e),
            Err(Err(e)) => Err(HandlerError::Host(format!("{e:?}"))),
        }
    }
}

struct Cancel {
    fx: Arc<Fixture>,
    actors: Vec<Actor>,
}

impl Handler for Cancel {
    type Args = StreamArgs;

    fn name(&self) -> &'static str {
        "cancel"
    }

    fn actors(&self) -> &[Actor] {
        &self.actors
    }

    fn strategy(&self) -> BoxedStrategy<StreamArgs> {
        stream_args()
    }

    fn call(
        &self,
        env: &Env,
        _ctx: &CallContext,
        _actor: &Actor,
        args: &StreamArgs,
    ) -> Result<CallOutcome, HandlerError> {
        let result = with_client!(self.fx, env, |c| c.try_cancel(&args.stream));
        match result {
            Ok(Ok(())) => Ok(CallOutcome::Success),
            Ok(Err(e)) => Err(HandlerError::Other(format!("conversion: {e:?}"))),
            Err(Ok(e)) => classify(e),
            Err(Err(e)) => Err(HandlerError::Host(format!("{e:?}"))),
        }
    }
}

/// Re-registers the native contract on a restored `Env`. `register_at` re-runs
/// the constructor; restores happen only at the start of a case, when the
/// instance holds exactly the values the constructor writes again, so nothing
/// is lost. Reading the instance here is avoided on purpose: an access would
/// auto-restore an archived instance and hide what the suite is looking for.
fn restore_native(env: &Env, fx: &Fixture) {
    let id = addr(env, &fx.contract);
    let token = addr(env, &fx.token);
    match fx.variant {
        Variant::Buggy => env.register_at(&id, PaymentStream, (token,)),
        Variant::Fixed => env.register_at(&id, PaymentStreamFixed, (token,)),
    };
}

fn suite(variant: Variant, seed: u64, cases: u32) -> Suite {
    let (env, fx) = setup(variant);
    let fx = Arc::new(fx);
    let sender = Actor::from_address("sender", &addr(&env, &fx.sender));
    let recipient = Actor::from_address("recipient", &addr(&env, &fx.recipient));
    let model = StreamModel::default();
    let key: breakwater_pack_ttl_archival::StreamKey =
        Arc::new(|env: &Env, id: u64| DataKey::Stream(id).into_val(env));

    let restore_fx = fx.clone();
    let restore: breakwater_pack_ttl_archival::RestoreHook =
        Arc::new(move |env: &Env| restore_native(env, &restore_fx));
    let probe_fx = fx.clone();
    let probe: breakwater_pack_ttl_archival::StreamProbe = Arc::new(move |env: &Env, id: u64| {
        // Withdraw may legitimately have nothing to pay; cancel must settle.
        let w = with_client!(probe_fx, env, |c| c.try_withdraw(&id));
        match w {
            Ok(Ok(_)) => {}
            Err(Ok(e))
                if e == soroban_sdk::Error::from_contract_error(
                    StreamError::NothingToWithdraw as u32,
                ) => {}
            other => return Err(format!("withdraw after restore: {other:?}")),
        }
        let c = with_client!(probe_fx, env, |c| c.try_cancel(&id));
        match c {
            Ok(Ok(())) => Ok(()),
            other => Err(format!("cancel after restore: {other:?}")),
        }
    });

    let hook_fx = fx.clone();
    Suite::new(
        match variant {
            Variant::Buggy => "ttl-archival",
            Variant::Fixed => "ttl-archival-fixed",
        },
        env,
    )
    .config(SuiteConfig {
        seed,
        steps: 31..=34,
        cases,
        auth: AuthMode::MockAll,
        ..SuiteConfig::default()
    })
    .ledger_advance(LedgerAdvance::daily_then_silence(30, 90))
    .handler(CreateStream {
        fx: fx.clone(),
        actors: vec![sender.clone()],
    })
    .handler(Withdraw {
        fx: fx.clone(),
        actors: vec![recipient],
    })
    .handler(Cancel {
        fx: fx.clone(),
        actors: vec![sender],
    })
    .invariant(NoMidLifecycleExpiry {
        contract: fx.contract.clone(),
        model: model.clone(),
        key: key.clone(),
    })
    .invariant(InstanceAlive {
        contract: fx.contract.clone(),
        model: model.clone(),
    })
    .invariant(RestoreRestoresFunction {
        contract: fx.contract.clone(),
        model,
        key,
        restore,
        probe,
    })
    .on_restore(move |env| restore_native(env, &hook_fx))
}

#[test]
fn finds_the_stream_archived_mid_lifecycle_and_shrinks_it() {
    let started = Instant::now();
    let report = suite(Variant::Buggy, 11, 16).run().expect("suite runs");
    let elapsed = started.elapsed();
    eprintln!(
        "ttl-archival: {} cases, {} steps in {:?} ({:.2} ms/step)",
        report.cases_run,
        report.steps_run,
        elapsed,
        elapsed.as_secs_f64() * 1_000.0 / report.steps_run.max(1) as f64
    );
    let failure = report
        .failure()
        .expect("the buggy stream must expire mid-lifecycle");
    eprintln!("shrunk sequence:");
    for step in failure.log.steps() {
        eprintln!(
            "  #{} {:<14} {:<10} +{:>8} ledgers ({:>5.1} d)  args={}  -> {:?}",
            step.index,
            step.handler,
            step.actor.name(),
            step.gap.ledgers,
            f64::from(step.gap.ledgers) / f64::from(LEDGERS_PER_DAY),
            step.args,
            step.outcome
        );
    }
    eprintln!(
        "violation: {} ({})",
        failure.violation.message, failure.violation.source
    );
    eprintln!(
        "detail: {}",
        failure
            .violation
            .detail
            .as_ref()
            .map(|d| d.to_string())
            .unwrap_or_default()
    );

    assert_eq!(failure.violation.kind, ViolationKind::Invariant);
    assert_eq!(failure.violation.source, "no_mid_lifecycle_expiry");
    let detail = failure.violation.detail.as_ref().expect("detail");
    assert_eq!(detail["stream"], 0);
    assert!(detail["days_remaining"].as_f64().unwrap() > 200.0);
    assert_eq!(failure.log.steps()[0].handler, "create_stream");
    assert!(
        failure.log.len() <= 3,
        "shrunk to {} steps",
        failure.log.len()
    );
}

#[test]
fn same_seed_reproduces_the_same_sequence() {
    let a = suite(Variant::Buggy, 11, 16).run().expect("suite runs");
    let b = suite(Variant::Buggy, 11, 16).run().expect("suite runs");
    assert_eq!(a.failure().map(|f| &f.log), b.failure().map(|f| &f.log));
}

#[test]
fn fixed_contract_survives_the_same_profile() {
    let report = suite(Variant::Fixed, 11, 16).run().expect("suite runs");
    assert!(
        matches!(report.outcome, Outcome::Held),
        "{:?}",
        report.failure().map(|f| &f.violation)
    );
}
