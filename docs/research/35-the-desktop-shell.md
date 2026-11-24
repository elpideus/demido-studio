# 35. The desktop shell, its installers, its updates, and server mode

Written 2026-11-24, directly, for plan item 10. Research note 01 (2026-11-18) chose the shape: a thin Tauri 2
shell beside one core (C1 to C3). This note checks what changed since, reads what the shell needs from Tauri's
own code, and says what was tried. Sections are added as each piece is built.

How sure each line is: **Read** (crates.io, the crate's own source as downloaded, the system's own files, on
24 November 2026), **Tried** (on this computer: CachyOS, KDE Plasma on Wayland, NVIDIA; the shell was run in a
virtual screen, Xvfb, with a D-Bus of its own, so that nothing reached the person's own desktop).

## Versions (Read, crates.io)

`tauri` 2.12.7 (18 November), `tauri-build` 2.7.6, `wry` 0.57.3, `tao` 0.37.3, `tray-icon` 0.26.1,
`tauri-plugin-notification` 2.8.1, `tauri-plugin-single-instance` 2.5.5, `tauri-plugin-opener` 2,
`tauri-plugin-updater` 2.14.1, `@tauri-apps/cli` 2.12.6 (npm). All Apache-2.0 OR MIT (`tao` Apache-2.0). The
same as note 01 found six days ago: nothing new to weigh. The licence check reads the shell's crates too (1,255
packages with the core's and the window's, all fit).

## Where it lives

`apps/desktop`: a package of the window's workspace (for Tauri's command line, from npm) and `src-tauri`, a Cargo
workspace of its own. The core's workspace never links GTK, WebKit or a window (C1), so `cargo test` in `core/`
and the server's build stay as they are; the checks build the shell apart.

## How it reaches the core

It links nothing of the core. It runs the `demido` program beside it (where an installer puts it; `DEMIDO_CORE`
for a development build):

- `demido open --print` gives a one-time sign-in link to a core that runs; the window goes there, and is then
  signed in as a browser would be (K: the cookie). The shell never reads Demido's key.
- With none running, `demido serve` starts one, in a process group of its own, so that it outlives the shell
  (schedules and bots go on with the window closed). The installer's copy of the window's files is handed to it
  (`--ui-dir`).
- `demido events --follow` (new) prints the core's news a line at a time; the shell reads the runs of
  schedules from it for the desktop notices.
- `demido stop` (new; `POST /api/v1/server/stop`) stops the core as the system's stop signal would. On Windows
  a core started without a console cannot be sent Ctrl+C, so this is the only clean way there.

## What the window's page may do (Read, Tauri's source)

- **No command of the shell can be called from the page.** Tauri 2 grants a page nothing unless a capability
  names it, and the core's address is named by none (`"capabilities": []`, no `remote.urls`). So the page is a
  page, as in a browser: a mistake in the window cannot reach the shell's powers, and a page a model gave
  (Q15) even less.
- **It goes nowhere but the core** (`on_navigation`): the shell's own page, the core's address and its `blob:`
  files, `about:srcdoc`. Nothing else, and nothing is opened elsewhere instead: a page a model gave must not be
  able to send the person's own browser to an address. A link that asks for a new window opens in the person's
  browser (`on_new_window`); a sandboxed frame cannot ask for one (Q15: no `allow-popups`).
- **Permissions** (`on_permission_request`, 2.12.7): the microphone for the core's own page (voice mode,
  dictation); camera, screen, place, notices and the rest refused for everybody.
- A flag on every page (`window.__DEMIDO_DESKTOP__`, by `initialization_script`) tells the page it is in the
  app: the page then does not offer the browser's own notices, which would come twice.

## WebRTC in the window (IDEAS 142, Z10)

**Linux (Read):** wry 0.57.3 sets WebGL, Web Audio, the page cache and the user agent, and leaves WebRTC and
MediaStream to WebKitGTK. The system's own description of WebKitGTK 2.54.3 (`/usr/share/gir-1.0/WebKit2-4.1.gir`)
says `enable-webrtc` "Enable WebRTC support for loaded pages", default FALSE, and `enable-media-stream` (the
microphone) default TRUE. The shell sets WebRTC off itself, so that no later default can turn it on, and keeps
MediaStream.

