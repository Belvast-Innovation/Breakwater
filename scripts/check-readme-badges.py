#!/usr/bin/env python3
"""Check the version badges at the top of README.md against the repository.

The stack badges for Rust, soroban-sdk, stellar-accounts and proptest are shields.io
dynamic TOML badges that read Cargo.toml on main, so they follow it on their own. This
script checks that they still point at the right Cargo.toml key, and, should one of them
be a static badge again, that its text matches Cargo.toml.

Two badges have no data source shields.io can read and are static:

  Stellar CLI   must show the stellar-cli version installed by .github/workflows/ci.yml
                and link to that release
  Protocol      must read "<P> · tested on <T>", where P is the soroban-sdk major version
                in Cargo.toml (an SDK major version targets the protocol of the same
                number) and the Protocol compatibility section of README.md names
                Protocol T; it links to that section

Exits 0 if every badge matches, 1 with one line per mismatch, 2 if a source cannot be
read. Standard library only; needs Python 3.11 or newer for tomllib.
"""

import re
import sys
import tomllib
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import parse_qs, unquote, urlsplit

ROOT = Path(__file__).resolve().parent.parent
CARGO_TOML_URL = "https://raw.githubusercontent.com/Belvast-Innovation/Breakwater/main/Cargo.toml"


class SourceError(Exception):
    pass


class Badge:
    def __init__(self, href, src, alt, line):
        self.href = href
        self.src = src
        self.alt = alt
        self.line = line
        parts = urlsplit(src)
        self.query = {k: v[0] for k, v in parse_qs(parts.query).items()}
        self.dynamic = parts.path == "/badge/dynamic/toml"
        self.label = self.message = None
        if self.dynamic:
            self.label = self.query.get("label")
        elif parts.path.startswith("/badge/"):
            # Static badge path: <label>-<message>-<color>, where "--" is a literal dash,
            # "__" a literal underscore and "_" a space.
            fields = re.split(r"(?<!-)-(?!-)", parts.path[len("/badge/"):])
            if len(fields) == 3:
                self.label, self.message = (unescape_static(f) for f in fields[:2])


def unescape_static(field):
    field = field.replace("--", "-").replace("__", "\0").replace("_", " ").replace("\0", "_")
    return unquote(field)


class BadgeParser(HTMLParser):
    """Collects every shields.io <img> inside an <a>, with the link and the line."""

    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.href = None
        self.badges = []

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if tag == "a":
            self.href = attrs.get("href")
        elif tag == "img" and attrs.get("src", "").startswith("https://img.shields.io/badge/"):
            self.badges.append(Badge(self.href, attrs["src"], attrs.get("alt", ""), self.getpos()[0]))

    def handle_endtag(self, tag):
        if tag == "a":
            self.href = None


def read(path):
    try:
        return (ROOT / path).read_text()
    except OSError as err:
        raise SourceError(f"cannot read {path}: {err.strerror}") from err


def stellar_cli_version():
    """The `version:` input of the stellar/stellar-cli step in ci.yml."""
    ci = read(".github/workflows/ci.yml")
    match = re.search(
        r"uses:\s*stellar/stellar-cli@\S+.*?\n\s*with:\s*\n(?:\s*#.*\n)*\s*version:\s*['\"]?([0-9][0-9A-Za-z.\-]*)",
        ci,
    )
    if not match:
        raise SourceError(".github/workflows/ci.yml: no `version:` input on the stellar/stellar-cli step")
    return match.group(1)


def requirement_version(spec):
    """The version of a Cargo requirement: "28.0.0", "^28.0.0" or { version = "28.0.0" }."""
    if isinstance(spec, dict):
        spec = spec.get("version")
    if not isinstance(spec, str):
        return None
    return spec.lstrip("^~= ")


def tested_protocol(readme):
    """The highest "Protocol N" the Protocol compatibility section mentions."""
    match = re.search(r"^## Protocol compatibility\n(.*?)(?=^## )", readme, re.S | re.M)
    if not match:
        raise SourceError("README.md: no `## Protocol compatibility` section")
    numbers = [int(n) for n in re.findall(r"Protocol (\d+)", match.group(1))]
    if not numbers:
        raise SourceError("README.md: the Protocol compatibility section names no protocol version")
    return max(numbers)


