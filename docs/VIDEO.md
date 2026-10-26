# Demo video

Storyboard and narration for the HackMeridian submission video, published at https://vimeo.com/1239334137 (3:43, 1920×1080, 30 fps, H.264 + AAC). Every number in the narration is listed with its source in [`docs/PITCH_DECK.md`](PITCH_DECK.md).

The video is assembled by the scripts in [`scripts/video/`](../scripts/video/); see "Producing the video" at the end. Each segment's picture is made as long as its narration, so the narration sets the pace. There are no zoom effects. Deck slides are shown for at most about 9 seconds, each with a highlight box fading in on the part the narration is about; everything else is a screen recording (terminal, report, GitHub, stellar.expert), and the long code image scrolls.

AI-assisted production (same statement as the README's "AI-assisted development" table): narration synthesised with ElevenLabs from a clone of the author's own voice (created with his consent); video assembled by scripts (`scripts/video/`). Human contribution: script, voice sample, review of every segment.

The narration format below is parsed by `scripts/video/assemble.sh`: each segment is a `## NN · title` heading followed by a `Narration:` paragraph. Keep that shape when editing.

## 01 · The gap

- Picture: deck cover (page 1 of `docs/deck/breakwater-pitch.pdf`) for 3 s; the PMF slide (page 3) for 9 s, whose subtitle is the narration's first sentence, with a highlight box on it; then the README's Problem section on GitHub, scrolling slowly.
- Source: `scripts/video/stills.py` (PDF pages 1 and 3, `hl-pmf.png`), then `scripts/video/record-browser.mjs` (`readme-problem`)

Narration: Soroban already has static analysis, AI review and formal verification. What it doesn't have is a reusable stateful fuzzer. That gap matters now: AI agents pay on Stellar through x402 and MPP, guarded only by smart-account policies, and contract state expires under TTL archival. Both break across many transactions, not one. Breakwater is that missing layer.

## 02 · A suite is just Rust

- Picture: the "Writing a test" suite from the README, rendered as code.
- Source: `scripts/video/stills.py` (`crates/breakwater/tests/counter.rs`, the file the README snippet is taken from)

Narration: A Breakwater suite is ordinary Rust. Handlers are the calls the fuzzer may make. Invariants are what must always hold. You run it with cargo test. Breakwater generates the sequences, checks every invariant after every step, and shrinks any failure.

## 03 · Finding the bypass

- Picture: terminal, `breakwater run examples/policy-bypass --seed 7`, live.
- Source: `scripts/video/policy-bypass.tape`

Narration: First demo: an OpenZeppelin smart account with an agent key. The agent may pay at most 50 USDC per ledger and 100 per day. Every step goes through real authorization. After 58 cases Breakwater finds a violation and shrinks it to three payments: 49, 49, 49.

## 04 · Correct code, wrong configuration

- Picture: deck slide 5, Findings (page 5 of the PDF), for 7 s with a highlight box on the misconfigured window; then `docs/reports/policy-bypass-seed-7.html`: scroll to the violation chart, where the cumulative spend crosses the limit of 100, then to the failing step #2 in the step table, which is highlighted.
- Source: `scripts/video/stills.py` (PDF page 5, `hl-findings.png`), then `scripts/video/record-browser.mjs` (`report-policy-bypass-findings`)

Narration: To be clear: the policy code is correct, and this is not an OpenZeppelin bug. The configuration is wrong: the daily window was set to 1,728 ledgers instead of 17,280. The third payment lands exactly one window after the first, so the policy has already forgotten it. Every single payment passes, yet the sequence spends 147 USDC in a day against a cap of 100. Over a full day, that is up to ten times the limit.

## 05 · The report

- Picture: `docs/reports/policy-bypass-seed-7.html`, slow scroll from the summary to the spend chart.
- Source: `scripts/video/record-browser.mjs` (`report-policy-bypass`)

Narration: Breakwater report turns the run into one static HTML page. The shrunk steps on a timeline, the step table, and the cumulative spend crossing the limit.

## 06 · A regression test for free

- Picture: terminal: `breakwater export examples/policy-bypass --seed 7`, the first lines of the exported test (what it replays and asserts), then `cargo test` running the committed copy, `repro_policy_bypass_7`, which passes.
- Source: `scripts/video/export.tape`

Narration: Breakwater export writes the shrunk sequence as a Rust test. It replays the exact steps and asserts the same invariant fails at the same step. Commit it, and this bug can't come back quietly.

## 07 · State that expires

- Picture: terminal, `breakwater run examples/ttl-archival --seed 11`; then `docs/reports/ttl-archival-seed-11.html` scrolling to the TTL chart; then a terminal running the pack's tests (`cargo test -p example-ttl-archival --test suite`, filtered with `grep`) that shows the `no_unbounded_ttl_extension` and `restore_cost_within_budget` violation lines and the three fixed-contract tests passing.
- Source: `scripts/video/ttl-archival.tape`, `scripts/video/record-browser.mjs` (`report-ttl-archival`), `scripts/video/ttl-tests.tape`

Narration: Second pack: state archival. A payment stream that never extends its TTL. Under a month of daily use and then a quiet quarter, Breakwater shrinks the failure to two calls: stream 0 is archived with 275 days left to run. On Stellar the data isn't lost, but the next transaction that touches it has to restore it and pay for that, and one built without the restore fails. Two more invariants fire: an unauthenticated bump keeps a stream alive 1.3 days past its end, and one call after the quiet period restores 5 entries against a budget the suite sets at 3. The fixed contract passes all three.

## 08 · On Testnet

- Picture: stellar.expert page of the third replayed payment.
- Source: `scripts/video/record-browser.mjs` (`stellar-expert`), https://stellar.expert/explorer/testnet/tx/3f30f306985585ff62b31271f7aa050a26ec82f6458925d17aa5c32ce157aa8e

Narration: Then I replayed the shrunk bypass on Stellar Testnet, through the real OpenZeppelin account, signed by the agent's key. Three payments, 147 USDC, in 3,537 ledgers, about 4.9 hours. The policies accepted every one: a real bypass on a real network.

## 09 · Built this weekend

- Picture: GitHub repository home page, slow scroll through the README, with a caption card showing the commit, pull request and test counts and the UTC time they were taken ("as of …"). The counts are not spoken; `scripts/video/make-segments.sh` takes them from `origin/main`, `gh pr list` and `cargo test --workspace` just before it records the page (without them, `scripts/video/stills.py` falls back to the facts table in `docs/PITCH_DECK.md`).
- Source: `scripts/video/record-browser.mjs` (`github`), https://github.com/Belvast-Innovation/Breakwater

Narration: All of this was built during the event, every change through a pull request with CI. Breakwater is a library and a CLI that any Soroban team or auditor can add to cargo test. Next: a rule-rotation probe, a restoration-cost CI gate and XDR replay export, then crates.io, a GitHub Action, and a Stellar Community Fund application.

## 10 · Where to find it

- Picture: deck slide 7, The Team, with the repository QR code (page 7 of the PDF), with a highlight box on the QR code and the repository address, and a small caption in the top right: "Narration: ElevenLabs voice clone of the author's own voice."
- Source: `scripts/video/stills.py` (PDF page 7, `hl-team.png`)

Narration: I'm Hannes Gao, Belvast Innovation. Breakwater is open source under Apache 2.0, and the repository is on screen. Thanks for watching.

## Pronunciation (text-to-speech only)

The narration above is written for reading and for the subtitles. When it is voiced with text-to-speech, apply these substitutions to the spoken text only; the subtitles keep the written form.

`scripts/video/spoken.py` applies the table to every segment and prints the spoken text; rows are applied top to bottom, longer written forms first where they overlap ("bump_ttl" before "TTL").

| Written | Spoken |
| --- | --- |
| 49, 49, 49 | forty-nine, forty-nine, forty-nine |
| 4.9 hours | four point nine hours |
| 1.3 days | one point three days |
| Apache 2.0 | Apache two point oh |
| USDC (every occurrence) | U S D C |
| MPP | M P P |
| CI | C I |
| XDR | X D R |
| crates.io | crates dot io |
| x402 | x four oh two |
| 1,728 | seventeen twenty-eight |
| 17,280 | seventeen thousand two hundred eighty |
| 3,537 | three thousand five hundred thirty-seven |
| 147 | one hundred forty-seven |
| bump_ttl | bump T T L |
| TTL | T T L |
| CLI | C L I |
| Gao | Gow |
| ten times | ten times (unchanged) |

## Producing the video

Tools: `vhs` (with `ttyd`), an `ffmpeg` built with libass (Homebrew `ffmpeg-full`), Node with Playwright, `gh`, and Python with `pymupdf`, `pygments` and Pillow. Pictures go to `breakwater-out/video/segments/`, narration to `breakwater-out/video/voice/`, the result to `breakwater-out/video/breakwater-demo.mp4`; `breakwater-out/` is not committed.

The narration is voiced with `scripts/video/tts.sh` (ElevenLabs text-to-speech, model `eleven_multilingual_v2`, stability 0.5, similarity boost 0.8, style 0, MP3 at 44.1 kHz / 192 kbps). It reads the API key from the macOS Keychain on every call and takes the voice ID from `ELEVENLABS_VOICE_ID`; neither is stored in the repository. A recorded `segNN.m4a` can replace any generated `segNN.mp3`.

```bash
ELEVENLABS_VOICE_ID=<voice id> scripts/video/tts.sh 01 02 03 04 05 06 07 08 09 10
scripts/video/make-segments.sh          # every segment's picture, as long as its narration
scripts/video/assemble.sh --preview     # silent preview, durations estimated from the narration
scripts/video/assemble.sh               # final video with narration, loudness-normalised, subtitled
```
