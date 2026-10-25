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
cd Breakwater
cargo build --workspace

# Run the bundled demos (each drives `cargo test --test suite` in the example crate)
cargo run -p breakwater-cli -- run examples/policy-bypass --seed 7
cargo run -p breakwater-cli -- run examples/ttl-archival --seed 11

# Export the shrunk failure as a regression test and render a report
cargo run -p breakwater-cli -- export examples/policy-bypass --seed 7 --out examples/policy-bypass/tests/repro_policy_bypass_7.rs
cargo run -p breakwater-cli -- run examples/policy-bypass --seed 7 --json run.json
cargo run -p breakwater-cli -- report run.json --html report.html
```

Real output of the policy demo:

```
suite: policy-bypass
seed: 7   auth: suite default   steps: suite default
────────────────────────────────────────────────────────────────────────
violation found after 58 cases and 185 steps, shrunk to 3 step(s):
  #0 transfer       agent      +1 ledgers               amount_usdc=49 -> Success
  #1 transfer       agent      +10 ledgers              amount_usdc=49 -> Success
  #2 transfer       agent      +1718 ledgers            amount_usdc=49 -> Success
violation: agent spent 147 within 17280 ledgers, limit is 100 (rolling_cap)
reproduce with --seed 7; report: /tmp/breakwater-policy-bypass-7.json
```

Three payments of 49 USDC, the third landing exactly one policy window (1 728 ledgers) after the first: every single call respected the 50 USDC per-call cap and the "daily" cap, yet the agent spent 147 USDC in a day. Every step ran through the host's real authorization path, so the OpenZeppelin smart account's `__check_auth` and its `SpendingLimit` policies genuinely let it through.

Real output of the archival demo:

```
suite: ttl-archival
seed: 11   auth: suite default   steps: suite default
────────────────────────────────────────────────────────────────────────
violation found after 63 cases and 621 steps, shrunk to 2 step(s):
  #0 create_stream  sender     +17280 ledgers (1.0 d)   amount_usdc=1000 duration_ledgers=6307200 -> Success
  #1 create_stream  sender     +1555200 ledgers (90.0 d) amount_usdc=1000 duration_ledgers=6307200 -> Success
violation: stream 0 is archived with 275 days left to run (TTL ran out 60.0 days ago) (no_mid_lifecycle_expiry)
reproduce with --seed 11; report: /tmp/breakwater-ttl-archival-11.json
```

Exit codes: `0` all invariants held, `1` a violation was found, `2` usage or configuration error. Colours are off when stdout is not a terminal, when `NO_COLOR` is set, or with `--no-color`.

## Writing a test

```rust
use breakwater::proptest::prelude::*;
use breakwater::{Actor, CallContext, CallOutcome, Handler, HandlerError, Invariant, Suite, TxLog, Violation};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct PayArgs { amount_usdc: i64 }

struct Pay { wallet: String, actors: Vec<Actor> }   // addresses are kept as strkeys

impl Handler for Pay {
    type Args = PayArgs;
    fn name(&self) -> &'static str { "pay" }
    fn actors(&self) -> &[Actor] { &self.actors }
    fn strategy(&self) -> BoxedStrategy<PayArgs> { (1i64..=50).prop_map(|amount_usdc| PayArgs { amount_usdc }).boxed() }
    fn call(&self, env: &Env, _ctx: &CallContext, actor: &Actor, args: &PayArgs) -> Result<CallOutcome, HandlerError> {
        let wallet = Address::from_str(env, &self.wallet);
        match WalletClient::new(env, &wallet).try_pay(&actor.address(env), &args.amount_usdc) {
            Ok(Ok(())) => Ok(CallOutcome::Success),
            Err(Ok(e)) => Ok(CallOutcome::ContractError(e.get_code())),
            other => Err(HandlerError::Host(format!("{other:?}"))),
        }
    }
}

struct DailyCap { limit: i64 }
impl Invariant for DailyCap {
    fn name(&self) -> &'static str { "daily_cap" }
    fn check(&self, _env: &Env, log: &TxLog) -> Result<(), Violation> {
        let spent: i64 = log.successes_of("pay").map(|s| s.args["amount_usdc"].as_i64().unwrap_or(0)).sum();
        if spent > self.limit { return Err(Violation::new("daily cap exceeded").with_detail(spent)); }
        Ok(())
    }
}

