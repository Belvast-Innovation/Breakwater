# Submission checklist — HackMeridian 2026

Deadline: **Monday 26 October 2026, 12:00 Europe/Lisbon. No late submissions.** Submit via the portal (app.hackmeridian.com → Team → Submission). The submission is `draft` until you hit submit.

## Rules that can disqualify us

From the portal "Rules & conduct" page:

- [ ] **Prior work disclosed** in `README.md`. Undisclosed prior work is disqualification. Our statement: no code before 09:00 WET 25 Oct; research and design only.
- [ ] **AI usage attributed** in `README.md` ("AI-assisted development"). Attribution is required, not permission. Update the table if a new module is mostly AI-drafted.
- [ ] **Hackathon progress is visible.** Commit history on `main` starts after the event opened and is linear.
- [ ] In person, 18+, alcohol-free event. Code of conduct applies on Discord and on site.

## Required deliverables

| Item | Requirement | Status |
| --- | --- | --- |
| Public GitHub repo | Source + README + architecture overview | [ ] |
| README | Summary, problem, setup, installation, demo instructions, how Stellar is used, prior work, AI usage | [ ] |
| Demo video | **2–4 minutes**, YouTube or Vimeo link | [ ] |
| Pitch deck | PDF / PPTX / PPT / HTML, **≤ 20 MiB** | [ ] |
| Testnet evidence | Contract IDs + at least one tx hash in README | [ ] |
| Track selection | Builder: **Genesis**. Thematic: **Stellar Unlocked** (FAQ says both may be selected; check the form) | [ ] |
| Submission form answers | Problem, target user, value proposition, what was built during the event | [ ] |

Developer tools may use a technical demonstration; no consumer-facing flow is required.

## Scoring rubric (1–10 per criterion)

| Criterion | Weight | What we show |
| --- | --- | --- |
| Thematic Alignment (Unlocked) | 20% | Deep use of core protocol: auth framework, state archival, resource metering |
| User Value & Problem | 20% | The missing stateful-fuzzing layer; agent wallets and TTL bugs as concrete pain |
| Technical Execution | 15% | Real auth path, shrinking, exported reproductions, CI |
| Innovation | 15% | First reusable invariant framework for Soroban; TTL attack pack has no EVM analogue |
| Hackathon Progress | 10% | Everything was built in 27 hours; commit history proves it |
| Market & Ecosystem Potential | 10% | Any Soroban project can adopt it; SCF Public Goods path |
| Product Design & UX | 10% | Readable CLI report, one-command repro export, 5-minute README |

Judging is asynchronous (video, deck, repo, form). Finalists pitch live at Meridian on **Wed 28 Oct, 13:50**.

## Demo video script (target 3:00)

1. 0:00–0:20 — Problem: Soroban has static analysis, AI review, formal verification; no stateful fuzzing. Agent wallets and TTL make it urgent.
2. 0:20–0:50 — Show the suite declaration: actors, one handler, one invariant. Fifteen lines.
3. 0:50–1:40 — `breakwater run examples/policy-bypass`. Watch it find the three-small-payments bypass, shrink it, export the test. Open the exported test.
4. 1:40–2:20 — `breakwater run examples/ttl-archival`. Usage profile fast-forwards; stream state expires mid-lifecycle; `withdraw` fails.
5. 2:20–2:45 — Replay the shrunk bypass on Testnet; show the tx on stellar.expert.
6. 2:45–3:00 — Why it is infrastructure; roadmap; SCF.

Record with a fixed seed. Keep a backup recording from the first successful run in case the live one misbehaves.

## Pitch deck outline (8 slides)

1. Title — Breakwater: invariant testing for Soroban
2. Problem — the missing layer, with the three existing tools named
3. Why now — x402/MPP agent wallets; TTL archival
4. How it works — the four-stage pipeline diagram
5. Demo results — the two bugs found, with shrunk sequences
6. Stellar features used — the table from README
7. What was built this weekend — commit graph screenshot
8. Next — crates.io, SCF, more attack packs; team

## Final-hour sequence (Mon 26 Oct)

| Time | Action |
| --- | --- |
| 09:00 | Attend morning briefing (submission format, late policy) |
| 09:15–10:30 | Finish TTL demo; Testnet deploy + replay; paste IDs into README |
| 10:30–11:00 | Record video (two takes), upload to YouTube, confirm link is public |
| 11:00–11:20 | Export deck to PDF, check size < 20 MiB |
| 11:20–11:40 | Final `main` merge: README, docs, CI green. `git log` sanity check |
| 11:40 | **Submit.** Screenshot the confirmation |
| 11:45–12:00 | Buffer. Touch nothing |

## Contacts

- Organisers: meridian@growwithguava.com
- Incident / safeguarding: portal → Report an incident (anonymous allowed). Emergency: 112
