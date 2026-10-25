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
reproduce with --seed 7; report: <temp dir>/breakwater-policy-bypass-7.json
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
reproduce with --seed 11; report: <temp dir>/breakwater-ttl-archival-11.json
```

The report is written to `std::env::temp_dir()` (`$TMPDIR` or `/tmp` on Unix) as `breakwater-<suite>-<seed>.json` unless `--json <file>` names a path.

Exit codes: `0` all invariants held, `1` a violation was found, `2` usage or configuration error. Colours are off when stdout is not a terminal, when `NO_COLOR` is set, or with `--no-color`.

## Writing a test

The suite below is `crates/breakwater/tests/counter.rs`, compiled and run by `cargo test`: a counter contract, one handler, one invariant. The framework finds the eleventh increment and shrinks the sequence to eleven steps.

```rust
use breakwater::proptest::prelude::*;
use breakwater::{Actor, CallContext, CallOutcome, Handler, HandlerError, Invariant, Suite, TxLog, Violation};
use soroban_sdk::{contract, contractimpl, symbol_short, Address, Env, Symbol};

const COUNT: Symbol = symbol_short!("COUNT");
const LIMIT: u32 = 10;

#[contract]
pub struct Counter;

#[contractimpl]
impl Counter {
    pub fn increment(env: Env) -> u32 {
        let n: u32 = env.storage().instance().get(&COUNT).unwrap_or(0) + 1;
        env.storage().instance().set(&COUNT, &n);
        n
    }
    pub fn get(env: Env) -> u32 {
        env.storage().instance().get(&COUNT).unwrap_or(0)
    }
}

struct Increment { contract: String, actors: Vec<Actor> }   // addresses are kept as strkeys

impl Handler for Increment {
    type Args = ();
    fn name(&self) -> &'static str { "increment" }
    fn actors(&self) -> &[Actor] { &self.actors }
    fn strategy(&self) -> BoxedStrategy<()> { Just(()).boxed() }
    fn call(&self, env: &Env, _ctx: &CallContext, _actor: &Actor, _args: &()) -> Result<CallOutcome, HandlerError> {
        let id = Address::from_str(env, &self.contract);
        match CounterClient::new(env, &id).try_increment() {
            Ok(Ok(count)) => Ok(CallOutcome::returning(count)),
            Ok(Err(e)) => Err(HandlerError::Other(format!("conversion error: {e:?}"))),
            Err(Ok(e)) => Ok(CallOutcome::ContractError(e.get_code())),
            Err(Err(e)) => Err(HandlerError::Host(format!("{e:?}"))),
        }
    }
}

struct NeverAbove { contract: String }

impl Invariant for NeverAbove {
    fn name(&self) -> &'static str { "count_never_above_limit" }
    fn check(&self, env: &Env, _log: &TxLog) -> Result<(), Violation> {
        let id = Address::from_str(env, &self.contract);
        let count = CounterClient::new(env, &id).get();
        if count > LIMIT {
            return Err(Violation::new("count exceeded the limit").with_detail(count));
        }
        Ok(())
    }
}

