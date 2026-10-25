"""Render the still images used by the demo video.

Writes 1920x1080 PNGs (and one tall PNG for a vertical pan) to
breakwater-out/video/stills/:

  deck-1.png, deck-5.png, deck-7.png   pages of docs/deck/breakwater-pitch.pdf
  code-suite.png                       the README "Writing a test" suite (tall, panned)
  code-repro.png                       first lines of the exported reproduction

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
    for page_no in (1, 5, 7):
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


def repro_head(lines=25):
    path = os.path.join(ROOT, "examples/policy-bypass/tests/repro_policy_bypass_7.rs")
    return "".join(open(path, encoding="utf-8").readlines()[:lines])


def stats_card():
    """Caption card for segment 09, with counts read from docs/PITCH_DECK.md's facts table."""
    from PIL import ImageDraw, ImageFont

    text = open(os.path.join(ROOT, "docs/PITCH_DECK.md"), encoding="utf-8").read()

    def count(pattern):
        m = re.search(pattern, text)
        if not m:
            sys.exit(f"docs/PITCH_DECK.md has no row matching {pattern!r}")
        return m.group(1)

    lines = [
        f"{count(r'[|] (\d+) commits on `main`')} commits on main",
        f"{count(r'[|] (\d+) merged pull requests')} merged pull requests",
        f"{count(r'[|] (\d+) tests passing')} tests passing",
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
    card = Image.new("RGBA", (760, 92 * len(lines) + 70), (15, 15, 15, 230))
    draw = ImageDraw.Draw(card)
    for i, line in enumerate(lines):
        draw.rectangle([40, 52 + 92 * i, 52, 86 + 92 * i], fill=(253, 218, 36, 255))
        draw.text((76, 40 + 92 * i), line, font=font, fill=(255, 255, 255, 255))
    card.save(os.path.join(OUT, "stats-card.png"))
    return lines


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
    print("code-suite.png", frame(code_image(readme_suite(), 30), "code-suite.png", tall=True))
    print("code-repro.png", frame(code_image(repro_head(), 30), "code-repro.png"))
    print("deck-1/5/7.png written")
