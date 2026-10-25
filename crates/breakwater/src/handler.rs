//! Handlers: the contract calls a suite may generate.

use std::any::Any;
use std::fmt::Debug;
use std::sync::Arc;

use proptest::prelude::*;
use serde::Serialize;
use soroban_sdk::Env;
use thiserror::Error;

use crate::actor::Actor;
use crate::config::AuthMode;

/// Result of a handler call that completed without a bug.
///
/// Both variants are *expected* behaviour: a call may succeed or fail
/// gracefully with a `contracterror` code. Anything else (a host error, a
/// conversion error, a bare panic) is reported as a [`HandlerError`] or a
/// panic and treated as a bug.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallOutcome {
    /// The call succeeded.
    Success,
    /// The contract rejected the call with the given `contracterror` code.
    ContractError(u32),
    /// The host rejected the call's authorization (a policy or signer check
    /// failed). Expected behaviour when fuzzing authorization.
    Unauthorized,
}

/// Per-step information a handler may need to build real authorization.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CallContext {
    /// Zero-based index of the step in the sequence.
    pub step: usize,
    /// Authorization mode of the running suite.
    pub auth: AuthMode,
}

impl CallContext {
    /// A nonce that is unique per step, so one address can authorize one call
    /// per step without nonce reuse.
    pub fn nonce(&self) -> i64 {
        self.step as i64 + 1
    }

    /// A signature expiration ledger comfortably ahead of the current one.
    pub fn signature_expiration_ledger(&self, env: &Env) -> u32 {
        env.ledger().sequence().saturating_add(100)
    }
}

/// A handler call failed for a reason other than a graceful contract error.
#[derive(Debug, Error)]
pub enum HandlerError {
    /// The host returned an error that is not a contract error.
    #[error("host error: {0}")]
    Host(String),
    /// The runner passed arguments of the wrong type; indicates a framework bug.
    #[error("argument type mismatch for handler `{0}`")]
    ArgsMismatch(&'static str),
    /// Any other failure the handler wants to surface as a bug.
    #[error("{0}")]
    Other(String),
}

/// One contract function the fuzzer may call.
///
/// Implementations wrap a single contract entry point: they declare which
/// actors may call it, how to generate arguments, and how to perform the call
/// against an [`Env`]. Use the generated `try_` client methods inside
/// [`Handler::call`] so that contract errors become
/// [`CallOutcome::ContractError`] instead of panics.
pub trait Handler: Send + Sync + 'static {
    /// Generated arguments for one call.
    type Args: Clone + Debug + Serialize + Send + Sync + 'static;

    /// Name used in logs and exports; usually the contract function name.
    fn name(&self) -> &'static str;

    /// Actors allowed to perform this call. Must not be empty.
    fn actors(&self) -> &[Actor];

    /// proptest strategy for the arguments.
    fn strategy(&self) -> BoxedStrategy<Self::Args>;

    /// Performs the call as `actor` with `args`.
    ///
    /// In [`AuthMode::Real`] the handler must install the authorization
    /// entries the call needs with `env.set_auths(..)` before invoking; see
    /// [`crate::auth`].
    fn call(
        &self,
        env: &Env,
        ctx: &CallContext,
        actor: &Actor,
        args: &Self::Args,
    ) -> Result<CallOutcome, HandlerError>;
}

/// Type-erased handler arguments carried inside a generated step.
#[derive(Clone)]
pub struct StepArgs(Arc<dyn ErasedArgs>);

impl StepArgs {
    /// JSON rendering of the arguments, used by the log and the exporter.
    pub fn to_json(&self) -> serde_json::Value {
        self.0.to_json()
    }

    fn downcast<T: 'static>(&self) -> Option<&T> {
        self.0.as_any().downcast_ref::<T>()
    }
}

impl Debug for StepArgs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

trait ErasedArgs: Debug + Send + Sync + 'static {
    fn as_any(&self) -> &dyn Any;
    fn to_json(&self) -> serde_json::Value;
}

impl<T> ErasedArgs for T
where
    T: Clone + Debug + Serialize + Send + Sync + 'static,
{
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self)
            .unwrap_or_else(|_| serde_json::Value::String(format!("{self:?}")))
    }
}

/// Object-safe view of a [`Handler`], used internally by the suite.
pub(crate) trait DynHandler: Send + Sync {
    fn name(&self) -> &'static str;
    fn actors(&self) -> &[Actor];
    fn strategy(&self) -> BoxedStrategy<StepArgs>;
    fn call(
        &self,
        env: &Env,
        ctx: &CallContext,
        actor: &Actor,
        args: &StepArgs,
    ) -> Result<CallOutcome, HandlerError>;
}

impl<H: Handler> DynHandler for H {
    fn name(&self) -> &'static str {
        Handler::name(self)
    }

    fn actors(&self) -> &[Actor] {
        Handler::actors(self)
    }

    fn strategy(&self) -> BoxedStrategy<StepArgs> {
        Handler::strategy(self)
            .prop_map(|args| StepArgs(Arc::new(args)))
            .boxed()
    }

    fn call(
        &self,
        env: &Env,
        ctx: &CallContext,
        actor: &Actor,
        args: &StepArgs,
    ) -> Result<CallOutcome, HandlerError> {
        let args = args
            .downcast::<H::Args>()
            .ok_or_else(|| HandlerError::ArgsMismatch(Handler::name(self)))?;
        Handler::call(self, env, ctx, actor, args)
    }
}
