"""Render the still images used by the demo video.

Writes 1920x1080 PNGs (and one tall PNG for a vertical pan) to
breakwater-out/video/stills/:

  deck-1/3/5/7.png                     pages of docs/deck/breakwater-pitch.pdf
  code-suite.png                       the README "Writing a test" suite (tall, panned)
  hl-*.png                             transparent highlight boxes for deck pages 3, 5 and 7

Requires: pymupdf, pygments, Pillow. Run from the repository root.
"""
import io
import os
import re
import sys

import pymupdf
from PIL import Image
from pygments import highlight
from pygments.formatters import ImageFormatter
from pygments.formatters.img import FontNotFound
from pygments.lexers import RustLexer

ROOT = os.getcwd()
OUT = os.path.join(ROOT, "breakwater-out/video/stills")
W, H = 1920, 1080
BG = (30, 31, 40)  # matches the Dracula terminal background
os.makedirs(OUT, exist_ok=True)


def deck_pages():
    pdf = pymupdf.open(os.path.join(ROOT, "docs/deck/breakwater-pitch.pdf"))
    for page_no in (1, 3, 5, 7):
        page = pdf[page_no - 1]
        zoom = W / page.rect.width
        pix = page.get_pixmap(matrix=pymupdf.Matrix(zoom, zoom))
        img = Image.open(io.BytesIO(pix.tobytes("png"))).convert("RGB").resize((W, H))
        img.save(os.path.join(OUT, f"deck-{page_no}.png"))


def code_image(source, font_size, font_name="JetBrains Mono"):
    try:
        return _code_image(source, font_size, font_name)
    except FontNotFound:
        return _code_image(source, font_size, "Menlo")  # macOS system monospace


def _code_image(source, font_size, font_name):
    fmt = ImageFormatter(
        style="dracula",
        font_name=font_name,
        font_size=font_size,
        line_numbers=True,
        line_number_bg="#1e1f28",
        line_number_fg="#6272a4",
        line_pad=6,
        image_pad=36,
    )
    return Image.open(io.BytesIO(highlight(source, RustLexer(), fmt))).convert("RGB")


