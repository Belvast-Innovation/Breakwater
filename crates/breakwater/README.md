# soroban-breakwater

The core library of [Breakwater](https://breakwater.belvast.de), a Foundry-style invariant testing framework for Soroban smart contracts.

A suite declares *actors* (who may call), *handlers* (what may be called) and *invariants* (what must always hold). Breakwater generates randomized call sequences against an in-process `soroban_sdk::Env`, checks every invariant after every step, shrinks any failing sequence to a minimal reproduction and exports it as JSON or a Rust regression test.

## Usage

Add the library to the `[dev-dependencies]` of the crate that holds your suite:

```toml
[dev-dependencies]
soroban-breakwater = "0.1"
```

The package is named `soroban-breakwater` because the name `breakwater` is taken on crates.io. The library itself is still called `breakwater`, so in code you write:

```rust
use breakwater::{Suite, Handler, Invariant};
```

A complete, runnable example is in the [crate documentation](https://docs.rs/soroban-breakwater).

## Related crates

- [`breakwater-cli`](https://crates.io/crates/breakwater-cli): the `breakwater` command (`run`, `export`, `report`).
- [`breakwater-pack-policy-bypass`](https://crates.io/crates/breakwater-pack-policy-bypass): invariants for OpenZeppelin smart-account policies.
- [`breakwater-pack-ttl-archival`](https://crates.io/crates/breakwater-pack-ttl-archival): invariants for Soroban TTL state archival.

## License

Apache-2.0. Source: <https://github.com/Belvast-Innovation/Breakwater>.
