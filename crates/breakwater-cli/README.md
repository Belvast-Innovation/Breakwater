# breakwater-cli

The `breakwater` command-line tool for [Breakwater](https://breakwater.belvast.de), a Foundry-style invariant testing framework for Soroban smart contracts.

## Install

```bash
cargo install breakwater-cli
```

This installs a binary named `breakwater`.

> **Name clash.** The crates.io crate [`breakwater`](https://crates.io/crates/breakwater) (a Pixelflut server) also installs a binary called `breakwater`. If you already have it, `cargo install` refuses to overwrite it. Either uninstall the other one first (`cargo uninstall breakwater`), or build this tool from source: clone the [repository](https://github.com/Belvast-Innovation/Breakwater), run `cargo build --release -p breakwater-cli` and use `target/release/breakwater` directly.

## Commands

```text
breakwater run <suite-dir> [--seed N] [--steps 1..=20] [--auth mock|real] [--json report.json]
breakwater export <suite-dir> --out tests/repro.rs [--seed N | --from report.json]
breakwater report report.json [--html report.html]
breakwater snapshot <suite-dir> [--seed N] [--steps 1..=20] [--auth mock|real] [--file .breakwater-snapshot.json]
breakwater snapshot <suite-dir> --check [--tolerance 5%|storage=0%,compute=5%] [--file .breakwater-snapshot.json]
```

`run` drives `cargo test --test suite` in the suite crate, so `cargo` must be on your `PATH`. It exits `0` when every invariant held, `1` when a violation was found and `2` on a usage or configuration error.

`snapshot` records what every handler of a suite costs (instructions, memory, ledger entries and bytes read and written, events, rent) in `.breakwater-snapshot.json`, and with `--check` compares a later run with it. It drives the suite's `breakwater_snapshot` test the same way, replays the seed, step range and authorization mode stored in the snapshot, and uses the same exit codes: `0` within the tolerance, `1` a value over it, `2` a usage or configuration error, including a missing, malformed or stale snapshot. `--tolerance` takes a bare percentage for every resource (the default is `5%`, which suits local use) or a comma-separated list that sets a class or a single resource: `--tolerance storage=0%,compute=5%`, `--tolerance 5%,storage=0%,instructions=3%`. The classes are `storage` (entries and bytes read and written, event size, rent) and `compute` (`instructions` and `mem_bytes`); a resource beats its class, a class beats a bare percentage, and what is not set is `5%`. Each key may appear once, as may the option; a list that does not parse exits with `2`. Every line over the tolerance shows the limit it was judged by, and the summary counts both classes. A CI gate can use `--tolerance storage=0%,compute=5%`; `--tolerance 0%` still holds every resource to exactly the snapshot. See [`breakwater-pack-budget-snapshot`](https://crates.io/crates/breakwater-pack-budget-snapshot) for how to write the test and what the numbers mean.

The suites themselves are written against [`soroban-breakwater`](https://crates.io/crates/soroban-breakwater) and, optionally, the attack packs [`breakwater-pack-policy-bypass`](https://crates.io/crates/breakwater-pack-policy-bypass), [`breakwater-pack-ttl-archival`](https://crates.io/crates/breakwater-pack-ttl-archival) and [`breakwater-pack-budget-snapshot`](https://crates.io/crates/breakwater-pack-budget-snapshot).

## License

Apache-2.0. Source: <https://github.com/Belvast-Innovation/Breakwater>.
