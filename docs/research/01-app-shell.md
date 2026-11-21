# 01. Application shell and process architecture

Research date: 2026-11-20. Every load-bearing fact has a source URL. Items I could not confirm from a primary source are listed under "Not verified" at the end. The shared web-search budget ran out near the end of this session, so two follow-up checks (Flatpak updater behaviour, prior art of other harnesses) were not completed.

## Question

Pick the application shell and process architecture for Demido Studio so that ONE codebase serves:

- a desktop app on Linux, Windows and macOS
- headless server mode on a VPS, with the same UI in a browser
- a CLI
- an HTTP API
- mobile later (must not be blocked)

It must also give: a lightweight installer that downloads components, in-app updates with optional auto-update, and an in-app browser with tabs that the model can fully drive (click elements or points, screenshots, scripts, network, console, tabs) on desktop AND in server mode.

## Short answer

1. **Process model:** one Rust core, run as a separate daemon process (`demido serve`). Desktop shell, web UI, CLI and third-party integrations are all clients of the same HTTP + WebSocket API. Exactly one process owns the GPU and the model lifecycle. The core is a library first, so it can also be linked in-process later (needed for iOS).
2. **Frontend:** one static SPA, embedded in the core binary and served by the daemon at `/`. The desktop window loads the daemon's URL, so desktop and web are the same origin model and the same code. Native extras sit behind a small `platform` adapter with web fallbacks.
3. **Shell:** Tauri, stable line 2.12.x, kept deliberately thin (window, tray, updater, a few native calls). Linux is the weak point (WebKitGTK). Run a go/no-go test on the dev machine before the first packaged release. If it fails, move the shell to Tauri 3 with the CEF runtime on Linux. Electron is the pre-agreed fallback.
4. **In-app browser:** one engine everywhere. The daemon launches and drives a Chromium (detected system browser, or Chrome for Testing downloaded from Google) over the Chrome DevTools Protocol (CDP). The UI shows it with CDP screencast frames on a canvas and forwards input. Same code on desktop and server. Native embedded tabs are a later upgrade once Tauri's CEF runtime is stable.
5. **Packaging:** small OS packages (deb, rpm, AppImage, AUR; NSIS; DMG) that contain only shell + core. Heavy parts are "components" fetched on first run. Server gets a single binary, an install script and a service installer.
6. **Updater:** Tauri updater plugin for the desktop app (free signing key, GitHub Releases as the endpoint), a signature-checked self-update for the standalone core.
7. **CI:** one tag-triggered workflow, six targets, standard GitHub runners, zero cost. The only spends worth considering are Apple (US$99/year) and, optionally, Windows signing. Everything works unsigned, with worse first-run experience.

## Findings

### F1. Tauri

Versions and license

- `tauri` 2.12.7 released 2026-11-18. License Apache-2.0 OR MIT. https://crates.io/api/v1/crates/tauri and https://v2.tauri.app/release/
- `wry` 0.57.3 and `tao` 0.37.3 (2026-11-18), updater plugin 2.14.1 (2026-10-28), shell plugin 2.6.0. https://v2.tauri.app/release/
- `tauri-action` v1.0.0 (29 June 2026) is the latest. It dropped Tauri v1 support, generates `latest.json` for the updater, and has "initial" Android/iOS build support. https://github.com/tauri-apps/tauri-action/releases

Tauri 3 alpha and the CEF runtime (new since September 2026)

- `tauri` 3.0.0-alpha.0 was published 2026-09-13; latest is 3.0.0-alpha.6 (2026-11-06). No stable 3.0. https://crates.io/api/v1/crates/tauri
- `tauri-runtime-cef` (Chromium Embedded Framework runtime): first published 2026-09-13, latest 3.0.0-alpha.7 on 2026-11-06, license Apache-2.0 OR MIT. https://crates.io/api/v1/crates/tauri-runtime-cef
- Official docs say: "The CEF runtime is new in the Tauri 3.0 alpha and is not available in Tauri 2.0", APIs may change, and "Keep production apps on 2.x". No stable date is given. https://v3.tauri.app/develop/cef/ and https://v3.tauri.app/start/migrate/from-tauri-2/
- In v3 the runtime is chosen in code (`tauri::Builder::runtime(...)`), with `tauri-runtime-wry` (system webview) or `tauri-runtime-cef` (bundled Chromium). One binary runs one runtime. CEF is desktop only; mobile stays on wry. https://v3.tauri.app/develop/webview-runtime/
- CEF costs, per the docs: first build downloads about 1 GB per platform; bundles are "hundreds of megabytes larger"; Windows currently runs unsandboxed; on Linux the sandbox needs unprivileged user namespaces or the setuid helper, which AppImages cannot install. https://v3.tauri.app/develop/cef/
- CEF gains, per the docs: same Chromium on every OS; DevTools Protocol from Rust (`send_dev_tools_message`, `on_dev_tools_protocol`); multi-webview via `WebviewBuilderCefExt` behind the `unstable` feature. Alpha.6 added windowless (off-screen) CEF browsers. https://v3.tauri.app/develop/cef/ and https://github.com/tauri-apps/tauri/releases/tag/tauri-runtime-cef-v3.0.0-alpha.6
- Linux GTK: wry uses GTK 3, CEF uses GTK 4; they cannot share a process. https://v3.tauri.app/start/migrate/from-tauri-2/
- Runtime alpha.7 pins CEF 154.5.0, while the `cef` crate itself is at 156.7.0 (2026-11-19). https://github.com/tauri-apps/tauri/releases/tag/tauri-runtime-cef-v3.0.0-alpha.7 and https://crates.io/api/v1/crates/cef
- Third-party reports, single source each, treat as hints: a draft PR dated 2026-11-04 measured installers about 170 MB larger with CEF, said every update downloads the full bundle, and said no Linux package could be built because `tauri-bundler` 3.0.0-alpha.4 copies libraries that CEF 154 no longer ships. https://github.com/notefig/notefig/pull/513

