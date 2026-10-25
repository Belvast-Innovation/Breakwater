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

stellar keys generate breakwater-dev --network testnet --fund
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
| `ed25519-dalek` | Signs the `AuthDigestPreimage` digest for External (Ed25519) smart-account signers in `AuthMode::Real` | https://docs.rs/ed25519-dalek |
| `soroban-fork` (not yet a dependency) | Lazy-loading mainnet/testnet fork for fuzzing live state. Currently supports `soroban-sdk` 25.x only; it will be added once it supports SDK 28 | https://docs.rs/soroban-fork |

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

# run a suite with a fixed seed (drives `cargo test --test suite breakwater_suite` in that crate)
cargo run -p breakwater-cli -- run examples/policy-bypass --seed 7 --steps 1..=10 --auth real

# export the shrunk failure as a regression test, or render a saved report
cargo run -p breakwater-cli -- export examples/policy-bypass --seed 7 --out examples/policy-bypass/tests/repro_policy_bypass_7.rs
cargo run -p breakwater-cli -- run examples/ttl-archival --seed 11 --json run.json
cargo run -p breakwater-cli -- report run.json --html docs/reports/ttl-archival-seed-11.html

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

## 8. Replaying on Testnet

`scripts/replay-testnet.sh` replays a shrunk `policy-bypass` sequence as real transactions through the smart account's `__check_auth`. Local fuzzing stays local; only the exported reproduction goes on-chain.

Prerequisites: funded identities in the Stellar CLI keystore (`stellar keys generate <name> --network testnet --fund`), `jq`, and the three helper contracts (owner key account, spending-limit policy, Ed25519 verifier) built as separate wasm files, passed as `OWNER_WASM`, `POLICY_WASM`, `VERIFIER_WASM`. Identities used by default:

| Identity | Role |
| --- | --- |
| `breakwater-dev` | pays fees, issues the test `USDC` asset, deploys the contracts |
| `breakwater-recipient` | merchant receiving the payments (holds a `USDC` trustline) |
| `breakwater-agent` | Ed25519 key behind the agent's `Signer::External` |
| `breakwater-owner` | Ed25519 key behind the owner's `Signer::Delegated` account |

```bash
# setup (deploy, configure, mint) and the first two payments
scripts/replay-testnet.sh breakwater-out/policy-bypass.json --until-step 1

# one policy window later: the third payment
scripts/replay-testnet.sh breakwater-out/policy-bypass.json --resume-from 2

# print every command instead of running it
scripts/replay-testnet.sh breakwater-out/policy-bypass.json --dry-run
```

State (contract ids, completed steps, ledger of the last step) lives in `breakwater-out/replay-<network>.json`. A step with a ledger gap prints the current ledger, the target ledger and the estimated wait.

How an agent payment is submitted: `stellar contract invoke --build-only` simulates `usdc.transfer(account, merchant, amount)` and records an authorization entry for the smart account; the script reads that entry's nonce, expiration ledger and root invocation, has `oz-auth-entry` (a binary in the `policy-bypass` pack) sign the OpenZeppelin `AuthPayload` for it with the agent key, splices the signed entry back into the envelope (`stellar tx decode` / `stellar xdr encode` / `stellar tx encode`), signs with the source key and sends. Signer seeds reach the helper only through the environment, read by `stellar keys secret`; nothing secret is printed or written.

Two things the on-chain path needs that the local demo does not: `DemoAccount` has a constructor that installs the initial context rules (an account with zero rules can never authorize `add_context_rule` on itself; the test suite passes an empty list), and the helper contracts are separate crates under `examples/policy-bypass/contracts/`, because one cdylib cannot export two `__check_auth` functions. `stellar contract build --package <crate>` builds each one.

Deployed on Testnet on 25 October 2026 (protocol 29 accepted the SDK 28 wasm; the smart account's 47 KB upload cost about 7 300 XLM in Testnet resource fees, so fund the source account generously or run `stellar keys fund`):

| Contract | Id |
| --- | --- |
| `DemoAccount` | `CBKDNENFLCLUE3U4NH55T5N5JIX6S6WS2FFDXCPWMXJM7ZQGPXVH3HJI` |
| `SpendingLimitPolicy` (per call) | `CCIEVOGO6RZSCHN6COE3NLMDT3WOXSPTDPYDJMPVSULFREACAJ6CS247` |
| `SpendingLimitPolicy` ("daily") | `CC6Z5UZYWG5CR43YYGUZCMOGQHO2NEQZIU7M4MBR2NG3ELZQN7G2RRD7` |
| `Ed25519Verifier` | `CA7UFPQ3MHRQGIOHG6B5TPBJZLZL6HCNCYNWPILVDDC6AHOMM5BJ5OJN` |
| `OwnerAccount` | `CC2N5OKRJ5MA2SEVIUSRSMPRODN73Z2OH6CYZ7YHIZVAVD72N6K4PSCB` |
| `USDC` Stellar Asset Contract | `CDDSS44LN2N4TNVTDH42PLEVHKDB5OZQRBHAD2KQRH6LIADRIBUMPDMM` |
| `PaymentStream` | `CBS6AO2L5RKKLHIC2FJOH66UIL2OHCPSSO6GAUCYNXMMI3RRQLG3D5KP` |

The payment-stream demo needs no custom authorization: `stellar contract deploy ... -- --token <sac>`, then `invoke -- create_stream --sender <G> --recipient <G> --amount 10000000000 --duration_ledgers 6307200` as the sender and `invoke --source <recipient> -- withdraw --id 0` as the recipient. Testnet closes a ledger about every 5 s (measured 31 ledgers in 155 s), so one policy window of 1 728 ledgers is about 2 h 25 min.

## 9. Troubleshooting

| Symptom | Fix |
| --- | --- |
| `error: target wasm32v1-none not found` | `rustup target add wasm32v1-none`; needs Rust ≥ 1.85 |
| `error: soroban-sdk requires stellar-cli v25.2.0+ to build a contract` | Use `stellar contract build --package <crate>` instead of `cargo build --target wasm32v1-none`; SDK 28 needs the CLI's spec shaking |
| `testutils` items missing | enable the feature: `soroban-sdk = { version = "...", features = ["testutils"] }` in `[dev-dependencies]` |
| OpenZeppelin crate version mismatch with `soroban-sdk` | both must target the same SDK major; check `Cargo.lock` and pin, do not bump mid-event |
| Testnet account not found | `stellar keys generate ... --fund` again, or use Friendbot: `https://friendbot.stellar.org/?addr=<G...>` |
| Fuzz run is slow | reuse an `Env` snapshot between iterations (`Env::from_ledger_snapshot`) instead of a fresh `Env` per step |
| Bare `panic!` reported as a bug | that is intended; contracts must use `panic_with_error!` |
