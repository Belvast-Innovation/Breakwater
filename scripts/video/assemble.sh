#!/usr/bin/env bash
# Assemble the demo video from segment pictures and recorded narration.
#
#   scripts/video/assemble.sh             # needs breakwater-out/video/voice/segNN.m4a
#   scripts/video/assemble.sh --preview   # silent preview, durations estimated from the text
#
# Inputs:  breakwater-out/video/segments/segNN.mp4   (scripts/video/make-segments.sh)
#          breakwater-out/video/voice/segNN.m4a or .mp3 (one recording per segment)
#          docs/VIDEO.md                             (narration, one "Narration:" paragraph per segment)
# Output:  breakwater-out/video/breakwater-demo.mp4  (or breakwater-demo-preview.mp4)
#
# Each segment lasts its narration plus 0.5 s. A longer picture is sped up (at
# most 1.5x) and then trimmed; a shorter one holds its last frame. Segments fade
# in and out over 0.3 s, the narration is normalised to -16 LUFS, and subtitles
# generated from docs/VIDEO.md are burned in. The script fails if the result is
# not between 2:00 and 4:00.
#
# FFMPEG / FFPROBE must point to builds with libass (Homebrew: ffmpeg-full).
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"
FFMPEG="${FFMPEG:-$( [ -x /opt/homebrew/opt/ffmpeg-full/bin/ffmpeg ] && echo /opt/homebrew/opt/ffmpeg-full/bin/ffmpeg || echo ffmpeg )}"
FFPROBE="${FFPROBE:-$( [ -x /opt/homebrew/opt/ffmpeg-full/bin/ffprobe ] && echo /opt/homebrew/opt/ffmpeg-full/bin/ffprobe || echo ffprobe )}"
OUT=breakwater-out/video
SEG=$OUT/segments
VOICE=$OUT/voice
WORK=$OUT/work
PREVIEW=0
[ "${1:-}" = "--preview" ] && PREVIEW=1

FADE=0.3
TAIL=0.5
MAX_SPEEDUP=1.5
WORDS_PER_SECOND=2.5   # preview estimate for unhurried narration

FILTERS="$("$FFMPEG" -hide_banner -filters 2>/dev/null || true)"
grep -q ' subtitles ' <<<"$FILTERS" \
  || { echo "error: $FFMPEG has no 'subtitles' filter (needs libass; Homebrew: brew install ffmpeg-full)" >&2; exit 2; }

rm -rf "$WORK"
mkdir -p "$WORK"
enc() { "$FFMPEG" -nostdin -hide_banner -loglevel error -y "$@"; }
dur() { "$FFPROBE" -v error -show_entries format=duration -of csv=p=0 "$1" </dev/null; }

# Narration per segment, from docs/VIDEO.md.
python3 - docs/VIDEO.md "$WORK/narration.tsv" <<'PY'
import re, sys
text = open(sys.argv[1], encoding="utf-8").read()
rows = []
for m in re.finditer(r"^## (\d\d) · .*?$(.*?)(?=^## |\Z)", text, re.S | re.M):
    n = re.search(r"^Narration: (.+?)\s*$", m.group(2), re.M)
    if n:
        rows.append((m.group(1), n.group(1).strip()))
if not rows:
    sys.exit("no 'Narration:' paragraphs found in docs/VIDEO.md")
with open(sys.argv[2], "w", encoding="utf-8") as f:
    for seg, line in rows:
        f.write(f"{seg}\t{line}\n")
PY

