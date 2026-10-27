# breakwater-pack-policy-bypass

The `policy-bypass` attack pack for [Breakwater](https://breakwater.belvast.de): invariants and generators that hunt for multi-step sequences slipping past OpenZeppelin smart-account policies (spending limits, context rules, signer scopes).

## Usage

Add it next to the core library in the `[dev-dependencies]` of your suite crate:

```toml
[dev-dependencies]
soroban-breakwater = "0.1"
breakwater-pack-policy-bypass = "0.1"
```

The core package is named `soroban-breakwater` but its library is called `breakwater`, so code still writes `use breakwater::...`. The pack provides the invariants `RollingCap`, `AllowlistOnly`, `RevokedNeverAuths` and `PinnedWasm`, authorization helpers for running a suite with real authorization entries, and generators biased toward known bypass shapes. See the [crate documentation](https://docs.rs/breakwater-pack-policy-bypass).

## Testnet replay tool

The Testnet replay helper `oz-auth-entry` is not part of the published package. To replay a sequence on Testnet, clone the repository and run `scripts/replay-testnet.sh`.

## License

Apache-2.0. Source: <https://github.com/Belvast-Innovation/Breakwater>.