Tauri 2 features this design depends on

- Sidecars: `bundle.externalBin`, one binary per target triple (`name-$TARGET_TRIPLE`), spawned through the shell plugin with an explicit permission. https://v2.tauri.app/develop/sidecar/
- IPC for remote origins: a capability can list `remote.urls`. Warning from the docs: on Linux and Android Tauri "is unable to distinguish between requests from an embedded `<iframe>` and the window itself". https://v2.tauri.app/security/capabilities/
- Multiple webviews in one window are still behind the `unstable` feature in 2.x. https://v2.tauri.app/reference/javascript/api/namespacewebview
- Mobile: Android and iOS are regular targets on wry. iOS builds need macOS and Xcode. https://v2.tauri.app/start/prerequisites/
- Updater: signature checking is mandatory ("This cannot be disabled"), with Tauri's own key pair (free, not an OS certificate). Update formats: AppImage on Linux, NSIS or MSI on Windows, `.app.tar.gz` on macOS. deb and rpm are not listed. A static `latest.json` on GitHub Releases works as the endpoint. No built-in channels; checks are triggered by app code. https://v2.tauri.app/plugin/updater/
- Windows installers: NSIS installs per-user by default with no admin rights; MSI builds only on Windows and need the VBSCRIPT feature, which is being deprecated; default WebView2 mode adds 0 MB. https://v2.tauri.app/distribute/windows-installer/ and https://v2.tauri.app/start/prerequisites/
- Linux packaging: deb, rpm, AppImage, Snap, AUR and Flatpak guides exist. AppImages must be built on the oldest base you support (Ubuntu 22.04 is the example) and ARM AppImages need an ARM builder. https://v2.tauri.app/distribute/ and https://v2.tauri.app/distribute/appimage/

The Linux problem (WebKitGTK)

- Tauri has an official page for it. Symptoms: blank window, flicker, crash on resize, "Error 71 (Protocol error) dispatching to Wayland display". Cause given: WebKitGTK's DMABUF renderer and the NVIDIA driver. Workarounds in order: `nvidia_drm.modeset=1`, `__NV_DISABLE_EXPLICIT_SYNC=1` (no performance cost), `WEBKIT_DISABLE_DMABUF_RENDERER=1` (loses the fast path), `WEBKIT_DISABLE_COMPOSITING_MODE=1` (last resort). The page warns against shipping the DMABUF override to everyone, and notes WebGL can silently fall back to software. https://v2.tauri.app/develop/debug/linux-graphics/
- The long-running performance issue was closed by a maintainer as "a webkit issue" with nothing Tauri could do. Later comments report roughly half the frame rate of a browser, lag with element shadows, and at least one port to Electron. A maintainer said in April 2025 the team still plans alternatives to WebKitGTK. https://github.com/tauri-apps/tauri/issues/3988
- Microphone capture (`getUserMedia`) on WebKitGTK needs the embedding app to enable media streams and handle the permission request; full WebRTC has needed custom WebKitGTK builds. https://github.com/tauri-apps/tauri/discussions/8426
- Current WebKitGTK stable is the 2.54 series (seen through Fedora advisories; I did not fetch the upstream 2.54.0 post). 2.52 notes list compositing and scrolling improvements. https://webkitgtk.org/2026/03/18/webkitgtk-2.52-highlights.html

The dev machine (CachyOS, RTX 3060, proprietary NVIDIA driver) is exactly the configuration this page is about.

### F2. Electron

- Latest stable 45.4.1 (2026-11-19), Chromium 156.0.8078.100, Node 24.22.3. License MIT. https://releases.electronjs.org/ and https://registry.npmjs.org/electron/latest
- `webContents.debugger` is a built-in CDP transport (attach, sendCommand, message events). https://www.electronjs.org/docs/latest/api/debugger
- Built-in auto-update is Squirrel for macOS and Windows; the free update service requires code-signed macOS builds; the page does not cover Linux. https://www.electronjs.org/docs/latest/tutorial/updates
- No mobile target.

### F3. Wails v3

