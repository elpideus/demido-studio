#!/usr/bin/env bash
# The desktop app's updater, tried with no real release (DECISIONS F10 to F13). A development key of this
# computer's signs the updates (never the project's, which is the owner's to make); a trial build of the shell
# holds its public half; a stand-in for the project's releases, on this computer, announces version 0.2.0 in
# four ways: well signed, changed after signing, signed for another version, signed with another key; then the
# well-signed one with the person's `updates.check` turned off. The shell,
# in a virtual screen with a D-Bus of its own, looks, fetches and checks the update as an install would, and
# replaces nothing (a development build's DEMIDO_DESKTOP_UPDATE_TRIAL). What it said is printed for each.
# Needs: pnpm install (Tauri's command line), cargo build (core), pnpm app:build, Xvfb, dbus-run-session.
# Usage: core/dev/updates-trial.sh   (KEY=<private key file>, default ~/.local/share/demido-dev-keys/updater.key)
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$HERE/../.." && pwd)"
KEY="${KEY:-$HOME/.local/share/demido-dev-keys/updater.key}"
[ -f "$KEY" ] && [ -f "$KEY.pub" ] || { echo "No key at $KEY: make one with  pnpm --filter @demido/desktop tauri signer generate --ci -p '' -w $KEY" >&2; exit 1; }
D="$REPO/core/target/debug/demido"
SCRATCH="$(mktemp -d "${TMPDIR:-/tmp}/demido-updates-XXXXXX")"
TARGET="${TRIAL_TARGET:-$SCRATCH/target}"
tauri() { (cd "$REPO/apps/desktop" && pnpm --silent tauri "$@"); }

echo "Building a trial shell that holds the development key's public half…"
# Only this build may fetch over plain HTTP from this computer: TAURI_CONFIG is merged into its configuration.
(cd "$REPO/apps/desktop/src-tauri" && CARGO_TARGET_DIR="$TARGET" DEMIDO_UPDATE_KEY="$(cat "$KEY.pub")" \
  TAURI_CONFIG='{"plugins":{"updater":{"dangerousInsecureTransportProtocol":true}}}' cargo build --quiet)
SHELL_BIN="$TARGET/debug/demido-desktop"

mkdir -p "$SCRATCH/releases" "$SCRATCH/other"
head -c 300000 /dev/urandom > "$SCRATCH/releases/good.bin"
cp "$SCRATCH/releases/good.bin" "$SCRATCH/releases/changed.bin"
cp "$SCRATCH/releases/good.bin" "$SCRATCH/releases/old.bin"
cp "$SCRATCH/releases/good.bin" "$SCRATCH/releases/foreign.bin"
tauri signer sign -f "$KEY" -p "" --app-version 0.2.0 "$SCRATCH/releases/good.bin" > /dev/null
tauri signer sign -f "$KEY" -p "" --app-version 0.2.0 "$SCRATCH/releases/changed.bin" > /dev/null
printf 'x' | dd of="$SCRATCH/releases/changed.bin" bs=1 seek=1000 conv=notrunc status=none
tauri signer sign -f "$KEY" -p "" --app-version 0.1.5 "$SCRATCH/releases/old.bin" > /dev/null
tauri signer generate --ci -p "" -w "$SCRATCH/other/other.key" > /dev/null
tauri signer sign -f "$SCRATCH/other/other.key" -p "" --app-version 0.2.0 "$SCRATCH/releases/foreign.bin" > /dev/null
for kind in good changed old foreign; do
  python3 -I - "$SCRATCH/releases" "$kind" <<'PY'
import json, sys
folder, kind = sys.argv[1], sys.argv[2]
sig = open(f"{folder}/{kind}.bin.sig").read().strip()
entry = {"signature": sig, "url": f"http://127.0.0.1:7396/{kind}.bin"}
json.dump({"version": "0.2.0", "notes": "A trial.", "pub_date": "2026-11-24T00:00:00Z",
           "platforms": {"linux-x86_64": entry, "windows-x86_64": entry, "darwin-aarch64": entry, "darwin-x86_64": entry}},
          open(f"{folder}/{kind}.json", "w"))
PY
done
python3 -I -m http.server 7396 --bind 127.0.0.1 --directory "$SCRATCH/releases" > "$SCRATCH/releases.log" 2>&1 &
SERVER=$!
Xvfb :78 -screen 0 1024x768x24 -nolisten tcp > "$SCRATCH/xvfb.log" 2>&1 &
XVFB=$!
mkdir "$SCRATCH/home"
export DEMIDO_HOME="$SCRATCH/home" DEMIDO_VAULT=file DEMIDO_CORE="$D" DISPLAY=:78
unset WAYLAND_DISPLAY
"$D" settings set server.address 127.0.0.1:7399 > /dev/null
finish() { "$D" stop > /dev/null 2>&1 || true; kill "$SERVER" "$XVFB" 2>/dev/null || true; wait 2>/dev/null || true; rm -rf "$SCRATCH/home"; }
trap finish EXIT INT TERM
sleep 1
for kind in good changed old foreign; do
  DEMIDO_DESKTOP_UPDATE_TRIAL=1 DEMIDO_DESKTOP_UPDATE_AFTER=2 DEMIDO_DESKTOP_UPDATES_AT="http://127.0.0.1:7396/$kind.json" \
    timeout 15 dbus-run-session -- "$SHELL_BIN" --hidden > "$SCRATCH/shell-$kind.log" 2>&1 || true
  echo "$kind: $(grep -E '^demido-desktop: (version|could not|no newer)' "$SCRATCH/shell-$kind.log" | tail -1 | sed 's/^demido-desktop: //')"
done
"$D" settings set updates.check off > /dev/null
DEMIDO_DESKTOP_UPDATE_TRIAL=1 DEMIDO_DESKTOP_UPDATE_AFTER=2 DEMIDO_DESKTOP_UPDATES_AT="http://127.0.0.1:7396/good.json" \
  timeout 15 dbus-run-session -- "$SHELL_BIN" --hidden > "$SCRATCH/shell-off.log" 2>&1 || true
echo "good, with updates.check off: $(grep -E '^demido-desktop: (version|updates.check|could not)' "$SCRATCH/shell-off.log" | tail -1 | sed 's/^demido-desktop: //')"
echo "requests the stand-in was sent: $(grep -c '"GET' "$SCRATCH/releases.log")"
