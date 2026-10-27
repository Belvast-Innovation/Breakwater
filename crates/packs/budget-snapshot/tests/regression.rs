//! End to end: measure a suite, store the snapshot, change the contract and
//! let the comparison catch it.
//!
//! `Light` and `Heavy` are the same counter, except that `Heavy::increment`
//! also writes an audit entry to persistent storage: the kind of regression a
//! snapshot is for. The counter refuses to go above four, so some calls fail
//! and must not be measured.

use breakwater::proptest::prelude::*;
use breakwater::{
    Actor, CallContext, CallOutcome, Gap, Handler, HandlerError, ReplayStep, Suite, SuiteConfig,
    ViolationKind,
};
use breakwater_pack_budget_snapshot::{
    compare, BudgetError, BudgetReport, Comparison, Recorder, Resource, Snapshot, StatKind, Target,
    Tolerance, Toolchain,
};
use soroban_sdk::{contract, contracterror, contractimpl, symbol_short, Address, Env, Symbol};

const COUNT: Symbol = symbol_short!("COUNT");
const AUDIT: Symbol = symbol_short!("AUDIT");

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Error {
    Full = 1,
}

fn bump(env: &Env) -> Result<u32, Error> {
    let n: u32 = env.storage().instance().get(&COUNT).unwrap_or(0) + 1;
    if n > 4 {
        return Err(Error::Full);
    }
    env.storage().instance().set(&COUNT, &n);
    Ok(n)
}

#[contract]
pub struct Light;

#[contractimpl]
impl Light {
    pub fn increment(env: Env) -> Result<u32, Error> {
        bump(&env)
    }

    pub fn get(env: Env) -> u32 {
        env.storage().instance().get(&COUNT).unwrap_or(0)
    }
}

#[contract]
pub struct Heavy;

#[contractimpl]
impl Heavy {
    pub fn increment(env: Env) -> Result<u32, Error> {
        let n = bump(&env)?;
        env.storage().persistent().set(&AUDIT, &n);
        Ok(n)
    }

