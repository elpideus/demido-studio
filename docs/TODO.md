# Work list: items 6, 8, 9, 7, 5, 10

The owner's direction, 2026-11-23: do plan items 6, 8, 9, 7, 5 and 10, in that order, while they are away.
Skip what needs them, and do the most that can be done without them. Each box is checked when it is done,
in the same commit as the work. The plan's table (`docs/PLAN.md`, "What is left, piece by piece") says what
each item is; DECISIONS says what is already decided for it.

How each item goes: research first where nothing is researched yet (a note in `docs/research/`), the
decisions written down in DECISIONS, then built and tested, and what is not built or not tried said so.
Marked **(needs you)**: skipped, and left for the owner.

## How to work (the owner's standing rules)

- Technical decisions are made here. Research first where nothing is researched yet (the next note in `docs/research/`); decisions go in `docs/DECISIONS.md`, progress in `docs/PLAN.md`, ideas in `docs/IDEAS.md`. Say plainly what is not built and what is not tried; never claim more than was tried.
- Never look at previous Demido Studio versions: never clone, read, search or reference the folder `/mnt/Storage/Development/Demido Studio Project - OLD - DO NOT READ OR EVEN OPEN/` or the repositories `elpideus/demido-studio-fourth` and `-third`. When unsure whether something is an old version, ask.
- GPL-3.0-or-later: every dependency must fit (`node scripts/check-licences.mjs`); a new licence name is read before it is added there, with the reason.
- Nothing irreversible and nothing that costs money without the owner: while they are away, skip it and report it.
- One worker: no Workflow tool, no subagents and no other sessions unless the owner asks.
- Every commit signed (SSH; `git log --format=%G?` shows G) and ending with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`; committed and pushed to main as each piece is done and checked. Never a version tag or a release.
- Downloads the work needs are allowed: official sources, cached, few requests.
- Never the owner's API keys, passwords or tokens, never their phone or its PIN; Demido never reads, keeps or passes on Claude Code's or OpenCode's login.
- Before each piece is committed: `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` (in `core/`), `pnpm app:typecheck`, `pnpm app:build`, the licence check.
- The owner's server on 127.0.0.1:7341 runs on their real data in `~/.local/share/demido`, and they use it: disturb neither. Restart it only on a new build, after checking nothing is answering (`sqlite3 -readonly ~/.local/share/demido/demido.db "SELECT count(*) FROM messages WHERE state='answering';"`): kill the pid from `pgrep -x demido` after checking its `/proc/<pid>/cmdline` and that `DEMIDO_HOME` is unset in its environment, then start it from the repository's root with `env -u DEMIDO_HOME setsid nohup core/target/debug/demido serve --ui-dir apps/web/dist > <scratchpad>/server-7341.log 2>&1 < /dev/null &`. Keep a copy of `demido.db` before a new migration.
- Trials on scratch servers on port 7398 or 7399, with `DEMIDO_VAULT=file` (the recipes are in `core/dev/`); a sign-in link with `DEMIDO_HOME=<home> DEMIDO_VAULT=file core/target/debug/demido open --print` (60 s). Never "delete model" on a home whose `models` is linked to the real folder. A scratch home has the internet tools on: test words with nothing personal in them.
- Before a trial on the graphics card or many processor threads, check `nvidia-smi`: while a game or another program holds the card, no model or voice trials; work that needs neither.
- What only Windows or macOS can try goes on a branch (`gh workflow run Checks --ref <branch>`), merged to main once green. Windows code is type-checked and linted here with `cargo xwin check|clippy --target x86_64-pc-windows-msvc`.
- The memory files (`demido-local-test-setup.md` and the rest) say what is installed here and how not to break it.

## Item 6: search services that need a key

- [x] Research the search services that need a key: Brave Search's API first, and the others worth having (what each costs, its free use, its terms, what may be kept of its results). A research note.
- [x] Decide where they go in the ladder of sources (DECISIONS N6) and write it down.
- [x] Build: a key for each service kept in the vault (U), added from the window and from `demido keys`; a keyed service asked first when it has a key; what it costs shown, as for providers.
- [x] Test against a stand-in that answers as the service's documentation shows (as `core/dev/providers-stand-in.py` does for providers). No real request.
- [ ] **(needs you)** The first real search with a key: your key, and maybe money.

## Item 8: the browser a model drives

- [x] Research: which browser is driven and how (a Chromium of Demido's own over its debugging protocol, or the system's), its licence; a profile of its own that holds none of the person's sign-ins; what the model is shown (text, the page's structure, a picture); what it may do (follow, click, type, fill a form) and at which permission level; how a page's text is kept from giving the model orders. A research note.
- [x] Decide and write it down (a section of DECISIONS of its own).
- [x] Build the browser tool: open a page, read it after its scripts have run, follow links; clicking and typing under the permission levels; what the model saw shown in the window.
- [x] Pages that only show their text after running scripts: reading from the internet (N) falls back to the browser when a page has no text of its own.
- [x] Tests, with pages served on this computer (as `core/dev/page-frame-trial/`).
- [x] The way out that stays open in a page a model gave: WebRTC (DECISIONS Q15, IDEAS 142). Measure what can shut it; build it where the webview allows (it may wait for item 10's shell).
- [ ] **(needs you)** Anything in the person's own logged-in accounts.

## Item 9: voice, email, messaging, markets

Voice:
- [ ] Reading an answer aloud in a session (a speaker on each answer), with voice mode's engine.
- [ ] Dictation in the message box (the microphone button), with voice mode's engine.
- [ ] Demido's voice a frame at a time (IDEAS 146): Pocket TTS's parts driven through `ort`, checked against sherpa-onnx's own sound, so the first sound comes sooner.
- [ ] **(needs you)** The second try with your own voice, and V9 (1.5 s more silence for an unsure judgement) to confirm.
- [ ] **(needs you)** What a voice interruption does to a running task (IDEAS 42).

Email:
- [x] Build Gmail by app password (DECISIONS Y1, research note 26): the preset, the password kept in the vault, reading by IMAP, sending by SMTP, and the email screen in the window made real. What other people write is data; sending asks first and cannot be undone.
- [x] Test against a mail server stand-in on this computer. No real mailbox.
- [ ] **(needs you)** A real mailbox and its app password.

Messaging (WhatsApp, Telegram, Discord):
- [x] Finish the research owed (DECISIONS G7 started it): the libraries and their licences, the services' terms, what each risks for the account. A research note.
- [x] Build what needs no phone and no account: the screens made real over a store of chats; Telegram and Discord bots (G4) tested against stand-ins.
- [ ] **(needs you)** Linking WhatsApp or Telegram as you (codes on your phone; never your phone or its PIN), and making a Discord bot of your own.

Markets:
- [x] Research price feeds (DECISIONS M7: public exchange feeds first; what there is for stocks and currencies) and the chart (M9: Lightweight Charts, Apache-2.0 with its attribution, or Demido's own). A research note.
- [x] Build the real Markets screen: Demido's own chart, the indicators worked out on this computer (M2), live prices from a public feed.
- [x] A Pine-compatible engine of Demido's own (M3, M4, M5): a first part of the language, so that a model can write a script, run it on the chart, read the error and mend it.
- [ ] **(needs you)** TradingView's sign-in (M10), with an account of yours.

## Item 7: what is left of agents and schedules

- [x] Messages between sessions, shown in the window.
- [ ] The two measurements research note 24 owes: a run's report from a small model (with item 9's email, against the stand-in), and injection inside a run.
- [x] The schedules' desktop notice: done with item 10's shell.
- [ ] **(needs you)** Agents with a model each: on hold by your decision (O13).

## Item 5: tools other people write

- [x] Research again where note 08 is old: the WebAssembly component runtime (Wasmtime) and its licence, the interface a tool is written to, what a tool may ask for, and how a package is signed and checked. A research note.
- [x] Decide and write it down (DECISIONS C5 is the direction).
- [x] Build: the package (manifest, guidance, the WebAssembly part), the runtime, asking before what a tool asks for, the signature check, installing and removing.
- [x] One tool of Demido's own made as a package, to prove the format.
- [x] The tools browser, the updater's window, editing a tool's guidance in the app.
- [x] Bringing in MCP servers and skills once the format stands (C5: "External", "Deprecated").

## Item 10: the desktop shell, the installer, the updater, server mode

- [x] The desktop shell: Tauri 2, kept thin (DECISIONS C1 to C3): a window, a tray, starting or joining the core. With it: the schedules' notice (item 7), and echo cancelling tried in its webview (V6). *(Echo cancelling is the box below: trying it plays sound aloud and listens to the room.)*
- [ ] **(needs you)** Echo cancelling tried in the shell's webview (V6), and the window on your own desktop (note 01's Linux gate: frame times, blur, resizing with the NVIDIA card).
- [ ] The installer for each system, built by the checks.
- [ ] The updater: what it checks, and an update checked against its signature before anything is replaced.
- [ ] Hardening server mode (research note 05, section 3.5): what may listen where, and never plain HTTP on an address other than this computer's own.
- [ ] **(needs you)** The key releases are signed with, and publishing a release or pushing a version tag.
- [ ] **(needs you)** Signing the Windows and macOS builds, which costs money every year (R7).
- [ ] **(needs you)** Whether the previous Demido checked `elpideus/demido-studio` for updates.

## Needs you, and not part of these items

- [ ] **(needs you)** Search: the two things left out on purpose (a browser's name in place of Demido's; Brave's and Bing's pages, N7).
- [ ] **(needs you)** Claude Code inside Demido: whether the project accepts Anthropic's Commercial Terms, and whether to ask them to confirm the design in writing (U15).
- [ ] **(needs you)** A first real request to a provider, with your own key.
- [ ] **(needs you)** A usage policy for the name and logo (A13).
- [ ] **(needs you)** Whether to run the research sweep again, or go on researching directly.
- [ ] **(needs you)** The eight items in the brand kit's "For your review".
- [ ] **(needs you)** Sixteen places in the navigation, or services shown only once connected (IDEAS 58).
- [ ] **(needs you)** The sandbox on Windows on a real Windows 10 or 11, with Windows's prompt for an administrator (T27).
