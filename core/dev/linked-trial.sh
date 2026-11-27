#!/usr/bin/env bash
# The person's own Telegram and WhatsApp accounts in the real window (DECISIONS G17 to G21), with no real account: a
# scratch home on port 7399 where Telegram and WhatsApp are stand-ins (DEMIDO_DEV_LINKED=stand-in, development builds
# only). Telegram's code is 12345 and its password heron; any api_id, api_hash and number do. WhatsApp links at once.
# Discord is tried by its test, against a stand-in of its REST API and Gateway (`cargo test -p demido-core linked`).
set -euo pipefail
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
D="$REPO/core/target/debug/demido"
[ -x "$D" ] || { echo "Build first: cargo build --manifest-path core/Cargo.toml" >&2; exit 1; }
SCRATCH="$(mktemp -d "${TMPDIR:-/tmp}/demido-linked-XXXXXX")"
export DEMIDO_HOME="$SCRATCH" DEMIDO_VAULT=file DEMIDO_DEV_LINKED=stand-in
"$D" serve --listen 127.0.0.1:7399 --ui-dir "$REPO/apps/web/dist" > "$SCRATCH/server.log" 2>&1 &
SERVER=$!
finish() { kill "$SERVER" 2>/dev/null || true; wait "$SERVER" 2>/dev/null || true; rm -rf "$SCRATCH"; }
trap finish EXIT INT TERM
sleep 2
echo "Scratch home: $SCRATCH"
echo "Sign in: $("$D" open --print 2>/dev/null | head -1)"
wait "$SERVER"
