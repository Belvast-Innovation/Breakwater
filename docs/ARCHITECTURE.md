# Architecture

This document describes the shape of Breakwater as designed at the start of HackMeridian. Where the implementation diverges during the event, update this file in the same PR.

## 1. Goals and non-goals

**Goals**

- Let a Soroban developer declare *actors*, *handlers* and *invariants* in plain Rust and get stateful fuzzing with shrinking, without writing harness code.
- Run entirely in-process against `soroban_sdk::Env` so a suite is just `cargo test`.
- Make findings reproducible: every failure is a deterministic, exportable unit test.
- Ship attack packs that encode Stellar-specific knowledge (smart-account policies, TTL archival).

**Non-goals (for the hackathon)**

- Coverage-guided fuzzing (`cargo-fuzz` / libFuzzer). Random generation plus shrinking is enough to demo; coverage guidance is a roadmap item.
- Formal verification. That is Certora Sunbeam's job.
- Mainnet execution. Testnet replay of a shrunk sequence is the on-chain evidence.

## 2. Core concepts

```
Suite
 ├── actors:     [Actor]            who may call (owner, agent, attacker, …)
 ├── handlers:   [Box<dyn Handler>] what may be called, with an argument strategy
 ├── invariants: [Box<dyn Invariant>] what must hold after every step
 ├── clock:      LedgerAdvance      how ledger seq / timestamp move between steps
 └── config:     seed, step range, max shrink iterations
```

### `Actor`

A named signer with a Soroban `Address`. For the `policy-bypass` pack, actors map onto smart-account signers (owner key, agent session key, revoked key) so that the authorization path is exercised with the right credential.

### `Handler`

```rust
pub trait Handler {
    type Args: Arbitrary + Debug;
    fn name(&self) -> &'static str;
    fn actors(&self) -> &[Actor];
    fn strategy(&self) -> BoxedStrategy<Self::Args>;        // proptest
    fn call(&self, env: &Env, actor: &Actor, args: &Self::Args) -> Result<CallOutcome, HandlerError>;
}
```

A handler wraps one contract function. `call` is expected to succeed or fail *gracefully* (a `contracterror`); a bare panic is reported as a bug, matching the convention in `soroban-token-fuzzer`.

### `Invariant`

```rust
pub trait Invariant {
    fn name(&self) -> &'static str;
    fn check(&self, env: &Env, log: &TxLog) -> Result<(), Violation>;
}
```

Invariants read contract state through the normal client and the `TxLog` (the sequence of steps executed so far, with outcomes). They are checked after **every** step, not only at the end, so the shrunk reproduction stops at the first violating step.

### `LedgerAdvance`

Controls how `env.ledger()` moves between steps:

- `Fixed { ledgers, seconds }`
- `Random { ledgers: Range, seconds: Range }`
- `Profile(Vec<(usize, Gap)>)` — a usage profile for the `ttl-archival` pack: e.g. "daily use for 30 days, then nothing for 90 days"

## 3. Execution pipeline

```
         ┌──────────┐
 seed ──▶│ proptest │──▶ Vec<Step{handler, actor, args}>
         │  state   │
         │ machine  │
         └────┬─────┘
              │ apply
              ▼
     ┌────────────────┐   per step:
     │ Runner         │   1. env.ledger().set(advance)
     │ (Env + TxLog)  │   2. handler.call(env, actor, args)   ← real auth for policy pack
     │                │   3. for inv in invariants: inv.check(env, log)?
     └────────┬───────┘
              │ on Violation
              ▼
     ┌────────────────┐
     │ Shrinker       │  proptest shrinks the Vec<Step>; Runner re-executes from a
     │                │  fresh / snapshot Env each candidate until minimal
     └────────┬───────┘
              ▼
     ┌────────────────┐
     │ Exporter       │  writes tests/repro_<suite>_<seed>.rs: a #[test] that rebuilds
     │                │  the Env, applies the steps, asserts the violation
     └────────────────┘
```

**Why `proptest-state-machine`.** It already provides sequence generation, preconditions (so we do not generate obviously invalid steps) and shrinking of sequences. Writing a custom shrinker in two days is not realistic; reusing this is the single biggest time saver in the plan.

