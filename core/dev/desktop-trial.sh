#!/usr/bin/env bash
# The desktop shell, tried without touching the person's own screen (plan item 10, DECISIONS F): in a virtual
# screen (Xvfb) with a D-Bus of its own, against a scratch home on port 7399. With no core running there, the
# shell starts one, signs in and shows it. With `probe`, the window's page is the page-frame trial's
# (core/dev/page-frame-trial): a page a model gave, in its frame, tries every way out, towards listeners on this
# computer, which says whether WebRTC reaches them from the shell's webview (IDEAS 142). A picture of the window
# is kept, and what reached the listeners is printed. With `notice`, the shell is given one run of a schedule
# in its news (a canned `schedule.ran`, through a stand-in for `demido events --follow`: a real run needs a
# model), a stand-in notification server (core/dev/notices-stand-in.py) writes down the notice and clicks it,
# and the window's address afterwards says whether the click opened the run's session.
# Needs: cargo build (core), cargo build (apps/desktop/src-tauri), pnpm app:build, Xvfb, dbus-run-session, import.
# Usage: core/dev/desktop-trial.sh [window|probe|notice] [picture.png]
#   WEBRTC=on   a development build of the shell with WebRTC on: the control, to show the probe would see it.
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$HERE/../.." && pwd)"
MODE="${1:-window}"
PICTURE="${2:-}"
D="$REPO/core/target/debug/demido"
SHELL_BIN="$REPO/apps/desktop/src-tauri/target/debug/demido-desktop"
[ -x "$D" ] && [ -x "$SHELL_BIN" ] && [ -f "$REPO/apps/web/dist/index.html" ] || { echo "Build the core, the shell and the window first." >&2; exit 1; }
SCRATCH="$(mktemp -d "${TMPDIR:-/tmp}/demido-desktop-XXXXXX")"
PICTURE="${PICTURE:-$SCRATCH/window.png}"
mkdir "$SCRATCH/home"
export DEMIDO_HOME="$SCRATCH/home" DEMIDO_VAULT=file DEMIDO_CORE="$D"
"$D" settings set server.address 127.0.0.1:7399 >/dev/null
if [ "$MODE" = probe ]; then
  cp -r "$REPO/apps/web/dist" "$SCRATCH/ui"
  # After signing in the window goes to "/", which is then the probe's parent page.
  cp "$HERE/page-frame-trial/parent.html" "$SCRATCH/ui/index.html"
  cp "$HERE/page-frame-trial/parent.js" "$SCRATCH/ui/"
  cp "$HERE/page-frame-trial/probe.html" "$SCRATCH/ui/probe.txt"
  export DEMIDO_UI_DIR="$SCRATCH/ui"
  python3 -I "$HERE/page-frame-trial/catch.py" "$SCRATCH/caught.log" &
  CATCH=$!
fi
SHELL_CORE="$D"
if [ "$MODE" = notice ]; then
  # The real program for everything but the news, which is one run that failed, six seconds in.
  cat > "$SCRATCH/demido" <<WRAP
#!/usr/bin/env bash
if [ "\$1 \${2:-}" = "events --follow" ]; then
  sleep 6
  echo '{"kind":"schedule.ran","subject":"sch1","data":{"state":"failed","what":"Morning report","note":"No model answered.","session":"0195test-session","late":false,"paused":false}}'
  sleep 60
  exit 0
fi
exec "$D" "\$@"
WRAP
  chmod +x "$SCRATCH/demido"
  SHELL_CORE="$SCRATCH/demido"
fi
DISPLAY_NO=77
Xvfb ":$DISPLAY_NO" -screen 0 1280x900x24 -nolisten tcp > "$SCRATCH/xvfb.log" 2>&1 &
XVFB=$!
finish() {
  "$D" stop > /dev/null 2>&1 || true
  kill "${SHELL_PID:-}" "${CATCH:-}" "$XVFB" 2>/dev/null || true
  wait 2>/dev/null || true
  rm -rf "$SCRATCH/home" "$SCRATCH/ui"
}
trap finish EXIT INT TERM
sleep 1
export DISPLAY=":$DISPLAY_NO"
unset WAYLAND_DISPLAY
cat > "$SCRATCH/in-the-bus.sh" <<RUN
#!/usr/bin/env bash
if [ "$MODE" = notice ]; then python3 -I "$HERE/notices-stand-in.py" "$SCRATCH/notices.log" & sleep 1; fi
DEMIDO_CORE="$SHELL_CORE" DEMIDO_DESKTOP_WEBRTC="${WEBRTC:-off}" exec "$SHELL_BIN" $( [ "$MODE" = notice ] && echo --hidden )
RUN
chmod +x "$SCRATCH/in-the-bus.sh"
dbus-run-session -- "$SCRATCH/in-the-bus.sh" > "$SCRATCH/shell.log" 2>&1 &
SHELL_PID=$!
sleep "${WAIT:-14}"
import -window root -display ":$DISPLAY_NO" "$PICTURE"
echo "core: $("$D" status | head -1)"
echo "shell: $(kill -0 "$SHELL_PID" 2>/dev/null && echo running || echo ended)"
echo "picture: $PICTURE"
grep "^demido-desktop:" "$SCRATCH/shell.log" || true
if [ "$MODE" = notice ]; then
  echo "--- the notification server was given:"
  cat "$SCRATCH/notices.log" 2>/dev/null || echo "nothing"
  grep "^demido-desktop: opened" "$SCRATCH/shell.log" || echo "no session opened"
fi
if [ "$MODE" = probe ]; then
  echo "--- reached the listeners:"
  grep -v listening "$SCRATCH/caught.log" || echo "nothing"
fi
