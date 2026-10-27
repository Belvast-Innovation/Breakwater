//! Breakwater core library.
//!
//! Stateful property testing for Soroban contracts. A suite declares *actors*
//! (who may call), *handlers* (what may be called) and *invariants* (what must
//! always hold). Breakwater generates randomized call sequences against an
//! in-process [`soroban_sdk::Env`], checks every invariant after every step,
//! shrinks any failing sequence to a minimal reproduction and exports it.
//!
//! # Example
//!
//! A counter contract whose count must never exceed 2. The suite calls
//! `increment` in random sequences, finds the violation and shrinks it to the
//! three increments that cause it.
//!
//! ```
//! use breakwater::proptest::prelude::*;
//! use breakwater::{
//!     Actor, CallContext, CallOutcome, Handler, HandlerError, Invariant, Suite, TxLog, Violation,
//! };
//! use soroban_sdk::{contract, contractimpl, symbol_short, Address, Env, Symbol};
//!
//! const COUNT: Symbol = symbol_short!("COUNT");
//!
//! #[contract]
//! pub struct Counter;
//!
//! #[contractimpl]
//! impl Counter {
//!     pub fn increment(env: Env) -> u32 {
//!         let n: u32 = env.storage().instance().get(&COUNT).unwrap_or(0) + 1;
//!         env.storage().instance().set(&COUNT, &n);
//!         n
//!     }
//!
//!     pub fn get(env: Env) -> u32 {
//!         env.storage().instance().get(&COUNT).unwrap_or(0)
//!     }
//! }
//!
//! // A handler describes one callable entry point and the arguments to feed it.
//! struct Increment {
//!     contract: String,
//!     actors: Vec<Actor>,
//! }
//!
//! impl Handler for Increment {
//!     type Args = ();
//!
//!     fn name(&self) -> &'static str {
//!         "increment"
//!     }
//!
//!     fn actors(&self) -> &[Actor] {
//!         &self.actors
//!     }
//!
//!     fn strategy(&self) -> BoxedStrategy<()> {
//!         Just(()).boxed()
//!     }
//!
//!     fn call(
//!         &self,
//!         env: &Env,
//!         _ctx: &CallContext,
//!         _actor: &Actor,
//!         _args: &(),
//!     ) -> Result<CallOutcome, HandlerError> {
//!         let id = Address::from_str(env, &self.contract);
//!         match CounterClient::new(env, &id).try_increment() {
//!             Ok(Ok(count)) => Ok(CallOutcome::returning(count)),
//!             Ok(Err(e)) => Err(HandlerError::Other(format!("conversion error: {e:?}"))),
//!             Err(Ok(e)) => Ok(CallOutcome::ContractError(e.get_code())),
//!             Err(Err(e)) => Err(HandlerError::Host(format!("{e:?}"))),
//!         }
//!     }
//! }
//!
//! // An invariant is checked after every step of every sequence.
//! struct CountAtMostTwo {
//!     contract: String,
//! }
//!
//! impl Invariant for CountAtMostTwo {
//!     fn name(&self) -> &'static str {
//!         "count_at_most_two"
//!     }
//!
//!     fn check(&self, env: &Env, _log: &TxLog) -> Result<(), Violation> {
//!         let id = Address::from_str(env, &self.contract);
//!         let count = CounterClient::new(env, &id).get();
//!         if count > 2 {
//!             return Err(Violation::new("count exceeded 2").with_detail(count));
//!         }
//!         Ok(())
//!     }
//! }
//!
//! fn main() {
//!     let env = Env::default();
//!     let contract = env.register(Counter, ()).to_string().to_string();
//!     let owner = Actor::generate(&env, "owner");
//!     let restore_id = contract.clone();
//!
//!     let report = Suite::new("counter", env)
//!         .seed(42)
//!         .steps(1..=8)
//!         .cases(16)
//!         .handler(Increment { contract: contract.clone(), actors: vec![owner] })
//!         .invariant(CountAtMostTwo { contract })
//!         // Re-registers the contract whenever the runner restores a snapshot.
//!         .on_restore(move |env| {
//!             env.register_at(&Address::from_str(env, &restore_id), Counter, ());
//!         })
//!         .run()
//!         .expect("the suite runs");
//!
//!     let failure = report.failure().expect("the third increment breaks the invariant");
//!     assert_eq!(failure.violation.source, "count_at_most_two");
//!     assert_eq!(failure.log.len(), 3, "shrunk to three increments");
//! }
//! ```
#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod actor;
pub mod auth;
mod config;
mod error;
mod export;
mod handler;
mod invariant;
mod ledger;
mod log;
mod replay;
mod runner;
mod step;
mod suite;

pub use actor::Actor;
pub use config::{parse_steps, AuthMode, SuiteConfig};
pub use error::SuiteError;
pub use export::Exporter;
pub use handler::{CallContext, CallOutcome, Handler, HandlerError, StepArgs};
pub use invariant::{Invariant, Violation, ViolationKind};
pub use ledger::{LedgerAdvance, LEDGERS_PER_DAY, SECONDS_PER_LEDGER};
pub use log::{Gap, StepOutcome, StepRecord, TxLog};
pub use replay::{ReplayError, ReplayStep};
pub use step::Step;
pub use suite::{Failure, Outcome, RunReport, Suite};

/// Re-export so suites can name proptest strategies without a direct dependency.
pub use proptest;
