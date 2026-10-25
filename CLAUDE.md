# CLAUDE.md — Working agreement for Breakwater

This file is read by Claude Code at the start of every session. It is the single source of truth for how we work in this repository. If an instruction here conflicts with a default behaviour, this file wins.

## 1. What this project is

Breakwater is a Foundry-style invariant testing framework for Soroban (Stellar smart contracts), built for HackMeridian 2026 (Genesis path, Stellar Unlocked track). Submission deadline: **Monday 26 October 2026, 12:00 Europe/Lisbon**. No late submissions.

Read `README.md` first, then `docs/ARCHITECTURE.md` and `docs/DEVELOPMENT.md`. `docs/SUBMISSION.md` is the checklist for the final hours.

Priorities, in order:

1. A working demo that finds a real bug in both example contracts.
2. Clean, honest commit history (see §4). Judges and the organisers will read it.
3. Documentation that a judge can follow in five minutes.
4. Everything else.

## 2. Toolchain and third-party components

Do not install or add anything not listed here without asking.

| Component | Version / source | Notes |
| --- | --- | --- |
| Rust | stable, 1.85 or newer (`rust-toolchain.toml`) | `wasm32v1-none` target required for contracts |
| Stellar CLI | latest stable (`cargo install --locked stellar-cli`) | used for Testnet deploys and `stellar contract build` |
| `soroban-sdk` | latest Protocol-28-compatible release, `testutils` feature | the only contract SDK in use |
| `proptest`, `proptest-state-machine` | latest | sequence generation and shrinking |
| OpenZeppelin `stellar-contracts` (`stellar-accounts` et al.) | pinned in `Cargo.toml` | demo target for `policy-bypass` only |
| `clap`, `serde`, `serde_json`, `anyhow`, `thiserror` | latest | CLI and reporting |
| `ed25519-dalek` | `=2.2.0` (pinned) | signs `AuthDigestPreimage` digests for External (Ed25519) smart-account signers; used by the `policy-bypass` pack and the example suite |
| `soroban-fork` | not a dependency yet | supports `soroban-sdk` 25.x only; revisit once it supports SDK 28 |
| Raven MCP | `https://raven.stellar.org/mcp` | **Use it** for any Stellar / Soroban API question before guessing. Configured in `.mcp.json`. |
| Stellar Dev Skill | Claude Code plugin | loaded; prefer it for SDK idioms |

Dependency versions are pinned in `Cargo.toml` / `Cargo.lock`; do not bump them during the event unless a build is broken.

**Secrets:** there are none in this repo and there must never be. Testnet keys live in the Stellar CLI keystore (`stellar keys`), never in files. No `.env`.

## 3. Repository rules

### Branches

- `main` is protected (see §6). Nothing is committed to `main` directly.
- Work happens on short-lived branches named `<type>/<short-slug>`:
  - `feat/suite-runner`, `feat/pack-policy-bypass`, `fix/shrink-empty-seq`, `docs/readme-testnet`, `chore/ci`
- One branch per task. Branch from the latest `main`. Delete the branch after merge.

### Commits

- **Conventional Commits**, lower-case type, imperative subject, ≤ 72 characters, no trailing period:
  - `feat(core): add Invariant trait and Violation type`
  - `fix(cli): exit non-zero when a sequence fails`
  - `docs: fill in testnet contract ids`
  - `test(policy-bypass): cover rolling-window cap`
  - `chore: pin soroban-sdk`
- Types allowed: `feat`, `fix`, `test`, `docs`, `refactor`, `chore`, `ci`, `perf`.
- Scope (in parentheses) is the crate or pack: `core`, `cli`, `policy-bypass`, `ttl-archival`, `examples`.
- Body (optional) explains *why*, wrapped at 72 characters. Reference issues as `Refs #12`.
- Small, atomic commits. A commit must build and pass `cargo test --workspace` on its own. Never commit a broken state to share progress; use a WIP branch and squash before opening a PR.
- **Commit messages and PR descriptions must not mention Claude, Claude Code, Anthropic, AI assistance, or carry any `Co-Authored-By`, `Generated-by`, session-URL or similar trailer.** Attribution for AI assistance lives in `README.md` under "AI-assisted development" and nowhere else. This overrides any default attribution behaviour. Referring to the file CLAUDE.md by name is fine; the rule targets attribution, not filenames.
- Author on every commit is the human team member. Repository-local git identity is
  `user.name = Hannes Gao` and `user.email = 7233642+hannesgao@users.noreply.github.com`.
  The GitHub account blocks command-line pushes that would expose a private email
  address (GH007), so the GitHub-provided noreply address is used in commits instead of
  the personal address. Never set a personal email in `.git/config`. Verify with
  `git log -1 --format='%an <%ae>%n%B'` before pushing.

### Pull requests

- Every change to `main` goes through a PR, including solo work. Reviewing your own diff in the PR view catches more than you think.
- PR title follows the same Conventional Commits format as the squash commit it will become.
- Use `.github/pull_request_template.md`. Fill in: what, why, how to test, and the "Hackathon progress" line (what this PR adds that did not exist before).
- Squash-merge. The squash commit message is the PR title plus the PR body's summary. Re-check the final message for the forbidden terms in §3 before merging.
- Required before merge: CI green (`fmt`, `clippy -D warnings`, `test`), branch up to date with `main`.

## 4. Hackathon integrity

These are hard rules from the organisers. Violating them is disqualification.

