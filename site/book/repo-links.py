#!/usr/bin/env python3
"""mdBook preprocessor: rewrite repository-relative links in included files.

The chapters include README.md and docs/*.md verbatim, so their links are relative to
the repository root or to docs/. This maps links to files that are chapters of the book
onto those chapters, and every other repository path onto its page on GitHub.
"""

import json
import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
GITHUB = "https://github.com/Belvast-Innovation/Breakwater"

# Repository files that are chapters of the book.
FILES = {
    "docs/ARCHITECTURE.md": "architecture.md",
    "docs/DEVELOPMENT.md": "getting-started.md",
}
# README sections that are included in the book, by heading id.
README_SECTIONS = {
    "problem": "introduction.md",
    "how-it-works": "introduction.md",
    "quick-start": "getting-started.md",
    "writing-a-test": "writing-a-suite.md",
    "attack-packs": "attack-packs.md",
    "testnet-evidence": "testnet-evidence.md",
    "roadmap": "roadmap.md",
}

LINK = re.compile(r"(\]\()([^)\s]+)(\))")
HEADING = re.compile(r"^#{1,6}\s+(.*?)\s*#*\s*$")
FENCE = re.compile(r"^\s*(```|~~~)")
# Include markers left in place when a whole file is included.
MARKER = re.compile(r"^<!-- ANCHOR(_END)?: [\w-]+ -->$")


def slug(text):
    text = re.sub(r"[`*_]|<[^>]+>", "", text).strip().lower()
    return "".join(c if c.isalnum() or c in "-_" else "-" if c == " " else "" for c in text)


def readme_link(fragment):
    if fragment in README_SECTIONS:
        return f"{README_SECTIONS[fragment]}#{fragment}"
    return f"{GITHUB}#{fragment}" if fragment else GITHUB


def rewrite(target, ids):
    if re.match(r"^[a-z][a-z0-9+.-]*:", target):
        return target  # absolute URL or mailto:
    path, _, fragment = target.partition("#")
    if not path:
        return target if fragment in ids else readme_link(fragment)
    if (REPO / "site/book/src" / path).is_file():
        return target  # a chapter of this book
    while path.startswith("../"):
        path = path[3:]
    path = path.removeprefix("./")
    if not (REPO / path).exists() and (REPO / "docs" / path).exists():
        path = f"docs/{path}"
    if path == "README.md":
        return readme_link(fragment)
    if path in FILES:
        return FILES[path] + (f"#{fragment}" if fragment else "")
    kind = "tree" if (REPO / path).is_dir() else "blob"
    return f"{GITHUB}/{kind}/main/{path}" + (f"#{fragment}" if fragment else "")


def process(content):
    lines = content.split("\n")
    ids = {slug(m.group(1)) for line in lines if (m := HEADING.match(line))}
    out, fenced = [], False
    for line in lines:
        if FENCE.match(line):
            fenced = not fenced
        elif MARKER.match(line):
            continue
        elif not fenced:
            line = LINK.sub(lambda m: m.group(1) + rewrite(m.group(2), ids) + m.group(3), line)
        out.append(line)
    return "\n".join(out)


def walk(items):
    for item in items:
        chapter = item.get("Chapter") if isinstance(item, dict) else None
        if chapter:
            chapter["content"] = process(chapter["content"])
            walk(chapter.get("sub_items", []))


def main():
    if len(sys.argv) > 1 and sys.argv[1] == "supports":
        sys.exit(0)
    _context, book = json.load(sys.stdin)
    walk(book.get("items", book.get("sections", [])))
    json.dump(book, sys.stdout)


if __name__ == "__main__":
    main()
