//! Invariants about TTL and archival of contract state.

use std::sync::Arc;

use breakwater::{Invariant, TxLog, Violation, LEDGERS_PER_DAY};
use soroban_sdk::testutils::EnvTestConfig;
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

#[cfg(test)]
mod tests {
    use super::*;
    use breakwater::{Actor, Gap, StepOutcome, StepRecord};
    use soroban_sdk::testutils::{Address as _, Ledger as _};
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
