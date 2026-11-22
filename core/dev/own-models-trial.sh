#!/usr/bin/env bash
# The window on a scratch data folder with models of its own (made-up files that name themselves Qwen3 and
# nothing at all), on port 7399 (or the port given): for trying what changes the models folder (deleting a
# model) without touching the real one, and the window's screens beside a trial that holds 7399. No
# runtime: nothing is loaded. Stays running until stopped.
#
#     core/dev/own-models-trial.sh [port]
set -euo pipefail
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
D="$REPO/core/target/debug/demido"
PORT="${1:-7399}"
[ -x "$D" ] || { echo "Build first: cargo build --manifest-path core/Cargo.toml" >&2; exit 1; }
SCRATCH="$(mktemp -d "${TMPDIR:-/tmp}/demido-own-models-XXXXXX")"
mkdir -p "$SCRATCH/models"
python3 -I - "$SCRATCH/models/Qwen3-1.7B-trial.gguf" <<'PY'
import struct, sys
keys = [("general.architecture", "qwen3"), ("general.basename", "Qwen3"), ("general.size_label", "1.7B"), ("general.finetune", "Instruct")]
b = b"GGUF" + struct.pack("<IQQ", 3, 0, len(keys))
for k, v in keys:
    b += struct.pack("<Q", len(k)) + k.encode() + struct.pack("<IQ", 8, len(v)) + v.encode()
open(sys.argv[1], "wb").write(b + b"\0" * 4096)
PY
head -c 8192 /dev/zero > "$SCRATCH/models/nameless-trial.gguf"
export DEMIDO_HOME="$SCRATCH" DEMIDO_VAULT=file
"$D" serve --listen "127.0.0.1:$PORT" --ui-dir "$REPO/apps/web/dist" > "$SCRATCH/server.log" 2>&1 &
SERVER=$!
finish() { kill "$SERVER" 2>/dev/null || true; wait "$SERVER" 2>/dev/null || true; rm -rf "$SCRATCH"; }
trap finish EXIT INT TERM
sleep 2
echo "Scratch home: $SCRATCH"
echo "Sign in: http://127.0.0.1:$PORT$("$D" open --print 2>/dev/null | sed -n "s#^http://127.0.0.1:$PORT##p" | head -1)"
wait "$SERVER"
