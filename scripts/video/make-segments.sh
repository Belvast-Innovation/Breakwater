#!/usr/bin/env bash
# Produce every segment's picture for the demo video as
# breakwater-out/video/segments/segNN.mp4 (1920x1080, 30 fps, no audio).
#
#   scripts/video/make-segments.sh
#
# Needs: vhs (+ ttyd), ffmpeg, node, gh, and a Python with pymupdf, pygments and
# Pillow (set PYTHON if it is not python3). Playwright is installed on first
# use under breakwater-out/video/node.
#
# Each picture is made at least as long as its narration (breakwater-out/video/voice/segNN.*,
# plus 0.5 s; an estimate when there is none yet), so that assemble.sh does not have to
# stretch or pad it. There are no zoom effects: they make the picture shimmer.
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
dur() { ffprobe -v error -show_entries format=duration -of csv=p=0 "$1"; }
VOPTS=(-an -c:v libx264 -preset medium -crf 18 -pix_fmt yuv420p -r 30)

target() { # NN fallback-seconds: narration length + 0.5 s, or the fallback
  local f
  for f in "$OUT/voice/seg$1.m4a" "$OUT/voice/seg$1.mp3"; do
    [ -f "$f" ] && { "$PYTHON" -c 'import sys; print(round(float(sys.argv[1]) + 0.5, 3))' "$(dur "$f")"; return; }
  done
  echo "$2"
}
still() { # png seconds out
  enc -loop 1 -framerate 30 -t "$2" -i "$1" -vf "scale=1920:1080,format=yuv420p" "${VOPTS[@]}" "$3"
}
highlighted() { # png seconds out overlay start: a still with a highlight box fading in at `start`
  enc -loop 1 -framerate 30 -t "$2" -i "$1" -loop 1 -framerate 30 -t "$2" -i "$4" \
    -filter_complex "[0:v]scale=1920:1080,setsar=1[b];[1:v]format=rgba,fade=t=in:st=$5:d=0.4:alpha=1[h];[b][h]overlay=0:0,format=yuv420p" \
    "${VOPTS[@]}" "$3"
}
hold() { # in seconds out: the clip, holding its last frame up to `seconds`
  local pad
  pad=$("$PYTHON" -c 'import sys; print(max(0.0, float(sys.argv[2]) - float(sys.argv[1])) + 0.1)' "$(dur "$1")" "$2")
  enc -i "$1" -vf "fps=30,scale=1920:1080,setsar=1,tpad=stop_mode=clone:stop_duration=$pad,trim=duration=$2,format=yuv420p" "${VOPTS[@]}" "$3"
}

echo "== build the CLI and the example tests (so no compiler output reaches the terminal recordings)"
cargo build -q -p breakwater-cli
cargo test -q -p example-policy-bypass -p example-ttl-archival --no-run 2>/dev/null

echo "== terminal segments (vhs)"
vhs scripts/video/policy-bypass.tape
vhs scripts/video/ttl-archival.tape
vhs scripts/video/ttl-tests.tape
vhs scripts/video/export.tape
rm -rf /tmp/breakwater-demo

echo "== counts for the segment 09 card, as of now (the GitHub page is recorded right after)"
commits=$(git fetch -q origin main && git rev-list --count origin/main)
prs=$(gh pr list --repo Belvast-Innovation/Breakwater --state merged --limit 500 --json number --jq length)
tests=$(cargo test -q --workspace 2>/dev/null | awk '/^test result:/ { n += $4 } END { print n }')
export BREAKWATER_COUNTS="$commits $prs $tests $(date -u '+%Y-%m-%d %H:%M UTC')"
echo "$BREAKWATER_COUNTS"

echo "== stills"
"$PYTHON" scripts/video/stills.py

