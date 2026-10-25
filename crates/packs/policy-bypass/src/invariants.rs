//! Invariants for smart-account policies, evaluated over the `TxLog`.
//!
//! The invariants key on what the suite's handlers record: the actor name,
//! the handler name and the JSON arguments. A suite names its handlers after
//! the contract function they call (for example `transfer`) and puts the
//! token amount in a numeric argument field.

use breakwater::{Invariant, StepRecord, TxLog, Violation};
use serde::{Deserialize, Serialize};
use soroban_sdk::{Address, Env, Executable};

/// Cumulative spend of one actor within a rolling ledger window never
/// exceeds `limit`, regardless of how many calls, rules or policies it took.
///
/// This is the property an operator *means* when they configure a daily
/// spending limit; the policy may enforce something weaker.
#[derive(Clone, Debug)]
pub struct RollingCap {
    /// Actor whose spend is summed.
    pub actor: String,
    /// Handler that moves funds.
    pub handler: String,
    /// JSON field of the handler's arguments holding the amount.
    pub amount_field: String,
    /// Maximum cumulative amount inside any window.
    pub limit: i64,
    /// Window length in ledgers, ending at the most recent step.
    pub window_ledgers: u32,
}

impl RollingCap {
    fn amount(&self, step: &StepRecord) -> i64 {
        step.args
            .get(&self.amount_field)
            .and_then(|v| v.as_i64())
            .unwrap_or(0)
    }
}

impl Invariant for RollingCap {
    fn name(&self) -> &'static str {
        "rolling_cap"
    }

    fn check(&self, _env: &Env, log: &TxLog) -> Result<(), Violation> {
        let Some(last) = log.last() else {
            return Ok(());
        };
        let window_start = last.ledger_sequence.saturating_sub(self.window_ledgers);
        let in_window: Vec<&StepRecord> = log
            .successes_of(&self.handler)
            .filter(|s| s.actor.name() == self.actor && s.ledger_sequence > window_start)
            .collect();
        let spent: i64 = in_window.iter().map(|s| self.amount(s)).sum();
        if spent > self.limit {
            return Err(Violation::new(format!(
                "{} spent {spent} within {} ledgers, limit is {}",
                self.actor, self.window_ledgers, self.limit
            ))
            .with_detail(serde_json::json!({
                "spent": spent,
                "limit": self.limit,
                "window_ledgers": self.window_ledgers,
                "steps": in_window.iter().map(|s| s.index).collect::<Vec<_>>(),
            })));
        }
        Ok(())
    }
}

/// An actor only ever succeeds with allow-listed handlers.
#[derive(Clone, Debug)]
pub struct AllowlistOnly {
    /// Actor under scrutiny.
    pub actor: String,
    /// Handler names the actor may succeed with.
    pub allowed: Vec<String>,
}

impl Invariant for AllowlistOnly {
    fn name(&self) -> &'static str {
        "allowlist_only"
    }

    fn check(&self, _env: &Env, log: &TxLog) -> Result<(), Violation> {
        if let Some(step) = log
            .successes()
            .find(|s| s.actor.name() == self.actor && !self.allowed.iter().any(|a| a == &s.handler))
        {
            return Err(Violation::new(format!(
                "{} succeeded with `{}`, which is not allow-listed",
                self.actor, step.handler
            ))
            .with_detail(serde_json::json!({ "step": step.index, "handler": step.handler })));
        }
        Ok(())
    }
}

/// Once a revoke handler succeeded, the revoked actor never succeeds again.
#[derive(Clone, Debug)]
pub struct RevokedNeverAuths {
    /// Actor that gets revoked.
    pub actor: String,
    /// Handler whose success revokes the actor.
    pub revoke_handler: String,
}

