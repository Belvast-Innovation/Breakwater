//! Invariants about TTL and archival of contract state.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use breakwater::{Invariant, TxLog, Violation, LEDGERS_PER_DAY};
use soroban_sdk::testutils::{EnvTestConfig, Ledger as _};
use soroban_sdk::{Address, Env, Val};

use crate::streams::{LiveStream, StreamModel};
use crate::ttl::{ttl, Entry};

/// Builds the persistent key of a stream for the contract under test.
pub type StreamKey = Arc<dyn Fn(&Env, u64) -> Val + Send + Sync>;

fn days(ledgers: i64) -> f64 {
    ledgers as f64 / f64::from(LEDGERS_PER_DAY)
}

fn stream_detail(stream: &LiveStream, now: u32, ttl: Option<i64>) -> serde_json::Value {
    serde_json::json!({
        "stream": stream.id,
        "created_step": stream.created_step,
        "created_at_ledger": stream.created_at,
        "end_ledger": stream.end_ledger,
        "now_ledger": now,
        "days_since_creation": days(i64::from(now) - i64::from(stream.created_at)),
        "days_remaining": days(i64::from(stream.end_ledger) - i64::from(now)),
        "ttl_ledgers": ttl,
    })
}

/// No running stream's persistent entry has run out of TTL.
///
/// A running stream is one the log created, did not cancel, and whose end
/// ledger is still ahead. Its entry must have a positive TTL: once the TTL
/// hits zero the entry is archived at the next ledger, and every later
/// transaction that touches it must restore it first (or fail if it does not
/// carry a restore list).
#[derive(Clone)]
pub struct NoMidLifecycleExpiry {
    /// Strkey of the contract under test.
    pub contract: String,
    /// How to read streams out of the log.
    pub model: StreamModel,
    /// Persistent key of stream `id`.
    pub key: StreamKey,
}

impl Invariant for NoMidLifecycleExpiry {
    fn name(&self) -> &'static str {
        "no_mid_lifecycle_expiry"
    }

    fn check(&self, env: &Env, log: &TxLog) -> Result<(), Violation> {
        let now = env.ledger().sequence();
        let contract = Address::from_str(env, &self.contract);
        for stream in self.model.running_streams(log, now) {
            let ttl = ttl(
                env,
                &contract,
                &Entry::Persistent((self.key)(env, stream.id)),
            );
            match ttl {
                Some(t) if t > 0 => {}
                Some(0) => {
                    return Err(Violation::new(format!(
                        "stream {} is on its last live ledger with {:.0} days left to run",
                        stream.id,
                        days(i64::from(stream.end_ledger) - i64::from(now)),
                    ))
                    .with_detail(stream_detail(&stream, now, Some(0))));
                }
                Some(t) => {
                    return Err(Violation::new(format!(
                        "stream {} is archived with {:.0} days left to run (TTL ran out {:.1} days ago)",
                        stream.id,
                        days(i64::from(stream.end_ledger) - i64::from(now)),
                        days(-t)
                    ))
                    .with_detail(stream_detail(&stream, now, Some(t))));
                }
                None => {
                    return Err(Violation::new(format!(
                        "stream {} has no persistent entry",
                        stream.id
                    ))
                    .with_detail(stream_detail(&stream, now, None)));
                }
            }
        }
        Ok(())
    }
}

/// While any stream is running, the contract instance has not expired.
///
/// Instance storage holds the token address and the id counter; if the
/// instance is archived, every call to the contract needs a restore first.
#[derive(Clone, Debug)]
pub struct InstanceAlive {
    /// Strkey of the contract under test.
    pub contract: String,
    /// How to read streams out of the log.
    pub model: StreamModel,
}

impl Invariant for InstanceAlive {
    fn name(&self) -> &'static str {
        "instance_alive"
    }

    fn check(&self, env: &Env, log: &TxLog) -> Result<(), Violation> {
        let now = env.ledger().sequence();
        let running = self.model.running_streams(log, now);
        if running.is_empty() {
            return Ok(());
        }
        let contract = Address::from_str(env, &self.contract);
        match ttl(env, &contract, &Entry::Instance) {
            Some(t) if t > 0 => Ok(()),
            t => Err(Violation::new(format!(
                "contract instance expired while {} stream(s) are running",
                running.len()
            ))
            .with_detail(serde_json::json!({
                "ttl_ledgers": t,
                "now_ledger": now,
                "running_streams": running.iter().map(|s| s.id).collect::<Vec<_>>(),
            }))),
        }
    }
}

