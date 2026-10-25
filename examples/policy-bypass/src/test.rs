extern crate std;

use soroban_sdk::Env;

use crate::{DemoWallet, DemoWalletClient, Error};

fn client(env: &Env) -> DemoWalletClient<'_> {
    let id = env.register(DemoWallet, ());
    DemoWalletClient::new(env, &id)
}

#[test]
fn ping_returns_the_value() {
    let env = Env::default();
    assert_eq!(client(&env).ping(&7), 7);
}

#[test]
fn ping_rejects_zero_with_a_contract_error() {
    let env = Env::default();
    // `ping` returns a plain `u32`, so the host surfaces the `panic_with_error!`
    // as a contract error code rather than as the `Error` enum itself.
    let expected = soroban_sdk::Error::from_contract_error(Error::ZeroValue as u32);
    assert_eq!(client(&env).try_ping(&0), Err(Ok(expected)));
}