- v3.0.0 was released 26 October 2026, v3.1.0 on 1 November 2026 (Latest). Linux defaults to GTK 4 + WebKitGTK 6.0. It has "server builds" (same app as an HTTP server, no window), an in-app updater, and experimental iOS/Android. https://github.com/wailsapp/wails/releases
- License MIT. https://raw.githubusercontent.com/wailsapp/wails/master/LICENSE
- It is Go. It still renders through the system webview, so the WebKitGTK problem stays.

### F4. Dioxus

- Latest stable 0.7.10; 0.8.0-alpha.3 on 2026-10-16. License MIT OR Apache-2.0. https://crates.io/api/v1/crates/dioxus and https://github.com/DioxusLabs/dioxus/releases

### F5. Flutter

- 3.47 (August 2026) is the latest release I could confirm; 3.50 is scheduled for November. Multi-window on Linux is still experimental; Impeller became the default desktop renderer in 3.47. https://docs.flutter.dev/release/release-notes/release-notes-3.47.0 and https://flutter.dev/blog/whats-new-in-flutter-3-47

### F6. Newer options

- Electrobun: npm 2.0.7, MIT. Bun main process, system webview by default, CEF optional; on Linux the two renderers cannot be mixed in one app. https://registry.npmjs.org/electrobun/latest and https://blackboard.sh/electrobun/docs/guides/compatability
- `deno desktop`: shipped as experimental in Deno 2.9.0 (25 June 2026), WebView or CEF backends. https://docs.deno.com/runtime/desktop/index.md
- Servo: `servo` crate 0.1.0 on crates.io since 13 April 2026, with an embedding API; the team has not defined 1.0. https://servo.org/blog/2026/04/13/servo-0.1.0-release/
- Lightpanda (headless browser for agents): AGPL-3.0, speaks CDP, but has "No graphical rendering engine", so no real screenshots or screencast. https://github.com/lightpanda-io/browser

### F7. Embedded browser building blocks

- Chrome for Testing (CfT): Stable is 158.0.8127.36 today. Platforms: linux64, linux-arm64 (added in v153), mac-arm64, mac-x64, win32, win64. No win-arm64. Binaries: `chrome`, `chromedriver`, `chrome-headless-shell`. JSON endpoints list versions with download URLs on `storage.googleapis.com`. https://googlechromelabs.github.io/chrome-for-testing/ , https://googlechromelabs.github.io/chrome-for-testing/last-known-good-versions-with-downloads.json and https://github.com/GoogleChromeLabs/chrome-for-testing
- Google says CfT is "not meant to be used for regular browsing". https://developer.chrome.com/docs/automation-and-testing/download-test-binaries.md.txt
- I found no page stating the license terms of the CfT binaries. Google's Chrome terms cover "the executable code version of Chrome". https://www.google.com/intl/en/chrome/terms/
- CDP: `Page.startScreencast`, `screencastFrame` and `screencastFrameAck` are marked experimental; formats are jpeg and png; frames are acknowledged one by one. `Page.captureScreenshot` (jpeg, png, webp), `Page.setInterceptFileChooserDialog` and `Page.javascriptDialogOpening` are stable. https://raw.githubusercontent.com/ChromeDevTools/devtools-protocol/master/pdl/domains/Page.pdl
- `chromiumoxide` 0.9.1 (2026-02-25), MIT OR Apache-2.0, tokio only, full generated CDP types (about 60K lines, slow to compile), can connect to a running browser. Its README lists a launch timeout when Chromium's language is not English. https://crates.io/api/v1/crates/chromiumoxide and https://github.com/mattsse/chromiumoxide
- Prior art for "screencast frames over WebSocket to a canvas, input back through `Input.dispatch*`": a draft PR in kernel-images reports near-zero overhead against about 30% for Xvfb + noVNC (self-reported). https://gh.r-mj.com/kernel/kernel-images/pull/176
- neko: Apache-2.0, streams a Linux desktop (any browser) over WebRTC, Docker-first. https://github.com/m1k1o/neko
- Ubuntu 23.10+ restricts unprivileged user namespaces with AppArmor. Chromium builds without a matching AppArmor profile lose their sandbox ("No usable sandbox"). The Chromium doc lists the fixes (a profile for the binary path, or a sysctl). https://chromium.googlesource.com/chromium/src/+/main/docs/security/apparmor-userns-restrictions.md

### F8. Signing, stores and CI