/// Re-registers native test contracts on a restored `Env`.
pub type RestoreHook = Arc<dyn Fn(&Env) + Send + Sync>;
/// Exercises one stream on a restored `Env`; `Err` describes what broke.
pub type StreamProbe = Arc<dyn Fn(&Env, u64) -> Result<(), String> + Send + Sync>;

/// After an archived stream entry is restored, the contract still works on it.
///
/// `soroban-sdk` 28 restores an archived persistent or instance entry on
/// access, the way a transaction with a restore list would on-chain. For each
/// running stream whose entry is archived, this invariant takes a throwaway
/// copy of the ledger (`Env::from_ledger_snapshot`), lets `probe` call the
/// contract there, and reports any failure. The fuzzed `Env` is untouched.
#[derive(Clone)]
pub struct RestoreRestoresFunction {
    /// Strkey of the contract under test.
    pub contract: String,
    /// How to read streams out of the log.
    pub model: StreamModel,
    /// Persistent key of stream `id`.
    pub key: StreamKey,
    /// Re-registers native contracts on the throwaway `Env`.
    pub restore: RestoreHook,
    /// Calls the contract on the throwaway `Env` for one stream.
    pub probe: StreamProbe,
}

impl Invariant for RestoreRestoresFunction {
    fn name(&self) -> &'static str {
        "restore_restores_function"
    }

    fn check(&self, env: &Env, log: &TxLog) -> Result<(), Violation> {
        let now = env.ledger().sequence();
        let contract = Address::from_str(env, &self.contract);
        let archived: Vec<LiveStream> = self
            .model
            .running_streams(log, now)
            .into_iter()
            .filter(|s| matches!(ttl(env, &contract, &Entry::Persistent((self.key)(env, s.id))), Some(t) if t < 0))
            .collect();
        if archived.is_empty() {
            return Ok(());
        }

        let mut scratch = Env::from_ledger_snapshot(env.to_ledger_snapshot());
        scratch.set_config(EnvTestConfig {
            capture_snapshot_at_drop: false,
        });
        scratch.mock_all_auths();
        scratch.cost_estimate().budget().reset_unlimited();
        (self.restore)(&scratch);

        for stream in archived {
            if let Err(reason) = (self.probe)(&scratch, stream.id) {
                return Err(Violation::new(format!(
                    "stream {} does not work after restoration: {reason}",
                    stream.id
                ))
                .with_detail(stream_detail(&stream, now, None)));
            }
        }
        Ok(())
    }
}

/// No entry is kept alive beyond what the business logic justifies.
///
/// `extend_ttl` can only lengthen a TTL, so a public, unauthenticated
/// extension entry point lets anyone keep entries alive indefinitely at the
/// contract's rent. A stream's entry is justified until the stream's end
/// ledger plus `slack_ledgers`; the instance is justified until the latest
/// live stream's end plus the slack (or `slack_ledgers` from now when no
/// stream is live). With `max_extension_ledgers` set, no watched entry may
/// have a `live_until` beyond `now + max_extension_ledgers` either.
#[derive(Clone)]
pub struct NoUnboundedTtlExtension {
    /// Strkey of the contract under test.
    pub contract: String,
    /// How to read streams out of the log.
    pub model: StreamModel,
    /// Persistent key of stream `id`.
    pub key: StreamKey,
    /// Also watch the contract instance.
    pub watch_instance: bool,
    /// Ledgers an entry may outlive its stream's end.
    pub slack_ledgers: u32,
    /// Absolute cap on `live_until - now`, if any.
    pub max_extension_ledgers: Option<u32>,
}

