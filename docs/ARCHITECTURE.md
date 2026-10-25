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
 ├── env:        Env                post-setup environment; snapshotted for every run
 ├── handlers:   [Box<dyn Handler>] what may be called, by which actors, with an argument strategy
 ├── invariants: [Box<dyn Invariant>] what must hold after every step
 ├── advance:    LedgerAdvance      how ledger seq / timestamp move between steps
 ├── on_restore: [Fn(&Env)]         re-registers native test contracts on each restored Env
 └── config:     SuiteConfig        seed, step range, cases, auth mode, max shrink iterations
```

### `Actor`

A named signer with a Soroban address. Because an `Address` is bound to the `Env` that created it and every run uses a freshly restored `Env`, an `Actor` stores the address as a strkey string and materialises it with `actor.address(&env)`. Handlers and invariants hold contract ids the same way (`Address::from_str(&env, &strkey)`). For the `policy-bypass` pack, actors map onto smart-account signers (owner key, agent session key, revoked key) so that the authorization path is exercised with the right credential.

### `Handler`

```rust
pub trait Handler: Send + Sync + 'static {
    type Args: Clone + Debug + Serialize + Send + Sync + 'static;
    fn name(&self) -> &'static str;
    fn actors(&self) -> &[Actor];
    fn strategy(&self) -> BoxedStrategy<Self::Args>;        // proptest
    fn call(&self, env: &Env, actor: &Actor, args: &Self::Args) -> Result<CallOutcome, HandlerError>;
}

pub enum CallOutcome { Success, ContractError(u32) }      // both are expected behaviour
```

A handler wraps one contract function. `call` is expected to succeed or fail *gracefully* (a `contracterror`, reported as `CallOutcome::ContractError`). A `HandlerError` or a bare panic is treated as a bug and ends the sequence with a `Violation` of kind `handler_error` or `panic`, matching the convention in `soroban-token-fuzzer`. `Args` must be `Serialize` so that steps can be logged and exported; the suite type-erases them internally.

### `Invariant`

```rust
pub trait Invariant {
    fn name(&self) -> &'static str;
    fn check(&self, env: &Env, log: &TxLog) -> Result<(), Violation>;
}
```

Invariants read contract state through the normal client and the `TxLog` (the sequence of steps executed so far, with outcomes). They are checked after **every** step, not only at the end, so the shrunk reproduction stops at the first violating step. A `Violation` carries `kind` (`invariant`, `panic`, `handler_error`), `source` (the invariant or handler name, filled in by the runner), `message` and an optional JSON `detail`.

### `LedgerAdvance`

Controls how `env.ledger()` moves between steps. The gap is generated as part of each step, so it is deterministic for a seed and shrinks with the sequence:

- `Still`
- `Fixed { ledgers, seconds }` (default: one ledger, five seconds)
- `Random { ledgers: RangeInclusive<u32>, seconds: RangeInclusive<u64> }`
- `Profile(Vec<(usize, Gap)>)` — a usage profile for the `ttl-archival` pack: e.g. "daily use for 30 days, then nothing for 90 days"; the last segment's gap repeats once the profile is exhausted

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

**Env reuse.** Setting up contracts from scratch per shrink candidate is slow and non-reusable. The suite keeps the post-setup `Env` and restores a fresh one for every generated sequence and every shrink candidate with `Env::from_ledger_snapshot(base.to_ledger_snapshot())`. Two facts about `soroban-sdk` 28 shape this:

- Ledger entries (instance, persistent and temporary storage, contract instances, ledger info) survive the round trip. Native Rust test contracts registered with `env.register(..)` do **not**: they live in the host, not in the ledger. The suite therefore runs the `on_restore` hooks on every restored `Env`, and the suite author re-registers native contracts there with `env.register_at(&address, Contract, ())`, which keeps the restored instance storage intact.
- `mock_all_auths()` and the budget are per-`Env`, so the runner re-applies them on every restore.

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
- Whether to export reproductions as `#[test]` functions or as a JSON step list plus a generic replay test. Decided in two stages: the core `Exporter` emits the JSON step list (`suite`, `seed`, `outcome.status`, `outcome.violation`, `outcome.failing_step`, `outcome.log.steps[]`); the CLI's `export` command renders a Rust `#[test]` from it.
