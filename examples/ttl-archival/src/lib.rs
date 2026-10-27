//! Demo target for the `ttl-archival` attack pack: a minimal payment stream.
//!
//! `create_stream` locks `amount` of a token and vests it linearly to the
//! recipient over `duration_ledgers`; `withdraw` pays out what has vested,
//! `cancel` settles both sides. Streams live in **persistent** storage, the
//! token address and the id counter in **instance** storage.
//!
//! The deliberate defect: [`PaymentStream`] never calls `extend_ttl`. Every
//! entry keeps the TTL it got when it was created, so a one-year stream is
//! archived as soon as the network's minimum persistent TTL runs out, even
//! though the business logic says it has months to go. Its second defect is
//! `bump_ttl`: a public, unauthenticated, uncapped TTL extension, which lets
//! anyone keep entries alive at the contract's rent. `fixed::PaymentStreamFixed`
//! is the same contract with the extensions in place and `bump_ttl` guarded,
//! used as the control.
#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, panic_with_error, token, Address, Env, Vec,
};

/// Storage layout. `Token` and `NextId` are instance entries, `Stream(id)` is
/// a persistent entry.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DataKey {
    /// Address of the token being streamed (instance).
    Token,
    /// Next stream id (instance).
    NextId,
    /// A stream (persistent).
    Stream(u64),
}

/// One payment stream.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stream {
    /// Who funded the stream and may cancel it.
    pub sender: Address,
    /// Who receives the vested amount.
    pub recipient: Address,
    /// Total amount locked at creation.
    pub amount: i128,
    /// Amount already paid to the recipient.
    pub withdrawn: i128,
    /// Ledger at which vesting starts.
    pub start_ledger: u32,
    /// Ledger at which everything has vested.
    pub end_ledger: u32,
    /// Set once the sender cancelled; the entry stays readable.
    pub cancelled: bool,
}

/// Errors of the payment stream.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum StreamError {
    /// No stream with this id.
    StreamNotFound = 1,
    /// `amount` must be positive.
    InvalidAmount = 2,
    /// `duration_ledgers` must be positive.
    InvalidDuration = 3,
    /// Nothing has vested since the last withdrawal.
    NothingToWithdraw = 4,
    /// The stream was cancelled already.
    AlreadyCancelled = 5,
}

/// Shared logic; `EXTEND` selects whether TTLs are extended on every touch.
mod logic {
    use super::*;

