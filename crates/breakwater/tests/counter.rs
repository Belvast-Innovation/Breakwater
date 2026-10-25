//! End-to-end check of the framework on a minimal counter contract.
//!
//! The invariant "count never exceeds 10" is violated by the eleventh
//! increment. The suite must find it and shrink the sequence to exactly
//! eleven steps.

use breakwater::proptest::prelude::*;
use breakwater::{
    Actor, CallContext, CallOutcome, Exporter, Handler, HandlerError, Invariant, Outcome, Suite,
    TxLog, Violation, ViolationKind,
};
use soroban_sdk::{contract, contractimpl, symbol_short, Address, Env, Symbol};

const COUNT: Symbol = symbol_short!("COUNT");
const LIMIT: u32 = 10;

#[contract]
pub struct Counter;

#[contractimpl]
impl Counter {
    pub fn increment(env: Env) -> u32 {
        let n: u32 = env.storage().instance().get(&COUNT).unwrap_or(0) + 1;
        env.storage().instance().set(&COUNT, &n);
        n
    }

    pub fn get(env: Env) -> u32 {
        env.storage().instance().get(&COUNT).unwrap_or(0)
    }
}

struct Increment {
    contract: String,
    actors: Vec<Actor>,
}

impl Handler for Increment {
    type Args = ();

    fn name(&self) -> &'static str {
        "increment"
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
        _ctx: &CallContext,
        _actor: &Actor,
        _args: &(),
    ) -> Result<CallOutcome, HandlerError> {
        let id = Address::from_str(env, &self.contract);
        match CounterClient::new(env, &id).try_increment() {
            Ok(Ok(_)) => Ok(CallOutcome::Success),
            Ok(Err(e)) => Err(HandlerError::Other(format!("conversion error: {e:?}"))),
            Err(Ok(e)) => Ok(CallOutcome::ContractError(e.get_code())),
            Err(Err(e)) => Err(HandlerError::Host(format!("{e:?}"))),
        }
    }
}

struct NeverAbove {
    contract: String,
}

impl Invariant for NeverAbove {
    fn name(&self) -> &'static str {
        "count_never_above_limit"
    }

    fn check(&self, env: &Env, _log: &TxLog) -> Result<(), Violation> {
        let id = Address::from_str(env, &self.contract);
        let count = CounterClient::new(env, &id).get();
        if count > LIMIT {
            return Err(Violation::new("count exceeded the limit").with_detail(count));
        }
        Ok(())
    }
}

fn suite(seed: u64, steps: std::ops::RangeInclusive<usize>) -> Suite {
    let env = Env::default();
    let contract = env.register(Counter, ()).to_string().to_string();
    let owner = Actor::generate(&env, "owner");
    let restore_id = contract.clone();

    Suite::new("counter", env)
        .seed(seed)
        .steps(steps)
        .cases(32)
        .handler(Increment {
            contract: contract.clone(),
            actors: vec![owner],
        })
        .invariant(NeverAbove { contract })
        .on_restore(move |env| {
            env.register_at(&Address::from_str(env, &restore_id), Counter, ());
        })
}

#[test]
fn finds_the_eleventh_increment_and_shrinks_to_eleven_steps() {
    let report = suite(42, 1..=30).run().expect("suite runs");

    let failure = report.failure().expect("the limit must be exceeded");
    assert_eq!(failure.violation.kind, ViolationKind::Invariant);
    assert_eq!(failure.violation.source, "count_never_above_limit");
    assert_eq!(failure.violation.detail, Some(serde_json::json!(LIMIT + 1)));
    assert_eq!(
        failure.failing_step, LIMIT as usize,
        "violation is detected after step 11"
    );
    assert_eq!(
        failure.log.len(),
        LIMIT as usize + 1,
        "shrunk to exactly eleven steps"
    );
    assert!(failure
        .log
        .steps()
        .iter()
        .all(|s| s.handler == "increment" && s.outcome.is_success()));
    assert_eq!(report.seed, 42);
}

#[test]
fn holds_when_sequences_are_too_short_to_break_it() {
    let report = suite(7, 1..=LIMIT as usize).run().expect("suite runs");
    assert!(
        matches!(report.outcome, Outcome::Held),
        "{:?}",
        report.outcome
    );
}

#[test]
fn same_seed_reproduces_the_same_failure() {
    let a = suite(123, 1..=40).run().expect("suite runs");
    let b = suite(123, 1..=40).run().expect("suite runs");
    let (fa, fb) = (a.failure().expect("fails"), b.failure().expect("fails"));
    assert_eq!(fa.log, fb.log);
    assert_eq!(a.cases_run, b.cases_run);
}

#[test]
fn json_export_carries_the_shrunk_steps() {
    let report = suite(42, 1..=30).run().expect("suite runs");
    let json = Exporter::to_json(&report).expect("serialises");
    assert_eq!(json["suite"], "counter");
    assert_eq!(json["seed"], 42);
    assert_eq!(json["outcome"]["status"], "violated");
    assert_eq!(json["outcome"]["failing_step"], LIMIT);
    assert_eq!(
        json["outcome"]["log"]["steps"].as_array().map(Vec::len),
        Some(LIMIT as usize + 1)
    );
    assert_eq!(json["outcome"]["log"]["steps"][0]["handler"], "increment");
    assert_eq!(json["outcome"]["violation"]["kind"], "invariant");
}
