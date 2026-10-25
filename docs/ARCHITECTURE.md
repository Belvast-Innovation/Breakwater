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
- `Random { ledgers: RangeInclusive<u32> }`; the timestamp follows at `SECONDS_PER_LEDGER` (5 s) per ledger
- `Profile(Vec<(usize, Gap)>)` — a usage profile: `(steps, gap)` segments applied in order, the last gap repeating once the profile is exhausted. `LedgerAdvance::daily_then_silence(30, 90)` builds "one call a day for 30 days, then nothing for 90 days", the profile the `ttl-archival` pack runs under.

Because the gap is stored in each generated step, shrinking keeps the gaps of the steps it retains: a profile-driven failure shrinks to the few steps that carry the decisive gaps (for the stream demo: the creation and the step after the 90-day silence).

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
| `MockAll` | `env.mock_all_auths()` on every restored `Env` | general handlers where auth is not what is under test |
| `Real` | No mocking. Before each call the handler installs real `SorobanAuthorizationEntry` values with `env.set_auths(..)`; the host runs the custom account's `__check_auth` and its policies | `policy-bypass` pack |

In `Real` mode the smart account's policies are genuinely enforced by the Soroban host, so a sequence that gets through is a real bypass, not a mocking artifact. The pieces:

- `breakwater::auth` (core, protocol-level): `contract_fn` builds the invocation tree, `signature_payload` hashes the `HashIdPreimage::SorobanAuthorization` the host expects to be signed, `address_entry` wraps it all in `SorobanAddressCredentials`. `CallContext` gives each step a unique nonce and a signature expiration ledger.
- `breakwater_pack_policy_bypass::auth::authorize` (OpenZeppelin-specific): produces the account entry whose signature is an `AuthPayload { signers, context_rule_ids }`. An `External` signer signs the SHA-256 digest of `AuthDigestPreimage { account, signature_payload, context_rule_ids }`. A `Delegated` signer gets a second entry whose root invocation is `account.__check_auth(preimage)`, because the account authenticates it with `require_auth_for_args`.
- `classify` maps the result of a `try_` call: a `Contract` error is `CallOutcome::ContractError`; an `Auth` error or `Error(Context, InvalidAction)` (how the host surfaces a `__check_auth` rejection to the caller) is `CallOutcome::Unauthorized`; anything else is a bug.

Measured on the demo suite (Apple Silicon, debug build): 86 cases and 280 steps in 1.3 s, about 5 ms per step including two policy calls, an Ed25519 signature and the nested owner entry. Generation therefore runs under `Real` directly; the fallback of generating under `MockAll` and re-verifying only the shrunk sequence under `Real` was not needed.

## 5. Attack packs

### `policy-bypass`

Target: an OpenZeppelin `stellar-contracts` 0.8.0 smart account (`examples/policy-bypass`) with two context rules: rule 0 (`Default`) for the owner, a `Signer::Delegated` hardware-key account, and rule 1 (`CallContract(usdc)`) for the agent, a `Signer::External` Ed25519 key, carrying two `SpendingLimit` policy instances: 50 USDC per ledger (the de-facto per-call cap) and 100 USDC per "day".

The deliberate defect is in that last number. OpenZeppelin's `SpendingLimit` measures its window in **ledgers** (`period_ledgers`), and at Stellar's ~5 s close time a day is 17 280 ledgers. The demo installs 1 728, as an operator who divides 86 400 s by 50 instead of 5 would. Every single call still respects both caps; only a sequence that waits out the 2.4-hour window shows that "100 USDC per day" is really "100 USDC per 2.4 hours". Mistaking a day for a ledger count is exactly the kind of real-world configuration error the pack exists to catch.

| Invariant | What it asserts |
| --- | --- |
| `RollingCap` | `Σ spend in window ≤ limit`, computed from `TxLog`, compared to what the policy let through |
| `AllowlistOnly` | every successful call hit a contract/function in the configured allow-list |
| `RevokedNeverAuths` | no call succeeded with a signer revoked earlier in the sequence |
| `PinnedWasm` | policy/verifier WASM hashes at check time equal those at setup |