- macOS: Developer ID signing and notarization need the paid Apple Developer Program, 99 USD per year. A free account cannot notarize. Ad-hoc signing (`signingIdentity: "-"`) satisfies Apple Silicon's "must be signed" rule but users still have to allow the app in Privacy & Security. https://v2.tauri.app/distribute/sign/macos/
- macOS 15 removed the Control-click bypass; the path is System Settings, Privacy & Security, "Open Anyway". https://support.apple.com/guide/mac-help/open-a-mac-app-from-an-unidentified-developer-mh40616/mac
- Homebrew announced that casks failing Gatekeeper checks are disabled from September 2026. https://brew.sh/2025/11/12/homebrew-5.0.0
- Windows: signing is not required to run. Unsigned downloads trigger SmartScreen. Windows 11 Smart App Control blocks unsigned apps with no per-app exception. EV certificates no longer give instant reputation. https://v2.tauri.app/distribute/sign/windows/ and https://support.microsoft.com/en-us/topic/what-is-smart-app-control-285ea03d-fa88-4d56-882e-6698afdb7003
- Azure Artifact Signing (formerly Trusted Signing): Public Trust is open to organizations in the US, Canada, EU, UK and some other countries, but "Individual developers must be located in the United States or Canada". It needs a paid Azure subscription. The public pricing page renders no figures. https://learn.microsoft.com/en-us/azure/artifact-signing/quickstart , https://learn.microsoft.com/en-us/azure/artifact-signing/faq and https://azure.microsoft.com/en-us/pricing/details/artifact-signing/
- SignPath Foundation: free code signing for open-source projects. The certificate names "SignPath Foundation" as publisher. Conditions include an OSI license with no commercial dual-licensing, no proprietary components, builds verified from the public repo, per-release manual approval, MFA, and a published code signing policy. https://signpath.org/terms
- Microsoft Store: registration is free for individuals (since September 2025). MSIX packages are signed by the Store; EXE/MSI submissions must already be signed. https://blogs.windows.com/windowsdeveloper/2025/09/10/free-developer-registration-for-individual-developers-on-microsoft-store/ and https://learn.microsoft.com/en-us/windows/apps/publish/faq/get-started-with-the-microsoft-store
- GitHub Actions: standard hosted runners are free for public repositories; larger runners are billed. https://docs.github.com/en/billing/concepts/product-billing/github-actions
- Runner labels today: `ubuntu-22.04`, `ubuntu-24.04` (= `ubuntu-latest`), `ubuntu-26.04`, the same three with `-arm`, `windows-2025` (= `windows-latest`), `windows-2022`, `windows-11-arm`, `macos-26` (= `macos-latest`, arm64), `macos-15`, `macos-26-intel`, `macos-15-intel`. macOS 14 was retired on 2 November 2026. Policy: at most two GA images and one beta per OS. https://github.com/actions/runner-images
- Artifact attestations (build provenance) are available to public repos on all plans. https://docs.github.com/en/actions/concepts/security/artifact-attestations
- `self_update` crate 1.3.3 (2026-10-27), MIT. https://crates.io/api/v1/crates/self_update

## Options compared

### Shell

| Option | Fits a Rust core | Linux rendering | Installer size | Mobile later | In-app browser on desktop | Maturity today | Verdict |
|---|---|---|---|---|---|---|---|
| Tauri 2.12 (system webview) | Native | WebKitGTK: documented NVIDIA/Wayland failures and lower frame rates | Small (engine not bundled) | Yes, same project (wry) | Screencast view only; child webviews are `unstable` and not CDP-drivable on WebKit | Stable | **Recommended baseline** |
| Tauri 3 alpha + CEF | Native | Chromium, consistent | About +170 MB (one report) | CEF is desktop only, mobile keeps wry | Native tabs with CDP possible (`unstable`) | Alpha, 10 weeks old, Linux packaging reported broken | **Planned Linux upgrade, not today** |
| Electron 45 | Core as child process, TypeScript main process | Chromium, consistent | Large (bundles Chromium) | No | Best today: native views + CDP | Very mature | **Fallback** |
| Wails 3.1 | No (Go) | Same system webview | Small | Experimental | Same limits as Tauri 2 | 4 weeks since 3.0 | Reject |
| Dioxus 0.7 | Yes, but UI must be written in Rust | Not better (see Not verified) | Small | Yes | No | 0.8 in alpha | Reject |
| Flutter 3.47 | FFI bridge to Rust | Own renderer, good | Medium | Best | Weak (needs webviews anyway) | Mature, Linux multi-window experimental | Reject |
| Electrobun, deno desktop | No (JS runtimes) | CEF optional | Medium to large | No | No | Young or experimental | Reject |
| No shell (browser tab or PWA only) | Yes | User's browser, excellent | Zero | Yes (PWA) | Screencast | n/a | Keep as a first-class mode, not the only one |

Why the web-stack UI is not negotiable here: the brief needs Markdown, Mermaid, model-made custom elements, HTML email, HTML/Markdown previews, a code editor and a PDF viewer. All of that is web content. Dioxus and Flutter would embed webviews for most of it anyway.

### Process model

| Option | Pros | Cons |
|---|---|---|
| A. Separate daemon, every client over one API (recommended) | Desktop and server run the identical code path. One owner of VRAM and model state. Schedules and chat bridges survive closing the window. A UI crash or shell update does not kill jobs. CLI attaches to the same state. | Lifecycle work: start, attach, version handshake, clean shutdown. Local port needs auth. |
| B. Core in-process inside the shell | Simplest to start. No port. | Server mode becomes a second code path. Closing the app kills schedules. CLI needs its own core instance, so two processes fight over the GPU. Two IPC styles (Tauri commands and HTTP). |
| C. Both (library-first core, daemon by default, in-process where the OS forces it) | Same as A, plus iOS and tests can link the core. | Must keep the HTTP API as the only contract even in-process. |

Recommendation is C, operated as A on desktop and server.

### In-app browser

