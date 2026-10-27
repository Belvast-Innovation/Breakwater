# breakwater-pack-ttl-archival

The `ttl-archival` attack pack for [Breakwater](https://breakwater.belvast.de): invariants and ledger-advance strategies that find contract state expiring under Soroban's TTL-based state archival while the business logic still depends on it.

## Usage

Add it next to the core library in the `[dev-dependencies]` of your suite crate:

```toml
[dev-dependencies]
soroban-breakwater = "0.1"
breakwater-pack-ttl-archival = "0.1"
```

The core package is named `soroban-breakwater` but its library is called `breakwater`, so code still writes `use breakwater::...`. The pack provides side-effect-free TTL probes, a model of live streams derived from the transaction log, and the invariants `NoMidLifecycleExpiry`, `InstanceAlive`, `RestoreRestoresFunction`, `NoUnboundedTtlExtension` and `RestoreCostWithinBudget`. See the [crate documentation](https://docs.rs/breakwater-pack-ttl-archival).

## License

Apache-2.0. Source: <https://github.com/Belvast-Innovation/Breakwater>.
