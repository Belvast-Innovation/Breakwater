//! Real authorization entries for an OpenZeppelin `SmartAccount`.
//!
//! The recipe follows OpenZeppelin's own tests for `stellar-accounts` 0.8.0:
//!
//! 1. The account's entry carries an `AuthPayload { signers, context_rule_ids }`
//!    as its signature. External signers sign the SHA-256 digest of the
//!    `AuthDigestPreimage { account, signature_payload, context_rule_ids }`.
//! 2. A delegated signer additionally needs its own entry whose root
//!    invocation is `account.__check_auth(preimage)`, because the account
//!    authenticates it with `require_auth_for_args((preimage,))`.

use breakwater::auth::{address_entry, contract_fn, signature_payload, to_scval, AuthError};
use breakwater::{CallContext, CallOutcome, HandlerError};
use soroban_sdk::xdr::{
    ScErrorCode, ScErrorType, ScVal, SorobanAuthorizationEntry, SorobanAuthorizedInvocation,
};
use soroban_sdk::{map, vec, Address, Bytes, BytesN, Env};
use stellar_accounts::smart_account::{AuthDigestPreimage, AuthPayload};

use crate::signer::{sign, SignerKey};

/// Authorization entries that let `key` authorize `root` on behalf of
/// `account` under context rule `rule_id`. Install them with
/// `env.set_auths(&entries)` right before the call.
pub fn authorize(
    env: &Env,
    account: &Address,
    key: &SignerKey,
    rule_id: u32,
    root: SorobanAuthorizedInvocation,
    ctx: &CallContext,
) -> Result<Vec<SorobanAuthorizationEntry>, AuthError> {
    let nonce = ctx.nonce();
    let expiration = ctx.signature_expiration_ledger(env);

    let preimage = AuthDigestPreimage {
        account: account.clone(),
        signature_payload: signature_payload(env, nonce, expiration, &root)?,
        context_rule_ids: vec![env, rule_id],
    };
    let signer = key.signer(env);

    let mut entries = Vec::with_capacity(2);
    let sig_data = match key {
        SignerKey::External { seed, .. } => {
            Bytes::from_array(env, &sign(seed, &preimage.digest(env).to_array()))
        }
        SignerKey::Delegated { delegate, seed } => {
            let delegate = Address::from_str(env, delegate);
            let delegate_root = contract_fn(
                account,
                "__check_auth",
                std::vec![to_scval(env, preimage.clone())?],
            )?;
            let delegate_signature = match seed {
                None => ScVal::Void,
                Some(seed) => {
                    let payload = signature_payload(env, nonce, expiration, &delegate_root)?;
                    to_scval(
                        env,
                        BytesN::from_array(env, &sign(seed, &payload.to_array())),
                    )?
                }
            };
            entries.push(address_entry(
                &delegate,
                nonce,
                expiration,
                delegate_signature,
                delegate_root,
            ));
            Bytes::new(env)
        }
    };

    let payload = AuthPayload {
        signers: map![env, (signer, sig_data)],
        context_rule_ids: vec![env, rule_id],
    };
    entries.insert(
        0,
        address_entry(account, nonce, expiration, to_scval(env, payload)?, root),
    );
    Ok(entries)
}

/// Maps a failed `try_` client call to a handler outcome: contract errors and
/// authorization failures are expected behaviour, anything else is a bug.
///
/// When a custom account's `__check_auth` rejects (a signer is unknown, a
/// policy panics with its own `contracterror`), the host surfaces the failure
/// of the callee's `require_auth` as `Error(Context, InvalidAction)`, not as an
/// `Auth` error; both are treated as [`CallOutcome::Unauthorized`].
pub fn classify(err: soroban_sdk::Error) -> Result<CallOutcome, HandlerError> {
    if err.is_type(ScErrorType::Contract) {
        Ok(CallOutcome::ContractError(err.get_code()))
    } else if err.is_type(ScErrorType::Auth)
        || (err.is_type(ScErrorType::Context) && err.is_code(ScErrorCode::InvalidAction))
    {
        Ok(CallOutcome::Unauthorized)
    } else {
        Err(HandlerError::Host(format!("{err:?}")))
    }
}

/// Wraps an [`AuthError`] as a handler error.
pub fn auth_failure(err: AuthError) -> HandlerError {
    HandlerError::Other(format!("authorization encoding failed: {err}"))
}
