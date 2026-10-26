# Pitch deck

Source for `docs/deck/breakwater-pitch.pptx` and `docs/deck/breakwater-pitch.pdf`. Seven slides on the HackMeridian template: the template's five (Cover, The Solution, PMF, Technical Workflow, The Team) in their original order, with Findings and Testnet & Progress inserted after Technical Workflow. Each slide maps to the judging criteria so a judge can score every criterion within three minutes:

| Criterion | Weight | Slide |
| --- | --- | --- |
| Thematic Alignment | 20% | 3 PMF, 4 Technical Workflow (auth framework, state archival) |
| User Value | 20% | 2 The Solution, 3 PMF |
| Technical Execution | 15% | 4 Technical Workflow, 5 Findings |
| Innovation | 15% | 4 Technical Workflow, 5 Findings |
| Hackathon Progress | 10% | 6 Testnet & Progress |
| Market | 10% | 3 PMF, 7 The Team |
| UX | 10% | 2 The Solution (terminal output), 5 Findings (HTML report) |

Every number below has a source: a command whose output contains it, or a file in the repository. Numbers were collected on 25 October 2026 from `main` at `33ca153`.

## Facts and their sources

| Fact | Source |
| --- | --- |
| Policy-bypass shrunk sequence 49 → 49 → 49 USDC, gaps +1 / +10 / +1718 ledgers; 58 cases, 185 steps | `breakwater run examples/policy-bypass --seed 7` |
| "agent spent 147 within 17280 ledgers, limit is 100 (rolling_cap)" | same command |
| Daily window installed as `period_ledgers` 1 728, intended 17 280 | `examples/policy-bypass/tests/common/mod.rs` (`BUGGY_DAILY_WINDOW`, `LEDGERS_PER_DAY`) |
| Up to 10× the daily cap per day | OpenZeppelin `spending_limit.rs` window semantics: the day's 17 280 ledgers split into 10 disjoint 1 728-ledger windows of at most 100 USDC each; 50 + 50 at the start of each window reaches it |
| "stream 0 is archived with 275 days left to run"; 63 cases, 621 steps, shrunk to 2 | `breakwater run examples/ttl-archival --seed 11` |
| `bump_ttl` keeps an entry alive 1.3 days past its stream (seed 5) | `cargo test -p example-ttl-archival --test suite anyone_can_keep -- --nocapture` |
| One `withdraw_many` after a quiet quarter restores 5 entries, budget 3 (seed 3) | `cargo test -p example-ttl-archival --test suite the_first_call -- --nocapture` |
| Testnet: three replayed payments, 147 USDC, ledgers 5383703 → 5387240 (3 537 ledgers, about 4.9 h at ~5 s) | README "Testnet evidence"; `stellar ledger entry fetch`, stellar.expert |
| Seven contracts, five transactions besides deployment and setup | README "Testnet evidence" |
| 21 commits on `main`, first at 2026-10-25 11:21 UTC (as of PR #20) | `git rev-list --count main`, `git log --reverse main` |
| 20 merged pull requests (as of PR #20) | `gh pr list --state merged` |
| 43 tests passing | `cargo test --workspace` |
| Protocol 23 archived 478 contract entries with stale values; 84 were restored into live state before Protocol 24 fixed it | [stellar.org, Addressing state archival inconsistencies](https://www.stellar.org/blog/developers/addressing-state-archival-inconsistencies-protocol-upgrade-vote-next-week); also `docs/ARCHITECTURE.md` §5 |
| x402 / MPP agent payments | README "Problem" |

## Screenshots

All in `docs/deck/shots/`, rendered headless (no desktop captures), no user names or local paths.

| ID | File | How it was captured | Crop |
| --- | --- | --- | --- |
| S1 | `S1-term-policy-bypass.png` | `breakwater run examples/policy-bypass --seed 7 --json /tmp/breakwater-demo/policy-bypass-7.json` under a pseudo-terminal (real ANSI colours), `aha --black`, dark HTML frame at 18 px Menlo, headless Chrome | whole window (last 25 lines; the output is 10) |
| S2 | `S2-term-ttl-archival.png` | same for `examples/ttl-archival --seed 11` | whole window |
| S3 | `S3-report-policy-bypass.png` | `docs/reports/policy-bypass-seed-7.html`, headless Chrome 1600×900 | x 384–1216, y 16–630: summary card, timeline, step table |
| S4 | `S4-report-ttl-archival.png` | `docs/reports/ttl-archival-seed-11.html`, headless Chrome 1600×900 | summary card (y 16–160) stacked on the violation chart (y 610–860) |
| S5 | `S5-stellar-expert-tx3.png` | stellar.expert page of tx `3f30f306…aa8e`, headless Chrome 1600×900 at device scale 2 | the Summary block's left column (status, ledger, source, sequence, size) stacked above the transfer row, which is split before `transfer(` so the card fits the slide's full height; nothing else from the page |
| S6 | `S6-github-merged-prs.png` | `github.com/Belvast-Innovation/Breakwater/pulls?q=is:pr is:merged`, headless Chrome 1600×900, author names masked | x 176–1424, y 190–900; kept in `shots/` but not used in the deck (the PR count is in a bullet) |
| S7 | `S7-repro-test.png` | first 25 lines of `examples/policy-bypass/tests/repro_policy_bypass_7.rs`, `pygmentize -l rust -O style=monokai,linenos=table`, headless Chrome | x 0–1060, y 20–710 |
| S8 | `S8-repo-qr.png` | `qrencode -s 12 -m 2 https://github.com/Belvast-Innovation/Breakwater` | none |

## Slides

### 1. Cover

- Header: `PLATFORM 01 / 07`
- Pill: `INVARIANT TESTING FOR SOROBAN`
- Title: `BREAK` / `WATER`
- Logo: `docs/assets/breakwater-logo.png` (transparent background), centred between the title and the template's motif

### 2. The Solution

- Header: `PLATFORM 02 / 07`, `NEXT ▸ PMF`, `PITCH STEP 01 / 06`
- Subtitle: Breakwater searches for the call sequence that breaks your Soroban invariants.
- Bullets:
  - Random multi-step sequences run against a local Soroban `Env`; every invariant is checked after every step.
  - A failure is shrunk to its minimal steps and exported as a Rust regression test.
  - Two attack packs for Stellar-specific risk: smart-account policies and TTL state archival.
- Callout (label **Why sequences**): Single-call tests prove each payment is fine. Only a sequence shows whether the account is.
- Picture: S1

### 3. PMF

- Header: `PLATFORM 03 / 07`, `NEXT ▸ TECHNICAL WORKFLOW`, `PITCH STEP 02 / 06`
- Subtitle: Soroban has static analysis, AI review and formal verification, but no reusable stateful fuzzing.
- Bullets:
  - Agent wallets paying over x402 / MPP rely on smart-account policies; a policy that holds for one call may fail for five.
  - Protocol 23 archived 478 contract entries with stale values; 84 were restored into live state before Protocol 24 fixed it.
  - For every Soroban team shipping custom accounts or long-lived storage; a suite runs as `cargo test`.
- Callout (label **The gap**): The bugs that cost money here only appear across many transactions and long gaps.
- Picture: S4

### 4. Technical Workflow

- Header: `PLATFORM 04 / 07`, `NEXT ▸ FINDINGS`, `PITCH STEP 03 / 06`
- Subtitle: Declare, generate, check, shrink and export: one cargo test.
- Bullets:
  - `proptest-state-machine` generates seeded sequences; each case restores a ledger snapshot instead of redeploying.
  - Real auth mode signs every step with Ed25519, so OpenZeppelin's `__check_auth` and policies really run.
  - TTL is read from ledger entries without touching them, so probing never auto-restores an archived entry.
- Callout (label **Real auth**): A finding under real authorization is a real bypass, not a mocking artifact.
- Picture: flow of four rounded rectangles (DECLARE → GENERATE → CHECK → SHRINK, the last labelled "minimal repro → .rs test") in the template's yellow on dark, above S7

### 5. Findings

- Header: `PLATFORM 05 / 07`, `NEXT ▸ TESTNET & PROGRESS`, `PITCH STEP 04 / 06`
- Subtitle: The policy code is correct; the configuration is not. Every payment passes alone; the sequence does not.
- Bullets:
  - Daily window 1 728 ledgers, not 17 280: 49 → 49 → 49 USDC (+1 / +10 / +1718) is 147 in 24 h vs a 100 cap; up to 10× a day.
  - No `extend_ttl`: "stream 0 is archived with 275 days left to run".
  - Unauthenticated `bump_ttl` keeps a stream alive 1.3 days past its end.
  - After 90 quiet days one `withdraw_many` restores 5 entries against a budget of 3.
- Callout (label **Takeaway**): Breakwater found the three-step sequence that no single-call test can see.
- Picture: S3

### 6. Testnet & Progress

- Header: `PLATFORM 06 / 07`, `NEXT ▸ THE TEAM`, `PITCH STEP 05 / 06`
- Subtitle: The shrunk bypass replayed on Stellar Testnet through the real OpenZeppelin account.
- Bullets:
  - Three agent-signed payments: 147 USDC in 3 537 ledgers (about 4.9 hours), all accepted by the policies.
  - Seven contracts and five transactions on Testnet, each linked from the README.
  - Built during the event: 21 commits on `main`, 20 merged PRs, 43 tests, CI on every PR.
- Callout (label **Reproducible**): Every number in this deck comes from a command anyone can rerun.
- Picture: S5 at the full height of the picture area

### 7. The Team

- Header: `PLATFORM 07 / 07`, `FINAL STOP`, `PITCH STEP 06 / 06`
- Subtitle: Hannes Gao · Belvast Innovation · Karlsruhe, Germany
- Bullets:
  - Solo builder: framework design, Rust implementation, attack packs, Testnet replay.
  - Repository: github.com/Belvast-Innovation/Breakwater (Apache-2.0).
  - Next: rule-rotation probe, restoration-cost CI gate, XDR replay export; then crates.io and a GitHub Action.
- Callout (label **Provenance**): Everything shown was built during the event; the commit history is the record.
- Picture: S8 with the repository URL

## Layout changes to the template

To fit a screenshot in the right 45% of each content slide without covering text, the subtitle, bullet and callout boxes are narrowed to the left 8.6 inches, the subtitle is set at 24 pt and bullets at 20 pt (template: 28 pt and 24 pt), and the decorative shapes on the right of the content slides are removed. On Findings, which has four bullets, the bullet box is taller and the callout box sits lower. The template's "👉 Think:" callout prefix is replaced by a short bold label per slide. Fonts, colours, the header bar, the footer and the theme are unchanged.
