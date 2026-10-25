# Development setup

Everything needed to build, test and demo Breakwater on a fresh machine. Target platforms: macOS (Apple Silicon or Intel) and Linux. Windows via WSL2.

## 1. Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup update stable
rustup target add wasm32v1-none          # Soroban contract target (Rust ≥ 1.85)
rustup component add rustfmt clippy
```

`rust-toolchain.toml` at the repo root pins the channel; `rustup` picks it up automatically.

Verify:

```bash
rustc --version          # 1.85.0 or newer
rustup target list --installed | grep wasm32v1-none
```

## 2. Stellar CLI

```bash
# macOS
brew install stellar-cli
# any platform
cargo install --locked stellar-cli
```

Configure Testnet and a funded identity. Keys stay in the CLI keystore; **never** write a secret key to a file in this repo.

```bash
stellar network add testnet \
  --rpc-url https://soroban-testnet.stellar.org:443 \
  --network-passphrase "Test SDF Network ; September 2015"

stellar keys generate --global breakwater-dev --network testnet --fund
stellar keys address breakwater-dev
```

## 3. Clone and build

```bash
git clone https://github.com/Belvast-Innovation/Breakwater
cd breakwater
cargo build --workspace
cargo test --workspace
```

The workspace:

| Crate | Path | Purpose |
| --- | --- | --- |
| `breakwater` | `crates/breakwater` | Core library: `Suite`, `Handler`, `Invariant`, sequence generation, shrinking, export |
| `breakwater-cli` | `crates/breakwater-cli` | `breakwater run \| export \| report` |
| `breakwater-pack-policy-bypass` | `crates/packs/policy-bypass` | Invariants and generators for OpenZeppelin smart accounts |
| `breakwater-pack-ttl-archival` | `crates/packs/ttl-archival` | Invariants and ledger-advance strategies for TTL |
| `example-policy-bypass` | `examples/policy-bypass` | Deliberately misconfigured smart account + suite |
| `example-ttl-archival` | `examples/ttl-archival` | Payment-stream contract that never extends TTL |

Build the example contracts to WASM (only needed for Testnet deploys; tests link them natively):

```bash
stellar contract build --package example-policy-bypass
stellar contract build --package example-ttl-archival
```

## 4. Dependencies and where they come from

| Dependency | Why | Docs |
| --- | --- | --- |
| `soroban-sdk` (+ `testutils`) | Soroban `Env`, ledger manipulation, auth, TTL getters | https://docs.rs/soroban-sdk |
| `proptest`, `proptest-state-machine` | Random sequence generation and automatic shrinking | https://docs.rs/proptest-state-machine |
| OpenZeppelin `stellar-contracts` | Smart-account, policy and signer contracts used as the demo target | https://docs.openzeppelin.com/stellar-contracts |
| `clap`, `serde`, `serde_json`, `anyhow`, `thiserror` | CLI, report serialisation, error handling | docs.rs |
| `soroban-fork` (optional, feature `fork`) | Lazy-loading mainnet/testnet fork for fuzzing live state | https://docs.rs/soroban-fork |

Versions are pinned in `Cargo.toml`; `Cargo.lock` is committed.

## 5. AI tooling: Claude Code, Raven MCP, Stellar Dev Skill

The project is developed with Claude Code. The working agreement is in [`CLAUDE.md`](../CLAUDE.md); read it before your first session.

### Raven MCP

Raven is Stellar's official remote MCP server: one connection that gives the agent the Stellar docs, SDK references and ecosystem context. Add it once per machine:

```bash
claude mcp add --transport http raven https://raven.stellar.org/mcp
```

The first call opens a browser for OAuth sign-in; tokens are managed by the client. The repo also ships `.mcp.json` so the server is picked up automatically in this project:

```json
{
  "mcpServers": {
    "raven": {
      "type": "http",
      "url": "https://raven.stellar.org/mcp"
    }
  }
}
```

Support channel: `#raven` in the Stellar Developers Discord.

### Stellar Dev Skill

The Stellar Dev Skill plugin gives the agent up-to-date knowledge of the full Stellar stack (SDK idioms, CLI, x402/MPP). Install from the Claude Code plugin marketplace or follow https://developers.stellar.org/tools/ai. Also useful: `https://developers.stellar.org/llms.txt` for feeding docs context directly.

### Other reference material

- Soroban fuzzing example: https://developers.stellar.org/docs/build/smart-contracts/example-contracts/fuzzing
- Testing TTL extension: https://developers.stellar.org/docs/build/guides/archival/test-ttl-extension
- Soroban authorization: https://developers.stellar.org/docs/learn/fundamentals/contract-development/authorization
- Protocol 28 (Adapter) upgrade notes: https://stellar.org/blog/developers/introducing-adapter-protocol-28-on-stellar

## 6. Day-to-day commands

```bash
# format, lint, test (what CI runs)
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# run a suite with a fixed seed
cargo run -p breakwater-cli -- run examples/policy-bypass --seed 42 --steps 30

# export the last failure as a unit test
cargo run -p breakwater-cli -- export examples/policy-bypass --out tests/repro.rs

# deploy a demo contract to Testnet
stellar contract deploy \
  --wasm target/wasm32v1-none/release/example_ttl_archival.wasm \
  --source breakwater-dev --network testnet
```

## 7. Git workflow (short version; full rules in CLAUDE.md §3)

```bash
git switch main && git pull
git switch -c feat/<slug>
# ... work, small atomic commits in Conventional Commits format ...
git push -u origin feat/<slug>
gh pr create --fill          # uses .github/pull_request_template.md
# CI green → squash-merge in the GitHub UI → branch auto-deleted
```

Before every push:

```bash
git log -1 --format='%an <%ae>%n%B'   # author is you; no AI attribution trailers
```

## 8. Troubleshooting

| Symptom | Fix |
| --- | --- |
| `error: target wasm32v1-none not found` | `rustup target add wasm32v1-none`; needs Rust ≥ 1.85 |
| `testutils` items missing | enable the feature: `soroban-sdk = { version = "...", features = ["testutils"] }` in `[dev-dependencies]` |
| OpenZeppelin crate version mismatch with `soroban-sdk` | both must target the same SDK major; check `Cargo.lock` and pin, do not bump mid-event |
| Testnet account not found | `stellar keys generate ... --fund` again, or use Friendbot: `https://friendbot.stellar.org/?addr=<G...>` |
| Fuzz run is slow | reuse an `Env` snapshot between iterations (`Env::from_ledger_snapshot`) instead of a fresh `Env` per step |
| Bare `panic!` reported as a bug | that is intended; contracts must use `panic_with_error!` |