: > "$WORK/concat.txt"
: > "$WORK/durations.tsv"
while IFS=$'\t' read -r id narration; do
  picture="$SEG/seg$id.mp4"
  [ -f "$picture" ] || { echo "error: missing $picture (run scripts/video/make-segments.sh)" >&2; exit 2; }

  if [ "$PREVIEW" = 1 ]; then
    words=$(printf '%s' "$narration" | wc -w | tr -d ' ')
    target=$(python3 -c "print(round(max(6.0, $words / $WORDS_PER_SECOND) + $TAIL, 3))")
    audio_in=(-f lavfi -t "$target" -i "anullsrc=channel_layout=stereo:sample_rate=48000")
  else
    voice="$VOICE/seg$id.m4a"
    [ -f "$voice" ] || voice="$VOICE/seg$id.mp3"
    [ -f "$voice" ] || { echo "error: missing $VOICE/seg$id.m4a or .mp3 (record it, or run with --preview)" >&2; exit 2; }
    target=$(python3 -c "print(round($(dur "$voice") + $TAIL, 3))")
    audio_in=(-i "$voice")
  fi

  vdur=$(dur "$picture")
  vf=$(python3 - "$vdur" "$target" "$MAX_SPEEDUP" "$FADE" <<'PY'
import sys
v, t, cap, fade = map(float, sys.argv[1:])
parts = ["fps=30", "scale=1920:1080", "setsar=1"]
if v > t:
    factor = min(v / t, cap)
    parts.append(f"setpts=PTS/{factor:.4f}")
    v = v / factor
if v < t:
    parts.append(f"tpad=stop_mode=clone:stop_duration={t - v + 0.1:.3f}")
parts.append(f"trim=duration={t:.3f}")
parts.append(f"fade=t=in:st=0:d={fade}")
parts.append(f"fade=t=out:st={t - fade:.3f}:d={fade}")
parts.append("format=yuv420p")
print(",".join(parts))
PY
)
  af="apad,atrim=duration=$target,afade=t=in:st=0:d=$FADE,afade=t=out:st=$(python3 -c "print(round($target - $FADE, 3))"):d=$FADE,aresample=48000"
  enc -i "$picture" "${audio_in[@]}" -map 0:v -map 1:a -vf "$vf" -af "$af" \
    -c:v libx264 -preset medium -crf 18 -r 30 -c:a aac -b:a 192k -ac 2 -ar 48000 -t "$target" "$WORK/seg$id.mp4"
  echo "file 'seg$id.mp4'" >> "$WORK/concat.txt"
  printf '%s\t%s\n' "$id" "$(dur "$WORK/seg$id.mp4")" >> "$WORK/durations.tsv"
  printf 'seg%s  picture %6.2fs  ->  %6.2fs\n' "$id" "$vdur" "$target"
done < "$WORK/narration.tsv"

enc -f concat -safe 0 -i "$WORK/concat.txt" -c copy "$WORK/joined.mp4"

# Subtitles: each segment's sentences share its time in proportion to their length.
python3 - "$WORK/narration.tsv" "$WORK/durations.tsv" "$WORK/subtitles.srt" <<'PY'
import re, sys
narr = dict(l.rstrip("\n").split("\t", 1) for l in open(sys.argv[1], encoding="utf-8"))
durs = [(i, float(d)) for i, d in (l.split("\t") for l in open(sys.argv[2]))]

def ts(s):
    ms = int(round(s * 1000)); h, ms = divmod(ms, 3600000); m, ms = divmod(ms, 60000); sec, ms = divmod(ms, 1000)
    return f"{h:02d}:{m:02d}:{sec:02d},{ms:03d}"

def chunks(text, limit=84):
    out = []
    for sentence in re.split(r"(?<=[.!?:])\s+", text):
        words, line = sentence.split(), ""
        for w in words:
            if len(line) + len(w) + 1 > limit and line:
                out.append(line); line = w
            else:
                line = f"{line} {w}".strip()
        if line: out.append(line)
    return out

cues, start = [], 0.0
for seg, d in durs:
    parts = chunks(narr[seg])
    total = sum(len(p) for p in parts)
    t, end_of_speech = start + 0.15, start + d - 0.5
    for p in parts:
        span = (end_of_speech - (start + 0.15)) * len(p) / total
        cues.append((t, t + span, p)); t += span
    start += d
with open(sys.argv[3], "w", encoding="utf-8") as f:
    for n, (a, b, p) in enumerate(cues, 1):
        f.write(f"{n}\n{ts(a)} --> {ts(b)}\n{p}\n\n")
PY

FINAL=$OUT/breakwater-demo.mp4
AF="loudnorm=I=-16:TP=-1.5:LRA=11,aresample=48000"
if [ "$PREVIEW" = 1 ]; then FINAL=$OUT/breakwater-demo-preview.mp4; AF="anull"; fi
STYLE="FontName=Inter Tight,FontSize=12,PrimaryColour=&H00FFFFFF,BackColour=&H99000000,BorderStyle=3,Outline=5,Shadow=0,MarginV=10"
enc -i "$WORK/joined.mp4" -vf "subtitles=$WORK/subtitles.srt:force_style='$STYLE'" -af "$AF" \
  -c:v libx264 -preset medium -crf 20 -r 30 -pix_fmt yuv420p -movflags +faststart \
  -c:a aac -b:a 192k -ar 48000 -ac 2 "$FINAL"

TOTAL=$(dur "$FINAL")
python3 - "$TOTAL" "$FINAL" <<'PY'
import sys
t, path = float(sys.argv[1]), sys.argv[2]
print(f"{path}: {int(t // 60)}:{t % 60:05.2f} ({t:.2f} s)")
if not 120 <= t <= 240:
    sys.exit(f"error: total duration {t:.1f} s is outside 2:00-4:00")
PY