impl NoUnboundedTtlExtension {
    /// The `live_until` of `entry` if it exceeds `justified_until` or the
    /// absolute cap.
    fn too_far(
        &self,
        env: &Env,
        contract: &Address,
        entry: &Entry,
        now: u32,
        justified_until: u32,
    ) -> Option<(u32, u32)> {
        let cap = self
            .max_extension_ledgers
            .map_or(u32::MAX, |m| now.saturating_add(m));
        // Any write or restoration gives an entry at least the network minimum,
        // so that much is never an extension anyone chose.
        let minimum = now.saturating_add(env.ledger().get().min_persistent_entry_ttl);
        let bound = justified_until.max(minimum).min(cap);
        crate::ttl::live_until(env, contract, entry)
            .filter(|lu| *lu > bound)
            .map(|lu| (lu, bound))
    }
}

impl Invariant for NoUnboundedTtlExtension {
    fn name(&self) -> &'static str {
        "no_unbounded_ttl_extension"
    }

    fn check(&self, env: &Env, log: &TxLog) -> Result<(), Violation> {
        let now = env.ledger().sequence();
        let contract = Address::from_str(env, &self.contract);
        let streams = self.model.live_streams(log);
        for stream in &streams {
            let entry = Entry::Persistent((self.key)(env, stream.id));
            let justified = stream.end_ledger.saturating_add(self.slack_ledgers);
            if let Some((lu, bound)) = self.too_far(env, &contract, &entry, now, justified) {
                return Err(Violation::new(format!(
                    "stream {} is kept alive {:.1} days beyond what it needs",
                    stream.id,
                    days(i64::from(lu) - i64::from(bound))
                ))
                .with_detail(serde_json::json!({
                    "stream": stream.id,
                    "live_until": lu,
                    "justified_until": bound,
                    "end_ledger": stream.end_ledger,
                    "now_ledger": now,
                })));
            }
        }
        if self.watch_instance {
            let latest_end = streams.iter().map(|s| s.end_ledger).max().unwrap_or(now);
            let justified = latest_end.saturating_add(self.slack_ledgers);
            if let Some((lu, bound)) =
                self.too_far(env, &contract, &Entry::Instance, now, justified)
            {
                return Err(Violation::new(format!(
                    "contract instance is kept alive {:.1} days beyond what any stream needs",
                    days(i64::from(lu) - i64::from(bound))
                ))
                .with_detail(serde_json::json!({
                    "instance_live_until": lu,
                    "justified_until": bound,
                    "now_ledger": now,
                })));
            }
        }
        Ok(())
    }
}

/// Resource ceilings for one step, read from the host's invocation metering.
///
/// Restoring archived entries shows up as disk reads and writes the call
/// would not otherwise do, and a call that restores several entries at once
/// can exceed the network's per-transaction limits
/// (`INVOKE_HOST_FUNCTION_RESOURCE_LIMIT_EXCEEDED`). `None` disables a ceiling.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResourceBudget {
    /// CPU instructions.
    pub max_instructions: Option<i64>,
    /// Memory bytes.
    pub max_mem_bytes: Option<i64>,
    /// Entries read from disk, which is where archived entries come from.
    pub max_disk_read_entries: Option<u32>,
    /// Bytes read from disk.
    pub max_disk_read_bytes: Option<u32>,
    /// Bytes written, including restored entries.
    pub max_write_bytes: Option<u32>,
    /// Entries written, including restored entries.
    pub max_write_entries: Option<u32>,
}

/// A step that touches archived entries stays within a [`ResourceBudget`].
///
/// The core evaluates invariants right after the step on the same `Env`, and
/// `env.cost_estimate()` reports the last top-level invocation, so the numbers
/// are those of the handler's call. The invariant remembers every watched
/// entry's `live_until` from its previous check; a step is measured only if
/// one of the measured handlers ran while at least one of those entries was
/// archived, because the test host also counts the first read of a live entry
/// as a disk read. The SDK meters a native test contract without Wasm
/// instantiation and without the transaction's own size, so the values are an
/// approximation of what the network would charge; disk reads and writes do
/// include the restored entries.
pub struct RestoreCostWithinBudget {
    /// Strkey of the contract under test.
    pub contract: String,
    /// How to read streams out of the log.
    pub model: StreamModel,
    /// Persistent key of stream `id`.
    pub key: StreamKey,
    /// Handlers to measure; empty means every handler.
    pub handlers: Vec<String>,
    /// Ceilings.
    pub budget: ResourceBudget,
    /// `live_until` of every watched entry at the previous check, keyed by
    /// stream id (`None` for the instance), with the log length it was taken at.
    seen: Mutex<(usize, HashMap<Option<u64>, u32>)>,
}

