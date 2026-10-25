#!/usr/bin/env bash
# Voice the demo narration with the ElevenLabs text-to-speech REST API.
#
#   ELEVENLABS_VOICE_ID=<voice id> scripts/video/tts.sh 01 04     # voice segments 01 and 04
#   ELEVENLABS_VOICE_ID=<voice id> scripts/video/tts.sh --voice   # show the voice's name
#   scripts/video/tts.sh --subscription                           # character usage and limit
#
# The text is docs/VIDEO.md's narration with its pronunciation table applied
# (scripts/video/spoken.py); the subtitles keep the written form. Output:
# breakwater-out/video/voice/segNN.mp3.
#
# The API key is read from the macOS Keychain on every call, never from a file.
# Store it once from a real terminal (the prompt needs a TTY; -U replaces an existing item):
#   security add-generic-password -U -a "$USER" -s elevenlabs-api-key -w
# It is passed to curl as a header read from a file descriptor, so it does not
# appear in the process list, in the shell history or in any output.
#
# Defaults (override with the environment):
#   ELEVENLABS_MODEL=eleven_multilingual_v2  ELEVENLABS_FORMAT=mp3_44100_192
#   ELEVENLABS_STABILITY=0.5  ELEVENLABS_SIMILARITY=0.8  ELEVENLABS_STYLE=0
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"
API=https://api.elevenlabs.io
MODEL="${ELEVENLABS_MODEL:-eleven_multilingual_v2}"
FORMAT="${ELEVENLABS_FORMAT:-mp3_44100_192}"
STABILITY="${ELEVENLABS_STABILITY:-0.5}"
SIMILARITY="${ELEVENLABS_SIMILARITY:-0.8}"
STYLE="${ELEVENLABS_STYLE:-0}"
OUT=breakwater-out/video/voice
mkdir -p "$OUT"

# curl with the API key header supplied through a file descriptor.
api() {
  local key
  key="$(security find-generic-password -s elevenlabs-api-key -w)" \
    || { echo "error: no 'elevenlabs-api-key' item in the Keychain" >&2; exit 2; }
  [ -n "$key" ] || { echo "error: the 'elevenlabs-api-key' Keychain item is empty" >&2; exit 2; }
  curl -sS --fail-with-body -H @<(printf 'xi-api-key: %s\n' "$key") "$@"
}

subscription() {
  local json
  json="$(api "$API/v1/user/subscription")"
  printf '%s' "$json" | python3 -c '
import json, sys
d = json.load(sys.stdin)
used, limit = d.get("character_count"), d.get("character_limit")
print("characters used", used, "of", limit, "(remaining", str(limit - used) + ");", "tier", d.get("tier"))'
}

case "${1:-}" in
  --subscription) subscription; exit 0 ;;
  --voice)
    : "${ELEVENLABS_VOICE_ID:?set ELEVENLABS_VOICE_ID}"
    json="$(api "$API/v1/voices/$ELEVENLABS_VOICE_ID")"
    printf '%s' "$json" | python3 -c '
import json, sys
d = json.load(sys.stdin)
print("voice", d.get("voice_id") + ":", d.get("name"), "(category", str(d.get("category")) + ")")'
    exit 0 ;;
  "") echo "usage: ELEVENLABS_VOICE_ID=<id> $0 SEGMENT... | --voice | --subscription" >&2; exit 2 ;;
esac

: "${ELEVENLABS_VOICE_ID:?set ELEVENLABS_VOICE_ID}"
for seg in "$@"; do
  text="$(python3 scripts/video/spoken.py "$seg" | cut -f2)"
  [ -n "$text" ] || { echo "error: no narration for segment $seg" >&2; exit 2; }
  body="$(python3 -c '
import json, sys
print(json.dumps({
    "text": sys.argv[1],
    "model_id": sys.argv[2],
    "voice_settings": {
        "stability": float(sys.argv[3]),
        "similarity_boost": float(sys.argv[4]),
        "style": float(sys.argv[5]),
    },
}))' "$text" "$MODEL" "$STABILITY" "$SIMILARITY" "$STYLE")"
  api -X POST "$API/v1/text-to-speech/$ELEVENLABS_VOICE_ID?output_format=$FORMAT" \
    -H 'Content-Type: application/json' -H 'Accept: audio/mpeg' \
    --data-binary @<(printf '%s' "$body") -o "$OUT/seg$seg.mp3"
  printf 'seg%s -> %s (%s characters)\n' "$seg" "$OUT/seg$seg.mp3" "${#text}"
done
