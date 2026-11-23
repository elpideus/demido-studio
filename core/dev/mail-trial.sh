#!/usr/bin/env bash
# Email in the real window (DECISIONS Y), with no real mailbox: a scratch home on port 7399, a stand-in mail server
# on this computer (`demido mail-stand-in`, development builds only), spoken to in the clear
# (DEMIDO_DEV_MAIL_PLAIN=1, development builds only, 127.0.0.1 only), and the providers' stand-in for a model.
# Add the account on the Email screen as "Other": 127.0.0.1:<the IMAP port printed>, 127.0.0.1:<the SMTP port>,
# me@example.org, password stand-in-password.
set -euo pipefail
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
D="$REPO/core/target/debug/demido"
[ -x "$D" ] || { echo "Build first: cargo build --manifest-path core/Cargo.toml" >&2; exit 1; }
SCRATCH="$(mktemp -d "${TMPDIR:-/tmp}/demido-mail-XXXXXX")"
"$D" mail-stand-in > "$SCRATCH/mail.log" 2>&1 &
MAIL=$!
python3 -I "$REPO/core/dev/providers-stand-in.py" 8455 > "$SCRATCH/stand-in.log" 2>&1 &
STAND=$!
export DEMIDO_HOME="$SCRATCH" DEMIDO_VAULT=file DEMIDO_DEV_PROVIDERS=http://127.0.0.1:8455 DEMIDO_DEV_MAIL_PLAIN=1
"$D" serve --listen 127.0.0.1:7399 --ui-dir "$REPO/apps/web/dist" > "$SCRATCH/server.log" 2>&1 &
SERVER=$!
finish() { kill "$SERVER" "$MAIL" "$STAND" 2>/dev/null || true; wait "$SERVER" 2>/dev/null || true; rm -rf "$SCRATCH"; }
trap finish EXIT INT TERM
sleep 2
head -1 "$SCRATCH/mail.log"
echo "Scratch home: $SCRATCH"
echo "Sign in: http://127.0.0.1:7399$("$D" open --print 2>/dev/null | sed -n 's#^http://127.0.0.1:7399##p' | head -1)"
wait "$SERVER"
