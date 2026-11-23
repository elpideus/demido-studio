#!/usr/bin/env bash
# Demido's browser in the real window (DECISIONS Z), with no key, no money and no second download from Google:
# a scratch home on port 7399, the providers' stand-in for a model (core/dev/providers-stand-in.py: "open the
# page <address>" in a message makes its model open it), and a stand-in for Chrome for Testing that serves a
# headless shell already downloaded (core/dev/browser-stand-in.py). The pages a model opens are real pages on the
# public internet: use addresses with nothing personal in them.
# Usage: core/dev/browser-trial.sh <chrome-headless-shell-linux64.zip> <its version>
set -euo pipefail
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
D="$REPO/core/target/debug/demido"
ZIP="${1:?the zip of a headless shell}"; VERSION="${2:?its version}"
[ -x "$D" ] || { echo "Build first: cargo build --manifest-path core/Cargo.toml" >&2; exit 1; }
SCRATCH="$(mktemp -d "${TMPDIR:-/tmp}/demido-browser-XXXXXX")"
python3 -I "$REPO/core/dev/providers-stand-in.py" 8455 > "$SCRATCH/stand-in.log" 2>&1 &
STAND=$!
python3 -I "$REPO/core/dev/browser-stand-in.py" 8456 "$ZIP" "$VERSION" > "$SCRATCH/browser-stand-in.log" 2>&1 &
BSTAND=$!
export DEMIDO_HOME="$SCRATCH" DEMIDO_VAULT=file DEMIDO_DEV_PROVIDERS=http://127.0.0.1:8455 DEMIDO_DEV_BROWSER_LIST=http://127.0.0.1:8456/list.json
"$D" serve --listen 127.0.0.1:7399 --ui-dir "$REPO/apps/web/dist" > "$SCRATCH/server.log" 2>&1 &
SERVER=$!
finish() { kill "$SERVER" "$STAND" "$BSTAND" 2>/dev/null || true; wait "$SERVER" 2>/dev/null || true; rm -rf "$SCRATCH"; }
trap finish EXIT INT TERM
sleep 2
echo "Scratch home: $SCRATCH"
echo "Sign in: http://127.0.0.1:7399$("$D" open --print 2>/dev/null | sed -n 's#^http://127.0.0.1:7399##p' | head -1)"
wait "$SERVER"
