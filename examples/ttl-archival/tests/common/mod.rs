//! Shared fixture, handlers and suite builder for the ttl-archival demo.
//! Used by `suite.rs` (the fuzzing suite) and by exported reproductions.
#![allow(dead_code)]

// Breakwater suite for the payment-stream demo, under `MockAll` auth with a
// usage profile: one call a day for thirty days, then ninety days of silence.
//
// The test `Env` sets the network's minimum persistent TTL to 30 days. That
// is a demo setting: the real value is a network parameter (see the network
// limits on Stellar Lab) and tends to be raised over time. The point is the
// shape, not the number: `PaymentStream` never extends a TTL, so a one-year
// stream keeps exactly the minimum TTL it was born with and is archived with
// most of its lifetime ahead. `PaymentStreamFixed` extends every entry it
// touches to the stream's remaining duration and survives the same profile.

use std::sync::Arc;

use breakwater::proptest::prelude::*;
use breakwater::{
    Actor, AuthMode, CallContext, CallOutcome, Handler, HandlerError, LedgerAdvance, ReplayStep,
    RunReport, Suite, SuiteConfig, LEDGERS_PER_DAY,
};
use breakwater_pack_ttl_archival::{
    InstanceAlive, NoMidLifecycleExpiry, NoUnboundedTtlExtension, ResourceBudget,
    RestoreCostWithinBudget, RestoreRestoresFunction, StreamModel,
};
use example_ttl_archival::fixed::{PaymentStreamFixed, PaymentStreamFixedClient};
use example_ttl_archival::{DataKey, PaymentStream, PaymentStreamClient, StreamError};
use serde::{Deserialize, Serialize};
use soroban_sdk::testutils::{Address as _, EnvTestConfig, Ledger as _};
use soroban_sdk::token::StellarAssetClient;
use soroban_sdk::xdr::ScErrorType;
use soroban_sdk::{Address, Env, IntoVal};

/// 1 USDC in stroops.
pub const USDC: i128 = 10_000_000;
/// Demo network setting: new persistent entries live 30 days.
pub const MIN_PERSISTENT_TTL_DAYS: u32 = 30;
pub const YEAR: u32 = 365 * LEDGERS_PER_DAY;

/// Which contract the suite drives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    Buggy,
    Fixed,
}

pub struct Fixture {
    pub variant: Variant,
    pub contract: String,
    pub token: String,
    pub sender: String,
    pub recipient: String,
}

pub fn addr(env: &Env, strkey: &str) -> Address {
    Address::from_str(env, strkey)
}

pub fn setup(variant: Variant) -> (Env, Fixture) {
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
pub fn classify(err: soroban_sdk::Error) -> Result<CallOutcome, HandlerError> {
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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreateArgs {
    pub amount_usdc: i64,
    pub duration_ledgers: u32,
}

pub struct CreateStream {
    pub fx: Arc<Fixture>,
    pub actors: Vec<Actor>,
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
            Ok(Ok(id)) => Ok(CallOutcome::returning(id)),
            Ok(Err(e)) => Err(HandlerError::Other(format!("conversion: {e:?}"))),
            Err(Ok(e)) => classify(e),
            Err(Err(e)) => Err(HandlerError::Host(format!("{e:?}"))),
        }
    }
}

/// Renders a token amount for the log: whole units when exact, else stroops.
pub fn amount_value(stroops: i128) -> serde_json::Value {
    if stroops % USDC == 0 {
        serde_json::json!({ "usdc": i64::try_from(stroops / USDC).unwrap_or(i64::MAX) })
    } else {
        serde_json::json!({ "stroops": stroops.to_string() })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StreamArgs {
    pub stream: u64,
}

pub fn stream_args() -> BoxedStrategy<StreamArgs> {
    (0u64..3).prop_map(|stream| StreamArgs { stream }).boxed()
}

pub struct Withdraw {
    pub fx: Arc<Fixture>,
    pub actors: Vec<Actor>,
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
            Ok(Ok(paid)) => Ok(CallOutcome::returning(amount_value(paid))),
            Ok(Err(e)) => Err(HandlerError::Other(format!("conversion: {e:?}"))),
            Err(Ok(e)) => classify(e),
            Err(Err(e)) => Err(HandlerError::Host(format!("{e:?}"))),
        }
    }
}

