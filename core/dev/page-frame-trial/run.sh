#!/usr/bin/env bash
# What a page a model gave can still send out while it runs (DECISIONS Q15). A scratch server serves the
# window's own build plus a test page that has the window's policy and frames /page-frame as the window does;
# headless Chromium opens it; the probe page tries every way out it knows, towards a listener on this computer
# (HTTP on 7390, UDP on 7391), and reports what it saw. What reached the listener is printed after.
# Needs a built core and window (cargo build; pnpm app:build), Chromium and Node. Takes about ten seconds.
# Usage: core/dev/page-frame-trial/run.sh [attr|csp]   (attr: the frame also has sandbox="allow-scripts")
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$HERE/../../.." && pwd)"
MODE="${1:-attr}"
D="$REPO/core/target/debug/demido"
[ -x "$D" ] && [ -f "$REPO/apps/web/dist/index.html" ] || { echo "Build first: cargo build --manifest-path core/Cargo.toml; pnpm app:build" >&2; exit 1; }
SCRATCH="$(mktemp -d "${TMPDIR:-/tmp}/demido-page-frame-XXXXXX")"
cp -r "$REPO/apps/web/dist" "$SCRATCH/ui"
cp "$HERE/parent.html" "$HERE/parent.js" "$SCRATCH/ui/"
cp "$HERE/probe.html" "$SCRATCH/ui/probe.txt"
mkdir "$SCRATCH/home"
python3 -I "$HERE/catch.py" "$SCRATCH/caught.log" &
CATCH=$!
DEMIDO_HOME="$SCRATCH/home" DEMIDO_VAULT=file "$D" serve --listen 127.0.0.1:7397 --ui-dir "$SCRATCH/ui" > "$SCRATCH/server.log" 2>&1 &
SERVER=$!
finish() { kill "$SERVER" "$CATCH" 2>/dev/null || true; wait 2>/dev/null || true; rm -rf "$SCRATCH"; }
trap finish EXIT INT TERM
sleep 2
node "$HERE/cdp-probe.mjs" "$SCRATCH/chrome" "http://127.0.0.1:7397/parent.html?mode=$MODE" 7000 | grep -v "favicon\|parent\.\|probe\.txt\|data:image"
echo "--- reached the listener:"
grep -v listening "$SCRATCH/caught.log" || echo "nothing"
