#!/usr/bin/env bash
# Produce every segment's picture for the demo video as
# breakwater-out/video/segments/segNN.mp4 (1920x1080, 30 fps, no audio).
#
#   scripts/video/make-segments.sh
#
# Needs: vhs (+ ttyd), ffmpeg, node, and a Python with pymupdf, pygments and
# Pillow (set PYTHON if it is not python3). Playwright is installed on first
# use under breakwater-out/video/node.
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"
OUT=breakwater-out/video
SEG=$OUT/segments
RAW=$OUT/raw
STILLS=$OUT/stills
PYTHON="${PYTHON:-python3}"
FFMPEG="${FFMPEG:-ffmpeg}"
PLAYWRIGHT_VERSION=1.48.2
mkdir -p "$SEG" "$RAW" "$STILLS"

enc() { "$FFMPEG" -nostdin -hide_banner -loglevel error -y "$@"; }
VOPTS=(-an -c:v libx264 -preset medium -crf 18 -pix_fmt yuv420p -r 30)

echo "== build the CLI and the example tests (so no compiler output reaches the terminal recordings)"
cargo build -q -p breakwater-cli
cargo test -q -p example-policy-bypass -p example-ttl-archival --no-run 2>/dev/null

echo "== terminal segments (vhs)"
vhs scripts/video/policy-bypass.tape
vhs scripts/video/ttl-archival.tape
rm -rf /tmp/breakwater-demo

echo "== stills"
"$PYTHON" scripts/video/stills.py

still() { # png seconds out
  enc -loop 1 -framerate 30 -t "$2" -i "$1" -vf "scale=1920:1080,format=yuv420p" "${VOPTS[@]}" "$3"
}
still "$STILLS/deck-1.png" 8 "$SEG/seg01.mp4"
still "$STILLS/deck-5.png" 12 "$SEG/seg04.mp4"
still "$STILLS/code-repro.png" 10 "$SEG/seg06.mp4"
still "$STILLS/deck-7.png" 10 "$SEG/seg10.mp4"

# Segment 02: slow vertical pan over the tall code image, 2 s hold at each end.
H=$("$PYTHON" -c 'import sys; from PIL import Image; print(Image.open(sys.argv[1]).height)' "$STILLS/code-suite.png")
PAN=18
enc -loop 1 -framerate 30 -t $((PAN + 4)) -i "$STILLS/code-suite.png" \
  -vf "crop=1920:1080:0:'min(max(0,(t-2)/$PAN),1)*($H-1080)',format=yuv420p" "${VOPTS[@]}" "$SEG/seg02.mp4"

echo "== browser segments (playwright)"
NODE_DIR=$OUT/node
if [ ! -d "$NODE_DIR/node_modules/playwright" ]; then
  mkdir -p "$NODE_DIR"
  (cd "$NODE_DIR" && npm init -y >/dev/null && npm install --silent "playwright@$PLAYWRIGHT_VERSION")
fi
# Playwright's bundled Chromium, unless PLAYWRIGHT_CHANNEL=chrome selects an installed Google Chrome.
if [ -z "${PLAYWRIGHT_CHANNEL:-}" ]; then (cd "$NODE_DIR" && npx playwright install chromium); fi
# ES modules resolve packages relative to the script, so run a copy next to node_modules;
# the working directory stays the repository root.
cp scripts/video/record-browser.mjs "$NODE_DIR/record-browser.mjs"
node "$NODE_DIR/record-browser.mjs"

webm() { # name out: drop the page-load lead-in recorded in name.json
  local start length
  start=$("$PYTHON" -c 'import json,sys; print(json.load(open(sys.argv[1]))["start"])' "$RAW/$1.json")
  length=$("$PYTHON" -c 'import json,sys; print(json.load(open(sys.argv[1]))["length"])' "$RAW/$1.json")
  enc -ss "$start" -t "$length" -i "$RAW/$1.webm" -vf "scale=1920:1080,fps=30,format=yuv420p" "${VOPTS[@]}" "$2"
}
webm report-policy-bypass "$SEG/seg05.mp4"
webm report-ttl-archival "$RAW/report-ttl-archival.mp4"
webm stellar-expert "$SEG/seg08.mp4"
webm github "$RAW/github.mp4"
# Segment 09: the repository page with the counts card (from docs/PITCH_DECK.md) in the top right.
enc -i "$RAW/github.mp4" -i "$STILLS/stats-card.png" \
  -filter_complex "[0:v][1:v]overlay=W-w-60:140:enable='gte(t,1)'" "${VOPTS[@]}" "$SEG/seg09.mp4"

# Segment 07: terminal run followed by the report.
enc -i "$SEG/seg07a.mp4" -i "$RAW/report-ttl-archival.mp4" \
  -filter_complex "[0:v]scale=1920:1080,fps=30,format=yuv420p,setsar=1[a];[1:v]setsar=1[b];[a][b]concat=n=2:v=1:a=0[v]" \
  -map "[v]" "${VOPTS[@]}" "$SEG/seg07.mp4"
rm -f "$SEG/seg07a.mp4"
# The terminal recording of segment 03 is re-encoded to the common format.
enc -i "$SEG/seg03.mp4" -vf "scale=1920:1080,fps=30,format=yuv420p" "${VOPTS[@]}" "$SEG/seg03.tmp.mp4" && mv "$SEG/seg03.tmp.mp4" "$SEG/seg03.mp4"

echo "== segments"
for f in "$SEG"/seg*.mp4; do
  printf '%s %ss\n' "$(basename "$f")" "$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$f")"
done