pub struct Cancel {
    pub fx: Arc<Fixture>,
    pub actors: Vec<Actor>,
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
            Ok(Ok(())) => Ok(CallOutcome::returning(())),
            Ok(Err(e)) => Err(HandlerError::Other(format!("conversion: {e:?}"))),
            Err(Ok(e)) => classify(e),
            Err(Err(e)) => Err(HandlerError::Host(format!("{e:?}"))),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BumpArgs {
    pub stream: u64,
    pub ledgers: u32,
}

/// `bump_ttl(stream, ledgers)`: the keep-alive entry point anyone may call on
/// the buggy contract. The strategy favours the largest extension the network
/// accepts (`max_entry_ttl - 1` in the test `Env`).
pub struct BumpTtl {
    pub fx: Arc<Fixture>,
    pub actors: Vec<Actor>,
}

/// Largest TTL the demo `Env` accepts (SDK default `max_entry_ttl` minus one).
pub const MAX_TTL: u32 = 6_312_000 - 1;

impl Handler for BumpTtl {
    type Args = BumpArgs;

    fn name(&self) -> &'static str {
        "bump_ttl"
    }

    fn actors(&self) -> &[Actor] {
        &self.actors
    }

    fn strategy(&self) -> BoxedStrategy<BumpArgs> {
        (
            0u64..3,
            prop_oneof![
                4 => Just(MAX_TTL),
                1 => (30 * LEDGERS_PER_DAY)..=MAX_TTL,
            ],
        )
            .prop_map(|(stream, ledgers)| BumpArgs { stream, ledgers })
            .boxed()
    }

