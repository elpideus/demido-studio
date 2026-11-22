#!/usr/bin/env bash
# The window on a scratch data folder that shares the real runtime and the real models (as real-home.sh
# does), on port 7398: for trying a screen with real answers, beside a trial that holds 7399. Nothing here
# touches the sessions or settings of the server a person is using. Its models folder is the real one, so
# nothing that deletes a model is tried here (own-models-trial.sh is for that). Stays running until stopped.
set -euo pipefail
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
D="$REPO/core/target/debug/demido"
REAL="${DEMIDO_REAL_HOME:-$HOME/.local/share/demido}"
[ -x "$D" ] || { echo "Build first: cargo build --manifest-path core/Cargo.toml" >&2; exit 1; }
[ -d "$REAL/models" ] || { echo "No models in $REAL/models" >&2; exit 1; }
SCRATCH="$(mktemp -d "${TMPDIR:-/tmp}/demido-real-ui-XXXXXX")"
ln -s "$REAL/runtimes" "$SCRATCH/runtimes"
ln -s "$REAL/models" "$SCRATCH/models"
export DEMIDO_HOME="$SCRATCH" DEMIDO_VAULT=file
"$D" serve --listen 127.0.0.1:7398 --ui-dir "$REPO/apps/web/dist" > "$SCRATCH/server.log" 2>&1 &
SERVER=$!
# The links are removed, never what they lead to.
finish() { kill "$SERVER" 2>/dev/null || true; wait "$SERVER" 2>/dev/null || true; rm -f "$SCRATCH/runtimes" "$SCRATCH/models"; rm -rf "$SCRATCH"; }
trap finish EXIT INT TERM
sleep 2
echo "Scratch home: $SCRATCH"
echo "Sign in: http://127.0.0.1:7398$("$D" open --print 2>/dev/null | sed -n 's#^http://127.0.0.1:7398##p' | head -1)"
wait "$SERVER"
