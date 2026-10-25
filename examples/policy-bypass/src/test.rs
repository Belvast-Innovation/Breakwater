extern crate std;

use soroban_sdk::testutils::Address as _;
use soroban_sdk::{vec, Address, Env, Map, String};
use stellar_accounts::smart_account::{ContextRuleType, Signer};

use crate::{DemoAccount, DemoAccountClient, InitialRule};

#[test]
fn account_deploys_and_accepts_a_context_rule() {
    let env = Env::default();
    env.mock_all_auths();
    let account = env.register(DemoAccount, (soroban_sdk::Vec::<InitialRule>::new(&env),));
    let client = DemoAccountClient::new(&env, &account);

    let rule = client.add_context_rule(
        &ContextRuleType::Default,
        &String::from_str(&env, "owner"),
        &None,
        &vec![&env, Signer::Delegated(Address::generate(&env))],
        &Map::new(&env),
    );

    assert_eq!(rule.id, 0);
    assert_eq!(client.get_context_rules_count(), 1);
}
