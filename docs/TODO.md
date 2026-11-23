# Work list: items 6, 8, 9, 7, 5, 10

The owner's direction, 2026-11-23: do plan items 6, 8, 9, 7, 5 and 10, in that order, while they are away.
Skip what needs them, and do the most that can be done without them. Each box is checked when it is done,
in the same commit as the work. The plan's table (`docs/PLAN.md`, "What is left, piece by piece") says what
each item is; DECISIONS says what is already decided for it.

How each item goes: research first where nothing is researched yet (a note in `docs/research/`), the
decisions written down in DECISIONS, then built and tested, and what is not built or not tried said so.
Marked **(needs you)**: skipped, and left for the owner.

## Item 6: search services that need a key

- [ ] Research the search services that need a key: Brave Search's API first, and the others worth having (what each costs, its free use, its terms, what may be kept of its results). A research note.
- [ ] Decide where they go in the ladder of sources (DECISIONS N6) and write it down.
- [ ] Build: a key for each service kept in the vault (U), added from the window and from `demido keys`; a keyed service asked first when it has a key; what it costs shown, as for providers.
- [ ] Test against a stand-in that answers as the service's documentation shows (as `core/dev/providers-stand-in.py` does for providers). No real request.
- [ ] **(needs you)** The first real search with a key: your key, and maybe money.

## Item 8: the browser a model drives

- [ ] Research: which browser is driven and how (a Chromium of Demido's own over its debugging protocol, or the system's), its licence; a profile of its own that holds none of the person's sign-ins; what the model is shown (text, the page's structure, a picture); what it may do (follow, click, type, fill a form) and at which permission level; how a page's text is kept from giving the model orders. A research note.
- [ ] Decide and write it down (a section of DECISIONS of its own).
- [ ] Build the browser tool: open a page, read it after its scripts have run, follow links; clicking and typing under the permission levels; what the model saw shown in the window.
- [ ] Pages that only show their text after running scripts: reading from the internet (N) falls back to the browser when a page has no text of its own.
- [ ] Tests, with pages served on this computer (as `core/dev/page-frame-trial/`).
- [ ] The way out that stays open in a page a model gave: WebRTC (DECISIONS Q15, IDEAS 142). Measure what can shut it; build it where the webview allows (it may wait for item 10's shell).
- [ ] **(needs you)** Anything in the person's own logged-in accounts.

## Item 9: voice, email, messaging, markets

Voice:
- [ ] Reading an answer aloud in a session (a speaker on each answer), with voice mode's engine.
- [ ] Dictation in the message box (the microphone button), with voice mode's engine.
- [ ] Demido's voice a frame at a time (IDEAS 146): Pocket TTS's parts driven through `ort`, checked against sherpa-onnx's own sound, so the first sound comes sooner.
- [ ] **(needs you)** The second try with your own voice, and V9 (1.5 s more silence for an unsure judgement) to confirm.
- [ ] **(needs you)** What a voice interruption does to a running task (IDEAS 42).

Email:
- [ ] Build Gmail by app password (DECISIONS Y1, research note 26): the preset, the password kept in the vault, reading by IMAP, sending by SMTP, and the email screen in the window made real. What other people write is data; sending asks first and cannot be undone.
- [ ] Test against a mail server stand-in on this computer. No real mailbox.
- [ ] **(needs you)** A real mailbox and its app password.

Messaging (WhatsApp, Telegram, Discord):
- [ ] Finish the research owed (DECISIONS G7 started it): the libraries and their licences, the services' terms, what each risks for the account. A research note.
- [ ] Build what needs no phone and no account: the screens made real over a store of chats; Telegram and Discord bots (G4) tested against stand-ins.
- [ ] **(needs you)** Linking WhatsApp or Telegram as you (codes on your phone; never your phone or its PIN), and making a Discord bot of your own.

Markets:
- [ ] Research price feeds (DECISIONS M7: public exchange feeds first; what there is for stocks and currencies) and the chart (M9: Lightweight Charts, Apache-2.0 with its attribution, or Demido's own). A research note.
- [ ] Build the real Markets screen: Demido's own chart, the indicators worked out on this computer (M2), live prices from a public feed.
- [ ] A Pine-compatible engine of Demido's own (M3, M4, M5): a first part of the language, so that a model can write a script, run it on the chart, read the error and mend it.
- [ ] **(needs you)** TradingView's sign-in (M10), with an account of yours.

## Item 7: what is left of agents and schedules

- [ ] Messages between sessions, shown in the window.
- [ ] The two measurements research note 24 owes: a run's report from a small model (with item 9's email, against the stand-in), and injection inside a run.
- [ ] The schedules' desktop notice: done with item 10's shell.
- [ ] **(needs you)** Agents with a model each: on hold by your decision (O13).

## Item 5: tools other people write

- [ ] Research again where note 08 is old: the WebAssembly component runtime (Wasmtime) and its licence, the interface a tool is written to, what a tool may ask for, and how a package is signed and checked. A research note.
- [ ] Decide and write it down (DECISIONS C5 is the direction).
- [ ] Build: the package (manifest, guidance, the WebAssembly part), the runtime, asking before what a tool asks for, the signature check, installing and removing.
- [ ] One tool of Demido's own made as a package, to prove the format.
- [ ] The tools browser, the updater's window, editing a tool's guidance in the app.
- [ ] Bringing in MCP servers and skills once the format stands (C5: "External", "Deprecated").

## Item 10: the desktop shell, the installer, the updater, server mode

- [ ] The desktop shell: Tauri 2, kept thin (DECISIONS C1 to C3): a window, a tray, starting or joining the core. With it: the schedules' notice (item 7), and echo cancelling tried in its webview (V6).
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
