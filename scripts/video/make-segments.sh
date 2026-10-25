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
# plus 0.5 s; an estimate when there is none yet) so that assemble.sh never has to hold a
# frame. Stills and the ends of terminal recordings get a slow push-in instead of a freeze.
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
# Slow push-in over the whole clip, towards (fx, fy) as fractions of the frame. The input
# is upscaled first so the zoom does not jitter.
pushin() { # zoom fx fy
  echo "scale=3840:2160,zoompan=z='1+($1-1)*on/(FRAMES-1)':x='(iw-iw/zoom)*$2':y='(ih-ih/zoom)*$3':d=1:s=1920x1080:fps=30,format=yuv420p"
}
still() { # png seconds out zoom fx fy
  local frames vf
  frames=$("$PYTHON" -c 'import sys; print(int(float(sys.argv[1]) * 30))' "$2")
  vf=$(pushin "$4" "$5" "$6"); vf=${vf//FRAMES/$frames}
  enc -loop 1 -framerate 30 -t "$2" -i "$1" -vf "scale=1920:1080,$vf" "${VOPTS[@]}" "$3"
}
moving() { # in seconds out zoom fx fy: hold the last frame up to `seconds`, push in throughout
  local frames vf pad
  frames=$("$PYTHON" -c 'import sys; print(int(float(sys.argv[1]) * 30))' "$2")
  pad=$("$PYTHON" -c 'import sys; print(max(0.0, float(sys.argv[2]) - float(sys.argv[1])) + 0.1)' "$(dur "$1")" "$2")
  vf=$(pushin "$4" "$5" "$6"); vf=${vf//FRAMES/$frames}
  enc -i "$1" -vf "fps=30,scale=1920:1080,setsar=1,tpad=stop_mode=clone:stop_duration=$pad,trim=duration=$2,$vf" "${VOPTS[@]}" "$3"
}

echo "== build the CLI and the example tests (so no compiler output reaches the terminal recordings)"
cargo build -q -p breakwater-cli
cargo test -q -p example-policy-bypass -p example-ttl-archival --no-run 2>/dev/null

echo "== terminal segments (vhs)"
vhs scripts/video/policy-bypass.tape
vhs scripts/video/ttl-archival.tape
vhs scripts/video/ttl-tests.tape
rm -rf /tmp/breakwater-demo

echo "== counts for the segment 09 card, as of now (the GitHub page is recorded right after)"
commits=$(git fetch -q origin main && git rev-list --count origin/main)
prs=$(gh pr list --repo Belvast-Innovation/Breakwater --state merged --limit 500 --json number --jq length)
tests=$(cargo test -q --workspace 2>/dev/null | awk '/^test result:/ { n += $4 } END { print n }')
export BREAKWATER_COUNTS="$commits $prs $tests $(date -u '+%Y-%m-%d %H:%M UTC')"
echo "$BREAKWATER_COUNTS"

echo "== stills"
"$PYTHON" scripts/video/stills.py

# Segment 01: the cover, then the slide that the narration opens with (page 3).
T01=$(target 01 28)
still "$STILLS/deck-1.png" 6 "$RAW/seg01-cover.mp4" 1.06 0.5 0.5
still "$STILLS/deck-3.png" "$("$PYTHON" -c "print($T01 - 6)")" "$RAW/seg01-gap.mp4" 1.15 0.2 0.35
enc -i "$RAW/seg01-cover.mp4" -i "$RAW/seg01-gap.mp4" \
  -filter_complex "[0:v][1:v]concat=n=2:v=1:a=0[v]" -map "[v]" "${VOPTS[@]}" "$SEG/seg01.mp4"
still "$STILLS/deck-5.png" 7 "$RAW/seg04-deck.mp4" 1.08 0.5 0.4
still "$STILLS/code-repro.png" "$(target 06 15)" "$SEG/seg06.mp4" 1.2 0.1 0.2
still "$STILLS/deck-7.png" "$(target 10 9)" "$RAW/seg10-plain.mp4" 1.08 0.5 0.5
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
webm report-policy-bypass "$SEG/seg05.mp4"
webm report-policy-bypass-findings "$RAW/seg04-report.mp4"
webm report-ttl-archival "$RAW/report-ttl-archival.mp4"
webm stellar-expert "$SEG/seg08.mp4"
webm github "$RAW/github.mp4"
# Segment 09: the repository page with the counts card (from docs/PITCH_DECK.md) in the top right.
enc -i "$RAW/github.mp4" -i "$STILLS/stats-card.png" \
  -filter_complex "[0:v][1:v]overlay=W-w-60:140:enable='gte(t,1)'" "${VOPTS[@]}" "$SEG/seg09.mp4"

# Segment 04: the findings slide, then the report tour (chart crossing the limit, step #2).
enc -i "$RAW/seg04-deck.mp4" -i "$RAW/seg04-report.mp4" \
  -filter_complex "[0:v]setsar=1[a];[1:v]setsar=1[b];[a][b]concat=n=2:v=1:a=0[v]" -map "[v]" "${VOPTS[@]}" "$SEG/seg04.mp4"

# Segment 07: the terminal run, the report's TTL chart, then the tests for the other two
# invariants. The terminal recordings push in towards their output so their ends do not freeze.
moving "$SEG/seg07a.mp4" "$(dur "$SEG/seg07a.mp4")" "$RAW/seg07a.mp4" 1.15 0 0.1
moving "$SEG/seg07c.mp4" "$(dur "$SEG/seg07c.mp4")" "$RAW/seg07c.mp4" 1.3 0 0.05
enc -i "$RAW/seg07a.mp4" -i "$RAW/report-ttl-archival.mp4" -i "$RAW/seg07c.mp4" \
  -filter_complex "[0:v]setsar=1[a];[1:v]setsar=1[b];[2:v]setsar=1[c];[a][b][c]concat=n=3:v=1:a=0[v]" \
  -map "[v]" "${VOPTS[@]}" "$SEG/seg07.mp4"
rm -f "$SEG/seg07a.mp4" "$SEG/seg07c.mp4"
# Segment 03: the terminal recording, held to the narration length with a push-in.
mv "$SEG/seg03.mp4" "$RAW/seg03.mp4"
T03=$("$PYTHON" -c "import sys; print(max(float(sys.argv[1]), float(sys.argv[2])))" "$(target 03 15)" "$(dur "$RAW/seg03.mp4")")
moving "$RAW/seg03.mp4" "$T03" "$SEG/seg03.mp4" 1.15 0 0.1

echo "== segments"
for f in "$SEG"/seg*.mp4; do
  printf '%s %ss\n' "$(basename "$f")" "$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$f")"
done