impl RestoreCostWithinBudget {
    /// Creates the invariant; `handlers` empty means every handler is measured.
    pub fn new(
        contract: String,
        model: StreamModel,
        key: StreamKey,
        handlers: Vec<String>,
        budget: ResourceBudget,
    ) -> Self {
        Self {
            contract,
            model,
            key,
            handlers,
            budget,
            seen: Mutex::new((0, HashMap::new())),
        }
    }

    /// `live_until` of the instance and of every stream that is still running
    /// at `now`. A finished stream's entry may expire; that is not archival
    /// mid-lifecycle and is not counted.
    fn snapshot(
        &self,
        env: &Env,
        contract: &Address,
        log: &TxLog,
        now: u32,
    ) -> HashMap<Option<u64>, u32> {
        let mut map = HashMap::new();
        let running = self.model.running_streams(log, now);
        if !running.is_empty() {
            if let Some(lu) = crate::ttl::live_until(env, contract, &Entry::Instance) {
                map.insert(None, lu);
            }
        }
        for stream in running {
            let entry = Entry::Persistent((self.key)(env, stream.id));
            if let Some(lu) = crate::ttl::live_until(env, contract, &entry) {
                map.insert(Some(stream.id), lu);
            }
        }
        map
    }
}

impl Invariant for RestoreCostWithinBudget {
    fn name(&self) -> &'static str {
        "restore_cost_within_budget"
    }

    fn check(&self, env: &Env, log: &TxLog) -> Result<(), Violation> {
        let now = env.ledger().sequence();
        let contract = Address::from_str(env, &self.contract);
        let current = self.snapshot(env, &contract, log, now);
        let Ok(mut seen) = self.seen.lock() else {
            return Ok(());
        };
        // A shorter log means a new sequence: forget the previous one.
        let previous = if log.len() > seen.0 {
            std::mem::take(&mut seen.1)
        } else {
            HashMap::new()
        };
        *seen = (log.len(), current);

        let Some(last) = log.last() else {
            return Ok(());
        };
        if !last.outcome.is_success() {
            return Ok(());
        }
        if !self.handlers.is_empty() && !self.handlers.iter().any(|h| h == &last.handler) {
            return Ok(());
        }
        let still_running: std::collections::HashSet<u64> = self
            .model
            .running_streams(log, now)
            .into_iter()
            .map(|s| s.id)
            .collect();
        let archived_before: Vec<(Option<u64>, u32)> = previous
            .iter()
            .filter(|(id, lu)| **lu < now && id.is_none_or(|id| still_running.contains(&id)))
            .map(|(id, lu)| (*id, *lu))
            .collect();
        if archived_before.is_empty() {
            return Ok(());
        }
        let Some(res) = env.host().get_last_invocation_resources() else {
            return Ok(());
        };
        let over = |name: &str, value: i64, max: Option<i64>| -> Option<(String, i64, i64)> {
            max.filter(|m| value > *m)
                .map(|m| (name.to_owned(), value, m))
        };
        let exceeded = over(
            "instructions",
            res.instructions,
            self.budget.max_instructions,
        )
        .or_else(|| over("mem_bytes", res.mem_bytes, self.budget.max_mem_bytes))
        .or_else(|| {
            over(
                "disk_read_entries",
                i64::from(res.disk_read_entries),
                self.budget.max_disk_read_entries.map(i64::from),
            )
        })
        .or_else(|| {
            over(
                "disk_read_bytes",
                i64::from(res.disk_read_bytes),
                self.budget.max_disk_read_bytes.map(i64::from),
            )
        })
        .or_else(|| {
            over(
                "write_bytes",
                i64::from(res.write_bytes),
                self.budget.max_write_bytes.map(i64::from),
            )
        })
        .or_else(|| {
            over(
                "write_entries",
                i64::from(res.write_entries),
                self.budget.max_write_entries.map(i64::from),
            )
        });
        match exceeded {
            None => Ok(()),
            Some((name, value, max)) => Err(Violation::new(format!(
                "`{}` used {value} {name} while restoring archived entries, budget is {max}",
                last.handler
            ))
            .with_detail(serde_json::json!({
                "step": last.index,
                "handler": last.handler,
                "resource": name,
                "value": value,
                "max": max,
                "now_ledger": now,
                "archived_before": archived_before.iter().map(|(id, lu)| match id {
                    Some(id) => serde_json::json!({ "stream": id, "live_until": lu }),
                    None => serde_json::json!({ "instance": true, "live_until": lu }),
                }).collect::<Vec<_>>(),
                "instructions": res.instructions,
                "mem_bytes": res.mem_bytes,
                "disk_read_entries": res.disk_read_entries,
                "disk_read_bytes": res.disk_read_bytes,
                "write_bytes": res.write_bytes,
                "write_entries": res.write_entries,
            }))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use breakwater::{Actor, Gap, StepOutcome, StepRecord};
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, IntoVal, Symbol};

    #[contracttype]
    #[derive(Clone)]
    pub enum Key {
        Stream(u64),
    }

    #[contract]
    struct Streams;

    #[contractimpl]
    impl Streams {
        pub fn create(env: Env, id: u64) {
            env.storage().instance().set(&symbol_short!("n"), &id);
            env.storage().persistent().set(&Key::Stream(id), &1u32);
        }
        pub fn touch(env: Env, id: u64) -> u32 {
            env.storage()
                .persistent()
                .get(&Key::Stream(id))
                .unwrap_or(0)
        }
        pub fn touch_many(env: Env, ids: soroban_sdk::Vec<u64>) -> u32 {
            let mut sum = 0;
            for id in ids.iter() {
                sum += env
                    .storage()
                    .persistent()
                    .get(&Key::Stream(id))
                    .unwrap_or(0u32);
            }
            sum
        }
        pub fn bump(env: Env, id: u64, ledgers: u32) {
            env.storage()
                .persistent()
                .extend_ttl(&Key::Stream(id), ledgers, ledgers);
            env.storage().instance().extend_ttl(ledgers, ledgers);
        }
    }

    const DAY: u32 = LEDGERS_PER_DAY;

    fn env() -> Env {
        let env = Env::new_with_config(EnvTestConfig {
            capture_snapshot_at_drop: false,
        });
        env.mock_all_auths();
        env.ledger().with_mut(|li| {
            li.sequence_number = 1_000_000;
            li.min_persistent_entry_ttl = 30 * DAY;
        });
        env
    }

    fn create_record(index: usize, ledger: u32, duration: u32) -> StepRecord {
        let e = Env::default();
        StepRecord {
            index,
            handler: "create_stream".into(),
            actor: Actor::from_address("sender", &Address::generate(&e)),
            args: serde_json::json!({ "duration_ledgers": duration }),
            gap: Gap::default(),
            ledger_sequence: ledger,
            timestamp: 0,
            outcome: StepOutcome::Success,
            return_value: None,
        }
    }

    fn key() -> StreamKey {
        Arc::new(|env: &Env, id: u64| Key::Stream(id).into_val(env))
    }

    fn model() -> StreamModel {
        StreamModel::default()
    }

    #[test]
    fn expiry_fires_once_the_minimum_ttl_has_passed_and_holds_before() {
        let env = env();
        let id = env.register(Streams, ());
        StreamsClient::new(&env, &id).create(&0);
        let log = TxLog::from_records(vec![create_record(0, 1_000_000, 365 * DAY)]);
        let inv = NoMidLifecycleExpiry {
            contract: id.to_string().to_string(),
            model: model(),
            key: key(),
        };
        assert!(inv.check(&env, &log).is_ok());

        // Fresh TTL is min - 1 ledgers: two ledgers before the end it is still fine.
        env.ledger()
            .with_mut(|li| li.sequence_number += 30 * DAY - 2);
        assert!(inv.check(&env, &log).is_ok());
        // On the last live ledger the TTL is 0: the rule is TTL > 0, so it fires.
        env.ledger().with_mut(|li| li.sequence_number += 1);
        let err = inv.check(&env, &log).unwrap_err();
        assert!(err.message.contains("last live ledger"), "{}", err.message);
        // One ledger later the entry is archived.
        env.ledger().with_mut(|li| li.sequence_number += 1);
        let err = inv.check(&env, &log).unwrap_err();
        assert!(err.message.contains("archived"), "{}", err.message);
        assert_eq!(err.detail.unwrap()["stream"], 0);

        // A stream that has finished is not mid-lifecycle: no violation.
        let finished = TxLog::from_records(vec![create_record(0, 1_000_000, 10)]);
        assert!(inv.check(&env, &finished).is_ok());
    }

    #[test]
    fn instance_alive_fires_when_the_instance_expires_under_a_running_stream() {
        let env = env();
        let id = env.register(Streams, ());
        StreamsClient::new(&env, &id).create(&0);
        let log = TxLog::from_records(vec![create_record(0, 1_000_000, 365 * DAY)]);
        let inv = InstanceAlive {
            contract: id.to_string().to_string(),
            model: model(),
        };
        assert!(inv.check(&env, &log).is_ok());
        env.ledger().with_mut(|li| li.sequence_number += 31 * DAY);
        assert!(inv.check(&env, &log).is_err());
        assert!(
            inv.check(&env, &TxLog::default()).is_ok(),
            "no running stream, nothing to protect"
        );
    }

    #[test]
    fn restore_check_runs_the_probe_on_a_copy_and_leaves_the_env_archived() {
        let env = env();
        let id = env.register(Streams, ());
        StreamsClient::new(&env, &id).create(&0);
        let log = TxLog::from_records(vec![create_record(0, 1_000_000, 365 * DAY)]);
        env.ledger().with_mut(|li| li.sequence_number += 31 * DAY);
        let contract = id.to_string().to_string();
        let restore: RestoreHook = Arc::new({
            let contract = contract.clone();
            move |e: &Env| {
                e.register_at(&Address::from_str(e, &contract), Streams, ());
            }
        });

        let working = RestoreRestoresFunction {
            contract: contract.clone(),
            model: model(),
            key: key(),
            restore: restore.clone(),
            probe: Arc::new({
                let contract = contract.clone();
                move |e: &Env, stream: u64| {
                    let v = StreamsClient::new(e, &Address::from_str(e, &contract)).touch(&stream);
                    if v == 1 {
                        Ok(())
                    } else {
                        Err(format!("value {v} after restore"))
                    }
                }
            }),
        };
        assert!(working.check(&env, &log).is_ok());
        // The probe restored the copy, not the fuzzed Env.
        let still = ttl(&env, &id, &Entry::Persistent(Key::Stream(0).into_val(&env)));
        assert!(matches!(still, Some(t) if t < 0), "{still:?}");

        let broken = RestoreRestoresFunction {
            probe: Arc::new(|_: &Env, _: u64| Err("withdraw panicked".into())),
            ..working
        };
        let err = broken.check(&env, &log).unwrap_err();
        assert!(err.message.contains("withdraw panicked"));
    }

    #[test]
    fn unbounded_extension_fires_once_an_entry_outlives_its_justification() {
        let env = env();
        let id = env.register(Streams, ());
        let client = StreamsClient::new(&env, &id);
        client.create(&0);
        let log = TxLog::from_records(vec![create_record(0, 1_000_000, 365 * DAY)]);
        let inv = NoUnboundedTtlExtension {
            contract: id.to_string().to_string(),
            model: model(),
            key: key(),
            watch_instance: true,
            slack_ledgers: DAY,
            max_extension_ledgers: None,
        };
        assert!(
            inv.check(&env, &log).is_ok(),
            "the minimum TTL is within bounds"
        );

        client.bump(&0, &(365 * DAY));
        assert!(
            inv.check(&env, &log).is_ok(),
            "a year is what the stream needs"
        );

        // Anyone can call `bump` with whatever the network allows: 400 days of
        // life for a 365-day stream is past the justification.
        env.ledger().with_mut(|li| li.max_entry_ttl = 500 * DAY);
        client.bump(&0, &(400 * DAY));
        let err = inv.check(&env, &log).unwrap_err();
        assert!(err.message.contains("kept alive"), "{}", err.message);
        assert_eq!(err.detail.unwrap()["stream"], 0);

        // The instance was bumped along; watching it alone fires too.
        let instance_only = NoUnboundedTtlExtension {
            model: StreamModel {
                create_handler: "nothing".into(),
                ..model()
            },
            ..inv
        };
        let err = instance_only.check(&env, &log).unwrap_err();
        assert!(err.message.contains("instance"), "{}", err.message);
    }

    #[test]
    fn restore_cost_fires_when_one_call_restores_several_archived_entries() {
        let env = env();
        let id = env.register(Streams, ());
        let client = StreamsClient::new(&env, &id);
        for i in 0..6u64 {
            client.create(&i);
        }
        env.ledger().with_mut(|li| li.sequence_number += 40 * DAY);

        // The first touch also restores the archived instance; measure a
        // single entry restoration after that, then budget for fewer than
        // three of them.
        client.touch_many(&soroban_sdk::vec![&env, 0u64]);
        client.touch_many(&soroban_sdk::vec![&env, 1u64]);
        let one = env.host().get_last_invocation_resources().expect("metered");
        assert!(
            one.disk_read_bytes > 0,
            "restoring reads the entry from disk"
        );
        let budget = ResourceBudget {
            max_disk_read_bytes: Some(one.disk_read_bytes * 2),
            max_write_entries: Some(one.write_entries * 2),
            ..ResourceBudget::default()
        };
        let step = |index: usize, ok: bool| StepRecord {
            index,
            handler: "touch_many".into(),
            actor: Actor::from_address("x", &Address::generate(&Env::default())),
            args: serde_json::json!({}),
            gap: Gap::default(),
            ledger_sequence: env.ledger().sequence(),
            timestamp: 0,
            outcome: if ok {
                StepOutcome::Success
            } else {
                StepOutcome::ContractError(1)
            },
            return_value: None,
        };

        let inv = RestoreCostWithinBudget::new(
            id.to_string().to_string(),
            model(),
            key(),
            vec!["touch_many".into()],
            budget,
        );
        // Streams 0..6 exist and are archived: the log says so.
        let log_of = |n: usize| {
            let mut records: Vec<StepRecord> = (0..6)
                .map(|i| create_record(i, 1_000_000, 365 * DAY))
                .collect();
            records.extend((0..n).map(|k| step(6 + k, true)));
            TxLog::from_records(records)
        };
        // Learn the archived state first (nothing measured yet).
        assert!(inv.check(&env, &log_of(0)).is_ok());
        // One more single restoration is within budget.
        client.touch_many(&soroban_sdk::vec![&env, 2u64]);
        assert!(inv.check(&env, &log_of(1)).is_ok());

        // Restoring three archived entries in one call exceeds it.
        client.touch_many(&soroban_sdk::vec![&env, 3u64, 4u64, 5u64]);
        let err = inv.check(&env, &log_of(2)).unwrap_err();
        assert!(err.message.contains("budget"), "{}", err.message);
        assert_eq!(err.detail.as_ref().unwrap()["handler"], "touch_many");

        // Failed steps are not measured.
        let mut failed = log_of(2).steps().to_vec();
        failed.push(step(8, false));
        assert!(inv.check(&env, &TxLog::from_records(failed)).is_ok());

        // With nothing archived any more, the same call is not measured.
        client.touch_many(&soroban_sdk::vec![&env, 0u64, 1u64, 2u64, 3u64, 4u64, 5u64]);
        assert!(inv.check(&env, &log_of(3)).is_ok());
        client.touch_many(&soroban_sdk::vec![&env, 0u64, 1u64, 2u64, 3u64, 4u64, 5u64]);
        assert!(inv.check(&env, &log_of(4)).is_ok());
    }

    #[test]
    fn ttl_probe_is_side_effect_free() {
        let env = env();
        let id = env.register(Streams, ());
        StreamsClient::new(&env, &id).create(&7);
        let k: Val = Key::Stream(7).into_val(&env);
        assert_eq!(
            ttl(&env, &id, &Entry::Persistent(k)),
            Some(i64::from(30 * DAY) - 1)
        );
        env.ledger().with_mut(|li| li.sequence_number += 40 * DAY);
        assert_eq!(
            ttl(&env, &id, &Entry::Persistent(k)),
            Some(-(i64::from(10 * DAY)) - 1)
        );
        assert_eq!(
            ttl(&env, &id, &Entry::Persistent(k)),
            Some(-(i64::from(10 * DAY)) - 1)
        );
        assert_eq!(
            ttl(&env, &id, &Entry::Persistent(Key::Stream(8).into_val(&env))),
            None
        );
        let _ = Symbol::new(&env, "unused");
    }
}
