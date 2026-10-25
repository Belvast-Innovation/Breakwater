# Demo video

Storyboard and narration for the HackMeridian submission video: 1920×1080, 30 fps, H.264 + AAC, about three minutes. Every number in the narration is listed with its source in [`docs/PITCH_DECK.md`](PITCH_DECK.md).

The video is assembled by the scripts in [`scripts/video/`](../scripts/video/); see "Producing the video" at the end. Each segment's picture is stretched or trimmed to its narration, so the narration sets the pace.

The narration format below is parsed by `scripts/video/assemble.sh`: each segment is a `## NN · title` heading followed by a `Narration:` paragraph. Keep that shape when editing.

## 01 · The gap

- Picture: deck cover (page 1 of `docs/deck/breakwater-pitch.pdf`), then fade.
- Source: `scripts/video/stills.py` (PDF page 1)

Narration: Soroban already has static analysis, AI review and formal verification. What it doesn't have is a reusable stateful fuzzer. And Stellar has two risks that only show up across many transactions: smart-account policies for agent wallets, and state that expires under TTL archival. Breakwater is that missing layer.

## 02 · A suite is just Rust

- Picture: the "Writing a test" suite from the README, rendered as code.
- Source: `scripts/video/stills.py` (`crates/breakwater/tests/counter.rs`, the file the README snippet is taken from)

Narration: A Breakwater suite is ordinary Rust. Handlers are the calls the fuzzer may make. Invariants are what must always hold. You run it with cargo test. Breakwater generates the sequences, checks every invariant after every step, and shrinks any failure.

## 03 · Finding the bypass

- Picture: terminal, `breakwater run examples/policy-bypass --seed 7`, live.
- Source: `scripts/video/policy-bypass.tape`

Narration: First demo: an OpenZeppelin smart account with an agent key. The agent may pay at most 50 USDC per ledger and 100 per day. Every step goes through real authorization. After 58 cases Breakwater finds a violation and shrinks it to three payments: 49, 49, 49.

## 04 · Correct code, wrong configuration

- Picture: deck slide 5, Findings (page 5 of the PDF).
- Source: `scripts/video/stills.py` (PDF page 5)

Narration: To be clear: the policy code is correct, and this is not an OpenZeppelin bug. The configuration is wrong. The daily window was set to 1,728 ledgers instead of 17,280. Every single payment passes. The sequence doesn't: 147 USDC in a day, against a cap of 100.

## 05 · The report

- Picture: `docs/reports/policy-bypass-seed-7.html`, slow scroll from the summary to the spend chart.
- Source: `scripts/video/record-browser.mjs` (`report-policy-bypass`)

Narration: Breakwater report turns the run into one static HTML page. The shrunk steps on a timeline, the step table, and the cumulative spend crossing the limit.

## 06 · A regression test for free

- Picture: the exported reproduction, `examples/policy-bypass/tests/repro_policy_bypass_7.rs`, rendered as code.
- Source: `scripts/video/stills.py`

Narration: Breakwater export writes the shrunk sequence as a Rust test. It replays the exact steps and asserts the same invariant fails at the same step. Commit it, and this bug can't come back quietly.

## 07 · State that expires

- Picture: terminal, `breakwater run examples/ttl-archival --seed 11`, then `docs/reports/ttl-archival-seed-11.html` scrolling to the TTL chart.
- Source: `scripts/video/ttl-archival.tape`, then `scripts/video/record-browser.mjs` (`report-ttl-archival`)

Narration: Second pack: state archival. A payment stream that never extends its TTL. Under daily use and then a quiet quarter, Breakwater shrinks it to two calls and reports: stream 0 is archived with 275 days left to run. Two related invariants: an unauthenticated bump keeps a stream alive 1.3 days past its end, and one call after the quiet period restores 5 entries against a budget the suite sets at 3.

## 08 · On Testnet

- Picture: stellar.expert page of the third replayed payment.
- Source: `scripts/video/record-browser.mjs` (`stellar-expert`), https://stellar.expert/explorer/testnet/tx/3f30f306985585ff62b31271f7aa050a26ec82f6458925d17aa5c32ce157aa8e

Narration: Then I replayed the shrunk bypass on Stellar Testnet, through the real OpenZeppelin account, signed by the agent's key. Three payments, 147 USDC, in 3,537 ledgers, about 4.9 hours. The policies accepted every one.

## 09 · Built this weekend

- Picture: GitHub repository home page, slow scroll through the README, with a caption card showing the commit, pull request and test counts. The counts are not spoken; `scripts/video/stills.py` reads them from the facts table in `docs/PITCH_DECK.md`, so updating that table updates the video.
- Source: `scripts/video/record-browser.mjs` (`github`), https://github.com/Belvast-Innovation/Breakwater

Narration: All of this was built during the event, every change through a pull request with CI. Next: a rule-rotation probe, a restoration-cost CI gate and XDR replay export, then crates.io and a GitHub Action.

## 10 · Where to find it

- Picture: deck slide 7, The Team, with the repository QR code (page 7 of the PDF).
- Source: `scripts/video/stills.py` (PDF page 7)

Narration: I'm Hannes Gao, Belvast Innovation. Breakwater is open source under Apache 2.0, and the repository is on screen. Thanks for watching.

## Producing the video

Tools: `vhs` (with `ttyd`), an `ffmpeg` built with libass (Homebrew `ffmpeg-full`), Node with Playwright, and Python with `pymupdf` and `pygments`. Pictures go to `breakwater-out/video/segments/`, narration to `breakwater-out/video/voice/`, the result to `breakwater-out/video/breakwater-demo.mp4`; `breakwater-out/` is not committed.

```bash
scripts/video/make-segments.sh          # every segment's picture, as segNN.mp4
scripts/video/assemble.sh --preview     # silent preview, durations estimated from the narration
# record seg01.m4a … seg10.m4a (one file per segment) into breakwater-out/video/voice/
scripts/video/assemble.sh               # final video with narration, loudness-normalised, subtitled
```