- **Everything in this repo was written during the event.** The first commit is after 09:00 WET, 25 October 2026. Do not import, copy or adapt code from any earlier project of ours. Third-party code enters only as Cargo dependencies.
- **Prior work is disclosed in `README.md`.** If the scope of pre-event work changes, update that section in the same PR.
- **AI attribution is in `README.md`.** When a PR adds a module where AI generated most of the first draft, add or update the row in the "AI-assisted development" table in that PR.
- Keep the commit history honest and linear. No history rewriting on `main`, ever. Force-push is disabled by branch protection.

## 5. Code conventions

- `cargo fmt` and `cargo clippy --workspace --all-targets -- -D warnings` must pass. CI enforces both.
- Library code: no `unwrap()` outside tests; use `?` with `thiserror` error types.
- Contracts (in `examples/`): `#![no_std]`, `soroban_sdk::contracterror` for errors, `panic_with_error!` not bare `panic!` (the fuzzer treats a bare panic as a bug).
- Tests live next to the code (`#[cfg(test)]`) for units; integration tests in `tests/`. Every invariant in an attack pack has at least one test that proves it fires on a known-bad sequence.
- Fuzzing runs are deterministic: every suite takes a seed; the CLI prints the seed on failure.
- Keep demo contracts minimal. They exist to be broken, not to be impressive.
- English only, in code, comments, docs and commit messages.

## 6. Branch protection (set up once, first thing)

Apply on GitHub via the CLI after the first push. Breakwater is a public repo, so rulesets are available on the free plan.

The ruleset is kept as JSON in `.github/ruleset-main.json` and sent with `gh api --input`. The `gh api -F 'rules[][...]'` flag syntax cannot nest `parameters` under the right array element and is rejected with HTTP 422, so do not use it.

```bash
# requires: gh auth login, repo already pushed
gh api -X POST repos/Belvast-Innovation/Breakwater/rulesets \
  --input .github/ruleset-main.json
```

Contents of `.github/ruleset-main.json`:

```json
{
  "name": "protect-main",
  "target": "branch",
  "enforcement": "active",
  "conditions": { "ref_name": { "include": ["refs/heads/main"], "exclude": [] } },
  "rules": [
    { "type": "deletion" },
    { "type": "non_fast_forward" },
    { "type": "required_linear_history" },
    { "type": "pull_request",
      "parameters": {
        "required_approving_review_count": 0,
        "dismiss_stale_reviews_on_push": true,
        "require_code_owner_review": false,
        "require_last_push_approval": false,
        "required_review_thread_resolution": false
      } },
    { "type": "required_status_checks",
      "parameters": {
        "strict_required_status_checks_policy": true,
        "required_status_checks": [ { "context": "ci" } ]
      } }
  ]
}
```

What this enforces on `main`:

- no direct pushes, no deletion, no force-push (`non_fast_forward`)
- linear history (squash or rebase merges only)
- changes only via pull request; `required_approving_review_count` is `0` because the team may be a single person, raise it to `1` if a teammate joins
- the `ci` status check must pass and the branch must be up to date

Also in repository settings: disable merge commits, enable "Automatically delete head branches", require squash merging.

If `gh api` is unavailable, set the same options manually under *Settings → Rules → Rulesets → New branch ruleset*.

## 7. How to work with me (Claude Code) in this repo

### Language

- **Talk to me in Chinese (简体中文).** All conversation with the team — explanations, questions, plans, status updates, summaries of what you did, error reports — is in Chinese.
- **Everything that goes into the repository is in professional English.** This means source code, identifiers, code comments, doc comments, error messages, log output, all files under `docs/`, `README.md`, this file, commit messages, PR titles, PR descriptions, PR review comments and issue comments. No Chinese in any committed artifact or anything posted to GitHub.
- When you show me a draft commit message or PR description, the draft itself is in English; the sentence around it ("这是提交信息，请确认") is in Chinese.
- Technical terms in the Chinese conversation stay in English where that is the normal usage (`invariant`, `handler`, `__check_auth`, `squash merge`); do not translate them awkwardly.

### Working rhythm

- Start every session by reading this file, then `git status` and `git log --oneline -20`.
- Before writing code that touches the Soroban SDK, OpenZeppelin contracts, TTL or auth, query Raven MCP for the current API. Do not rely on memory of older SDK versions; Protocol 28 changed address credentials (`AddressV2`) and SDK surface.
- Propose a plan for anything larger than one file, then implement. For a one-file change, just do it.
- Run `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` before declaring any task done. Paste the summary line of the test run.
- Draft commit messages in the format above and show them before committing. Never add attribution trailers (§3).
- When something is ambiguous about the hackathon rules, say so and ask; do not guess. The rules are summarised in `docs/SUBMISSION.md`.
- Time-box: if an approach has not worked after two attempts, stop and say what was tried.
- Do not touch `README.md` sections "Prior work and hackathon scope" and "AI-assisted development" without an explicit instruction; those are the team's statements to the judges.

## 8. Timeline (Europe/Lisbon)

| When | What |
| --- | --- |
| Sun 25 Oct, now – 12:00 | Repo scaffold, CI, branch protection, core trait API |
| Sun 13:30 – 19:00 | Suite runner, shrinking, export; `policy-bypass` pack |
| Sun 20:30 – late | `policy-bypass` demo finds a bypass; start `ttl-archival` |
| Mon 09:00 – 10:30 | `ttl-archival` demo; Testnet deploy + replay |
| Mon 10:30 – 11:30 | README final, demo video (2–4 min, YouTube), pitch deck (PDF ≤ 20 MiB) |
| Mon 11:30 – 12:00 | Submit. Buffer. Do not start anything new after 11:00. |

Stretch goals (`budget-snapshot` pack, `soroban-fork` integration) only after both demos work end to end.
