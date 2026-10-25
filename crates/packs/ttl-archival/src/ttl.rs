//! TTL probes that do not touch the entry.
//!
//! In `soroban-sdk` 28 any access to an archived persistent or instance entry
//! from a test `Env` restores it (emulating Protocol 23 auto-restore), and the
//! `testutils` `get_ttl` getters go through the same path. To observe an
//! archived entry without reviving it, read `live_until` straight from the
//! ledger entries the host holds.

use soroban_sdk::xdr::{ContractDataDurability, LedgerKey, ScAddress, ScVal};
use soroban_sdk::{Address, Env, TryFromVal, Val};

/// Which entry of a contract to look at.
#[derive(Clone, Debug)]
pub enum Entry {
    /// The contract instance (and with it the instance storage).
    Instance,
    /// A persistent entry under `key`.
    Persistent(Val),
    /// A temporary entry under `key`.
    Temporary(Val),
}

/// The ledger up to and including which the entry is live, or `None` if the
/// entry does not exist in the ledger.
pub fn live_until(env: &Env, contract: &Address, entry: &Entry) -> Option<u32> {
    let want_contract = ScAddress::from(contract);
    let (want_key, want_durability) = match entry {
        Entry::Instance => (
            ScVal::LedgerKeyContractInstance,
            ContractDataDurability::Persistent,
        ),
        Entry::Persistent(key) => (
            ScVal::try_from_val(env, key).ok()?,
            ContractDataDurability::Persistent,
        ),
        Entry::Temporary(key) => (
            ScVal::try_from_val(env, key).ok()?,
            ContractDataDurability::Temporary,
        ),
    };
    let entries = env.host().get_stored_entries().ok()?;
    entries.iter().find_map(|(k, v)| match k.as_ref() {
        LedgerKey::ContractData(cd)
            if cd.contract == want_contract
                && cd.key == want_key
                && cd.durability == want_durability =>
        {
            v.as_ref().and_then(|(_, live_until)| *live_until)
        }
        _ => None,
    })
}

/// Ledgers left before the entry is archived: `live_until - current`. Zero
/// means this is the last ledger it is live; negative means it is archived
/// already. `None` if the entry does not exist.
pub fn ttl(env: &Env, contract: &Address, entry: &Entry) -> Option<i64> {
    live_until(env, contract, entry).map(|lu| i64::from(lu) - i64::from(env.ledger().sequence()))
}