#[test]
fn breakwater_suite() {
    let report = Suite::new("wallet", env)            // `env` holds the deployed contracts
        .seed(42).steps(1..=20)
        .handler(Pay { wallet, actors: vec![agent] })
        .invariant(DailyCap { limit: 100 })
        .on_restore(move |env| { env.register_at(&Address::from_str(env, &id), Wallet, ()); })
        .run()
        .unwrap();
    breakwater::Exporter::write_if_requested(&report).unwrap();   // hands the result to the CLI
    assert!(report.failure().is_none());
}
```

A suite is an ordinary `cargo test`; the `breakwater` CLI drives the test named `breakwater_suite` in `tests/suite.rs`, passing seed, step range and auth mode through `BREAKWATER_SEED`, `BREAKWATER_STEPS` and `BREAKWATER_AUTH`, and reads the JSON report the suite writes to `BREAKWATER_REPORT`. The full API, including real-authorization mode and replay, is in [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md); the two example suites under `examples/*/tests/` are the reference implementations.

## Attack packs

### `policy-bypass`

Targets OpenZeppelin `stellar-contracts` 0.8.0 smart accounts. Built-in invariants:

- `RollingCap`: cumulative spend of an actor within a rolling ledger window never exceeds the limit the operator meant
- `AllowlistOnly`: an actor only ever succeeds with allow-listed calls
- `RevokedNeverAuths`: once revoked, a signer never authorizes a call again
- `PinnedWasm`: the policy and verifier executables pinned at setup are the ones in place at every step

Generators bias toward bypass shapes (amounts just under a cap, ledger gaps that straddle a policy window). Every step runs through the host's real authorization path (`AuthMode::Real`): the pack builds the `SorobanAuthorizationEntry` values an OpenZeppelin account expects, including the nested entry for a delegated signer, so a finding is a real bypass, not an artifact of mocked auth.

The bundled demo (`examples/policy-bypass`) is an unmodified OpenZeppelin smart account whose agent rule carries two `SpendingLimit` policies: 50 USDC per ledger and 100 USDC per "day", where the day was configured as 1 728 ledgers instead of 17 280. `SpendingLimit` counts its window in ledgers, and dividing 86 400 seconds by 50 instead of by 5 is a one-character mistake. The suite finds the three-payment bypass shown above and shrinks it to the window boundary.

### `ttl-archival`

Advances the ledger according to a usage profile (`LedgerAdvance::daily_then_silence(30, 90)`: one call a day for a month, then a quiet quarter) and asserts:

- `NoMidLifecycleExpiry`: no persistent entry a running stream depends on runs out of TTL
- `InstanceAlive`: the contract instance stays alive while any stream is running
- `RestoreRestoresFunction`: after an archived entry is restored, `withdraw` and `cancel` still work (checked on a throwaway copy of the ledger, so the probe never disturbs the run)

What expiry means on Soroban since Protocol 23: an archived persistent or instance entry is not lost, and a call that touches it does not simply fail. Every later transaction must carry a restore footprint (simulation adds it) and pay restoration fees, and a transaction built without one fails. The pack therefore reads each entry's `live_until` directly from the ledger entries, because any contract read, and the SDK's own `get_ttl` test helper, would restore the entry and hide the finding.

The bundled demo (`examples/ttl-archival`) is a payment stream that keeps streams in persistent storage and never calls `extend_ttl`. Under the profile above, a one-year stream is archived after the quiet quarter with 275 days left to run; `PaymentStreamFixed`, which extends every touched entry to the stream's remaining duration, survives the same profile.

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

**Near term**

- Rule-rotation probe for `policy-bypass`: generate `remove_context_rule` + `add_context_rule` under real auth, since a re-created rule starts `SpendingLimit` with an empty history
- Malicious keep-alive: bias generators toward unbounded `extend_ttl` calls on every public entry point, building on `NoUnboundedTtlExtension`
- Restoration-cost budget as a CI gate: per-step resource ceilings from `RestoreCostWithinBudget`, compared run over run
- Export of `stellar-cli` replay scripts and unsigned transaction XDR next to the Rust reproduction

**Later**

- Coverage-guided mutation of sequences instead of random generation
- Declarative construction of authorization trees for custom accounts
- Shadow-state assertions: a reference model of the contract checked against the ledger after every step
- Mainnet fork via `soroban-fork` once it supports `soroban-sdk` 28 (it currently targets 25.x)
- AI-assisted generation of handlers and invariants from a contract's interface
- GitHub Action packaging; publish to crates.io; apply to the Stellar Community Fund (Build / Public Goods)

## Team

**Belvast Innovation** — Hannes Gao ([belvast.de](https://belvast.de)) · Karlsruhe, Germany

## License

Apache-2.0. See [`LICENSE`](LICENSE).