**Linux (Tried, `core/dev/desktop-trial.sh probe`):** the page-frame trial's probe (note 31) in the shell's
window, a page in its frame trying every way out towards listeners on this computer. Nothing reached them. In
this WebKitGTK (Arch's 2.54.3) `RTCPeerConnection` is not there at all, in the window or in the frame, **even
with the setting turned on** (a development build's switch, the control): this build has no WebRTC. So here it
cannot leak; on a system whose WebKitGTK has WebRTC, the setting is what shuts it, and that was **not tried**.
The other ways out were refused as in Chromium (Q15): `sendBeacon` said "true", a picture and a form were
"set" and "submitted", and none of them arrived.

**Windows (not tried):** WebView2 is Chromium and takes its switches. The shell gives it
`--force-webrtc-ip-handling-policy=disable_non_proxied_udp` (UDP only through a proxy: what stopped UDP in the
headless shell, note 31) and `--proxy-server=http://127.0.0.1:9`, a proxy that is not there, so that TCP
(TURN) reaches nothing either. Chromium does not send loopback through a proxy, so the core still answers.
wry's own switches (`msWebOOUI`, `msPdfOOUI`, `msSmartScreenProtection` off) are kept, since giving any replaces
them. Whether WebView2 keeps to them is for a Windows computer to measure.

**macOS (not built):** WKWebView has WebRTC, and no public setting turns it off. The note above each page (Q15)
says so.

## Notices on the desktop (J6)

`tauri-plugin-notification` 2.8.1 reports a click on a notice on desktop systems (`on_action`, "tap"), on Linux
through the notification server's `ActionInvoked` (Read). The shell says what the window says for a run (what a
task gave, a monitor that spoke up, a run that failed or did not run, a schedule that paused itself), only while
the window is not in front, and a click opens the run's session. **Tried** (`core/dev/desktop-trial.sh notice`):
the shell, started hidden, was given one failed run in its news (a canned line: a real run needs a model, and
the graphics card was in use); a stand-in notification server on the private D-Bus (`core/dev/notices-stand-in.py`)
was given "Morning report", "Failed. No model answered.", clicked it, and the shell showed the window on the
run's session. **Not tried:** a real notification server (KDE's, GNOME's, Windows', macOS'), and a real run.

## The tray, closing and quitting

A tray icon with "Open Demido", "Close the app (Demido keeps working)" and "Stop Demido and close". Closing the
window hides it; the core goes on. `--hidden` starts in the tray only (for starting with the computer). A second
start shows the window that is there (single instance). On Linux the tray needs a desktop that shows
StatusNotifier items (KDE does; GNOME needs an extension), through libayatana-appindicator, which is installed
here: **not seen** on the real desktop, since the trials kept off it.

## Tried, in short

In Xvfb with a private D-Bus, against a scratch home on port 7399: the shell found no core, started one, signed
in and showed the real window (a picture was taken); `demido stop` stopped it; the probe and the notice as above.
**Not tried:** the window on the real Wayland desktop with the NVIDIA card (note 01's gate: frame times, blur,
resizing), the microphone in the shell, the tray on the real desktop, Windows and macOS.

## Installers (Tried, partly)

Tauri's bundler makes them from `tauri.bundle.json`, merged in by the release workflow, with the core (as
Tauri's "sidecar", `binaries/demido-<target>`) and the web UI (`ui/`, a resource) inside. On this computer a
`.deb` was made (with a development build of the core inside, so 184 MB): `/usr/bin/demido-desktop` and
`/usr/bin/demido`, the window's files in `/usr/lib/Demido Studio/ui`, depending on bubblewrap,
libwebkit2gtk-4.1-0, libgtk-3-0 and libayatana-appindicator3-1. The core now finds that folder, and macOS'
`Contents/Resources/ui`, by itself. The other formats are made by the checks' machines.

## Updates (Read, `tauri-plugin-updater` 2.14.1's source; Tried)

- `Update::download` fetches the file and checks it before giving it back: a minisign signature (prehashed),
  against the public key the app was built with; then, when `requireSignedVersion` is set, that the version
  written in the signature's trusted comment (which the signature covers) is the one `latest.json` announced.
  Its own words: without that, "anyone able to serve a crafted response can pair an inflated `version` field
  with the `url` and `signature` of an older release and force a downgrade". `Update::install` checks the bytes
  again.
- It installs an AppImage in place, a `.deb` or `.rpm` with `pkexec` (the system's own password prompt), the
  NSIS setup on Windows, the `.app` on macOS. It looks for `<os>-<arch>-<installer>` in `latest.json`, then
  `<os>-<arch>`.
- `tauri build` signs each updater file when given the private key and the public key in its configuration
  (without the public key it stops: "Missing comment in public key", Tried), and writes the version into the
  signature (`version:0.1.0` in its trusted comment, Tried on the `.deb`).

**Tried** (`core/dev/updates-trial.sh`): a trial build of the shell holding a development key's public half,
and a stand-in for the project's releases on this computer announcing 0.2.0. Well signed: fetched, "signed with
the key this build holds, for this version", not installed (a trial). Changed after signing: "The signature
verification failed". Signed for 0.1.5 and announced as 0.2.0: refused, "The update was signed for version
0.1.5 but the update endpoint announced version 0.2.0". Signed with another key: "The signature was created
with a different key than the one provided". With `updates.check` off: nothing was asked (the stand-in saw no
request). **Not tried:** installing (replacing a running app), the project's real key and a real release.

## Server mode (note 05, section 3.5; Tried)

Note 05 compared the ways to reach Demido from elsewhere and gave one verdict without options: "Plain HTTP on a
non-loopback address: Refuse. No flag." Until now the server listened wherever `server.address` said, with a
warning. Now:

- `server.address` must be this computer's own; anything else stops the start, saying how else to be reached.
- `server.remote` is a second listener, HTTPS only, with the person's certificate and key; without them the
  start stops. The handshakes are done apart from the listener, ten seconds each at most, 64 at once.
- The `Host` list covers both listeners: this computer's spellings, the remote address, and `server.names`.
  **Found trying it:** over HTTPS curl speaks HTTP/2, which carries the name as `:authority`, not as a `Host`
  header; the guard saw no name and refused everything. It now reads the authority when there is no `Host`,
  holding it to the same list, with a test.
- The one-time sign-in link can be made for the remote address (`demido open --remote`); its cookie is
  `Secure` there.
- Not as root.

**Tried** on a scratch home (ports 7399 and 7398, a certificate made with openssl for `demido.test`): plain HTTP
on `0.0.0.0` refused; HTTPS without a certificate refused; with one, `health` answered over HTTP/2 and
HTTP/1.1; another name refused both ways; the API without the key refused, with it answered; the link's cookie
came `HttpOnly; SameSite=Strict; Path=/; Secure`; the window's page was served; plain HTTP to the HTTPS port got
no answer; this computer's own plain address went on answering. **Not tried:** a real certificate, another
device on the network, Tailscale Serve, a reverse proxy.
