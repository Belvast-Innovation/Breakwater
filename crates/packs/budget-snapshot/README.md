# breakwater-pack-budget-snapshot

The `budget-snapshot` attack pack for [Breakwater](https://breakwater.belvast.de): a `forge snapshot` for Soroban. It measures the resources every handler of a suite consumes, stores `max` and `p50` per resource in a file you commit (`.breakwater-snapshot.json`), and lets CI fail when a later run is over the recorded numbers.

It looks for regressions in cost, not for violations of an invariant. A change that adds a storage write, grows an entry or emits one more event does not break any functional test, but it makes every call more expensive.

## Usage

Add it next to the core library in the `[dev-dependencies]` of your suite crate:

```toml
[dev-dependencies]
soroban-breakwater = "0.1"
breakwater-pack-budget-snapshot = "0.1"
```

The core package is named `soroban-breakwater` but its library is called `breakwater`, so code still writes `use breakwater::...`.

Add a `breakwater_snapshot` test next to `breakwater_suite` in `tests/suite.rs`. Wrap each handler with the recorder and name the contract function it calls, add the recorder's invariant (a suite needs at least one), run the suite, and write the result where the CLI asks for it:

```rust
use breakwater_pack_budget_snapshot::{Recorder, Target};

#[test]
fn breakwater_snapshot() {
    let config = SuiteConfig::from_env(my_config());
    let recorder = Recorder::new();
    let report = Suite::new("my-contract-budget", env)
        .config(config.clone())
        .handler(recorder.handler(Withdraw { /* ... */ }, Target::new(contract_id, "withdraw")))
        .invariant(recorder.invariant())
        .run()
        .expect("suite runs");
    let measured = recorder.finish(&config, &report).expect("complete measurement");
    measured.write_if_requested().expect("measurement is written");
}
```

Then, with the `breakwater` command-line tool (`cargo install breakwater-cli`):

```bash
breakwater snapshot path/to/suite --seed 7              # record .breakwater-snapshot.json
breakwater snapshot path/to/suite --check               # exit 1 on a regression
breakwater snapshot path/to/suite --check --tolerance 0%
```

`--check` replays the seed, step range and authorization mode stored in the snapshot. It exits `0` when every value is within the tolerance (default `5%`), `1` on a regression, and `2` on a usage or configuration error, which includes a missing, malformed or stale snapshot (other `soroban-sdk` or `soroban-env-host` version, other seed, step range, authorization or metering mode, or a changed generated workload).

The measured call must be the last top-level call the handler makes, because the host reports only the last one. A handler that calls the contract again afterwards makes the measurement fail instead of being misread. Failed calls are not measured.

## What the numbers mean

The snapshot's `mode` is `native`: the contract runs as Rust code in the test host.

- **Storage-related figures are reliable.** Read and write entries and bytes, contract event size and rent (ledger-bytes and bumped entries) follow the contract's behaviour directly, so a new storage write, a larger entry or an extra event shows up as it would on the network.
- **`instructions` and `mem_bytes` are relative only.** They leave out Wasm instantiation and VM execution. Use them to compare one revision of the same code base with another, never as an on-chain fee.
- **No figure includes the transaction's own size.**
- **A snapshot is the distribution a seed generates**, not an upper bound on what a handler can cost.

The numbers come from the host's cost model, not from a clock, and the generated workload is fixed by the seed. Under the same locked dependencies the figures are identical value for value on any machine; the Breakwater repository's CI verifies it with `--tolerance 0%`.

See the [crate documentation](https://docs.rs/breakwater-pack-budget-snapshot).

## License

Apache-2.0. Source: <https://github.com/Belvast-Innovation/Breakwater>.