def frame(img, out_name, tall=False):
    """Centre on a 1920-wide dark canvas; full height for stills, natural height for pans."""
    if img.width > W - 120:
        scale = (W - 120) / img.width
        img = img.resize((int(img.width * scale), int(img.height * scale)))
    height = max(H, img.height + 120) if tall else H
    if not tall and img.height > H - 80:
        scale = (H - 80) / img.height
        img = img.resize((int(img.width * scale), int(img.height * scale)))
    canvas = Image.new("RGB", (W, height), BG)
    canvas.paste(img, ((W - img.width) // 2, (height - img.height) // 2 if not tall else 60))
    canvas.save(os.path.join(OUT, out_name))
    return canvas.size


def readme_suite():
    text = open(os.path.join(ROOT, "README.md"), encoding="utf-8").read()
    m = re.search(r"## Writing a test\n.*?```rust\n(.*?)\n```", text, re.S)
    if not m:
        sys.exit("README has no Writing-a-test rust block")
    return m.group(1) + "\n"


def stats_card():
    """Caption card for segment 09.

    The counts come from BREAKWATER_COUNTS ("<commits> <pull requests> <tests> <as-of time>",
    set by make-segments.sh just before the GitHub page is recorded) or, without it, from
    docs/PITCH_DECK.md's facts table.
    """
    from PIL import ImageDraw, ImageFont

    if os.environ.get("BREAKWATER_COUNTS"):
        commits, prs, tests, as_of = os.environ["BREAKWATER_COUNTS"].split(" ", 3)
    else:
        text = open(os.path.join(ROOT, "docs/PITCH_DECK.md"), encoding="utf-8").read()

        def count(pattern):
            m = re.search(pattern, text)
            if not m:
                sys.exit(f"docs/PITCH_DECK.md has no row matching {pattern!r}")
            return m.group(1)

        commits = count(r'[|] (\d+) commits on `main`')
        prs = count(r'[|] (\d+) merged pull requests')
        tests = count(r'[|] (\d+) tests passing')
        as_of = None

    lines = [
        f"{commits} commits on main",
        f"{prs} merged pull requests",
        f"{tests} tests passing",
        "CI on every pull request",
    ]
    font = None
    for name in ("InterTight[wght].ttf", "Arial Bold.ttf", "Helvetica.ttc"):
        for base in (os.path.expanduser("~/Library/Fonts"), "/Library/Fonts", "/System/Library/Fonts",
                     "/System/Library/Fonts/Supplemental"):
            try:
                font = ImageFont.truetype(os.path.join(base, name), 46)
                break
            except OSError:
                continue
        if font:
            break
    font = font or ImageFont.load_default()
    card = Image.new("RGBA", (760, 92 * len(lines) + (130 if as_of else 70)), (15, 15, 15, 230))
    draw = ImageDraw.Draw(card)
    for i, line in enumerate(lines):
        draw.rectangle([40, 52 + 92 * i, 52, 86 + 92 * i], fill=(253, 218, 36, 255))
        draw.text((76, 40 + 92 * i), line, font=font, fill=(255, 255, 255, 255))
    if as_of:
        small = font.font_variant(size=30) if hasattr(font, "font_variant") else font
        draw.text((76, 40 + 92 * len(lines) + 4), f"as of {as_of}", font=small, fill=(190, 190, 190, 255))
        lines.append(f"as of {as_of}")
    card.save(os.path.join(OUT, "stats-card.png"))
    return lines


def highlight_box(page_no, out_name, phrases=(), image_index=None, min_x=0, pad=(14, 12, 12)):
    """A transparent 1920x1080 overlay with a rounded box around part of a deck page.

    The box covers the text from the first phrase to the last one (every line in between,
    in the same column; only matches right of min_x count) and, if image_index is set, that
    image on the page; positions come
    from the PDF, so the box follows the deck when it is rebuilt. make-segments.sh fades it
    in over the still, which leaves the still itself untouched (no zoom, no resampling).
    """
    from PIL import ImageDraw

    pdf = pymupdf.open(os.path.join(ROOT, "docs/deck/breakwater-pitch.pdf"))
    page = pdf[page_no - 1]
    rects = []
    if phrases:
        first = [r for r in page.search_for(phrases[0]) if r.x0 >= min_x][0]
        last = [r for r in page.search_for(phrases[-1]) if r.x0 >= min_x][-1]
        rects += [first, last]
        for block in page.get_text("dict")["blocks"]:
            for line in block.get("lines", []):
                r = pymupdf.Rect(line["bbox"])
                if r.y0 >= first.y0 - 1 and r.y1 <= last.y1 + 1 and abs(r.x0 - first.x0) < 40:
                    rects.append(r)
    if image_index is not None:
        rects.append(pymupdf.Rect(page.get_image_info()[image_index]["bbox"]))
    box = rects[0]
    for r in rects[1:]:
        box |= r
    scale = W / page.rect.width
    side, top, bottom = pad  # pixels around the box: left and right, top, bottom
    x0, y0, x1, y1 = (box.x0 * scale - side, box.y0 * scale - top, box.x1 * scale + side, box.y1 * scale + bottom)
    overlay = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    draw = ImageDraw.Draw(overlay)
    draw.rounded_rectangle([x0, y0, x1, y1], radius=14, outline=(15, 15, 15, 255), width=10)
    draw.rounded_rectangle([x0 + 2, y0 + 2, x1 - 2, y1 - 2], radius=12, outline=(253, 218, 36, 255), width=6)
    overlay.save(os.path.join(OUT, out_name))
    return [round(v) for v in (x0, y0, x1, y1)]


def voice_caption():
    """Small caption for the closing segment: how the narration was produced."""
    from PIL import ImageDraw, ImageFont

    text = "Narration: ElevenLabs voice clone of the author's own voice."
    font = None
    for path in (os.path.expanduser("~/Library/Fonts/InterTight[wght].ttf"),
                 "/System/Library/Fonts/Supplemental/Arial.ttf", "/System/Library/Fonts/Helvetica.ttc"):
        try:
            font = ImageFont.truetype(path, 26)
            break
        except OSError:
            continue
    font = font or ImageFont.load_default()
    left, top, right, bottom = ImageDraw.Draw(Image.new("RGB", (1, 1))).textbbox((0, 0), text, font=font)
    card = Image.new("RGBA", (right - left + 40, bottom - top + 24), (15, 15, 15, 200))
    ImageDraw.Draw(card).text((20 - left, 12 - top), text, font=font, fill=(255, 255, 255, 255))
    card.save(os.path.join(OUT, "voice-caption.png"))
    return text


if __name__ == "__main__":
    print("stats-card.png", stats_card())
    print("voice-caption.png", voice_caption())
    deck_pages()
    print("hl-pmf.png", highlight_box(3, "hl-pmf.png", ("Soroban has static", "stateful fuzzing")))
    print("hl-findings.png", highlight_box(5, "hl-findings.png", ("Daily window", "a day."), pad=(14, 4, 9)))
    print("hl-team.png", highlight_box(7, "hl-team.png", ("github.com/Belvast-Innovation/Breakwater",),
                                       image_index=1, min_x=700, pad=(18, 14, 20)))
    print("code-suite.png", frame(code_image(readme_suite(), 30), "code-suite.png", tall=True))
    print("deck-1/3/5/7.png written")
