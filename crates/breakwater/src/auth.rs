//! Building blocks for real Soroban authorization entries.
//!
//! In [`crate::AuthMode::Real`] the suite does not mock authorization, so a
//! handler has to hand the host the `SorobanAuthorizationEntry` values a real
//! transaction would carry, via `env.set_auths(..)`. These helpers cover the
//! protocol-level plumbing that every custom account needs: the invocation
//! tree, the signature payload the host expects to be signed, and the
//! credentials envelope. Account-specific payloads (for example OpenZeppelin's
//! `AuthPayload`) live in the attack packs.

use soroban_sdk::testutils::Ledger as _;
use soroban_sdk::xdr::{
    Hash as XdrHash, HashIdPreimage, HashIdPreimageSorobanAuthorization, InvokeContractArgs,
    Limits, ScVal, SorobanAddressCredentials, SorobanAuthorizationEntry, SorobanAuthorizedFunction,
    SorobanAuthorizedInvocation, SorobanCredentials, VecM, WriteXdr,
};
use soroban_sdk::{Address, Bytes, BytesN, Env, IntoVal, TryFromVal, Val};
use thiserror::Error;

/// Failures while encoding authorization data.
#[derive(Debug, Error)]
pub enum AuthError {
    /// A value could not be converted to `ScVal`.
    #[error("value cannot be represented as ScVal")]
    Conversion,
    /// An XDR limit was exceeded (too many arguments, name too long, ...).
    #[error("xdr encoding failed: {0}")]
    Xdr(String),
}

/// Converts any contract value into its `ScVal` form.
pub fn to_scval<T: IntoVal<Env, Val>>(env: &Env, value: T) -> Result<ScVal, AuthError> {
    ScVal::try_from_val(env, &value.into_val(env)).map_err(|_| AuthError::Conversion)
}

/// A leaf invocation `contract.name(args)` with no sub-invocations.
pub fn contract_fn(
    contract: &Address,
    name: &str,
    args: Vec<ScVal>,
) -> Result<SorobanAuthorizedInvocation, AuthError> {
    Ok(SorobanAuthorizedInvocation {
        function: SorobanAuthorizedFunction::ContractFn(InvokeContractArgs {
            contract_address: contract.into(),
            function_name: name
                .try_into()
                .map_err(|e| AuthError::Xdr(format!("{e:?}")))?,
            args: args
                .try_into()
                .map_err(|e| AuthError::Xdr(format!("{e:?}")))?,
        }),
        sub_invocations: VecM::default(),
    })
}

/// The 32-byte payload the host asks an address to sign for `invocation`.
pub fn signature_payload(
    env: &Env,
    nonce: i64,
    signature_expiration_ledger: u32,
    invocation: &SorobanAuthorizedInvocation,
) -> Result<BytesN<32>, AuthError> {
    let preimage = HashIdPreimage::SorobanAuthorization(HashIdPreimageSorobanAuthorization {
        network_id: XdrHash(env.ledger().get().network_id),
        nonce,
        signature_expiration_ledger,
        invocation: invocation.clone(),
    });
    let xdr = preimage
        .to_xdr(Limits::none())
        .map_err(|e| AuthError::Xdr(format!("{e:?}")))?;
    Ok(env
        .crypto()
        .sha256(&Bytes::from_slice(env, &xdr))
        .to_bytes())
}

/// An authorization entry for `address` over `root_invocation` carrying
/// `signature` (already encoded as the account's `Signature` type).
pub fn address_entry(
    address: &Address,
    nonce: i64,
    signature_expiration_ledger: u32,
    signature: ScVal,
    root_invocation: SorobanAuthorizedInvocation,
) -> SorobanAuthorizationEntry {
    SorobanAuthorizationEntry {
        credentials: SorobanCredentials::Address(SorobanAddressCredentials {
            address: address.into(),
            nonce,
            signature_expiration_ledger,
            signature,
        }),
        root_invocation,
    }
}