| Option | Works on server | Model control | Human view quality | Cost and risk | Verdict |
|---|---|---|---|---|---|
| System webview tabs (wry) | No | Poor: no CDP on WebKitGTK or WKWebView, so no real network or console access | Native | Three engines, three behaviours | Reject |
| Daemon-managed Chromium + CDP, shown by screencast | Yes | Full CDP | Good, not native: no audio, limited frame rate, some native popups missing | One code path; a few weeks to make the viewer good | **Recommended** |
| Same, plus "open in a real window" (headful) on desktop | Desktop only | Full CDP | Native, in a separate window | Small addition | **Recommended add-on** |
| Shell-hosted CEF tabs (Tauri 3) | No | Full CDP | Native, embedded | Alpha today | Later, as a second host |
| Electron views | No | Full CDP | Native, embedded | Requires Electron shell | Only if Electron is chosen |
| Xvfb + VNC, or WebRTC (neko) | Linux only | Separate from the view | VNC: mediocre. WebRTC: smooth, with audio | Extra system packages, Linux only | Optional later for audio/video on Linux servers |
| Servo, Lightpanda | n/a | Incomplete | Servo: not web-compatible enough. Lightpanda: no rendering | n/a | Reject |

## Recommendation

### 1. Process model

- Crates: `demido-core` (library: everything), `demido` (binary: daemon + CLI, no GUI libraries), `demido-desktop` (Tauri shell, ships `demido` as a sidecar).
- Two binaries on purpose. A GUI binary links GTK/WebKitGTK and cannot start on a headless VPS; Windows also separates console and GUI programs.
- Desktop start: the shell looks for a running daemon (state file with pid, port, API level), probes it, and otherwise spawns the sidecar. The daemon binds `127.0.0.1` on a free port and prints a one-time bootstrap token. The shell then opens `http://127.0.0.1:<port>/`.
- Auth is mandatory even on loopback (other local users and web pages can reach the port): token or session cookie, `Host` allow-list against DNS rebinding, `Origin` check on WebSocket and state-changing calls.
- One API for everyone: REST + JSON under `/api/v1`, described by OpenAPI generated from the Rust types; one WebSocket for events and binary streams (browser frames, audio, terminals); OpenAI-compatible `/v1/...` endpoints for "use it as a model manager". The TypeScript client is generated; the CLI uses a Rust client of the same API.
- The CLI never starts a second core next to a running daemon. If none runs, it starts one (with an idle timeout) and attaches. This is what keeps the resource-aware juggling of brief 4.17 correct.
- "Quit" must be explicit: closing the window hides to tray; quitting asks whether the daemon keeps running when schedules or chat bridges exist. `demido service install` registers the daemon with systemd (user or system), launchd or Task Scheduler.
- Version handshake: `/api/v1/meta` returns version and API level. Shell and CLI refuse a mismatched daemon and offer to restart it.

### 2. Same frontend in the shell and in a browser

- A static SPA (no server-side rendering), built once, embedded into the `demido` binary and served at `/`. The UI always matches the core that serves it, so there is no version skew.
- Desktop: the Tauri window navigates to the daemon URL. Web: the user opens the same URL. "Connect to a remote Demido" in the desktop app is the same mechanism with an `https://` URL.
- `platform` adapter in the SPA with two implementations, `tauri` and `web`: notifications, window controls, open-external, updater UI, deep links, global shortcuts. Rule: no feature may require the Tauri side. Anything shell-only is visibly marked in web mode (brief rule: no silent stubs).
- Tauri IPC for the daemon origin: grant a minimal capability (window, notification, updater, opener) to that exact origin. Never grant shell or filesystem plugins to it. Because Tauri on Linux cannot tell an iframe from the window, all untrusted HTML (model-made elements, HTML previews, email bodies) must render in sandboxed iframes on a different origin.
- File picking is a daemon API with an in-app picker, not a native dialog. In server mode, and when the desktop app is attached to a remote server, the files live on the server.
- Browsers only allow the microphone, clipboard API, service workers and passkeys in a secure context (HTTPS or loopback). Server mode therefore needs TLS: built-in ACME when a domain is set, or a reverse proxy, or a private network such as Tailscale, or an SSH tunnel. Default bind stays `127.0.0.1`; binding a public interface without auth and TLS should require an explicit flag.
- The interactive preview the owner asked for (brief 4.33) falls out of this design: the same SPA with a mock transport, runnable in any browser and hostable as a static page.

### 3. Shell