    pub fn get(env: Env) -> u32 {
        env.storage().instance().get(&COUNT).unwrap_or(0)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Variant {
    Light,
    Heavy,
}

/// Calls `increment`; with `read_back` it then reads the count through a
/// client, which is a second top-level invocation after the measured one.
struct Increment {
    contract: String,
    variant: Variant,
    read_back: bool,
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
        let result = match self.variant {
            Variant::Light => LightClient::new(env, &id).try_increment(),
            Variant::Heavy => HeavyClient::new(env, &id).try_increment(),
        };
        let outcome = match result {
            Ok(Ok(n)) => CallOutcome::returning(n),
            Ok(Err(e)) => return Err(HandlerError::Other(format!("conversion: {e:?}"))),
            Err(Ok(e)) => CallOutcome::ContractError(e as u32),
            Err(Err(e)) => return Err(HandlerError::Host(format!("{e:?}"))),
        };
        if self.read_back {
            match self.variant {
                Variant::Light => LightClient::new(env, &id).get(),
                Variant::Heavy => HeavyClient::new(env, &id).get(),
            };
        }
        Ok(outcome)
    }
}

fn config(seed: u64) -> SuiteConfig {
    SuiteConfig {
        seed,
        steps: 3..=8,
        cases: 8,
        ..SuiteConfig::default()
    }
}

fn toolchain() -> Toolchain {
    Toolchain {
        soroban_sdk: "28.0.0".into(),
        soroban_env_host: "28.0.2".into(),
        breakwater: "0.1.0".into(),
    }
}

fn measure(variant: Variant, seed: u64, read_back: bool) -> Result<BudgetReport, BudgetError> {
    measure_target(variant, seed, read_back, "increment")
}

fn measure_target(
    variant: Variant,
    seed: u64,
    read_back: bool,
    function: &'static str,
) -> Result<BudgetReport, BudgetError> {
    let env = Env::default();
    let contract = match variant {
        Variant::Light => env.register(Light, ()),
        Variant::Heavy => env.register(Heavy, ()),
    }
    .to_string()
    .to_string();
    let actor = Actor::generate(&env, "caller");
    let recorder = Recorder::new();
    let config = config(seed);
    let restore = contract.clone();
    let report = Suite::new("counter-budget", env)
        .config(config.clone())
        .handler(recorder.handler(
            Increment {
                contract: contract.clone(),
                variant,
                read_back,
                actors: vec![actor],
            },
            Target::new(contract, function),
        ))
        .invariant(recorder.invariant())
        .on_restore(move |env| {
            let id = Address::from_str(env, &restore);
            match variant {
                Variant::Light => env.register_at(&id, Light, ()),
                Variant::Heavy => env.register_at(&id, Heavy, ()),
            };
        })
        .run()
        .expect("suite runs");
    recorder.finish(&config, &report)
}

fn compared(baseline: &BudgetReport, current: &BudgetReport, tolerance: &str) -> Comparison {
    compare(
        &toolchain(),
        &baseline.snapshot,
        &toolchain(),
        &current.snapshot,
        Tolerance::parse(tolerance).expect("tolerance"),
    )
}

#[test]
fn a_measurement_is_a_pure_function_of_contract_and_seed() {
    let a = measure(Variant::Light, 7, false).expect("measures");
    let b = measure(Variant::Light, 7, false).expect("measures");
    assert_eq!(a, b);
    assert_eq!(a.to_json_string().unwrap(), b.to_json_string().unwrap());

    let stats = &a.snapshot.handlers["increment"];
    assert!(stats.samples > 0);
    // The counter refuses past four, so with up to eight steps some calls fail
    // and are counted apart instead of measured.
    assert!(a.skipped.get("increment").copied().unwrap_or(0) > 0);
    let writes = stats.get(Resource::WriteEntries).unwrap();
    assert_eq!(
        (writes.max, writes.p50),
        (1, 1),
        "only the instance is written"
    );
    assert!(stats.get(Resource::Instructions).unwrap().max > 0);
}

#[test]
fn an_unchanged_contract_passes_and_an_extra_write_is_caught() {
    let baseline = measure(Variant::Light, 7, false).expect("measures");

    let same = measure(Variant::Light, 7, false).expect("measures");
    match compared(&baseline, &same, "0%") {
        Comparison::Compared(v) => assert!(v.passed() && v.improvements.is_empty()),
        other => panic!("{other:?}"),
    }

    let heavy = measure(Variant::Heavy, 7, false).expect("measures");
    // Same seed, same generated steps: the digest is unchanged, so this is a
    // resource regression and not a stale snapshot.
    assert_eq!(
        baseline.snapshot.workload_digest,
        heavy.snapshot.workload_digest
    );
    let Comparison::Compared(verdict) = compared(&baseline, &heavy, "5%") else {
        panic!("the workload is identical");
    };
    assert!(!verdict.passed());
    let hit = |resource: Resource, stat: StatKind| {
        verdict
            .regressions
            .iter()
            .any(|f| f.handler == "increment" && f.resource == resource && f.stat == stat)
    };
    assert!(hit(Resource::WriteEntries, StatKind::Max));
    assert!(hit(Resource::WriteEntries, StatKind::P50));
    assert!(hit(Resource::WriteBytes, StatKind::Max));
    assert!(hit(Resource::PersistentEntryRentBumps, StatKind::Max));
    assert!(hit(Resource::Instructions, StatKind::Max));
    // Nothing else moved: the number of measured and failed calls is the same.
    assert!(verdict.sample_changes.is_empty());
    assert_eq!(baseline.skipped, heavy.skipped);

    // A tolerance wide enough to cover a doubled write count lets it through.
    match compared(&baseline, &heavy, "10000%") {
        Comparison::Compared(v) => assert!(v.passed()),
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_snapshot_file_carries_a_baseline_between_runs() {
    let dir = std::env::temp_dir().join(format!("breakwater-pack-{}", std::process::id()));
    let path = dir.join(".breakwater-snapshot.json");

    let baseline = measure(Variant::Light, 7, false).expect("measures");
    let mut file = Snapshot::new(toolchain());
    file.upsert(&baseline.suite, toolchain(), baseline.snapshot.clone())
        .expect("insert");
    file.write(&path).expect("writes");

    let stored = Snapshot::read(&path).expect("reads");
    let heavy = measure(Variant::Heavy, 7, false).expect("measures");
    let Comparison::Compared(verdict) = compare(
        &stored.toolchain,
        &stored.suites["counter-budget"],
        &toolchain(),
        &heavy.snapshot,
        Tolerance::default(),
    ) else {
        panic!("comparable");
    };
    assert!(!verdict.passed());

    // Another seed is another workload: stale, not a regression.
    let other_seed = measure(Variant::Light, 8, false).expect("measures");
    assert!(matches!(
        compare(
            &stored.toolchain,
            &stored.suites["counter-budget"],
            &toolchain(),
            &other_seed.snapshot,
            Tolerance::default(),
        ),
        Comparison::Stale(_)
    ));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_call_made_after_the_measured_one_is_refused_not_misread() {
    // Reading the count back is a second top-level invocation, so the host's
    // "last invocation" is `get`, not `increment`.
    match measure(Variant::Light, 7, true) {
        Err(BudgetError::Unattributed { handlers }) => assert!(handlers.contains("increment")),
        other => panic!("expected Unattributed, got {other:?}"),
    }
}

#[test]
fn a_target_that_is_not_what_the_handler_called_is_refused() {
    match measure_target(Variant::Light, 7, false, "get") {
        Err(BudgetError::Unattributed { handlers }) => assert!(handlers.contains("increment")),
        other => panic!("expected Unattributed, got {other:?}"),
    }
}

#[test]
fn a_suite_that_ends_on_a_violation_is_not_measured() {
    // An invariant that fires ends the sequence early, so the samples are
    // incomplete. The measurement must be refused, not truncated.
    let env = Env::default();
    let contract = env.register(Light, ()).to_string().to_string();
    let actor = Actor::generate(&env, "caller");
    let recorder = Recorder::new();
    let config = config(7);
    let restore = contract.clone();
    let report = Suite::new("counter-budget", env)
        .config(config.clone())
        .handler(recorder.handler(
            Increment {
                contract: contract.clone(),
                variant: Variant::Light,
                read_back: false,
                actors: vec![actor],
            },
            Target::new(contract, "increment"),
        ))
        .invariant(recorder.invariant())
        .invariant(AlwaysFails)
        .on_restore(move |env| {
            env.register_at(&Address::from_str(env, &restore), Light, ());
        })
        .run()
        .expect("suite runs");
    assert_eq!(
        report.failure().map(|f| f.violation.kind),
        Some(ViolationKind::Invariant)
    );
    assert!(matches!(
        recorder.finish(&config, &report),
        Err(BudgetError::Violated { .. })
    ));
}

struct AlwaysFails;

impl breakwater::Invariant for AlwaysFails {
    fn name(&self) -> &'static str {
        "always_fails"
    }

    fn check(&self, _env: &Env, _log: &breakwater::TxLog) -> Result<(), breakwater::Violation> {
        Err(breakwater::Violation::new("by design"))
    }
}

/// Reports success without invoking anything: the "no-op for unsuitable
/// arguments" path a handler is allowed to have.
struct Idle {
    actors: Vec<Actor>,
}

impl Handler for Idle {
    type Args = ();

    fn name(&self) -> &'static str {
        "idle"
    }

    fn actors(&self) -> &[Actor] {
        &self.actors
    }

    fn strategy(&self) -> BoxedStrategy<()> {
        Just(()).boxed()
    }

    fn call(
        &self,
        _env: &Env,
        _ctx: &CallContext,
        _actor: &Actor,
        _args: &(),
    ) -> Result<CallOutcome, HandlerError> {
        Ok(CallOutcome::Success)
    }
}

/// Replays exactly `handlers` (names, in order) against a `Light` counter in
/// which both `increment` and `idle` are measured against `Light::increment`.
fn replay(handlers: &[&str]) -> Result<BudgetReport, BudgetError> {
    let env = Env::default();
    let contract = env.register(Light, ()).to_string().to_string();
    let actor = Actor::generate(&env, "caller");
    let recorder = Recorder::new();
    let config = config(7);
    let restore = contract.clone();
    let steps: Vec<ReplayStep> = handlers
        .iter()
        .map(|h| ReplayStep {
            handler: (*h).to_owned(),
            actor: "caller".to_owned(),
            args: serde_json::Value::Null,
            gap: Gap::default(),
        })
        .collect();
    let report = Suite::new("counter-budget", env)
        .config(config.clone())
        .handler(recorder.handler(
            Increment {
                contract: contract.clone(),
                variant: Variant::Light,
                read_back: false,
                actors: vec![actor.clone()],
            },
            Target::new(contract.clone(), "increment"),
        ))
        .handler(recorder.handler(
            Idle {
                actors: vec![actor],
            },
            Target::new(contract, "increment"),
        ))
        .invariant(recorder.invariant())
        .on_restore(move |env| {
            env.register_at(&Address::from_str(env, &restore), Light, ());
        })
        .replay(&steps)
        .expect("replay runs");
    assert!(report.failure().is_none(), "{:?}", report.failure());
    recorder.finish(&config, &report)
}

#[test]
fn a_success_without_a_call_is_not_credited_with_the_previous_step() {
    // `increment` invokes the target; `idle` then succeeds without invoking
    // anything, so the host's last invocation is still the one `increment`
    // made, and a target check alone would sample it a second time.
    match replay(&["increment", "idle"]) {
        Err(BudgetError::Unattributed { handlers }) => {
            assert!(handlers.contains("`idle` (1)"), "{handlers}");
            assert!(!handlers.contains("increment"), "{handlers}");
        }
        other => panic!("expected Unattributed, got {other:?}"),
    }
    // Also when nothing at all has been invoked yet.
    assert!(matches!(
        replay(&["idle"]),
        Err(BudgetError::Unattributed { .. })
    ));
}

#[test]
fn two_real_calls_with_identical_costs_are_still_two_samples() {
    // The guard must not be a comparison of consecutive readings: the host's
    // costs are deterministic, so a repeated call can read exactly the same.
    let report = replay(&["increment", "increment"]).expect("measures");
    assert_eq!(report.snapshot.handlers["increment"].samples, 2);
    assert!(!report.snapshot.handlers.contains_key("idle"));
}

#[test]
fn a_target_with_no_function_name_never_matches() {
    match measure_target(Variant::Light, 7, false, "") {
        Err(BudgetError::Unattributed { handlers }) => assert!(handlers.contains("increment")),
        other => panic!("expected Unattributed, got {other:?}"),
    }
}
