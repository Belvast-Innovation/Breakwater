#!/usr/bin/env bash
# Builds the website into one directory (default: _site at the repository root):
# the landing page and static files from site/, the mdBook in site/book at /docs/,
# and the logo, social preview and HTML reports from docs/.
#
#   site/build.sh [out-dir]
#
# Requires mdbook, python3 (the book's link preprocessor) and curl.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
OUT=${1:-$ROOT/_site}
case "$OUT" in /*) ;; *) OUT="$PWD/$OUT" ;; esac

# Mermaid renders the diagrams in the book. It is served from the site itself, so readers'
# browsers never contact a CDN; the download is pinned by version and checksum.
MERMAID_VERSION=11.4.1
MERMAID_SHA256=a43bc1afd446f9c4cc66ac5dd45d02e8d65e26fc5344ec0ef787f88d6ddb6f9e

rm -rf "$OUT"
mkdir -p "$OUT"
cp -R "$ROOT/site/." "$OUT/"
rm -rf "$OUT/book" "$OUT/build.sh"

mkdir -p "$OUT/assets" "$OUT/reports"
cp "$ROOT/docs/assets/breakwater-logo.png" "$ROOT/docs/assets/social-preview.png" "$OUT/assets/"
cp "$ROOT/docs/reports/"*.html "$OUT/reports/"

mdbook build "$ROOT/site/book" --dest-dir "$OUT/docs"

curl -fsSL "https://cdn.jsdelivr.net/npm/mermaid@$MERMAID_VERSION/dist/mermaid.min.js" -o "$OUT/docs/mermaid.min.js"
python3 - "$OUT/docs/mermaid.min.js" "$MERMAID_SHA256" <<'PY'
import hashlib, sys
digest = hashlib.sha256(open(sys.argv[1], "rb").read()).hexdigest()
if digest != sys.argv[2]:
    sys.exit(f"mermaid.min.js checksum mismatch: {digest}")
PY

echo "site written to $OUT"