- Use Tauri 2.12.x now. Keep the shell under roughly a thousand lines: window management, tray, single instance, autostart, deep links, updater, sidecar supervision. No product logic.
- Linux mitigations in the shell: detect NVIDIA + Wayland before creating the webview; set `__NV_DISABLE_EXPLICIT_SYNC=1` first; escalate to the DMABUF override only after a failed start (crash marker); show all of it under Settings, Advanced, Rendering, plus a `--safe-rendering` flag. In the design system, animate only `transform` and `opacity`, and use blur sparingly.
- Linux gate, run on the dev machine as soon as the preview exists, with no manual environment variables: the window renders and resizes without crashing; the preview's animations and a long chat scroll hold the display refresh rate without visible stutter; blur and shadows render correctly; microphone capture works. Measure frame times, do not eyeball.
- If the gate fails: build the same shell on Tauri 3 with `tauri-runtime-cef` for Linux only (wry stays on Windows and macOS, selected with `cfg`). Ship that when Linux packaging works upstream. The cost is about 170 MB more on Linux and an alpha dependency.
- If Tauri 3 + CEF is not shippable when the first public Linux release is due: either ship WebKitGTK with a visible "best experience: open in your browser" option, or swap the shell to Electron. The thin shell and the daemon-served UI make that swap small.
- My honest expectation: the gate fails on this hardware, and Linux ends up on CEF. I still would not start on the alpha, because nothing ships for months, Windows and macOS do not need it, and Linux packaging is reported broken right now.

### 4. In-app browser

- `BrowserHost` trait in the core. First implementation: managed Chromium over CDP.
- Which Chromium, in this order, user-configurable: a path the user set; a detected system browser (Edge is always present on Windows, so zero download there); otherwise download Chrome for Testing from Google's official endpoints. Demido must keep a downloaded CfT current itself (check the Stable channel JSON regularly), because CfT does not auto-update and will load arbitrary pages.
- Always a dedicated profile directory under Demido's data folder, never the user's own browser profile.
- Launch with `--remote-debugging-pipe` (no open debugging port), `--user-data-dir`, headless by default. Keep the sandbox on. On Ubuntu 24.04 that needs an AppArmor profile for the binary: `demido setup` should offer to install it (needs sudo, ask first). Never fall back to `--no-sandbox` silently; make it an explicit, warned opt-in.
- Model tools as a small curated set over CDP: list/open/close/activate tabs; navigate; page snapshot as an accessibility tree with element references (cheap for small models); click by reference, click by point as fallback; type, press, scroll; screenshot; evaluate script; network log with bodies on demand; console and exception log; upload and download; dialogs. Plus a permission-gated raw CDP call for power users.
- Human view, identical on desktop and web: `Page.startScreencast` on the visible tab only; the daemon sends raw JPEG bytes as binary WebSocket messages and acknowledges frames to Chromium only as the client keeps up; the UI paints to a canvas and forwards mouse, wheel, keyboard and touch through `Input.dispatch*`; the viewport follows the panel size and device pixel ratio.
- Gaps to plan for, and to state openly in the UI: no audio; frame rate below native; native popups such as `<select>` lists, file choosers, JavaScript dialogs, auth prompts and permission prompts must be intercepted and re-drawn by Demido; clipboard, IME and drag-and-drop need bridging; protected video will likely not play.
- Desktop add-on: "Open in a real window" relaunches the same profile as a normal Chromium window for logins, captchas and passkeys.
- Later: a second host that uses CEF tabs inside the Tauri 3 shell for native rendering. Same tools, because both hosts speak CDP.
- CDP client: start with `chromiumoxide` behind the trait. If its compile time or handler model gets in the way, a thin own client is realistic (CDP is JSON-RPC over a pipe or WebSocket). Do not use its fetcher; write a small downloader against the CfT JSON endpoints.

### 5. Packaging

Desktop packages contain only the shell and the core. Target: tens of megabytes, not hundreds (on Linux this holds only while WebKitGTK is used).

| OS | Formats | Notes |
|---|---|---|
| Linux x86_64, aarch64 | `.deb`, `.rpm`, AppImage, AUR package built from the `.deb` | AppImage is the only self-updating format. Build on Ubuntu 22.04 for old-glibc compatibility. Flatpak and Snap: not in v1 (sandbox conflicts with spawning runtimes, GPU access and the managed browser). |
| Windows x64, arm64 | NSIS `-setup.exe`, per-user, no admin | WebView2 via the default bootstrapper (0 MB). No MSI in v1. |
| macOS arm64, x64 | `.dmg` with the `.app`; `.app.tar.gz` for the updater | Ad-hoc signed unless the Apple spend is approved. |
| Server and CLI, all six targets | `demido-<version>-<target>.tar.gz` or `.zip`, plus checksums and signatures | `install.sh` and `install.ps1` download from the GitHub release, verify, install to the user's bin folder, and offer `demido service install`. A Docker image is a later convenience. |

The "lightweight installer that detects components" is the first-run Setup flow, not the OS installer. One implementation in the core, two faces: the GUI wizard and `demido setup` on a headless server. It detects GPU and existing software and fetches components (inference runtimes, the browser, voice models) from their official sources, driven by a signed component manifest in the repo.

### 6. Updater

- Desktop: `tauri-plugin-updater`, endpoint `https://github.com/<owner>/demido-studio/releases/latest/download/latest.json`, generated by `tauri-action`. Check at launch and on a timer. Default: notify, one click to install. "Install updates automatically" is a setting.
- Channels: a second JSON file for beta, selected at runtime.
- Installs owned by a package manager (deb, rpm, AUR) cannot self-update. The app detects this and shows the new version with instructions instead of a broken button.
- Updating replaces shell and core together (the core is inside the app bundle). Before installing, the shell asks the daemon to drain running work, then stops it, installs and restarts both.
- Standalone core: `demido update` reads the same release, verifies the signature against a public key compiled into the binary, swaps atomically, keeps the previous binary for rollback, restarts the service. Use the `self_update` crate or a small own implementation. Off by default on servers; the UI shows "update available".
- The updater private key is the one secret that must never be lost or leaked: losing it strands every installed copy. Store it in GitHub secrets and offline.

