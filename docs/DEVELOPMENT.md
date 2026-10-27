# Development setup

Everything needed to build, test and demo Breakwater on a fresh machine. Target platforms: macOS (Apple Silicon or Intel) and Linux. Windows via WSL2.

## 1. Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup update stable
rustup target add wasm32v1-none          # Soroban contract target
rustup component add rustfmt clippy
```

`rust-toolchain.toml` at the repo root pins the channel; `rustup` picks it up automatically.

Verify:

```bash
rustc --version          # 1.91.0 or newer (soroban-sdk 28.0.0 requires it)
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
cd Breakwater
cargo build --workspace
cargo test --workspace
```

The workspace:

| Crate | Path | Purpose |
| --- | --- | --- |
| `breakwater` | `crates/breakwater` | Core library: `Suite`, `Handler`, `Invariant`, sequence generation, shrinking, export |
| `breakwater-cli` | `crates/breakwater-cli` | `breakwater run \| export \| report \| snapshot` |
| `breakwater-pack-policy-bypass` | `crates/packs/policy-bypass` | Invariants and generators for OpenZeppelin smart accounts |
| `breakwater-pack-ttl-archival` | `crates/packs/ttl-archival` | Invariants and ledger-advance strategies for TTL |
| `breakwater-pack-budget-snapshot` | `crates/packs/budget-snapshot` | Per-handler resource sampling, the `.breakwater-snapshot.json` model and the regression comparison |
| `example-policy-bypass` | `examples/policy-bypass` | `DemoAccount`, an OpenZeppelin smart account with a mis-sized spending window, plus the suite |
| `example-owner-account` | `examples/policy-bypass/contracts/owner-account` | The owner's Ed25519 key account (`Signer::Delegated` of the demo account) |
| `example-spending-limit-policy` | `examples/policy-bypass/contracts/spending-limit-policy` | OpenZeppelin `SpendingLimit` as a deployable contract |
| `example-ed25519-verifier` | `examples/policy-bypass/contracts/ed25519-verifier` | OpenZeppelin Ed25519 verifier as a deployable contract |
| `example-ttl-archival` | `examples/ttl-archival` | `PaymentStream`, which never extends TTL, and `PaymentStreamFixed` behind the `fixed` feature |

Publishing is automated. The five published crates share the workspace version; a release is a PR titled `release: vX.Y.Z` that bumps it (prepared by the `release` skill, `.claude/skills/release/SKILL.md`). When that PR is merged, `.github/workflows/release.yml` plans the release (`scripts/release-plan.py`), runs the CI checks again, waits for one approval of the `crates-io` environment, and publishes through crates.io Trusted Publishing with a short-lived token. It publishes in dependency order as read from `cargo metadata` (`soroban-breakwater` first; `breakwater-cli` after `breakwater-pack-budget-snapshot`, which it uses for the snapshot format), skips any crate whose version is already on the index, so a failed run can simply be re-run, and then tags the commit `vX.Y.Z` and creates the GitHub Release. Dispatching the workflow by hand with `dry_run` (the default) runs only `cargo publish --workspace --dry-run`. CLAUDE.md §10 has the rules, including when a manual `cargo publish` is acceptable.

Not crates: `scripts/replay-testnet.sh` (Testnet replay, §8) and `docs/reports/` (HTML reports rendered by `breakwater report --html`).

Build the contracts to WASM (needed for Testnet deploys; tests link them natively). This is what CI runs, with warnings denied; a bare `cargo build --target wasm32v1-none` is rejected by `soroban-sdk` 28:

```bash
export RUSTFLAGS="-D warnings"
stellar contract build --package example-policy-bypass
stellar contract build --package example-owner-account
stellar contract build --package example-spending-limit-policy
stellar contract build --package example-ed25519-verifier
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

`Cargo.toml` declares minimum versions (caret ranges, not `=` pins), because the requirements are copied into the published library crates. `Cargo.lock` is committed and fixes the exact versions the workspace is built and tested with.

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
# format, lint, test, wasm (what CI runs; the wasm step denies warnings too)
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
RUSTFLAGS="-D warnings" stellar contract build --package example-ttl-archival   # and the other four packages, see §3

