"""Print the text-to-speech version of the narration in docs/VIDEO.md.

    python3 scripts/video/spoken.py            # all segments, one per line: "NN<TAB>text"
    python3 scripts/video/spoken.py 01 04      # only these segments

Applies the "Pronunciation (text-to-speech only)" table of docs/VIDEO.md to
each segment's Narration paragraph. The subtitles keep the written form; only
the voice uses this output. Standard library only.
"""
import os
import re
import sys

ROOT = os.getcwd()
TEXT = open(os.path.join(ROOT, "docs/VIDEO.md"), encoding="utf-8").read()


def narration():
    out = {}
    for m in re.finditer(r"^## (\d\d) · .*?$(.*?)(?=^## |\Z)", TEXT, re.S | re.M):
        n = re.search(r"^Narration: (.+?)\s*$", m.group(2), re.M)
        if n:
            out[m.group(1)] = n.group(1).strip()
    return out


def table():
    section = re.search(r"^## Pronunciation \(text-to-speech only\)$(.*?)(?=^## |\Z)", TEXT, re.S | re.M)
    if not section:
        sys.exit("docs/VIDEO.md has no pronunciation table")
    rows = []
    for line in section.group(1).splitlines():
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if len(cells) != 2 or cells[0] in ("Written", "---") or set(cells[0]) <= {"-"}:
            continue
        written = re.sub(r"\s*\(every occurrence\)$", "", cells[0])
        spoken = re.sub(r"\s*\(unchanged\)$", "", cells[1])
        rows.append((written, spoken))
    # Longer written forms first, so "bump_ttl" is replaced before "TTL".
    return sorted(rows, key=lambda r: -len(r[0]))


def speak(text, rows):
    for written, spoken in rows:
        # Whole-token match: not inside a longer number or word.
        text = re.sub(rf"(?<![\w.,]){re.escape(written)}(?![\w]|[.,]\d)", spoken, text)
    return text


if __name__ == "__main__":
    rows = table()
    wanted = set(sys.argv[1:])
    for seg, line in narration().items():
        if not wanted or seg in wanted:
            print(f"{seg}\t{speak(line, rows)}")