**Env reuse.** Creating a fresh `Env` per shrink candidate is slow. The runner snapshots the post-setup `Env` once (`Env::to_ledger_snapshot`) and restores from it for each candidate.

**Determinism.** Seed → sequence is reproducible. The CLI prints the seed on every failure and accepts `--seed` to replay.

## 4. Authorization model

Two modes, chosen per suite:

| Mode | How | Used by |
| --- | --- | --- |
| `MockAll` | `env.mock_all_auths()` | general handlers where auth is not what is under test |
| `Real` | Build `AuthorizedInvocation` trees and let the host invoke the custom account's `__check_auth` | `policy-bypass` pack |

In `Real` mode the smart account's policies are genuinely enforced by the Soroban host, so a sequence that gets through is a real bypass, not a mocking artifact. The pack also uses `try_invoke_contract_check_auth` in unit tests to exercise `__check_auth` directly.

## 5. Attack packs

### `policy-bypass`

Target: an OpenZeppelin `stellar-contracts` smart account with an agent signer plus a spending-limit policy and a context rule.

| Invariant | What it asserts |
| --- | --- |
| `RollingCap` | `Σ spend in window ≤ limit`, computed from `TxLog`, compared to what the policy let through |
| `AllowlistOnly` | every successful call hit a contract/function in the configured allow-list |
| `RevokedNeverAuths` | no call succeeded with a signer revoked earlier in the sequence |
| `PinnedWasm` | policy/verifier WASM hashes at check time equal those at setup |

Generators bias toward known bypass shapes: many small payments just under per-tx caps, calls straddling window boundaries, interleaved signer rotation.

### `ttl-archival`

Target: a payment-stream contract that writes streams to persistent storage and config to instance storage but never calls `extend_ttl`.

| Invariant | What it asserts |
| --- | --- |
| `NoMidLifecycleExpiry` | for every live stream, `env.storage().persistent().get_ttl(key) > 0` while `stream.end > now` |
| `InstanceAlive` | the contract instance TTL has not expired while any stream is live |
| `RestoreRestoresFunction` | after simulating archive + restore, `withdraw` / `cancel` still succeed |

The `LedgerAdvance::Profile` clock is what makes this pack bite: a 30-day active period followed by a long gap pushes entries past their TTL while the stream is logically still running.

## 6. CLI

```
breakwater run <suite-dir> [--seed N] [--steps A..=B] [--auth mock|real] [--json]
breakwater export <suite-dir> --out <file.rs> [--seed N]
breakwater report <json>            # human-readable summary of a run
```

Exit codes: `0` all invariants held, `1` violation found (reproduction exported), `2` usage/config error. The `--json` output is stable so it can be consumed in CI.

## 7. Stellar features exercised

| Feature | Where |
| --- | --- |
| `require_auth` / `__check_auth` / `CustomAccountInterface` | `Real` auth mode |
| State archival: `extend_ttl`, `get_ttl`, instance vs persistent durability | `ttl-archival` pack |
| Ledger info manipulation via `testutils::Ledger` | `LedgerAdvance` |
| `Env::budget()` | optional per-step CPU/mem capture for reports |
| Stellar CLI deploy + invoke on Testnet | demo replay |

## 8. Open questions to resolve during the event

**Decided**

- How the CLI drives a suite: `breakwater run <suite-dir>` executes `cargo test --test suite` in the target directory, passes the seed and step range through environment variables, and parses the JSON the suite prints. No separate `suite` binary per example. Implementation lands in the `feat(cli)` PR.
- `soroban-fork` is not a dependency for the hackathon build: it currently supports `soroban-sdk` 25.x only and the workspace is pinned to 28.0.0. Mainnet-state fuzzing stays a roadmap item.

**Open**

- Whether `proptest-state-machine` preconditions are expressive enough for the policy generators, or whether a custom `Strategy` is needed.
- Cost of `Real` auth per step; if too slow, run `MockAll` for generation and re-verify only the shrunk sequence under `Real`.
- Whether to export reproductions as `#[test]` functions or as a JSON step list plus a generic replay test (JSON is simpler to generate; Rust is nicer to read in a PR).