# run a suite with a fixed seed (drives `cargo test --test suite breakwater_suite` in that crate)
cargo run -p breakwater-cli -- run examples/policy-bypass --seed 7 --steps 1..=10 --auth real

# record per-handler resource usage, then check a later run against it (exit 1 on a regression)
cargo run -p breakwater-cli -- snapshot examples/ttl-archival --seed 7
cargo run -p breakwater-cli -- snapshot examples/ttl-archival --check
cargo run -p breakwater-cli -- snapshot examples/ttl-archival --check --features regress-demo   # exit 1

# export the shrunk failure as a regression test, or render a saved report
cargo run -p breakwater-cli -- export examples/policy-bypass --seed 7 --out examples/policy-bypass/tests/repro_policy_bypass_7.rs
cargo run -p breakwater-cli -- run examples/ttl-archival --seed 11 --json run.json
cargo run -p breakwater-cli -- report run.json --html docs/reports/ttl-archival-seed-11.html

# deploy a demo contract to Testnet (constructor arguments follow `--`)
stellar contract deploy \
  --wasm target/wasm32v1-none/release/example_ttl_archival.wasm \
  --source breakwater-dev --network testnet \
  -- --token <usdc-sac-contract-id>
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

Replayed payments (49 USDC each, agent-signed, `SUCCESS`): ledger 5383703 `b5ea44983473462379a6909abc335105a040f6d4fee83908cf97dee49cec6772`, ledger 5383715 `82be1eb8d373bf91aecc9ca4743041ea1f50d1961f7b5fb7ba7a3f9add1e5608`, ledger 5387240 `3f30f306985585ff62b31271f7aa050a26ec82f6458925d17aa5c32ce157aa8e`. The merchant's balance went from 0 to 147 USDC.

The state file and the report are local (`breakwater-out/` is gitignored). If they are lost, regenerate the report with `breakwater run examples/policy-bypass --seed 7 --json "$PWD/breakwater-out/policy-bypass.json"` and rebuild the state file from the ids above with `jq -n`; the keys `account`, `token`, `verifier`, `policy_per_call`, `policy_daily`, `owner_account`, `agent_rule` and `last_step_ledger` are what `--resume-from` reads.

The payment-stream demo needs no custom authorization: `stellar contract deploy ... -- --token <sac>`, then `invoke -- create_stream --sender <G> --recipient <G> --amount 10000000000 --duration_ledgers 6307200` as the sender and `invoke --source <recipient> -- withdraw --id 0` as the recipient. Testnet closes a ledger about every 5 s (measured 31 ledgers in 155 s), so one policy window of 1 728 ledgers is about 2 h 25 min.

## 9. Troubleshooting

| Symptom | Fix |
| --- | --- |
| `error: target wasm32v1-none not found` | `rustup target add wasm32v1-none`; the workspace needs Rust ≥ 1.91 |
| `error: soroban-sdk requires stellar-cli v25.2.0+ to build a contract` | Use `stellar contract build --package <crate>` instead of `cargo build --target wasm32v1-none`; SDK 28 needs the CLI's spec shaking |
| `testutils` items missing | enable the feature: `soroban-sdk = { version = "...", features = ["testutils"] }` in `[dev-dependencies]` |
| OpenZeppelin crate version mismatch with `soroban-sdk` | both must target the same SDK major; check `Cargo.lock` and pin, do not bump mid-event |
| Testnet account not found | `stellar keys generate <name> --network testnet --fund` again, or use Friendbot: `https://friendbot.stellar.org/?addr=<G...>` |
| Deploy fails with insufficient balance | Uploading the 47 KB `DemoAccount` wasm cost about 7 300 XLM in Testnet resource fees; top up with `stellar keys fund breakwater-dev --network testnet` |
| Fuzz run is slow | reuse an `Env` snapshot between iterations (`Env::from_ledger_snapshot`) instead of a fresh `Env` per step |
| Bare `panic!` reported as a bug | that is intended; contracts must use `panic_with_error!` |
