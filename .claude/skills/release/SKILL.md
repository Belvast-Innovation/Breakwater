---
name: release
description: Prepare a Breakwater release PR (version bump only). Use when asked to release, cut, bump or prepare a new version of the crates, e.g. "release patch", "prepare 0.2.0". Takes patch, minor or an explicit X.Y.Z; with no argument it proposes a version from the Conventional Commits since the last tag. Never tags or publishes; release.yml does that after the PR is merged and the crates-io environment is approved.
---

# Release: prepare a version-bump PR

A release of Breakwater is one merged PR titled `release: vX.Y.Z` plus one approval of the
`crates-io` environment. This skill prepares that PR. Publishing, the tag and the GitHub
Release are done by `.github/workflows/release.yml` after the merge, with crates.io Trusted
Publishing (no registry token anywhere).

**Never** run `cargo publish` (other than `--dry-run`), create or push a tag, or merge the
PR. Manual publishing is an emergency procedure only (CLAUDE.md, "Releasing").

## Input

`$ARGUMENTS` is one of:

- `patch` or `minor`: bump the current workspace version accordingly
- an explicit version `X.Y.Z` (must be greater than the current version)
- nothing: propose a version (step 2), state the reasoning, and ask for confirmation before
  changing any file

## 1. Start from the latest main

```bash
git fetch origin --prune --tags
git switch main && git pull --ff-only
git status --short            # must be empty
```

Current version and previous release tag (the `-hackmeridian` tags are submission
snapshots, not releases):

```bash
cargo metadata --format-version 1 --no-deps --locked \
  | python3 -c 'import json,sys; print({p["version"] for p in json.load(sys.stdin)["packages"] if p.get("publish") is None})'
prev=$(git tag --list 'v*' --sort=-v:refname | grep -E '^v[0-9]+\.[0-9]+\.[0-9]+$' | head -1)
echo "$prev"
```

Stop if the current version is not published yet (`python3 scripts/release-plan.py plan`
must report `noop`); a previous release is still in flight or needs recovery.

## 2. Choose the version

List what changed since the previous tag:

```bash
git log --format='%h %s' "$prev"..HEAD
git log --format='%B' "$prev"..HEAD | grep -n 'BREAKING CHANGE' || true
git diff "$prev"..HEAD -- Cargo.toml   # dependency requirement changes
```

While the version is `0.x`:

- **minor** if anything is breaking: a `!` after the type or scope (`feat!:`,
  `fix(core)!:`), a `BREAKING CHANGE:` footer, or a new major version of a dependency whose
  types appear in the public API. Those are `soroban-sdk` (every pack and the core take and
  return SDK types), `proptest` (re-exported as `breakwater::proptest`) and
  `stellar-accounts` (types used by `breakwater-pack-policy-bypass`). A `soroban-sdk` major
  bump also needs OpenZeppelin `stellar-accounts` support first (CLAUDE.md §2).
- **patch** otherwise: fixes, new features that add API without removing or changing any,
  documentation and metadata changes (a crate's README or `documentation` URL only reaches
  crates.io with a release).

From `1.0.0` on, use regular SemVer: major for breaking, minor for features, patch for fixes.

When proposing, give the version, the rule that decided it and the commits that triggered it.

## 3. Bump the version

In the root `Cargo.toml`, set the new version `X.Y.Z` in:

- `[workspace.package] version`
- the `version` of each internal crate in `[workspace.dependencies]`: `breakwater`
  (package `soroban-breakwater`), `breakwater-pack-policy-bypass`,
  `breakwater-pack-ttl-archival`, `breakwater-pack-budget-snapshot`

Then update the lockfile for the workspace members only and prove that no third-party
dependency moved:

```bash
cargo update -w
git diff -U0 Cargo.lock | grep -E '^[-+](name|version|source|checksum)' 
```

The diff must contain only `version` lines of the workspace packages (the five published
crates and the examples, which share the workspace version), nothing with `source` or
`checksum`. If anything else changed, stop and report.

On a **minor** bump, update the install snippets that pin the previous minor, e.g.
`soroban-breakwater = "0.1"` → `"0.2"`, in `README.md` and every crate README:

```bash
grep -rn '"0\.1"' README.md crates/*/README.md crates/packs/*/README.md
```

Also grep `README.md` and `docs/` for the old full version and update statements about
"the current release". Leave historical statements (submission documents, the Protocol
compatibility note about what a given release was built with) as they are.

## 4. Checks

Everything CI runs, then the publish dry run and the release plan:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
for c in example-policy-bypass example-owner-account example-spending-limit-policy \
         example-ed25519-verifier example-ttl-archival; do
  RUSTFLAGS="-D warnings" stellar contract build --package "$c"
done
cargo publish --workspace --dry-run --locked
python3 scripts/release-plan.py plan     # expect mode "release" for the new version
```

In a cloud session (CLAUDE.md §9) prefer `cargo check -p <crate>` while iterating, but run
the full list above once before opening the PR.

## 5. Branch, commit, PR

```bash
git switch -c release/vX.Y.Z
git add Cargo.toml Cargo.lock README.md crates/   # only what changed
git commit -m "release: vX.Y.Z"
git push -u origin release/vX.Y.Z
```

The commit message has no trailers of any kind (CLAUDE.md §3).

Open the PR titled exactly `release: vX.Y.Z` with the repository template. The body lists:

- the version and why (patch or minor, and the deciding commits)
- the changes since the previous tag, one line per merged PR (`git log --format='- %s' "$prev"..HEAD`)
- the check results (test summary line, dry run)
- under "How to test": `python3 scripts/release-plan.py plan`

Locally use `gh pr create`; in a cloud session use the REST API
(`gh api repos/Belvast-Innovation/Breakwater/pulls -f title=... -f head=release/vX.Y.Z -f base=main -F body=@body.md`).

Wait for CI. Do not merge: merging is the team's decision.

## 6. Hand over

Tell the user, in so many words:

1. Merge the PR (squash) when ready.
2. The merge starts the `release` workflow. Its `publish` job waits for approval of the
   **`crates-io` environment**: open the run under
   <https://github.com/Belvast-Innovation/Breakwater/actions/workflows/release.yml> and
   choose *Review deployments → crates-io → Approve and deploy*.
3. The job publishes the five crates in dependency order, tags the merge commit as `vX.Y.Z`
   and creates the GitHub Release.

After the run, verify:

```bash
for c in soroban-breakwater breakwater-pack-policy-bypass breakwater-pack-ttl-archival \
         breakwater-pack-budget-snapshot breakwater-cli; do
  curl -s -A 'breakwater-release-check' "https://crates.io/api/v1/crates/$c" \
    | python3 -c 'import json,sys; c=json.load(sys.stdin)["crate"]; print(c["name"], c["max_version"])'
done
git ls-remote --tags origin "refs/tags/vX.Y.Z"
gh release view vX.Y.Z --json url --jq .url
```

## If a release run fails

`release-plan.py` treats any partial state (some crates published, published but not
tagged, tagged but not published) as `recover` and only does what is missing. After fixing
the cause, use *Re-run failed jobs* on the same run, or dispatch `release.yml` from `main`
with `dry_run` unchecked. Never bump the version to get past a half-finished release.