def main():
    try:
        manifest = tomllib.loads(read("Cargo.toml"))
        readme = read("README.md")
        cli_version = stellar_cli_version()
        tested = tested_protocol(readme)
    except (SourceError, tomllib.TOMLDecodeError) as err:
        print(f"error: {err}", file=sys.stderr)
        return 2

    workspace = manifest.get("workspace", {})
    deps = workspace.get("dependencies", {})
    rust_version = workspace.get("package", {}).get("rust-version")
    sdk_version = requirement_version(deps.get("soroban-sdk"))
    sdk_major = sdk_version.split(".")[0] if sdk_version else None

    parser = BadgeParser()
    parser.feed(readme)
    badges = {}
    for badge in parser.badges:
        if badge.label:
            badges.setdefault(badge.label, badge)

    errors = []
    checked = []

    def fail(badge, label, text):
        where = f"README.md:{badge.line}" if badge else "README.md"
        errors.append(f"{where}: badge '{label}': {text}")

    # label: (JSONPath the dynamic badge must query, text a static badge must show)
    toml_badges = {
        "Rust": ("$.workspace.package['rust-version']", f"{rust_version}+"),
        "soroban-sdk": ("$.workspace.dependencies['soroban-sdk']", sdk_version),
        "stellar-accounts": (
            "$.workspace.dependencies['stellar-accounts']",
            requirement_version(deps.get("stellar-accounts")),
        ),
        "proptest": ("$.workspace.dependencies.proptest", requirement_version(deps.get("proptest"))),
    }
    for label, (query, expected) in toml_badges.items():
        badge = badges.get(label)
        if badge is None:
            fail(None, label, "not found")
        elif badge.dynamic:
            if badge.query.get("url") != CARGO_TOML_URL:
                fail(badge, label, f"reads {badge.query.get('url')!r}, expected {CARGO_TOML_URL!r}")
            elif badge.query.get("query") != query:
                fail(badge, label, f"queries {badge.query.get('query')!r}, expected {query!r}")
            else:
                checked.append(f"{label}: dynamic, {query}")
        elif badge.message != expected:
            fail(badge, label, f"shows {badge.message!r}, but Cargo.toml says {expected!r}")
        else:
            checked.append(f"{label}: static, {badge.message}")

    badge = badges.get("Stellar CLI")
    release = f"https://github.com/stellar/stellar-cli/releases/tag/v{cli_version}"
    if badge is None:
        fail(None, "Stellar CLI", "not found")
    elif badge.message != cli_version:
        fail(badge, "Stellar CLI", f"shows {badge.message!r}, but ci.yml installs stellar-cli {cli_version!r}")
    elif badge.href != release:
        fail(badge, "Stellar CLI", f"links to {badge.href!r}, expected {release!r}")
    elif cli_version not in badge.alt:
        fail(badge, "Stellar CLI", f"alt text {badge.alt!r} does not name {cli_version}")
    else:
        checked.append(f"Stellar CLI: {badge.message} (ci.yml)")

    badge = badges.get("Protocol")
    expected = f"{sdk_major} · tested on {tested}"
    if badge is None:
        fail(None, "Protocol", "not found")
    elif badge.message != expected:
        fail(
            badge,
            "Protocol",
            f"shows {badge.message!r}, expected {expected!r} (soroban-sdk {sdk_version} in Cargo.toml, "
            f"Protocol {tested} in the Protocol compatibility section)",
        )
    elif badge.href != "#protocol-compatibility":
        fail(badge, "Protocol", f"links to {badge.href!r}, expected '#protocol-compatibility'")
    elif f"Protocol {sdk_major}" not in badge.alt or f"Protocol {tested}" not in badge.alt:
        fail(badge, "Protocol", f"alt text {badge.alt!r} does not name Protocol {sdk_major} and Protocol {tested}")
    else:
        checked.append(f"Protocol: {badge.message}")

    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    print("README badges match the repository:")
    print("\n".join(f"  {line}" for line in checked))
    return 0


if __name__ == "__main__":
    sys.exit(main())