### 7. GitHub Actions release matrix

Trigger: `on: push: tags: ['v*']` plus `workflow_dispatch` with a tag input. Publishing a release in the GitHub UI with a new tag also pushes that tag, so one trigger covers both ways the owner described. A first job creates a draft release if none exists; the last job publishes it, so the updater never sees a half-filled release.

| Target | Runner | Outputs |
|---|---|---|
| `x86_64-unknown-linux-gnu` | `ubuntu-22.04` | core archive; deb, rpm, AppImage |
| `aarch64-unknown-linux-gnu` | `ubuntu-22.04-arm` | core archive; deb, rpm, AppImage |
| `x86_64-pc-windows-msvc` | `windows-2025` | core zip; NSIS installer |
| `aarch64-pc-windows-msvc` | `windows-11-arm` | core zip; NSIS installer |
| `aarch64-apple-darwin` | `macos-26` | core archive; dmg, `.app.tar.gz` |
| `x86_64-apple-darwin` | `macos-26` with `--target x86_64-apple-darwin` (or `macos-26-intel`) | core archive; dmg, `.app.tar.gz` |

Per target: build the SPA, build `demido`, copy it to `src-tauri/binaries/demido-<triple>`, run `tauri-apps/tauri-action@v1` to build, sign for the updater and upload. Final job: `SHA256SUMS`, build provenance attestation, publish the draft, update the AUR package.

Rules: pin every action to a commit SHA; give `contents: write`, `id-token: write` and `attestations: write` only to the release workflow; never expose signing secrets to pull-request builds; pin exact runner labels, not `-latest`.

Secrets needed for an unsigned release (free): `TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.

### 8. What costs money (owner must approve each)

| Item | Cost | What it buys | Without it |
|---|---|---|---|
| GitHub Actions on standard runners, public repo | Free | All builds | n/a |
| GitHub Releases hosting, artifact attestations | Free for public repos | Downloads, updater endpoint, provenance | n/a |
| Tauri updater key | Free | Mandatory update signatures | Updater does not work |
| Linux packages, AUR | Free | n/a | n/a |
| **Apple Developer Program** | **US$99 per year** | Developer ID signing and notarization on macOS; also required for any iOS distribution later | App is ad-hoc signed. Users must approve it under System Settings, Privacy & Security, "Open Anyway". No Homebrew cask. Works, but looks untrustworthy. |
| **Windows signing: SignPath Foundation** | Free, by application | Signed installers, publisher shown as "SignPath Foundation" | SmartScreen warning ("More info", "Run anyway"). Blocked outright on PCs with Smart App Control enforcing. |
| **Windows signing: Azure Artifact Signing** | Paid monthly, needs a paid Azure subscription; exact price not shown on the public page | Signed installers under your own name | Same as above. Individuals qualify only in the US and Canada; elsewhere you need a registered organization. |
| **Windows signing: certificate from a CA** | Paid yearly (not priced here) | Same | Same |
| Microsoft Store listing | Free | Store-signed MSIX | Not planned for v1 (MSIX sandbox rules) |
| Larger GitHub runners | Paid | Faster builds | Not needed |
| Custom domain for install script and docs | Paid yearly, optional | Nicer URLs | Use GitHub URLs |

My advice: approve nothing now. Before the first public macOS build, approve Apple's US$99. Once the repo is public with a first release, apply to SignPath Foundation for Windows.

### 9. License position of the shell parts

- Tauri, wry, tao, plugins, `tauri-action`: Apache-2.0 OR MIT (or MIT), linked. Compatible.
- WebKitGTK and WKWebView are system libraries used dynamically. WebView2 is a proprietary Microsoft runtime that ships with Windows; it is used as a platform component, not redistributed (Tauri's default installer mode downloads it from Microsoft only if missing). This is the standard position for GPL apps on these platforms.
- Chromium or Chrome for Testing: a separate program, downloaded onto the user's machine from the vendor, run as its own process and controlled over CDP. Not linked, not redistributed. The GPL does not reach it.
- CEF (BSD-3-Clause) and Electron (MIT) would be bundled if chosen. Both compatible.

## Risks

1. WebKitGTK on NVIDIA + Wayland can make the Linux desktop app look broken or slow, on the owner's own machine first. Mitigation: the gate and the CEF plan above; the browser UI as an always-available alternative.
2. Tauri 3 and its CEF runtime are alpha with no stable date. The Linux upgrade path may arrive late. Fallback is Electron.
3. CDP screencast is marked experimental and the viewer is real work (popups, clipboard, IME, uploads). It will never feel like a native browser. No audio.
4. Chrome for Testing has no auto-update and Google says it is not for regular browsing. If Demido does not update it, users browse with a stale engine. Its binary license terms are unconfirmed.
5. Chromium's sandbox fails on stock Ubuntu 24.04 for unpackaged binaries. The lazy fix (`--no-sandbox`) is dangerous for a model-driven browser reading hostile pages.
6. A browser on a VPS can reach that server's localhost and private network. The model-driven browser needs network egress rules (block loopback and private ranges by default in server mode).
7. Remote-origin IPC in Tauri plus the iframe caveat on Linux: one mistake exposes native commands to untrusted HTML. Keep the capability minimal and sandbox all foreign HTML.
8. Unsigned builds: macOS first-run friction; Windows Smart App Control blocks them completely.
9. Losing or leaking the updater private key.
10. `ubuntu-22.04` runners will be retired at some point (GitHub keeps two GA images; 26.04 exists). Plan to build inside a Debian 12 or Ubuntu 22.04 container.
11. Loading `http://127.0.0.1` as the main window may need platform settings (for example macOS local-networking transport security). Prove this in the first spike on all three systems.
12. Daemon lifecycle bugs (orphaned daemons, two daemons, stale state file, update while jobs run) are the classic failure of this architecture. Needs a lock, a handshake and tests from day one.
13. GPL-3.0 and Apple's App Store terms have clashed before. The owner can license his own code as he likes, but any third-party GPL code linked into an iOS build would block the store.