    fn token(env: &Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::Token)
            .unwrap_or_else(|| panic_with_error!(env, StreamError::StreamNotFound))
    }

    fn load(env: &Env, id: u64) -> Stream {
        env.storage()
            .persistent()
            .get(&DataKey::Stream(id))
            .unwrap_or_else(|| panic_with_error!(env, StreamError::StreamNotFound))
    }

    /// Keeps the entry and the instance alive for as long as the stream runs.
    fn keep_alive<const EXTEND: bool>(env: &Env, id: u64, stream: &Stream) {
        if !EXTEND {
            return;
        }
        let remaining = stream
            .end_ledger
            .saturating_sub(env.ledger().sequence())
            .max(1);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Stream(id), remaining, remaining);
        env.storage().instance().extend_ttl(remaining, remaining);
    }

    pub fn vested(env: &Env, stream: &Stream) -> i128 {
        let now = env.ledger().sequence();
        if now >= stream.end_ledger {
            return stream.amount;
        }
        if now <= stream.start_ledger {
            return 0;
        }
        let elapsed = i128::from(now - stream.start_ledger);
        let total = i128::from(stream.end_ledger - stream.start_ledger);
        stream.amount * elapsed / total
    }

    pub fn constructor(env: &Env, token: Address) {
        env.storage().instance().set(&DataKey::Token, &token);
        env.storage().instance().set(&DataKey::NextId, &0u64);
    }

    pub fn create_stream<const EXTEND: bool>(
        env: &Env,
        sender: Address,
        recipient: Address,
        amount: i128,
        duration_ledgers: u32,
    ) -> u64 {
        sender.require_auth();
        if amount <= 0 {
            panic_with_error!(env, StreamError::InvalidAmount);
        }
        if duration_ledgers == 0 {
            panic_with_error!(env, StreamError::InvalidDuration);
        }
        let this = env.current_contract_address();
        token::Client::new(env, &token(env)).transfer(&sender, &this, &amount);

        let id: u64 = env.storage().instance().get(&DataKey::NextId).unwrap_or(0);
        env.storage().instance().set(&DataKey::NextId, &(id + 1));
        let start_ledger = env.ledger().sequence();
        let stream = Stream {
            sender,
            recipient,
            amount,
            withdrawn: 0,
            start_ledger,
            end_ledger: start_ledger.saturating_add(duration_ledgers),
            cancelled: false,
        };
        env.storage()
            .persistent()
            .set(&DataKey::Stream(id), &stream);
        keep_alive::<EXTEND>(env, id, &stream);
        id
    }

    pub fn withdraw<const EXTEND: bool>(env: &Env, id: u64) -> i128 {
        let mut stream = load(env, id);
        stream.recipient.require_auth();
        if stream.cancelled {
            panic_with_error!(env, StreamError::AlreadyCancelled);
        }
        let due = vested(env, &stream) - stream.withdrawn;
        if due <= 0 {
            panic_with_error!(env, StreamError::NothingToWithdraw);
        }
        token::Client::new(env, &token(env)).transfer(
            &env.current_contract_address(),
            &stream.recipient,
            &due,
        );
        stream.withdrawn += due;
        env.storage()
            .persistent()
            .set(&DataKey::Stream(id), &stream);
        keep_alive::<EXTEND>(env, id, &stream);
        due
    }

    pub fn cancel<const EXTEND: bool>(env: &Env, id: u64) {
        let mut stream = load(env, id);
        stream.sender.require_auth();
        if stream.cancelled {
            panic_with_error!(env, StreamError::AlreadyCancelled);
        }
        let token = token::Client::new(env, &token(env));
        let vested = vested(env, &stream);
        let to_recipient = vested - stream.withdrawn;
        let to_sender = stream.amount - vested;
        if to_recipient > 0 {
            token.transfer(
                &env.current_contract_address(),
                &stream.recipient,
                &to_recipient,
            );
        }
        if to_sender > 0 {
            token.transfer(&env.current_contract_address(), &stream.sender, &to_sender);
        }
        stream.withdrawn = vested;
        stream.cancelled = true;
        env.storage()
            .persistent()
            .set(&DataKey::Stream(id), &stream);
        keep_alive::<EXTEND>(env, id, &stream);
    }

    pub fn get_stream(env: &Env, id: u64) -> Stream {
        load(env, id)
    }

    /// Withdraws from several streams in one invocation. Every stream in
    /// `ids` must be withdrawable; the total paid is returned.
    pub fn withdraw_many<const EXTEND: bool>(env: &Env, ids: Vec<u64>) -> i128 {
        let mut total = 0i128;
        for id in ids.iter() {
            total += withdraw::<EXTEND>(env, id);
        }
        total
    }

    /// Extends the TTL of a stream entry and of the instance so that both
    /// live at least `ledgers` more ledgers. The buggy contract exposes this
    /// without any authorization or cap: anyone can keep any entry alive for
    /// as long as the network allows, at the contract's rent.
    pub fn bump_ttl_unchecked(env: &Env, id: u64, ledgers: u32) {
        if !env.storage().persistent().has(&DataKey::Stream(id)) {
            panic_with_error!(env, StreamError::StreamNotFound);
        }
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Stream(id), ledgers, ledgers);
        env.storage().instance().extend_ttl(ledgers, ledgers);
    }

    /// The fixed contract's version: only the sender may extend, and never
    /// beyond the stream's remaining duration.
    #[cfg(any(test, feature = "fixed"))]
    pub fn bump_ttl_checked(env: &Env, id: u64, ledgers: u32) {
        let stream = load(env, id);
        stream.sender.require_auth();
        let remaining = stream
            .end_ledger
            .saturating_sub(env.ledger().sequence())
            .max(1);
        let ledgers = ledgers.min(remaining);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Stream(id), ledgers, ledgers);
        env.storage().instance().extend_ttl(ledgers, ledgers);
    }
}

