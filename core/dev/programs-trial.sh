#!/usr/bin/env bash
# The Providers screen with the real Claude Code and OpenCode found on this computer, in a scratch data
# folder on port 7399. Keys stay in a file (never the system keyring); OpenCode's own data folders point at
# the scratch folder, so the person's OpenCode history is not touched. Nothing is sent to a model unless a
# message is written in a session, and that is the person's call: Claude Code then uses their plan.
set -euo pipefail
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
D="$REPO/core/target/debug/demido"
REAL="${DEMIDO_REAL_HOME:-$HOME/.local/share/demido}"
[ -x "$D" ] || { echo "Build first: cargo build --manifest-path core/Cargo.toml" >&2; exit 1; }
SCRATCH="$(mktemp -d "${TMPDIR:-/tmp}/demido-programs-XXXXXX")"
[ -d "$REAL/runtimes" ] && ln -s "$REAL/runtimes" "$SCRATCH/runtimes"
[ -d "$REAL/models" ] && ln -s "$REAL/models" "$SCRATCH/models"
for d in data config cache state; do mkdir -p "$SCRATCH/opencode-$d"; done
export DEMIDO_HOME="$SCRATCH" DEMIDO_VAULT=file
export XDG_DATA_HOME="$SCRATCH/opencode-data" XDG_CONFIG_HOME="$SCRATCH/opencode-config" XDG_CACHE_HOME="$SCRATCH/opencode-cache" XDG_STATE_HOME="$SCRATCH/opencode-state"
"$D" serve --listen 127.0.0.1:7399 --ui-dir "$REPO/apps/web/dist" > "$SCRATCH/server.log" 2>&1 &
SERVER=$!
finish() { kill "$SERVER" 2>/dev/null || true; wait "$SERVER" 2>/dev/null || true; rm -rf "$SCRATCH"; }
trap finish EXIT INT TERM
sleep 2
echo "Scratch home: $SCRATCH"
echo "Sign in: http://127.0.0.1:7399$("$D" open --print 2>/dev/null | sed -n 's#^http://127.0.0.1:7399##p' | head -1)"
wait "$SERVER"