#[test]
fn finds_the_eleventh_increment_and_shrinks_to_eleven_steps() {
    let env = Env::default();
    let contract = env.register(Counter, ()).to_string().to_string();
    let owner = Actor::generate(&env, "owner");
    let restore_id = contract.clone();

    let report = Suite::new("counter", env)
        .seed(42)
        .steps(1..=30)
        .cases(32)
        .handler(Increment { contract: contract.clone(), actors: vec![owner] })
        .invariant(NeverAbove { contract })
        // Native test contracts live in the host, not the ledger: re-register them
        // on every Env the suite restores from its snapshot.
        .on_restore(move |env| { env.register_at(&Address::from_str(env, &restore_id), Counter, ()); })
        .run()
        .expect("suite runs");

    let failure = report.failure().expect("the limit must be exceeded");
    assert_eq!(failure.violation.source, "count_never_above_limit");
    assert_eq!(failure.log.len(), LIMIT as usize + 1);   // shrunk to exactly eleven steps
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
- `NoUnboundedTtlExtension`: no entry is kept alive beyond what its stream needs; catches a public, unauthenticated `extend_ttl` entry point used to park data at the contract's rent
- `RestoreCostWithinBudget`: a call made while entries are archived stays within a resource budget (instructions, memory, disk read and write entries and bytes, from the host's metering), so restoration cannot push a transaction over the network limits

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

Replaying a shrunk failing sequence on Testnet is part of the demo (`scripts/replay-testnet.sh`, see [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md#8-replaying-on-testnet)). Local fuzzing stays local; only the exported reproduction goes on-chain. Deployed and replayed on Testnet on 25 October 2026:

| Item | Value |
| --- | --- |
| Demo smart-account contract (policy-bypass) | [`CBKDNENFLCLUE3U4NH55T5N5JIX6S6WS2FFDXCPWMXJM7ZQGPXVH3HJI`](https://stellar.expert/explorer/testnet/contract/CBKDNENFLCLUE3U4NH55T5N5JIX6S6WS2FFDXCPWMXJM7ZQGPXVH3HJI) |
| Spending-limit policies (50 USDC per ledger, 100 USDC per 1 728 ledgers) | [`CCIEVOGO…CS247`](https://stellar.expert/explorer/testnet/contract/CCIEVOGO6RZSCHN6COE3NLMDT3WOXSPTDPYDJMPVSULFREACAJ6CS247), [`CC6Z5UZY…RRD7`](https://stellar.expert/explorer/testnet/contract/CC6Z5UZYWG5CR43YYGUZCMOGQHO2NEQZIU7M4MBR2NG3ELZQN7G2RRD7) |
| Ed25519 verifier, owner key account | [`CA7UFPQ3…5OJN`](https://stellar.expert/explorer/testnet/contract/CA7UFPQ3MHRQGIOHG6B5TPBJZLZL6HCNCYNWPILVDDC6AHOMM5BJ5OJN), [`CC2N5OKR…PSCB`](https://stellar.expert/explorer/testnet/contract/CC2N5OKRJ5MA2SEVIUSRSMPRODN73Z2OH6CYZ7YHIZVAVD72N6K4PSCB) |
| Test asset `USDC` (Stellar Asset Contract) | [`CDDSS44L…PDMM`](https://stellar.expert/explorer/testnet/contract/CDDSS44LN2N4TNVTDH42PLEVHKDB5OZQRBHAD2KQRH6LIADRIBUMPDMM) |
| Replayed bypass: payment 1 (49 USDC, ledger 5383703) | [`b5ea4498…6772`](https://stellar.expert/explorer/testnet/tx/b5ea44983473462379a6909abc335105a040f6d4fee83908cf97dee49cec6772) |
| Replayed bypass: payment 2 (49 USDC, ledger 5383715) | [`82be1eb8…5608`](https://stellar.expert/explorer/testnet/tx/82be1eb8d373bf91aecc9ca4743041ea1f50d1961f7b5fb7ba7a3f9add1e5608) |
| Replayed bypass: payment 3 (49 USDC, one window after payment 1) | pending until ledger 5385433 |
| Demo payment-stream contract (ttl-archival) | [`CBS6AO2L5RKKLHIC2FJOH66UIL2OHCPSSO6GAUCYNXMMI3RRQLG3D5KP`](https://stellar.expert/explorer/testnet/contract/CBS6AO2L5RKKLHIC2FJOH66UIL2OHCPSSO6GAUCYNXMMI3RRQLG3D5KP) |
| `create_stream` (1 000 USDC over 365 days, ledger 5383730) | [`aa8cb83f…5f99`](https://stellar.expert/explorer/testnet/tx/aa8cb83fa989c411bbe37e83eb97893144891018ce726b4f653e462076eb5f99) |
| `withdraw` by the recipient six ledgers later (ledger 5383736) | [`43ce190b…14d2`](https://stellar.expert/explorer/testnet/tx/43ce190be2d0fd49f88584c774d6c139bdf497ff7ee636954f6ef452c47714d2) |

Each payment above went through the smart account's real `__check_auth`: the agent's Ed25519 key signed the OpenZeppelin `AuthPayload`, and the two `SpendingLimit` policies ran on-chain and let 49 + 49 USDC through.

The stream's expiry itself cannot be waited for on Testnet: the entry lives for the network's minimum persistent TTL, which is weeks. What the on-chain evidence shows is the stream being created and used; what the local suite shows is the entry being archived with most of the stream still to run. On a real network the consequence is not a failing call but a cost: every later transaction that touches the stream must carry a restore footprint and pay restoration fees, and a transaction built without one fails.

## Project layout

```
breakwater/
├── crates/
│   ├── breakwater/                  # core library: Suite, Handler, Invariant, replay, JSON export
│   ├── breakwater-cli/              # `breakwater run | export | report`
│   └── packs/
│       ├── policy-bypass/           # invariants, generators and real-auth entries for OZ smart accounts
│       └── ttl-archival/            # TTL probes, stream model and archival invariants
├── examples/
│   ├── policy-bypass/               # DemoAccount: an OZ smart account with a mis-sized spending window
│   │   ├── contracts/
│   │   │   ├── owner-account/       # the owner's Ed25519 key account (Signer::Delegated)
│   │   │   ├── spending-limit-policy/
│   │   │   └── ed25519-verifier/
│   │   └── tests/                   # suite.rs, common/ (fixture, handlers), repro_policy_bypass_7.rs
│   └── ttl-archival/                # PaymentStream (never extends TTL) and PaymentStreamFixed
│       └── tests/                   # suite.rs, common/, repro_ttl_archival_11.rs
├── scripts/
│   └── replay-testnet.sh            # replays an exported sequence on Testnet
├── docs/
│   ├── ARCHITECTURE.md
│   ├── DEVELOPMENT.md
│   ├── SUBMISSION.md
│   └── reports/                     # HTML reports rendered by `breakwater report --html`
├── CLAUDE.md                        # working agreement for AI-assisted development
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
| CLI, HTML report and Testnet replay scripts | First drafts of the `run` / `export` / `report` commands, the HTML renderer and `scripts/replay-testnet.sh` | Pipeline design (how the CLI drives a suite, how an agent-signed authorization entry is spliced into a transaction), on-chain verification |
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