/// The buggy contract: correct accounting, no TTL management.
#[contract]
pub struct PaymentStream;

#[contractimpl]
impl PaymentStream {
    /// Sets the token that streams are denominated in.
    pub fn __constructor(env: Env, token: Address) {
        logic::constructor(&env, token);
    }

    /// Locks `amount` from `sender` and vests it to `recipient` over
    /// `duration_ledgers`. Returns the stream id.
    pub fn create_stream(
        env: Env,
        sender: Address,
        recipient: Address,
        amount: i128,
        duration_ledgers: u32,
    ) -> u64 {
        logic::create_stream::<false>(&env, sender, recipient, amount, duration_ledgers)
    }

    /// Pays the recipient what has vested since the last withdrawal.
    pub fn withdraw(env: Env, id: u64) -> i128 {
        logic::withdraw::<false>(&env, id)
    }

    /// Settles the stream: vested funds to the recipient, the rest back to
    /// the sender.
    pub fn cancel(env: Env, id: u64) {
        logic::cancel::<false>(&env, id)
    }

    /// Reads a stream.
    pub fn get_stream(env: Env, id: u64) -> Stream {
        logic::get_stream(&env, id)
    }

    /// Withdraws from several streams in one call; returns the total paid.
    pub fn withdraw_many(env: Env, ids: Vec<u64>) -> i128 {
        logic::withdraw_many::<false>(&env, ids)
    }

    /// Extends the stream's and the instance's TTL by `ledgers`. Deliberately
    /// unauthenticated and uncapped: the second defect of this contract.
    pub fn bump_ttl(env: Env, id: u64, ledgers: u32) {
        logic::bump_ttl_unchecked(&env, id, ledgers)
    }
}

/// The control contract: identical accounting, TTLs extended on every touch.
#[cfg(any(test, feature = "fixed"))]
pub mod fixed {
    use super::*;

    /// Same as [`PaymentStream`] but keeps its entries alive.
    #[contract]
    pub struct PaymentStreamFixed;

    #[contractimpl]
    impl PaymentStreamFixed {
        /// Sets the token that streams are denominated in.
        pub fn __constructor(env: Env, token: Address) {
            logic::constructor(&env, token);
        }

        /// See [`PaymentStream::create_stream`]; also extends the stream and
        /// instance TTL to the stream's remaining duration.
        pub fn create_stream(
            env: Env,
            sender: Address,
            recipient: Address,
            amount: i128,
            duration_ledgers: u32,
        ) -> u64 {
            logic::create_stream::<true>(&env, sender, recipient, amount, duration_ledgers)
        }

        /// See [`PaymentStream::withdraw`].
        pub fn withdraw(env: Env, id: u64) -> i128 {
            logic::withdraw::<true>(&env, id)
        }

        /// See [`PaymentStream::cancel`].
        pub fn cancel(env: Env, id: u64) {
            logic::cancel::<true>(&env, id)
        }

        /// Reads a stream.
        pub fn get_stream(env: Env, id: u64) -> Stream {
            logic::get_stream(&env, id)
        }

        /// Withdraws from several streams in one call; returns the total paid.
        pub fn withdraw_many(env: Env, ids: Vec<u64>) -> i128 {
            logic::withdraw_many::<true>(&env, ids)
        }

        /// Sender-only TTL extension, capped at the stream's remaining
        /// duration.
        pub fn bump_ttl(env: Env, id: u64, ledgers: u32) {
            logic::bump_ttl_checked(&env, id, ledgers)
        }
    }
}

#[cfg(test)]
mod test;