Generators bias toward known bypass shapes: `amount_near_cap` draws mostly `cap - 1`, and `window_straddling_advance` draws ledger gaps uniformly over one and a half policy windows so about a third of them cross the boundary. Signer rotation is represented by the owner's `revoke_agent` handler (`remove_signer`). Re-creating the agent's rule (`remove_context_rule` + `add_context_rule`) is not generated yet: under `Real` auth the policy `install` callbacks require the sub-invocation tree, including the not-yet-assigned rule id, to be pre-built in the owner's authorization entry. That is deferred.

Invariants evaluate the `TxLog`: `RollingCap` sums the agent's successful `transfer` amounts inside the intended window (17 280 ledgers) and fires when the sum exceeds 100 USDC, however the policy was configured. `PinnedWasm` compares `Address::executable()` of the account, policies and verifier against the values captured at setup.

### `ttl-archival`

Target: `examples/ttl-archival`, a payment stream (`create_stream`, `withdraw`, `cancel`) that keeps streams in persistent storage and the token address plus id counter in instance storage, and never calls `extend_ttl`. `PaymentStreamFixed` (behind the `fixed` feature) is the same contract extending every touched entry to the stream's remaining duration; it is the control.

Two facts about `soroban-sdk` 28 shape the pack:

- **Archived persistent and instance entries are restored on access.** Since Protocol 23 a transaction whose restore list names an archived entry restores it before running (simulation fills that list); the test `Env` emulates this, so a contract read of an archived entry succeeds and resets the entry's TTL to the network minimum. Only temporary entries vanish. The `testutils` `get_ttl` getters go through the same path and therefore cannot observe an archived entry.
- **`live_until` can be read without touching the entry.** `env.host().get_stored_entries()` lists every ledger entry with its `live_until`; `breakwater_pack_ttl_archival::ttl` filters it by contract, key and durability. That is the only side-effect-free probe, and it is what the invariants use.

Invariants read the set of running streams from the `TxLog` (`StreamModel`: creations, cancellations, durations; ids follow creation order), never from the contract, so checking never restores anything:

| Invariant | What it asserts |
| --- | --- |
| `NoMidLifecycleExpiry` | every running stream's persistent entry has TTL > 0 (0 is the last live ledger; negative means archived) |
| `InstanceAlive` | while any stream is running, the contract instance has TTL > 0 |
| `RestoreRestoresFunction` | for every running stream whose entry is archived, a throwaway `Env::from_ledger_snapshot` copy is taken, natives are re-registered, and a suite-supplied probe calls `withdraw` and `cancel` there; both must succeed (or `withdraw` may report `NothingToWithdraw`). The fuzzed `Env` is untouched |

The usage profile is what makes the pack bite. The demo `Env` sets the minimum persistent TTL to 30 days (a demo value; the real one is a network parameter shown on Stellar Lab). Under `daily_then_silence(30, 90)` the buggy contract fails in two steps: `create_stream` (365 days), then 90 days of silence, and the next call finds stream 0 archived with 275 days left to run, its TTL having run out 60 days earlier. What this means on-chain: the stream is not lost, but every later transaction touching it must carry a restore list and pay restoration fees, and a hand-built transaction without one fails. The fixed contract holds under the same profile. Measured: 63 cases and 621 steps in 0.6 s under `MockAll`.

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
- `Real` auth runs end to end at about 5 ms per step on the demo suite, so generation uses `Real` directly (see §4).

**Open**

- Whether `proptest-state-machine` preconditions are expressive enough for the policy generators, or whether a custom `Strategy` is needed.
- Generating smart-account rule rotation (`remove_context_rule` + `add_context_rule`) under `Real` auth: policy `install` callbacks need pre-built sub-invocations. Interesting because re-creating a rule reinstalls `SpendingLimit` with an empty history, which is a second way to reset the window.
- Stream ids in the `ttl-archival` pack are inferred from creation order because the log carries no return values. Recording handler return values in `StepRecord` would make the model exact.
- Whether to export reproductions as `#[test]` functions or as a JSON step list plus a generic replay test. Decided in two stages: the core `Exporter` emits the JSON step list (`suite`, `seed`, `outcome.status`, `outcome.violation`, `outcome.failing_step`, `outcome.log.steps[]`); the CLI's `export` command renders a Rust `#[test]` from it.