# Stills are shown for at most about 9 s, each with a highlight box on the part the narration
# is about; the rest of each segment is a recording.
still "$STILLS/deck-1.png" 3 "$RAW/seg01-cover.mp4"
highlighted "$STILLS/deck-3.png" 9 "$RAW/seg01-pmf.mp4" "$STILLS/hl-pmf.png" 1.0
highlighted "$STILLS/deck-5.png" 7 "$RAW/seg04-deck.mp4" "$STILLS/hl-findings.png" 1.0
highlighted "$STILLS/deck-7.png" "$(target 10 9)" "$RAW/seg10-plain.mp4" "$STILLS/hl-team.png" 1.5
# Segment 10: the team slide with the narration credit in the top right, below the header bar.
enc -i "$RAW/seg10-plain.mp4" -i "$STILLS/voice-caption.png" \
  -filter_complex "[0:v][1:v]overlay=W-w-60:96" "${VOPTS[@]}" "$SEG/seg10.mp4"

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
webm readme-problem "$RAW/readme-problem.mp4"
webm report-policy-bypass "$SEG/seg05.mp4"
webm report-policy-bypass-findings "$RAW/seg04-report.mp4"
webm report-ttl-archival "$RAW/report-ttl-archival.mp4"
webm stellar-expert "$SEG/seg08.mp4"
webm github "$RAW/github.mp4"
# Segment 09: the repository page with the counts card (from docs/PITCH_DECK.md) in the top right.
enc -i "$RAW/github.mp4" -i "$STILLS/stats-card.png" \
  -filter_complex "[0:v][1:v]overlay=W-w-60:140:enable='gte(t,1)'" "${VOPTS[@]}" "$SEG/seg09.mp4"

# Segment 01: the cover, the PMF slide (whose subtitle is the narration's first sentence), then
# the README's Problem section on GitHub.
hold "$RAW/readme-problem.mp4" "$("$PYTHON" -c "import sys; print(max(float(sys.argv[1]) - 12, float(sys.argv[2])))" "$(target 01 28)" "$(dur "$RAW/readme-problem.mp4")")" "$RAW/seg01-readme.mp4"
enc -i "$RAW/seg01-cover.mp4" -i "$RAW/seg01-pmf.mp4" -i "$RAW/seg01-readme.mp4" \
  -filter_complex "[0:v]setsar=1[a];[1:v]setsar=1[b];[2:v]setsar=1[c];[a][b][c]concat=n=3:v=1:a=0[v]" \
  -map "[v]" "${VOPTS[@]}" "$SEG/seg01.mp4"

# Segment 06: the export terminal, its last frame held to the narration length.
mv "$SEG/seg06.mp4" "$RAW/seg06.mp4"
hold "$RAW/seg06.mp4" "$("$PYTHON" -c "import sys; print(max(float(sys.argv[1]), float(sys.argv[2])))" "$(target 06 15)" "$(dur "$RAW/seg06.mp4")")" "$SEG/seg06.mp4"

# Segment 04: the findings slide, then the report tour (chart crossing the limit, step #2).
enc -i "$RAW/seg04-deck.mp4" -i "$RAW/seg04-report.mp4" \
  -filter_complex "[0:v]setsar=1[a];[1:v]setsar=1[b];[a][b]concat=n=2:v=1:a=0[v]" -map "[v]" "${VOPTS[@]}" "$SEG/seg04.mp4"

# Segment 07: the terminal run, the report's TTL chart, then the tests for the other two
# invariants.
hold "$SEG/seg07a.mp4" "$(dur "$SEG/seg07a.mp4")" "$RAW/seg07a.mp4"
hold "$SEG/seg07c.mp4" "$(dur "$SEG/seg07c.mp4")" "$RAW/seg07c.mp4"
enc -i "$RAW/seg07a.mp4" -i "$RAW/report-ttl-archival.mp4" -i "$RAW/seg07c.mp4" \
  -filter_complex "[0:v]setsar=1[a];[1:v]setsar=1[b];[2:v]setsar=1[c];[a][b][c]concat=n=3:v=1:a=0[v]" \
  -map "[v]" "${VOPTS[@]}" "$SEG/seg07.mp4"
rm -f "$SEG/seg07a.mp4" "$SEG/seg07c.mp4"
# Segment 03: the terminal recording, its last frame held to the narration length.
mv "$SEG/seg03.mp4" "$RAW/seg03.mp4"
T03=$("$PYTHON" -c "import sys; print(max(float(sys.argv[1]), float(sys.argv[2])))" "$(target 03 15)" "$(dur "$RAW/seg03.mp4")")
hold "$RAW/seg03.mp4" "$T03" "$SEG/seg03.mp4"

echo "== segments"
for f in "$SEG"/seg*.mp4; do
  printf '%s %ss\n' "$(basename "$f")" "$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$f")"
done
