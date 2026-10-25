//! Builds the `SorobanAuthorizationEntry` values an OpenZeppelin smart
//! account needs for one invocation, for replaying a sequence on a real
//! network with the Stellar CLI.
//!
//! Everything comes from the environment so that no secret touches a command
//! line or a log:
//!
//! | variable | meaning |
//! | --- | --- |
//! | `BREAKWATER_NETWORK_PASSPHRASE` | network passphrase (hashes to the network id) |
//! | `BREAKWATER_ACCOUNT` | smart-account contract id (`C...`) |
//! | `BREAKWATER_RULE_ID` | context rule id the signer acts under |
//! | `BREAKWATER_NONCE` | nonce from the simulated entry |
//! | `BREAKWATER_EXPIRATION_LEDGER` | signature expiration ledger from the simulated entry |
//! | `BREAKWATER_INVOCATION_XDR` | base64 `SorobanAuthorizedInvocation` (the root invocation) |
//! | `BREAKWATER_VERIFIER` | verifier contract id for an external signer |
//! | `BREAKWATER_SIGNER_SECRET` | the signer's `S...` seed (pipe it from `stellar keys secret`) |
//!
//! Output: one base64 `SorobanAuthorizationEntry` per line on stdout.
//!
//! With `BREAKWATER_MODE=pubkey-hex` the tool instead prints the hex of the
//! Ed25519 public key behind `BREAKWATER_PUBLIC_KEY` (a `G...` address), which
//! is the `key_data` of an OpenZeppelin `Signer::External`.

use std::env;
use std::process::ExitCode;

use breakwater_pack_policy_bypass::auth::authorize_with;
use breakwater_pack_policy_bypass::strkey::{decode_public, decode_seed, hex, validate_contract};
use breakwater_pack_policy_bypass::SignerKey;
use soroban_sdk::testutils::{EnvTestConfig, Ledger as _};
use soroban_sdk::xdr::{Limits, ReadXdr, SorobanAuthorizedInvocation, WriteXdr};
use soroban_sdk::{Address, Bytes, Env};

fn var(name: &str) -> Result<String, String> {
    env::var(name).map_err(|_| format!("missing environment variable {name}"))
}

fn run() -> Result<(), String> {
    if env::var("BREAKWATER_MODE").as_deref() == Ok("pubkey-hex") {
        let public = decode_public(var("BREAKWATER_PUBLIC_KEY")?.trim())
            .map_err(|e| format!("public key: {e}"))?;
        println!("{}", hex(&public));
        return Ok(());
    }
    let passphrase = var("BREAKWATER_NETWORK_PASSPHRASE")?;
    let account = var("BREAKWATER_ACCOUNT")?;
    let rule_id: u32 = var("BREAKWATER_RULE_ID")?
        .parse()
        .map_err(|e| format!("rule id: {e}"))?;
    let nonce: i64 = var("BREAKWATER_NONCE")?
        .parse()
        .map_err(|e| format!("nonce: {e}"))?;
    let expiration: u32 = var("BREAKWATER_EXPIRATION_LEDGER")?
        .parse()
        .map_err(|e| format!("expiration ledger: {e}"))?;
    let invocation_xdr = var("BREAKWATER_INVOCATION_XDR")?;
    let verifier = var("BREAKWATER_VERIFIER")?;
    validate_contract(account.trim()).map_err(|e| format!("account id: {e}"))?;
    validate_contract(verifier.trim()).map_err(|e| format!("verifier id: {e}"))?;
    let seed = decode_seed(var("BREAKWATER_SIGNER_SECRET")?.trim())
        .map_err(|e| format!("signer secret: {e}"))?;

    let mut env = Env::new_with_config(EnvTestConfig {
        capture_snapshot_at_drop: false,
    });
    env.set_config(EnvTestConfig {
        capture_snapshot_at_drop: false,
    });
    let network_id = env
        .crypto()
        .sha256(&Bytes::from_slice(&env, passphrase.as_bytes()))
        .to_array();
    env.ledger().with_mut(|li| li.network_id = network_id);

    let root = SorobanAuthorizedInvocation::from_xdr_base64(invocation_xdr.trim(), Limits::none())
        .map_err(|e| format!("invocation xdr: {e}"))?;
    let account = Address::from_str(&env, &account);
    let key = SignerKey::external_ed25519(&Address::from_str(&env, &verifier), seed);

    let entries = authorize_with(&env, &account, &key, rule_id, root, nonce, expiration)
        .map_err(|e| e.to_string())?;
    for entry in entries {
        let b64 = entry
            .to_xdr_base64(Limits::none())
            .map_err(|e| format!("entry xdr: {e}"))?;
        println!("{b64}");
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("oz-auth-entry: {message}");
            ExitCode::from(2)
        }
    }
}