impl Invariant for RevokedNeverAuths {
    fn name(&self) -> &'static str {
        "revoked_never_auths"
    }

    fn check(&self, _env: &Env, log: &TxLog) -> Result<(), Violation> {
        let Some(revoked_at) = log
            .successes_of(&self.revoke_handler)
            .map(|s| s.index)
            .next()
        else {
            return Ok(());
        };
        if let Some(step) = log.successes().find(|s| {
            s.index > revoked_at && s.actor.name() == self.actor && s.handler != self.revoke_handler
        }) {
            return Err(Violation::new(format!(
                "{} succeeded with `{}` after being revoked at step {revoked_at}",
                self.actor, step.handler
            ))
            .with_detail(serde_json::json!({ "revoked_at": revoked_at, "step": step.index })));
        }
        Ok(())
    }
}

/// Serialisable mirror of [`Executable`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PinnedExecutable {
    /// Contract code identified by its hash (hex).
    Wasm(String),
    /// The built-in Stellar Asset Contract.
    StellarAsset,
    /// A classic account.
    Account,
    /// No contract instance at the address.
    Missing,
}

impl PinnedExecutable {
    fn of(address: &Address) -> Self {
        match address.executable() {
            Some(Executable::Wasm(hash)) => {
                PinnedExecutable::Wasm(hash.to_array().iter().map(|b| format!("{b:02x}")).collect())
            }
            Some(Executable::StellarAsset) => PinnedExecutable::StellarAsset,
            Some(Executable::Account) => PinnedExecutable::Account,
            None => PinnedExecutable::Missing,
        }
    }
}

/// One pinned contract.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pin {
    /// Human-readable role, for example `daily_policy`.
    pub name: String,
    /// Strkey of the contract.
    pub strkey: String,
    /// Executable captured at setup.
    pub executable: PinnedExecutable,
}

/// The executables of the policy and verifier contracts captured at setup are
/// the ones in place at every step: nobody swapped the code that enforces
/// the rules.
#[derive(Clone, Debug)]
pub struct PinnedWasm {
    /// Pinned contracts.
    pub pins: Vec<Pin>,
}

impl PinnedWasm {
    /// Captures the executables of `contracts` from `env`.
    pub fn capture(env: &Env, contracts: &[(&str, &Address)]) -> Self {
        let _ = env;
        Self {
            pins: contracts
                .iter()
                .map(|(name, address)| Pin {
                    name: (*name).to_owned(),
                    strkey: address.to_string().to_string(),
                    executable: PinnedExecutable::of(address),
                })
                .collect(),
        }
    }
}

