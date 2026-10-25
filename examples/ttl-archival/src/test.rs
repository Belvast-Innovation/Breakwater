extern crate std;

use soroban_sdk::testutils::storage::{Instance as _, Persistent as _};
use soroban_sdk::testutils::{Address as _, Ledger as _};
use soroban_sdk::token::StellarAssetClient;
use soroban_sdk::{Address, Env};

use crate::fixed::{PaymentStreamFixed, PaymentStreamFixedClient};
use crate::{DataKey, PaymentStream, PaymentStreamClient, StreamError};

const DAY: u32 = 17_280;

/// Contract errors reach `try_` callers as host errors carrying the code.
fn err(e: StreamError) -> soroban_sdk::Error {
    soroban_sdk::Error::from_contract_error(e as u32)
}

fn setup() -> (Env, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|li| {
        li.sequence_number = 1_000_000;
        li.min_persistent_entry_ttl = 30 * DAY;
    });
    let admin = Address::generate(&env);
    let token = env.register_stellar_asset_contract_v2(admin).address();
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    StellarAssetClient::new(&env, &token).mint(&sender, &1_000_000);
    (env, token, sender, recipient)
}

#[test]
fn vests_linearly_and_settles_on_cancel() {
    let (env, token, sender, recipient) = setup();
    let id = env.register(PaymentStream, (token,));
    let client = PaymentStreamClient::new(&env, &id);

    let stream = client.create_stream(&sender, &recipient, &1_000, &100);
    assert_eq!(stream, 0);
    assert_eq!(
        client.try_withdraw(&stream),
        Err(Ok(err(StreamError::NothingToWithdraw)))
    );

    env.ledger().with_mut(|li| li.sequence_number += 25);
    assert_eq!(client.withdraw(&stream), 250);
    env.ledger().with_mut(|li| li.sequence_number += 25);
    client.cancel(&stream);
    let s = client.get_stream(&stream);
    assert!(s.cancelled);
    assert_eq!(s.withdrawn, 500);
    assert_eq!(
        client.try_cancel(&stream),
        Err(Ok(err(StreamError::AlreadyCancelled)))
    );
}

#[test]
fn buggy_contract_keeps_the_minimum_ttl_while_the_fixed_one_extends_it() {
    let (env, token, sender, recipient) = setup();
    let buggy = env.register(PaymentStream, (token.clone(),));
    let fixed = env.register(PaymentStreamFixed, (token,));
    let year = 365 * DAY;

    PaymentStreamClient::new(&env, &buggy).create_stream(&sender, &recipient, &1_000, &year);
    PaymentStreamFixedClient::new(&env, &fixed).create_stream(&sender, &recipient, &1_000, &year);

    env.as_contract(&buggy, || {
        assert_eq!(
            env.storage().persistent().get_ttl(&DataKey::Stream(0)),
            30 * DAY - 1
        );
        assert_eq!(env.storage().instance().get_ttl(), 30 * DAY - 1);
    });
    env.as_contract(&fixed, || {
        assert_eq!(
            env.storage().persistent().get_ttl(&DataKey::Stream(0)),
            year
        );
        assert_eq!(env.storage().instance().get_ttl(), year);
    });
}
