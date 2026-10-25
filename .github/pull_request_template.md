## What

<!-- One or two sentences. This becomes the squash commit body. -->

## Why

<!-- The problem this solves or the feature it enables. Link an issue if there is one: Refs #N -->

## How to test

```bash
cargo test --workspace
# plus any manual step, e.g.
cargo run -p breakwater-cli -- run examples/policy-bypass --seed 42
```

## Hackathon progress

<!-- What exists after this PR that did not exist before. One line. Judges read this. -->

## Checklist

- [ ] `cargo fmt --all` and `cargo clippy --workspace --all-targets -- -D warnings` pass
- [ ] Tests added or updated; every new invariant has a test proving it fires
- [ ] Docs updated if behaviour or API changed (`README.md`, `docs/ARCHITECTURE.md`)
- [ ] If this PR adds a mostly AI-drafted module, the "AI-assisted development" table in `README.md` is updated
- [ ] PR title is in Conventional Commits format; no attribution trailers anywhere in title or body