impl Invariant for PinnedWasm {
    fn name(&self) -> &'static str {
        "pinned_wasm"
    }

    fn check(&self, env: &Env, _log: &TxLog) -> Result<(), Violation> {
        for pin in &self.pins {
            let now = PinnedExecutable::of(&Address::from_str(env, &pin.strkey));
            if now != pin.executable {
                return Err(Violation::new(format!(
                    "executable of `{}` changed since setup",
                    pin.name
                ))
                .with_detail(serde_json::json!({ "pin": pin, "now": now })));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use breakwater::{Actor, Gap, StepOutcome};
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::{contract, contractimpl};

    #[contract]
    struct Dummy;

    #[contractimpl]
    impl Dummy {
        pub fn noop() {}
    }

    fn actor(env: &Env, name: &str) -> Actor {
        Actor::from_address(name, &Address::generate(env))
    }

    fn record(
        index: usize,
        handler: &str,
        actor: &Actor,
        amount: i64,
        ledger: u32,
        outcome: StepOutcome,
    ) -> StepRecord {
        StepRecord {
            index,
            handler: handler.to_owned(),
            actor: actor.clone(),
            args: serde_json::json!({ "amount_usdc": amount }),
            gap: Gap::default(),
            ledger_sequence: ledger,
            timestamp: u64::from(ledger) * 5,
            outcome,
        }
    }

    fn cap() -> RollingCap {
        RollingCap {
            actor: "agent".into(),
            handler: "transfer".into(),
            amount_field: "amount_usdc".into(),
            limit: 100,
            window_ledgers: 17_280,
        }
    }

    #[test]
    fn rolling_cap_fires_on_three_payments_spread_over_a_short_policy_window() {
        let env = Env::default();
        let agent = actor(&env, "agent");
        // 49 + 49 + 49 = 147 inside 17_280 ledgers, even though each pair is
        // more than 1_728 ledgers apart (the mis-sized policy window).
        let log = TxLog::from_records(vec![
            record(0, "transfer", &agent, 49, 100_000, StepOutcome::Success),
            record(1, "transfer", &agent, 49, 101_800, StepOutcome::Success),
            record(2, "transfer", &agent, 49, 103_600, StepOutcome::Success),
        ]);
        let err = cap().check(&env, &log).unwrap_err();
        assert_eq!(err.detail.as_ref().unwrap()["spent"], 147);
    }

    #[test]
    fn rolling_cap_ignores_rejected_calls_other_actors_and_old_steps() {
        let env = Env::default();
        let agent = actor(&env, "agent");
        let owner = actor(&env, "owner");
        let log = TxLog::from_records(vec![
            record(0, "transfer", &agent, 90, 100_000, StepOutcome::Success),
            record(
                1,
                "transfer",
                &agent,
                90,
                100_001,
                StepOutcome::Unauthorized,
            ),
            record(2, "transfer", &owner, 90, 100_002, StepOutcome::Success),
            record(
                3,
                "transfer",
                &agent,
                90,
                100_000 + 17_280,
                StepOutcome::Success,
            ),
        ]);
        assert!(cap().check(&env, &log).is_ok());
    }

    #[test]
    fn allowlist_fires_when_the_agent_succeeds_outside_the_list() {
        let env = Env::default();
        let agent = actor(&env, "agent");
        let inv = AllowlistOnly {
            actor: "agent".into(),
            allowed: vec!["transfer".into()],
        };
        let ok = TxLog::from_records(vec![
            record(0, "transfer", &agent, 1, 1, StepOutcome::Success),
            record(1, "approve", &agent, 1, 2, StepOutcome::Unauthorized),
        ]);
        assert!(inv.check(&env, &ok).is_ok());
        let bad = TxLog::from_records(vec![record(
            0,
            "approve",
            &agent,
            1,
            1,
            StepOutcome::Success,
        )]);
        assert_eq!(
            inv.check(&env, &bad).unwrap_err().detail.unwrap()["handler"],
            "approve"
        );
    }

    #[test]
    fn revoked_never_auths_fires_on_success_after_revocation() {
        let env = Env::default();
        let agent = actor(&env, "agent");
        let owner = actor(&env, "owner");
        let inv = RevokedNeverAuths {
            actor: "agent".into(),
            revoke_handler: "revoke_agent".into(),
        };
        let ok = TxLog::from_records(vec![
            record(0, "transfer", &agent, 1, 1, StepOutcome::Success),
            record(1, "revoke_agent", &owner, 0, 2, StepOutcome::Success),
            record(2, "transfer", &agent, 1, 3, StepOutcome::Unauthorized),
        ]);
        assert!(inv.check(&env, &ok).is_ok());
        let bad = TxLog::from_records(vec![
            record(0, "revoke_agent", &owner, 0, 1, StepOutcome::Success),
            record(1, "transfer", &agent, 1, 2, StepOutcome::Success),
        ]);
        assert_eq!(
            inv.check(&env, &bad).unwrap_err().detail.unwrap()["step"],
            1
        );
    }

    #[test]
    fn pinned_wasm_holds_for_an_unchanged_contract_and_fires_on_a_different_pin() {
        let env = Env::default();
        let id = env.register(Dummy, ());
        let inv = PinnedWasm::capture(&env, &[("dummy", &id)]);
        assert!(matches!(inv.pins[0].executable, PinnedExecutable::Wasm(_)));
        assert!(inv.check(&env, &TxLog::default()).is_ok());

        let mut tampered = inv.clone();
        tampered.pins[0].executable = PinnedExecutable::Wasm("00".repeat(32));
        let err = tampered.check(&env, &TxLog::default()).unwrap_err();
        assert!(err.message.contains("dummy"));
    }
}
