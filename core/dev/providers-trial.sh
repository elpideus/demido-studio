#!/usr/bin/env bash
# The Providers screen and sessions with a model elsewhere, for real, against stand-ins on this computer
# (core/dev/providers-stand-in.py): no key, no money, nothing leaves the computer. A scratch data folder that
# shares the real runtime and models, keys kept in a file (never the system keyring), a server on 7399.
# Stays running until stopped; prints a one-use sign-in link for a browser.
set -euo pipefail
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
D="$REPO/core/target/debug/demido"
REAL="${DEMIDO_REAL_HOME:-$HOME/.local/share/demido}"
[ -x "$D" ] || { echo "Build first: cargo build --manifest-path core/Cargo.toml" >&2; exit 1; }
SCRATCH="$(mktemp -d "${TMPDIR:-/tmp}/demido-providers-XXXXXX")"
[ -d "$REAL/runtimes" ] && ln -s "$REAL/runtimes" "$SCRATCH/runtimes"
[ -d "$REAL/models" ] && ln -s "$REAL/models" "$SCRATCH/models"
python3 -I "$REPO/core/dev/providers-stand-in.py" 8455 > "$SCRATCH/stand-in.log" 2>&1 &
STAND=$!
export DEMIDO_HOME="$SCRATCH" DEMIDO_VAULT=file DEMIDO_DEV_PROVIDERS=http://127.0.0.1:8455
"$D" serve --listen 127.0.0.1:7399 --ui-dir "$REPO/apps/web/dist" > "$SCRATCH/server.log" 2>&1 &
SERVER=$!
finish() { kill "$SERVER" "$STAND" 2>/dev/null || true; wait "$SERVER" 2>/dev/null || true; rm -rf "$SCRATCH"; }
trap finish EXIT INT TERM
sleep 2
echo "Scratch home: $SCRATCH"
echo "Sign in: http://127.0.0.1:7399$("$D" open --print 2>/dev/null | sed -n 's#^http://127.0.0.1:7399##p' | head -1)"
wait "$SERVER"
