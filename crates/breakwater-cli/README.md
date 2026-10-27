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
```

`run` drives `cargo test --test suite` in the suite crate, so `cargo` must be on your `PATH`. It exits `0` when every invariant held, `1` when a violation was found and `2` on a usage or configuration error.

The suites themselves are written against [`soroban-breakwater`](https://crates.io/crates/soroban-breakwater) and, optionally, the attack packs [`breakwater-pack-policy-bypass`](https://crates.io/crates/breakwater-pack-policy-bypass) and [`breakwater-pack-ttl-archival`](https://crates.io/crates/breakwater-pack-ttl-archival).

## License

Apache-2.0. Source: <https://github.com/Belvast-Innovation/Breakwater>.
