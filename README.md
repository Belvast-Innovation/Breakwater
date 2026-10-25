# Breakwater

**Foundry-style invariant testing for Soroban.**

Breakwater is a Rust framework and CLI for stateful property testing of Stellar smart contracts. You declare *actors* (who can call), *handlers* (what they can call) and *invariants* (what must always hold). Breakwater generates randomized multi-step transaction sequences against a local Soroban `Env`, checks every invariant after every step, shrinks any failing sequence to its minimal form, and exports it as a reproducible Rust unit test.

It ships with two **attack packs** for risks that are specific to Stellar:

| Attack pack | Risk it hunts | Why it matters |
| --- | --- | --- |
| `policy-bypass` | Multi-step sequences that slip past OpenZeppelin smart-account policies (spending limits, context rules, signer scopes) | These policies are the safety layer for AI-agent wallets paying over x402 / MPP. A policy that holds for one transaction may fail for five. |
| `ttl-archival` | Contract state that expires under Soroban's TTL-based state archival while the business logic still depends on it | A one-year payment stream whose storage entry is archived at month six is a correctness bug no EVM tool knows to look for. |

> **Status:** HackMeridian 2026 submission (Genesis path · Stellar Unlocked track). Built 25–26 October 2026 in Lisbon. See [Prior work and hackathon scope](#prior-work-and-hackathon-scope).

---

## Table of contents

- [Problem](#problem)
- [How it works](#how-it-works)
- [Quick start](#quick-start)
- [Writing a test](#writing-a-test)
- [Attack packs](#attack-packs)
- [How Stellar is used](#how-stellar-is-used)
- [Testnet evidence](#testnet-evidence)
- [Project layout](#project-layout)
- [Prior work and hackathon scope](#prior-work-and-hackathon-scope)
- [AI-assisted development](#ai-assisted-development)
- [Roadmap](#roadmap)
- [Team](#team)
- [License](#license)

---

## Problem

Soroban already has good tools for three layers of contract security:

- **Static analysis** — Scout (CoinFabrik)
- **AI review** — Almanax
- **Formal verification** — Certora Sunbeam

What is missing is the layer in between: **stateful fuzzing**. The official `soroban-examples` repository shows how to wire up `cargo-fuzz` and `proptest` by hand, and `soroban-token-fuzzer` covers the standard token interface, but there is no reusable framework where a developer can declare invariants and let a tool search for the transaction sequence that breaks them. Every team that wants this rebuilds it from scratch.

Two Stellar-specific developments make the gap urgent:

1. **Agentic payments.** x402 and MPP are live on Stellar mainnet. AI agents hold smart accounts whose on-chain policies are the only thing standing between a prompt injection and a drained wallet. Nobody has a tool that attacks those policies with sequences rather than single calls.
2. **State archival.** Soroban storage entries have a time-to-live. Contracts that forget to extend it, or extend it on the wrong schedule, fail silently months after deployment. The standard advice is "test it by bumping the ledger sequence manually".

Breakwater is the reusable layer for both.

## How it works

```
┌─────────────┐    ┌──────────────┐    ┌──────────────┐    ┌───────────────┐
│  Declare    │ -> │  Generate    │ -> │  Check       │ -> │  Shrink +     │
│  actors,    │    │  random tx   │    │  invariants  │    │  export as    │
│  handlers,  │    │  sequences   │    │  after every │    │  unit test    │
│  invariants │    │  (proptest)  │    │  step        │    │               │
└─────────────┘    └──────────────┘    └──────────────┘    └───────────────┘
```

1. **Declare.** Implement the `Handler` trait for each contract call the fuzzer may make, give each one an `Actor`, and implement `Invariant` for each property that must hold.
2. **Generate.** Breakwater drives a Soroban `Env` (with `testutils`) using `proptest-state-machine`. Each step picks a handler, an actor and arguments; ledger sequence and timestamp advance between steps so TTL and time-based logic are exercised.
3. **Check.** After every step every invariant is evaluated. Authorization runs through the real `__check_auth` path for the `policy-bypass` pack so that smart-account policies are actually enforced, not mocked.
4. **Shrink and export.** A failing sequence is shrunk to the shortest reproduction and written out as a self-contained `#[test]` you can drop into your crate.

A full architecture description is in [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md).

## Quick start

Prerequisites are listed in [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md). In short: Rust stable with the `wasm32v1-none` target, the Stellar CLI, and a working `cargo test` on a Soroban contract.

```bash
git clone https://github.com/Belvast-Innovation/Breakwater
cd breakwater
cargo build --workspace

# Run the bundled demos
cargo run -p breakwater-cli -- run examples/policy-bypass
cargo run -p breakwater-cli -- run examples/ttl-archival

# Export the shrunk failure as a unit test
cargo run -p breakwater-cli -- export examples/policy-bypass --out tests/repro_policy_bypass.rs
```

Expected output for the policy demo is a shrunk sequence of the form *agent pays 49 → agent pays 49 → agent pays 49 → cumulative spend exceeds the 100 USDC daily limit* plus a generated test that replays it.

## Writing a test

```rust
use breakwater::{Actor, Handler, Invariant, Suite};

struct Pay;
impl Handler for Pay {
    fn actors(&self) -> &[Actor] { &[Actor::Agent] }
    fn call(&self, env: &Env, actor: &Actor, args: &PayArgs) -> Result<(), Error> {
        wallet::Client::new(env, &self.wallet).pay(actor.address(), &args.to, &args.amount)
    }
}

struct DailyCap { limit: i128 }
impl Invariant for DailyCap {
    fn check(&self, env: &Env, log: &TxLog) -> Result<(), Violation> {
        let spent = log.spent_in_window(env.ledger().timestamp(), 86_400);
        if spent > self.limit { return Err(Violation::new("daily cap exceeded", spent)); }
        Ok(())
    }
}

#[test]
fn policy_holds_under_fuzz() {
    Suite::new()
        .handler(Pay)
        .invariant(DailyCap { limit: 100_0000000 })
        .steps(1..=20)
        .run();
}
```

The exact API is documented in [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) and will settle during the event; the README is updated at submission time.

## Attack packs

### `policy-bypass`

Targets OpenZeppelin `stellar-contracts` smart accounts. Built-in invariants:

- cumulative spend within a rolling window never exceeds the configured limit
- only allow-listed contracts and functions are ever invoked
- a revoked or expired signer never authorizes a call
- the policy / verifier WASM hash pinned at setup is the one that actually ran

Transactions are executed through the real authorization path, so a finding is a real bypass, not an artifact of mocked auth.

### `ttl-archival`

Advances the ledger according to a simulated usage profile and asserts:

- no storage key the business logic still depends on reaches TTL expiry mid-lifecycle
- after an archived entry is restored, the contract's public functions still succeed
- instance and persistent entries are extended on the schedule the contract claims

## How Stellar is used

Breakwater is infrastructure, so "use of Stellar" means use of the protocol itself rather than a consumer flow:

| Stellar feature | Where Breakwater uses it |
| --- | --- |
| Soroban authorization framework (`require_auth`, `__check_auth`, `CustomAccountInterface`) | `policy-bypass` executes through real custom-account auth |
| Soroban state archival (TTL, `extend_ttl`, restore) | `ttl-archival` manipulates ledger sequence and TTL via `testutils` |
| Resource metering (`Env::budget()`) | Optional per-step CPU / memory snapshot for regression reports |
| OpenZeppelin smart-account contracts (ecosystem) | Demo target for the policy pack |
| Stellar CLI + Testnet | Demo contracts are deployed to Testnet and the shrunk reproductions are replayed on-chain |

## Testnet evidence

Replaying a shrunk failing sequence on Testnet is part of the demo. Contract IDs and transaction hashes are filled in at submission time:

| Item | Value |
| --- | --- |
| Demo smart-account contract (policy-bypass) | `C...` |
| Demo payment-stream contract (ttl-archival) | `C...` |
| Replayed bypass sequence | `https://stellar.expert/explorer/testnet/tx/...` |

## Project layout

```
breakwater/
├── crates/
│   ├── breakwater/          # core library: Suite, Handler, Invariant, shrinking, export
│   ├── breakwater-cli/      # `breakwater run | export | report`
│   └── packs/
│       ├── policy-bypass/   # invariants + generators for OZ smart accounts
│       └── ttl-archival/    # invariants + ledger-advance strategies for TTL
├── examples/
│   ├── policy-bypass/       # deliberately misconfigured smart account + suite
│   └── ttl-archival/        # payment-stream contract that never extends TTL
├── docs/
│   ├── ARCHITECTURE.md
│   ├── DEVELOPMENT.md
│   └── SUBMISSION.md
├── CLAUDE.md                # working agreement for AI-assisted development
└── README.md
```

## Prior work and hackathon scope

HackMeridian requires that prior work is disclosed and that work done during the event is distinguishable. Breakwater is a new project; this section states precisely what existed before 09:00 WET on 25 October 2026.

**Before the event (research and design only, no code):**

- Survey of existing Soroban security tooling (Scout, Almanax, Sunbeam, `soroban-token-fuzzer`, `soroban-fork`, `erst`, `soroban-upgrades-core`) and of adjacent projects (Policywright, Soneso `stellar-agent-wallet`, Agent Card, PennyAgent) to confirm the gap this project fills.
- Reading of the soroban-sdk `testutils` API, OpenZeppelin `stellar-contracts` smart-account docs, and the Stellar state-archival docs.
- A written plan (module split, demo targets, pitch outline).

**During the event (everything in this repository):**

- All source code, tests, examples, documentation and CI configuration. The first commit on `main` is timestamped after the event opened; the commit history is the record.

No code from any earlier project, hackathon or regional activation was reused.

**Third-party code** is pulled in as Cargo dependencies and is not vendored: `soroban-sdk`, `proptest` / `proptest-state-machine`, OpenZeppelin `stellar-contracts`, and the crates listed in `Cargo.lock`. Each keeps its own license.

## AI-assisted development

This project was built with **Claude Code** (Anthropic) as a pair programmer, as permitted and encouraged by the HackMeridian rules. The working agreement is in [`CLAUDE.md`](CLAUDE.md).

Where AI meaningfully generated code or content:

| Area | AI contribution | Human contribution |
| --- | --- | --- |
| Core library (`crates/breakwater`) | Boilerplate for trait definitions, `proptest-state-machine` wiring, error types | Design of the `Handler` / `Invariant` / `Suite` API, shrinking strategy, review of every file |
| Attack packs | First drafts of invariant implementations from written specs | Threat modelling, choice of invariants, verification against real contracts |
| Demo contracts | Scaffolding from OpenZeppelin examples | Deliberate misconfigurations, TTL bug |
| CLI and report output | Argument parsing, report formatting | UX decisions |
| Documentation | Drafts of README, `docs/`, CI config | All claims checked against the code and Stellar docs |

Stellar documentation was pulled into the AI context through the official **Raven MCP** server and the **Stellar Dev Skill** plugin. All AI-generated code was read, tested and, where needed, rewritten by the team before being committed.

## Roadmap

- Attack pack for resource-budget regression (`forge snapshot`-style CI gate)
- `soroban-fork` integration to fuzz against live mainnet state (blocked until `soroban-fork` supports `soroban-sdk` 28; it currently targets 25.x)
- Macro-based suite declaration (`#[breakwater::suite]`)
- Publish to crates.io; apply to the Stellar Community Fund (Build / Public Goods)

## Team

**Belvast Innovation** — Hannes Gao ([belvast.de](https://belvast.de)) · Karlsruhe, Germany

## License

Apache-2.0. See [`LICENSE`](LICENSE).
