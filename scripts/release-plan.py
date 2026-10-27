#!/usr/bin/env python3
"""Release planning for .github/workflows/release.yml.

Reads the workspace version V and the publishable packages from `cargo metadata`,
checks whether tag vV exists on origin and whether each package's version V is on
the crates.io sparse index, and decides what the release workflow has to do:

  noop     tag vV exists and every package is published: nothing to do
  release  no tag and no package published: publish everything, tag, release
  recover  any mixed state: publish only what is missing, then tag and release
           whatever is missing

Usage:
  release-plan.py plan          print the plan; in GitHub Actions also write the
                                job outputs and the job summary
  release-plan.py unpublished   print the packages whose version V is not on the
                                index yet, one per line, in dependency order
                                (checked live, so a re-run skips what is done)
  release-plan.py packages      print every publishable package, one per line,
                                in dependency order

Standard library only; needs Python 3.11 or newer for tomllib.
"""

import json
import os
import subprocess
import sys
import time
import tomllib
import urllib.error
import urllib.request
from pathlib import Path

INDEX = "https://index.crates.io"
USER_AGENT = "breakwater-release (https://github.com/Belvast-Innovation/Breakwater)"


class PlanError(Exception):
    pass


def workspace():
    out = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--no-deps", "--locked"],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    meta = json.loads(out)
    # `publish = true` shows up as null; `publish = false` as an empty list.
    packages = [p for p in meta["packages"] if p.get("publish") is None]
    if not packages:
        raise PlanError("no publishable package in the workspace")
    versions = {p["version"] for p in packages}
    if len(versions) != 1:
        raise PlanError(f"publishable packages disagree on the version: {sorted(versions)}")
    return versions.pop(), packages, Path(meta["workspace_root"])


def check_internal_requirements(root, version, names):
    """Every internal crate in [workspace.dependencies] must require exactly V."""
    manifest = tomllib.loads((root / "Cargo.toml").read_text())
    deps = manifest.get("workspace", {}).get("dependencies", {})
    problems = []
    for key, spec in deps.items():
        if not isinstance(spec, dict) or "path" not in spec:
            continue
        package = spec.get("package", key)
        if package not in names:
            continue
        if spec.get("version") != version:
            problems.append(f"{key} (package {package}) has version {spec.get('version')!r}, expected {version!r}")
    if problems:
        raise PlanError("[workspace.dependencies] is out of step with the workspace version: " + "; ".join(problems))


def dependency_order(packages):
    """Topological order over normal and build dependencies between publishable packages."""
    names = {p["name"] for p in packages}
    deps = {
        p["name"]: sorted(
            {d["name"] for d in p["dependencies"] if d["name"] in names and d["kind"] in (None, "build")}
        )
        for p in packages
    }
    order, done, visiting = [], set(), set()

    def visit(name):
        if name in done:
            return
        if name in visiting:
            raise PlanError(f"dependency cycle through {name}")
        visiting.add(name)
        for dep in deps[name]:
            visit(dep)
        visiting.discard(name)
        done.add(name)
        order.append(name)

    for name in sorted(names):
        visit(name)
    return order


def index_path(name):
    name = name.lower()
    if len(name) <= 2:
        return f"{len(name)}/{name}"
    if len(name) == 3:
        return f"3/{name[0]}/{name}"
    return f"{name[:2]}/{name[2:4]}/{name}"


def is_published(name, version):
    """True if `name` has `version` on the crates.io index, yanked or not."""
    url = f"{INDEX}/{index_path(name)}"
    for attempt in range(4):
        request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
        try:
            with urllib.request.urlopen(request, timeout=30) as response:
                body = response.read().decode()
            return any(json.loads(line)["vers"] == version for line in body.splitlines() if line.strip())
        except urllib.error.HTTPError as err:
            if err.code == 404:
                return False
            if attempt == 3:
                raise PlanError(f"index lookup of {name} failed: HTTP {err.code}") from err
        except urllib.error.URLError as err:
            if attempt == 3:
                raise PlanError(f"index lookup of {name} failed: {err.reason}") from err
        time.sleep(2 ** attempt)
    raise AssertionError("unreachable")


def tag_exists(tag):
    result = subprocess.run(
        ["git", "ls-remote", "--exit-code", "--tags", "origin", f"refs/tags/{tag}"],
        capture_output=True,
        text=True,
    )
    if result.returncode == 0:
        return True
    if result.returncode == 2:
        return False
    raise PlanError(f"git ls-remote failed: {result.stderr.strip()}")


def write_github(path_var, text):
    path = os.environ.get(path_var)
    if path:
        with open(path, "a") as handle:
            handle.write(text)


def plan():
    version, packages, root = workspace()
    order = dependency_order(packages)
    check_internal_requirements(root, version, set(order))
    tag = f"v{version}"
    has_tag = tag_exists(tag)
    published = {name: is_published(name, version) for name in order}
    unpublished = [name for name in order if not published[name]]

    if has_tag and not unpublished:
        mode = "noop"
    elif not has_tag and len(unpublished) == len(order):
        mode = "release"
    else:
        mode = "recover"

    dry_run = os.environ.get("DRY_RUN", "false") == "true"
    result = {
        "version": version,
        "tag": tag,
        "tag_exists": has_tag,
        "mode": mode,
        "dry_run": dry_run,
        "packages": order,
        "unpublished": unpublished,
    }
    print(json.dumps(result, indent=2))

    write_github(
        "GITHUB_OUTPUT",
        f"version={version}\ntag={tag}\nmode={mode}\nneeded={'false' if mode == 'noop' else 'true'}\n"
        f"dry_run={'true' if dry_run else 'false'}\nunpublished={' '.join(unpublished)}\n",
    )

    headline = {
        "noop": f"Nothing to do: tag `{tag}` exists and every package is published at {version}.",
        "release": f"Release needed: {version} is not published and tag `{tag}` does not exist.",
        "recover": f"Recovery needed: {version} is only partly released. Only the missing steps run.",
    }[mode]
    rows = "\n".join(
        f"| `{name}` | {'published' if published[name] else 'not published'} |" for name in order
    )
    summary = (
        f"## Release plan: {version}\n\n{headline}\n\n"
        f"- Tag `{tag}`: {'exists' if has_tag else 'missing'}\n"
        f"- Dry run: {'yes' if dry_run else 'no'}\n\n"
        f"| Package (dependency order) | {version} on crates.io |\n| --- | --- |\n{rows}\n"
    )
    if mode == "recover":
        steps = [f"publish {', '.join(f'`{n}`' for n in unpublished)}" if unpublished else "publish nothing"]
        steps.append("keep the existing tag" if has_tag else f"create tag `{tag}` on this commit")
        summary += "\nRecovery steps: " + "; ".join(steps) + "; create the GitHub Release if it is missing.\n"
    write_github("GITHUB_STEP_SUMMARY", summary)


def unpublished():
    version, packages, _ = workspace()
    for name in dependency_order(packages):
        if not is_published(name, version):
            print(name)


def packages():
    _, publishable, _ = workspace()
    for name in dependency_order(publishable):
        print(name)


def main():
    commands = {"plan": plan, "unpublished": unpublished, "packages": packages}
    if len(sys.argv) != 2 or sys.argv[1] not in commands:
        print(__doc__, file=sys.stderr)
        return 2
    try:
        commands[sys.argv[1]]()
    except PlanError as err:
        print(f"error: {err}", file=sys.stderr)
        write_github("GITHUB_STEP_SUMMARY", f"## Release plan failed\n\n{err}\n")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