## Open questions for the owner

1. Is a Linux desktop package about 150 to 200 MB larger acceptable if that is the price of smooth rendering (bundled Chromium via CEF)?
2. Do you approve US$99 per year for Apple before the first public macOS release? Without it macOS users get a scary first launch.
3. Windows signing: are you fine with "SignPath Foundation" shown as publisher (free), or do you want your own name (paid, and as an individual outside the US and Canada you would need a registered organization for Azure's service)?
4. Is the screencast-based browser view acceptable for v1 (model drives it fully; you watch and can take over; no audio; a real window is one click away on desktop)?
5. When you quit the desktop app, should the daemon keep running by default so schedules and chat bridges fire? I suggest: ask once, remember the answer.
6. Server mode: do you have a domain for the VPS (enables automatic HTTPS), or should the documented default be a private network such as Tailscale or an SSH tunnel?
7. Linux formats for v1: deb, rpm, AppImage and AUR. Is skipping Flatpak and Snap acceptable?
8. Do you want Windows on ARM and Linux on ARM desktop builds from day one, or only ARM for the server binary? (They are free to build; they add test surface.)
9. Mobile: is "installable web app (PWA) against your own server" acceptable as the first mobile step, with native shells later?

## Ideas

- Ship the fake-data preview as the same SPA with a mock transport, and keep it forever as the demo site and the UI test harness. End-to-end tests against `demido serve` then cover web and desktop at once.
- "Take over" and "hand back" for the browser: when the user touches the view, the model's input pauses; a ghost cursor shows where the model clicks. Makes the model's browsing transparent, which matches the project's principles.
- Record browser sessions (frames plus actions) for replay and audit.
- Default page representation for small models: accessibility-tree snapshot with element references, not screenshots. Far fewer tokens, more reliable clicks.
- On Windows, use the installed Edge as the browser engine by default: zero download.
- A rendering diagnostics page (engine, GPU path, frame time test) and a one-click safe mode. Turns "the app is laggy on my distro" reports into data.
- Desktop app as a client of a remote Demido: enter a URL or scan a pairing QR code. The same pairing flow works for phones.
- Sign the component manifest with the same key as updates, and show the source URL and license of every component before download.
- Pushback on the brief: "lightweight installer" and "Apple-level polish on Linux" pull against each other, because the only small option on Linux is WebKitGTK. Decide which wins on Linux (question 1).
- Pushback on the brief: in-app auto-update cannot cover deb, rpm and AUR installs. Say so in the UI instead of pretending.
- Skip MSI, Flatpak, Snap, the Microsoft Store and a Docker image in v1. Each is a support burden with little gain before there are users.

## Not verified

- License terms of the Chrome for Testing binaries (no primary source found).
- Azure Artifact Signing prices (the public pricing page shows no figures); CA certificate prices; domain prices; the Google Play fee.
- Whether Tauri's `remote.urls` patterns accept a port wildcard for `127.0.0.1`, and whether macOS needs a transport-security exception to load `http://127.0.0.1` in the main window. Both belong in the first spike.
- That Dioxus desktop renders through the system webview and that its native renderer is experimental (from memory, not re-checked today).
- Flutter's and Servo's licenses (BSD-3-Clause and MPL-2.0 from memory; neither is recommended, so not re-checked).
- Whether Flutter 3.50 has shipped.
- WebKitGTK 2.54.0 upstream release notes (version seen only through distribution advisories).
- Native Wayland support in Tauri's CEF runtime on Linux (one third-party gist says X11 only).
- Whether the Tauri updater works for an un-notarized macOS app that the user never moved out of the Downloads folder.
- Whether recent Chrome versions refuse remote debugging on the default profile (does not matter here because Demido always uses its own profile).
- The size limit for a single GitHub release asset.
- Wails v3 server-build details (docs page returned 403; only the release notes were read).