    fn call(
        &self,
        env: &Env,
        _ctx: &CallContext,
        _actor: &Actor,
        args: &BumpArgs,
    ) -> Result<CallOutcome, HandlerError> {
        let result = with_client!(self.fx, env, |c| c
            .try_bump_ttl(&args.stream, &args.ledgers));
        match result {
            Ok(Ok(())) => Ok(CallOutcome::returning(())),
            Ok(Err(e)) => Err(HandlerError::Other(format!("conversion: {e:?}"))),
            Err(Ok(e)) => classify(e),
            Err(Err(e)) => Err(HandlerError::Host(format!("{e:?}"))),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ManyArgs {
    pub streams: Vec<u64>,
}

/// `withdraw_many(streams)`: one call that touches several stream entries.
pub struct WithdrawMany {
    pub fx: Arc<Fixture>,
    pub actors: Vec<Actor>,
}

impl Handler for WithdrawMany {
    type Args = ManyArgs;

    fn name(&self) -> &'static str {
        "withdraw_many"
    }

    fn actors(&self) -> &[Actor] {
        &self.actors
    }

    fn strategy(&self) -> BoxedStrategy<ManyArgs> {
        prop::collection::btree_set(0u64..4, 1..=4)
            .prop_map(|set| ManyArgs {
                streams: set.into_iter().collect(),
            })
            .boxed()
    }

    fn call(
        &self,
        env: &Env,
        _ctx: &CallContext,
        _actor: &Actor,
        args: &ManyArgs,
    ) -> Result<CallOutcome, HandlerError> {
        let ids = soroban_sdk::Vec::from_slice(env, &args.streams);
        let result = with_client!(self.fx, env, |c| c.try_withdraw_many(&ids));
        match result {
            Ok(Ok(paid)) => Ok(CallOutcome::returning(amount_value(paid))),
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
pub fn restore_native(env: &Env, fx: &Fixture) {
    let id = addr(env, &fx.contract);
    let token = addr(env, &fx.token);
    match fx.variant {
        Variant::Buggy => env.register_at(&id, PaymentStream, (token,)),
        Variant::Fixed => env.register_at(&id, PaymentStreamFixed, (token,)),
    };
}

/// Default configuration: enough steps to reach the 90-day gap of the
/// `daily_then_silence(30, 90)` profile.
pub fn default_config(seed: u64, cases: u32) -> SuiteConfig {
    SuiteConfig {
        seed,
        steps: 31..=34,
        cases,
        auth: AuthMode::MockAll,
        ..SuiteConfig::default()
    }
}

pub fn suite(variant: Variant, seed: u64, cases: u32) -> Suite {
    suite_with_config(variant, default_config(seed, cases))
}

/// Replays exported steps against a freshly built buggy demo and returns the
/// report. Used by generated `repro_*.rs` tests.
pub fn replay_json(json: &str) -> RunReport {
    let steps: Vec<ReplayStep> = serde_json::from_str(json).expect("replay steps are valid JSON");
    suite_with_config(Variant::Buggy, default_config(0, 1))
        .replay(&steps)
        .expect("replay runs")
}

pub fn suite_with_config(variant: Variant, config: SuiteConfig) -> Suite {
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
    .config(config)
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

/// A suite for the keep-alive defect: streams are created and anyone calls
/// `bump_ttl`. The only invariant is `NoUnboundedTtlExtension` with one day of
/// slack past each stream's end.
pub fn keep_alive_suite(variant: Variant, seed: u64, cases: u32) -> Suite {
    let (env, fx) = setup(variant);
    let fx = Arc::new(fx);
    let sender = Actor::from_address("sender", &addr(&env, &fx.sender));
    let anyone = Actor::generate(&env, "anyone");
    let key: breakwater_pack_ttl_archival::StreamKey =
        Arc::new(|env: &Env, id: u64| DataKey::Stream(id).into_val(env));
    let hook_fx = fx.clone();
    Suite::new("ttl-archival-keep-alive", env)
        .config(SuiteConfig {
            seed,
            steps: 2..=8,
            cases,
            auth: AuthMode::MockAll,
            ..SuiteConfig::default()
        })
        .ledger_advance(LedgerAdvance::Fixed {
            ledgers: LEDGERS_PER_DAY,
            seconds: 86_400,
        })
        .handler(CreateStream {
            fx: fx.clone(),
            actors: vec![sender],
        })
        .handler(BumpTtl {
            fx: fx.clone(),
            actors: vec![anyone],
        })
        .invariant(NoUnboundedTtlExtension {
            contract: fx.contract.clone(),
            model: StreamModel::default(),
            key,
            watch_instance: true,
            slack_ledgers: LEDGERS_PER_DAY,
            max_extension_ledgers: None,
        })
        .on_restore(move |env| restore_native(env, &hook_fx))
}

/// A suite for the restoration-cost defect: streams are created during the
/// active month, then a single `withdraw_many` after the silence restores all
/// of them at once. The only invariant is `RestoreCostWithinBudget`.
pub fn restore_cost_suite(
    variant: Variant,
    seed: u64,
    cases: u32,
    budget: ResourceBudget,
) -> Suite {
    let (env, fx) = setup(variant);
    let fx = Arc::new(fx);
    let sender = Actor::from_address("sender", &addr(&env, &fx.sender));
    let recipient = Actor::from_address("recipient", &addr(&env, &fx.recipient));
    let hook_fx = fx.clone();
    Suite::new("ttl-archival-restore-cost", env)
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
            actors: vec![sender],
        })
        .handler(WithdrawMany {
            fx: fx.clone(),
            actors: vec![recipient],
        })
        .invariant(RestoreCostWithinBudget::new(
            fx.contract.clone(),
            StreamModel::default(),
            Arc::new(|env: &Env, id: u64| DataKey::Stream(id).into_val(env)),
            vec!["withdraw_many".into()],
            budget,
        ))
        .on_restore(move |env| restore_native(env, &hook_fx))
}
