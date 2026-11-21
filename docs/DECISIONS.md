# Decisions

Every decision, its reason, and its status. Newest thinking wins; superseded entries are struck, not deleted.

**Status words**

- **Decided**: done and in the repo. Say so if you disagree.
- **Proposed**: built as a proposal; waits for the owner.
- **Provisional**: comes from research that has not been independently fact-checked yet. Do not build on it without the check.
- **Open**: not decided.

Last updated: 2026-11-20 (interactive preview checkpoint).

---

## A. Brand

| # | Decision | Status | Why |
|---|---|---|---|
| A1 | The logo is the owner's mark from `Logo.ai`, rebuilt as stroke-based SVG masters with the geometry untouched (mean pixel difference to the owner's PNG: 0.1%, edge anti-aliasing only). | Decided | It is the owner's design. |
| A2 | A small cut (ring 24, stems 26, eyes +12%) for 16 to 39 px. | Proposed | Below 40 px the master's strokes fall under 2 px. |
| A3 | True neutral greys, dark by default (`#141414` canvas), light complete. | Decided | Matches the owner's screenshot; leaves the accent as the only hue. Warm reads as Claude, cool as generic dev-tool slate (colour lab). |
| A4 | The accent is **Demido green**: one colour, `oklch(0.86 0.15 160)`, shown as `#68efaf` on dark and `#038a5a` on light. It means "a model is awake" and is the focus ring. Never a button or large fill. | Decided (owner: "the colour of Demido is green") | Chosen from nine greens in the real UI and by measurement: clearly green rather than teal, soft beside the round mark, and under red-green colour blindness it stays 11 or more from caution, danger and info on dark (target 8). One near miss: 7.8 against caution on light, covered by glyph shape. Indicative distance to the nearest other brand green (Android) is 7.6. ~~Earlier proposal: magenta hue 338.~~ |
| A4a | **The accent is a setting**: Automatic (default), Demido green, Custom. Automatic follows the system accent whenever the system reports one, from the wallpaper or set by hand; with none it is Demido green. | Decided (owner, 2026-11-20: "any system accent") | Owner: follow the system colour on systems that have one, let users change it, green is the default. Consequence the owner accepted: on machines with a system accent, Demido shows that colour, not its own green. A separate "System accent" choice would now equal Automatic, so there is none. |
| A4b | Whatever colour comes in, Demido keeps its hue and strength and holds its lightness in a band per theme (dark 0.74 to 0.90, light 0.42 to 0.56; text use 0.78 to 0.92 and 0.30 to 0.50). | Decided | A system or custom accent can be very dark or very pale; the "awake" mark and the focus ring must stay visible. Checked with a near-black navy and a pale pink. |
| A5 | Primary action is monochrome (the logo's white or black). | Decided | Keeps colour for signals; an accent-filled Send button fused with the focus ring in tests. |
| A6 | Signals: success, caution, danger, info, each a glyph + word + colour. Success is always Demido green, whatever the accent. Light-theme danger is a deep crimson. | Decided | Caution, danger and info were chosen by grid search for separation under normal, deuteranopic and protanopic vision. With the default accent, "awake" and "success" share the green on purpose (both good news; glyphs differ). A custom accent can collide with a signal: meaning never rests on colour alone. |
| A7 | Typeface: Geologica (text, wordmark) at tuned weights 250 / 300 / 350 / 450 / 550; JetBrains Mono (data). Both bundled, SIL OFL 1.1. | Proposed | Roundest of 19 candidates (closest to the mark), covers Latin, Cyrillic, Greek, Vietnamese, true italics. Cost: non-standard weights, tuned on Linux only. Runner-up: Inter. |
| A8 | Icons: Lucide (ISC), 1.5 px stroke, round caps and joins. | Decided | Round caps match the mark; large, maintained set; permissive licence. |
| A9 | Two glyph families: circles = work (from the owner's task-list reference), eyes = models (the leaning ellipse of the logo). Four task states added: with an agent, needs you, failed, skipped. | Proposed | State must be readable at a glance. The owner calls the logo's dots "the eyes"; the model metaphor (awake / asleep) follows from that and needs confirming. |
| A10 | Motion: Apple-style springs (response + damping), ease-out entries, UI under 300 ms, no animation for keyboard actions, real interruptible spring for anything draggable. | Decided | Brief asks for Apple-level care and subtle motion. |
| A11 | Three writing registers: Manual (people), Caveman (models), Reference (settings, tool pages). | Decided, with one point to confirm | From brief 4.13 and 4.14. To confirm: a Tool carries two texts, a description for people (Reference) and guidance for models (Caveman). |
| A12 | Working line under the name: "Make small models behave." | Proposed | The brief's own main goal. |
| A13 | Name and logo licence. | **Open** | The repo is GPL; a logo usually should not be. The repo went public on 2026-11-21 with a holding notice (README, `brand/README.md`): the name and logo are the owner's, and the GPL should not be assumed to cover them. A usage policy still has to replace that notice. |

## B. How the brand is built

| # | Decision | Status | Why |
|---|---|---|---|
| B1 | Tokens are JSON (`brand/tokens/src`), built by a zero-dependency Node script into CSS custom properties, JSON (with sRGB hex, for the CLI and non-web clients) and TypeScript. | Decided | One source of truth; every value editable in one place (brief: "everything easily edited"). |
| B2 | Colours are authored in OKLCH. | Decided | Lightness is predictable across hues; the build gamut-maps to hex. |
| B3 | Sizes are rem. | Decided | The whole UI follows the user's text size; a UI-scale setting is one number. |
| B4 | State glyphs are SVG files turned into CSS mask variables, drawn by one element and coloured by `currentColor`. | Decided | Works without a framework, including inside model-made HTML. |
| B5 | The reference components are plain CSS (`brand/css/components`, one file each, cascade layers, state as attributes). The app uses these files directly; its React components are thin wrappers with no styles of their own. | Decided | Framework-neutral until the frontend stack is fixed. The app reuses or re-implements them; names, states and tokens stay. |
| B6 | The kit page is static HTML assembled by a zero-dependency script, and also opens from disk. | Decided | Nothing to install to review it. |
| B7 | Every accent shade derives live from one custom property, `--d-color-accent-source`, using CSS relative colours behind `@supports`; static green values are the fallback and feed the hex export. Tokens gain a `$derived` field for this. | Decided | One input changes the accent everywhere, at runtime, with no rebuild: what a user setting and a system accent need. |

## C. Technical direction (Provisional)

From the four research reports that completed (`docs/research/01`, `05`, `06`, `08`). **None has been
fact-checked yet.** Licences, legal points and version claims in them must be verified before code depends on them.

| # | Direction | Source |
|---|---|---|
| C1 | One Rust core. Two binaries: `demido` (daemon + CLI, no GUI libraries, runs headless) and a thin desktop shell that starts or attaches to the daemon. | 01 |
| C2 | The UI is one static web app, embedded in and served by the daemon. Desktop shell, browser and remote server all load the same UI from the core they talk to. | 01 |
| C3 | Desktop shell: Tauri 2, kept thin (window, tray, updater, sidecar). No product logic in the shell. | 01 |
| C4 | One API for every client: REST + JSON with generated OpenAPI, one WebSocket for events and streams, OpenAI-compatible endpoints for "use it as a model manager". Auth is mandatory even on loopback. | 01 |
| C5 | Tools: one package format (manifest + guidance + optional runtime). Built-in tools are native Rust; installable tools are Rust compiled to WebAssembly components (sandboxed, one artifact for all platforms); converted MCP servers are "External"; raw MCP and classic skills are "Deprecated". | 08 |
| C6 | Injection defence is enforced by code, not by the model: provenance labels on all content, a per-session taint state, sanitising on ingestion, sandbox before prompts. | 05 |
| C7 | Providers: two interfaces, model providers (Demido owns the loop) and agent providers (Claude Code and OpenCode run their own loop). Own thin clients. Claude Code is driven through the user's installed `claude` in headless stream mode, never bundled or spoofed. | 06 |
| C8 | Reading the system accent: on Linux through the XDG desktop portal (`org.freedesktop.appearance`, key `accent-color`). Checked on the owner's machine (Plasma 6.8.3): it returns the current accent. Still to find out: the Windows and macOS calls, and how to hear about changes while Demido runs. A browser cannot read it: CSS `AccentColor` returned a default blue here, so the web UI gets the accent from the core, and falls back to green when used from another machine. | Measured here (2026-11-20); the rest unverified |

## C2. Frontend (Decided for the preview; carried into the app unless the preview shows a problem)

Research note: `docs/research/02-frontend-stack.md`. Versions and licences read from the npm registry.

| # | Decision | Why |
|---|---|---|
| F1 | React 19 + TypeScript 7 + Vite 8. Static single-page app, hash routing, relative base. | Widest choice of accessible parts and contributors; the same build runs in the shell, from a server and as static preview files. |
| F2 | Base UI (`@base-ui/react`, MIT) for dialog, menu, tooltip, popover. | Stable since December 2025; unstyled, so the brand CSS styles it. |
| F3 | No CSS framework. The app imports `brand/css/demido.css`; `apps/web/src/styles/app.css` holds layout only. | One source of truth for the look. |
| F4 | A feature is a folder that exports `feature` (title, icon, route, screen, settings). The registry finds folders; nothing lists features by hand. | Brief: add and remove features without editing many places. |
| F5 | Settings are data (`features/settings/schema.ts`). One screen renders, searches and resets all of them; features contribute their own. | Brief: every setting reachable; nothing hidden. |
| F6 | One store (zustand). The UI calls functions in `data/store.ts`; in the preview these are a mock core on timers. | The real core client replaces those functions; components stay. |
| F7 | Navigation: a labelled icon rail (14 areas in four groups), a list beside each screen, an inspector where useful, and a "Now" panel (memory, loaded model, running work) reachable from anywhere. | Fourteen areas need a home that scales; state must always be visible. |
| F8 | Motion library not installed yet. CSS transitions and the brand spring curves cover the preview. | No dependency before it is used. Gesture-driven pieces will need one. |

All dependencies are MIT, ISC or Apache-2.0: compatible with GPL-3.0-or-later.

## V. Voice mode (owner's direction, 2026-11-20)

Shown in the preview at `#/voice/talk`. The conversation there is simulated (`features/voice/sim.ts`); no
microphone, no speech model.

| # | Decision | Status |
|---|---|---|
| V1 | Voice mode is its own screen, without the navigation. Two things move and never share a meaning. The Demido symbol in the middle is Demido: it swells with every word it speaks, its two eyes most, rings travel outward, and it takes the accent colour while it has the floor. The line at the bottom is you: it rises with your voice. | Owner asked for it; built in the preview |
| V2 | Demido answers when your sentence is finished, not at the first silence. If you stop in the middle of a sentence to think, it stays quiet and shows that it is waiting. There is no time limit. "Answer now" ends the wait. Setting: "Wait until my sentence is finished", on by default; off means "answer after 0.7 s of silence". | Owner asked for it; simulated |
| V3 | Speaking over Demido stops it at once. What it had not said yet is dropped from the conversation, so the model knows what you actually heard. The cut is marked "Stopped here". Setting: "Stop when I speak", on by default; off closes the microphone while Demido speaks. | Owner asked for it; simulated |
| V4 | Before a task that takes time, Demido says so, then works, and the screen names the task. | From the brief; simulated |
| V5 | A feature may take the whole window: `immersive` routes in the feature registry. The rail steps aside; the feature shows the way back. | Decided |
| V6 | What the real thing needs, none of it chosen: a detector for speech, a small model that judges whether a sentence is finished, echo cancelling so Demido does not hear itself and stop, and a speech engine that can stop mid-word and report how far it got. Candidates and their licences are not checked. | Not researched (section D) |

## M. Markets (owner's direction, 2026-11-20)

Research note: `docs/research/09-markets-tradingview-pine.md`.

| # | Decision | Status |
|---|---|---|
| M1 | Candles are green and red unless the user changes them. Setting "Candle colours": Green and red, Hollow and solid, Custom (two colours). This replaces the earlier default (hollow and solid, idea 30), which stays as an option for people who cannot tell red from green. The green and red are the brand's success and danger colours, which also differ in lightness. | Owner decided; in the preview |
| M2 | Demido draws its own chart and works indicators out itself, on this computer. No limit on how many. In the preview: EMA, Bollinger Bands, Volume, RSI and MACD, with real maths on made-up prices. | In the preview |
| M3 | Pine Script indicators run locally, in Demido's own engine, and are never added to an account. A model can write one, run it against the chart, read the error, and fix it. "Copy for TradingView" puts the source on the clipboard; pasting it into TradingView is the user's own act. | Owner asked for it; scripted in the preview. Engine not chosen |
| M4 | Engine candidates: PineTS (AGPL-3.0 or commercial; allowed by the brief only through GPLv3 section 13 or as a separate process), an engine of our own, PyneCore (ruled out: its Pine compiler is a closed, paid online service). Model-written scripts are untrusted code and run in the sandbox whichever engine is used. | Open. PineTS licence read; the rest search-level |
| M5 | The UI says plainly: Demido's engine is not TradingView's, so a script can behave differently there. "Pine Script" is TradingView's trademark; Demido says "compatible" and states it is not affiliated. | Decided |
| M6 | **Reading the TradingView account automatically.** Asked: after sign-in, load the account's indicators and candle colours. Found: TradingView's terms forbid automated data collection by any means and for any purpose, say features are for manual use only, and ban accounts for it (for a time, then for good). No official API reads a user's layouts. So the automatic route breaks their terms, risks the user's account, and exposes a project that ships a tool for it. **Recommended: do not build it.** Build the allowed route instead: add built-in indicators from Demido's list, paste the source of your own scripts, set the two colours once. The preview shows both: by hand as allowed, automatic as "Not built", with a made-up account to show what it would copy. Closed-source scripts cannot be copied by any route. | **Decided by the owner on 2026-11-20: build it**, behind a warning, then automatic. See M10. My recommendation against it stands and is on record. |
| M7 | Prices do not come from TradingView, whose data may only be shown on TradingView. Public exchange feeds first. Which feeds, and what for stocks and currencies: not researched. | Open |
| M8 | Demido never draws into TradingView's own chart and never lifts TradingView's plan limits there. "No limit on indicators" is true of Demido's chart only. | Decided |
| M9 | Chart drawing in the real app: Lightweight Charts (Apache-2.0, asks for a TradingView attribution notice and link) or a renderer of our own. The preview uses plain SVG. | Open |

### Markets, second round (2026-11-20)

The owner allowed one look at the previous Demido Studio's TradingView code, for this feature only. What it
did is written down in `docs/research/10-accounts-and-blocker.md`, section 2.

| # | Decision | Status |
|---|---|---|
| M10 | **TradingView sign-in is offered, behind a warning.** The warning says what TradingView's terms forbid, what can happen to the account, what Demido will read and do, and what it never does. The user ticks "I understand that TradingView may ban this account". Then everything is automatic: sign in on TradingView's own page in a window and profile of its own, keep the session in the keyring, read favourites, own scripts and chart layouts, load the newest layout (indicators and candle colours). "Sign out and forget" deletes the session. | Owner decided; scripted in the preview with a made-up account |
| M11 | **Two engines, and every indicator says which.** "TradingView": worked out by TradingView with the user's session, exact values, the plan's limit per chart applies; past it Demido takes over by itself (M13). "Local": worked out by Demido, no account, no limit. The Indicators menu has shelves like TradingView's own: favourites, my scripts, chart layouts, TradingView built-ins, community scripts, Demido built-ins, Pine on this computer. | In the preview |
| M12 | **Pine scripts are kept on this computer either way.** Run by Demido's engine (no limit, may differ), or, when signed in, compiled and run by TradingView without saving anything to the account (exact, counts toward the plan). | In the preview; engine still open (M4) |
| M13 | **Past the plan's limit, Demido works the extra indicators out itself.** TradingView works out as many per chart as the account's plan allows; every indicator beyond that runs on Demido's own engine, automatically, and is labelled "Demido". Nothing extra is loaded at TradingView: no hidden charts, nothing added to the account. This covers what Demido has maths for (the built-ins) and scripts whose source can be read (the user's own, and open-source community scripts). A closed-source script can only run at TradingView, so past the limit it is refused, with the reason. | **Decided by the owner on 2026-11-20.** In the preview, with a made-up free account (2 per chart) |
| M14 | **Markets is "a quieter TradingView".** In the preview: symbol search over kinds, thirteen timeframes, five chart types, zoom and pan, a time axis, a crosshair with price and time, drawing tools (trend line, horizontal line, rectangle, measure, erase), price alerts, an editable watchlist, the Indicators menu. Still missing, and marked so: live prices, history download, backtesting, saved layouts of Demido's own, comparing symbols, a logarithmic scale, more drawing tools. | In the preview |

## S. Sessions (owner's direction, 2026-11-20)

| # | Decision | Status |
|---|---|---|
| S1 | **Groups of sessions.** You make a group and move sessions in: from a session's menu, or by dragging. A group folds, can be renamed, and deleting it keeps its sessions. Ungrouped sessions stay listed by time. One level, no groups inside groups. | In the preview |
| S2 | **A session has a folder.** Either Demido's own private one, or one you assign; the model works there. With an assigned folder, `@` in the message box points at files and folders, and `@folder/` lists what is inside. The header shows the folder. | In the preview, on a made-up file tree |
| S3 | **The permission level reads from its icon**: a whole shield (Cautious), half (Balanced), deciding (Smart), off (Autonomous, the one level shown in the caution colour). | In the preview |
| S4 | **The panel on the right can be closed**, per screen, and stays that way. | In the preview (Sessions, Markets, messaging) |

## X. Other screens, second round (2026-11-20)

| # | Decision | Status |
|---|---|---|
| X1 | **Models: a list and a model card**, the way LM Studio does it. The list has "Rising this week", "Recommended for this computer", "New and updated", and everything in pages with sorting. The card has what the model is good at, its limits on this computer, its files, and what it does here. In the preview the card is written from the model's listed facts; the real one loads the publisher's card. | In the preview |
| X2 | **A tool is a folder of files, and every file is yours to change**: the line models always see, the guidance they read when they pick the tool, a classic skill's `SKILL.md` and its references and scripts. Each file says who reads it and when. Saving an edit clears the audit. Edits survive updates. | In the preview |
| X3 | **Annotation in the browser.** Point at an element, draw, highlight, box, arrow. A pin records what it points at. "Send to a session" gives a model the marked picture, the notes as text, and the elements pointed at. | In the preview; sending is not built |
| X4 | **An ad blocker in the built-in browser**, on by default. Models are told it exists. When a page breaks because of it, a model may read its log and loosen it for one site (allow a domain, pause), says so, and the change is listed with its reason and an Undo. A model can never switch it off for all sites. Setting: models may change it never, for one site (default), or only after asking. | In the preview (scripted). Engine: adblock-rust proposed, research note 10 |
| X5 | **Email**: accounts behind one switch at the top ("All accounts" included), folders and labels with icons, search, filters (unread, has files, needs you), a time filter with quick picks and a from/until date-and-time pair, and sorting. | In the preview |
| X5a | **Email does what a mail program does**: write a new message, reply, reply to everyone, forward (with the files), keep a draft on closing, undo a send; file as done, move, label, star, mark read or unread, report spam, delete, and delete for good from the bin; choose several messages and act on all of them. The writing window floats over the corner so the message being answered stays readable. A model can write the message or finish what you started; it never presses Send. | Owner asked; in the preview. Blocking a sender, reminders, printing and the raw message are marked not built |
| X5b | **Asking the model about a mail happens beside the mail.** "Ask Demido about this" opens a panel in the same screen; the message stays in view and nothing navigates away. The model sees that one message, answers, and can open a reply with its text written or file the message. The same panel as beside a chat (`ui/ask.tsx`). | Owner asked; scripted in the preview |
| X5c | **"Always load images and styles"** is a switch in the email screen and a setting. Off by default: Demido asks per message, because loading a picture tells the sender the mail was opened. A model reading mail never loads them. | Owner asked; in the preview |
| X6 | **Providers hold several accounts each.** One is the default; each shows its own usage; a program-based provider gets one profile folder per account. Picking an account per session comes later. | In the preview |
| X7 | **The version is always on screen**, at the foot of the navigation, with a dot when a newer one exists. An Updates screen holds the check, what is new, download and restart, the versions of the downloaded parts, the history and "about". An update never interrupts a running session; the download is checked against the project's signature; the old version is kept until the new one has started once. | In the preview (scripted) |
| X8 | The feature registry gained two things: a feature may give itself several places in the navigation or none (`nav`), and may show something small at its foot (`rail`). | Decided |

## G. Messaging (owner's direction, 2026-11-20)

Research note: `docs/research/10-accounts-and-blocker.md`, section 1. This replaces my first reading of the
brief, which had only bots.

| # | Decision | Status |
|---|---|---|
| G1 | **WhatsApp, Telegram and Discord each have their own place in the navigation**, and each is browsable inside Demido: chats, a conversation, search. | In the preview |
| G2 | **Demido can act through the user's own account**: read chats, mark them as seen, draft or send replies, write reports. Connecting comes after a warning with that service's facts and a tick box; then it is automatic. The risk differs: Telegram allows other apps on an account, WhatsApp names unofficial apps and automation among bannable things, Discord forbids it outright. | Owner decided; scripted in the preview |
| G3 | **How far a model goes is set per account**: reads only; drafts (default: marks as seen and drafts, you send); replies on its own, only to people you choose. Everything it does is listed. | In the preview; choosing the people is not built |
| G4 | **Bots for Telegram and Discord**, the services' own allowed way: you talk to Demido from the chat app. Only the owner's account may give orders. | In the preview |
| G5 | **A model is told where it is being reached**, in a short text added whenever a message comes through a bot: the service, who may give orders, the length limit, and what is missing there (no task list, no permission dialog, no artifacts panel) with what to do instead. The screen shows this text and a table of the differences. | In the preview |
| G6 | **What other people write is data.** It can never give a model orders. Messages that look like a trick are flagged and not answered. | Decided |
| G8 | **Beside the open chat sits a small conversation with the model about that chat.** You ask ("what needs an answer here?"), or have it act: mark as seen, or type a reply into the message box. It types; it does not send. The panel shares the right side with the rules and the log of what was done. | Owner asked; scripted in the preview |
| G9 | **Help with writing, in every box that speaks for you** (the three chat services and email). "Draft" writes a reply; if you already typed something, the draft starts with your words. "Improve" rewrites what you typed: fix spelling, shorter, formal, friendly, happy, angry, apologetic, or an instruction of your own. "Back to mine" undoes it. | Owner asked; in the preview, with fixed rules standing in for a model |
| G10 | **Discord is laid out as Discord is**: servers down the left edge, then the server's channels in their categories (voice channels show who is in them), then the talk as rows with a name, then the member list by role. Drawn in Demido's own look, with none of Discord's artwork. WhatsApp and Telegram keep the list-and-chat layout they have themselves. | Owner asked; in the preview |
| G11 | **Chats show profile pictures**, in the list and at the top. Pressing the chat's name at the top opens its details in the right panel, as the chat apps do: who it is (number or handle, about or bio, last seen), what was shared in it under three tabs (media, links, docs or files), the members of a group, and mute, starred, search, block and report. The right panel now has three tabs: Ask Demido, Details, Rules. | Owner asked; in the preview. Pictures and shared items are drawn |
| G7 | Open: the libraries and their licences; a human pace for sending; what "seen" means on each service; generic icons stand in for the services' marks until their brand rules are read. | Not researched |

## K. The core (decided 2026-11-21; first slice built)

Research notes: `docs/research/11-core-architecture.md`, `docs/research/03-inference-runtime.md`. Versions and
licences were read from crates.io and the GitHub API, not from memory.

| # | Decision | Status |
|---|---|---|
| K1 | **One process owns everything: `demido serve`.** One program is both the server and the command line. The command line asks a running server; without one it works on the files directly. This confirms C1 and C2. | Built |
| K2 | **Parts:** tokio, axum 0.8, SQLite through rusqlite, utoipa 6 for the API description, clap, sysinfo and NVML for hardware. All MIT or Apache-2.0. | Built |
| K3 | **One API under `/api/v1`**, JSON, with its OpenAPI description generated from the code and served by the core. Errors are always `{ error: { code, message } }`, the message written for a person. | Built |
| K4 | **Events go out as Server-Sent Events.** WebSockets only where talk goes both ways (voice, the built-in browser). Narrows C4. A client that reconnects says where it stopped and misses nothing. | Built |
| K5 | **The event log is the memory of what happened**: append-only, numbered, kept for a number of days that is itself a setting. | Built |
| K6 | **Settings are declared once, in the core.** The registry holds id, kind, default, range, the words, and whether it is built. Only values that differ from the default are stored. A change writes an event. The UI's own setting list (`features/settings`) moves onto this registry next. | Built. The Settings screen draws the core's settings from the core's own description (K13) |
| K7 | **A key is needed even on this computer.** Made on first start, kept in a file only the user can read. A browser gets in by a one-use link traded for a session cookie; the key never appears in an address. A foreign `Host` is refused; a change made with a cookie must come from Demido's own pages. Remote use (encryption, passkeys) is not built. | Built |
| K8 | **The model runtime is llama.cpp's `llama-server`, run as a separate process**, downloaded from llama.cpp's own releases, never shipped by Demido. A process, so that unloading a model is certain. | Decided; only the advice is built |
| K9 | **Demido reads the computer and recommends a build.** NVIDIA: ask the driver which CUDA it supports and take the newest build it can run. AMD and Intel: Vulkan first. No card: the processor build. The rule is a tested, pure function. | Built. Tested on Linux, Windows and macOS by the checks on GitHub (R3), where the program also runs and reads the build machine. No real Windows or Mac with a graphics card has run it yet |
| K10 | **The UI talks to the core through one file** (`apps/web/src/data/core.ts`). Screens move from scripted to real one at a time, and each says which it is showing. Setup's "This computer" is the first. | Built |
| K11 | **The core finds the web UI in a folder named `ui` beside the program.** That is how a release is laid out: unpack, start, and it works from any folder. `--ui-dir` and `DEMIDO_UI_DIR` still win; in the repository it falls back to `apps/web/dist`. Carrying the UI inside the program is not built. | Built |
| K13 | **Two kinds of setting, side by side, until the preview is gone.** A setting the core declares is real: the core keeps it, checks it, logs the change, and tells every open window (change the theme on the command line and the window follows). Everything else is still the preview's, kept in the browser, and is marked "Not built yet" whenever a core is behind the pages. A setting moves into the core's registry when the thing it controls is built there, and its copy in the UI is deleted then. The UI never chooses: `setting`, `useSetting` and `setSetting` look the id up. A slider tells the core its last value once the hand rests, so a drag is one entry in the log, not thirty. | Built for Appearance (theme, accent, custom accent, text size), the listen address and the log's length |
| K14 | **"Automatic" accent follows the real system accent, read by the core.** A browser cannot read it; the core can, on its own computer. On Linux it asks the desktop's portal (`org.freedesktop.appearance`, `accent-color`: KDE Plasma 6, GNOME 47 and later) through `busctl`, or `gdbus` if that is missing; both come with nearly every Linux desktop. A D-Bus library (zbus 6.5, MIT) was tried and dropped: about thirty more packages to answer one question. It comes back when a second thing needs D-Bus. The window asks again each time it gets the focus back, since the accent is changed in the system's own settings. A window on another computer than the core does not use it and says why. | Built for Linux. **Not built for Windows and macOS**, and the Settings screen says so there |
| K12 | **A release's version comes from its tag.** The release build stamps the program with it (`DEMIDO_VERSION`), and the build stops if the program then says anything else. Other builds say what `Cargo.toml` says. So nobody has to remember to raise a number before tagging. | Built |

## D. Not researched yet

The research run stalled when the session's usage limit was reached and ended with that session. Since then,
the inference runtime (note 03) and the core's parts (note 11) were researched directly. Still without a report:
model loading and tuning, small-model techniques (including "vector reasoning" and
adding capabilities through embeddings), search engines and the named third-party projects (licences, AGPL
handling, look-alike repos), voice / email / messaging / markets, core architecture (storage, scheduler,
resource manager). The frontend stack was researched directly since (section C2). The remaining five are
needed before any feature work.

Added by the owner's direction on 2026-11-20, to cover in the voice and markets research: end-of-sentence
detection, interruption with echo cancelling, and speech that can stop mid-word (section V); a local Pine
Script engine, price feeds, and the TradingView account question (section M; first findings in research note 09).

## R. Repository, checks and releases (decided 2026-11-21)

Action versions and runner names were read from GitHub (each action's latest release, and the runner images
list) on 2026-11-21, not from memory.

| # | Decision | Status |
|---|---|---|
| R1 | **The project lives at `github.com/elpideus/demido-studio`**, public, GPL-3.0-or-later. The repository that had that name was renamed to `demido-studio-fourth` first (the brief's `demido-studio-third` was already taken by an older repository of the owner's; the owner chose the new name), by its name alone: it was not cloned, read or searched. | Done |
| R2 | **Every commit and tag is signed.** This computer signs with an SSH key that is registered on the GitHub account as a signing key, so GitHub shows the commits as verified. There is no OpenPGP (GPG) secret key on this computer; if the owner imports one, signing can move to it. | Done |
| R3 | **Checks on every change** (`.github/workflows/ci.yml`): the web UI's types and build; the core's formatting and lints; the core's tests on Linux, Windows and macOS, and on each the program must run and read the computer. | Passing. The first run failed on a fresh copy of the repository (the brand's built tokens were ignored by git), which is exactly what it is for; the checks now also rebuild the brand kit and compare |
| R4 | **Releases build themselves** (`.github/workflows/release.yml`). Pushing a version tag (`v0.1.0`) or publishing a release on GitHub builds six archives: Linux, Windows and macOS, each for x64 and arm64, plus `SHA256SUMS.txt`, and attaches them to the release. A tag without a release gets one; a release published by hand keeps its text. | Built; all six archives were made by a rehearsal on 2026-11-21, and on each platform the program ran and reported its version. **Attaching to a release has not been tried:** that needs a real tag |
| R5 | **A rehearsal publishes nothing.** Started by hand, the same workflow builds everything and keeps the files with the run for 14 days. That is how the pipeline is tested without making a release. | Done twice on 2026-11-21 |
| R6 | **What a release holds today:** the program (server and command line), the web UI in `ui/` beside it, the licence, and a note that says plainly what does not work yet. No desktop window, no installer: those are step 10. The status lines are one file (`scripts/release-status.txt`) shared by the note and the release page. | Built |
| R7 | **Releases are not signed for Windows or macOS.** Windows shows a warning; macOS refuses to open the program until its download mark is removed, and the note in the archive says how. Signing costs money every year (an Apple developer membership, a Windows certificate), so it is the owner's decision. | **Open, owner's** |
| R8 | **Linux is built on the oldest Ubuntu GitHub offers** (22.04, glibc 2.35), so the program also starts on older systems. | Built |
| R9 | **One Rust version for everybody** (`rust-toolchain.toml`, now 1.100.0): this computer, the checks and the releases compile with the same compiler. Raised on purpose. | Built |
| R10 | **Actions are pinned to an exact commit**, not a moving tag. Two are not GitHub's own: `pnpm/action-setup` and `Swatinem/rust-cache`. The cache is used by the checks only; a release is always built from nothing. Releases are attached with GitHub's own command line, not a third-party action. | Built |
| R11 | **`.claude/` is not in the repository.** It holds one person's local tool settings. | Done |

## E. Repository

```
apps/web/            the web UI; today the interactive preview with fake data (see apps/web/README.md)
brand/               the brand kit (see brand/README.md)
core/                the core, in Rust (see core/README.md)
docs/                BRIEF.md, DECISIONS.md, PLAN.md, IDEAS.md, research/
scripts/             package-release.mjs puts one release archive together; release-status.txt is its status text
.github/workflows/   ci.yml (checks on every change), release.yml (builds on a tag or a release)
LICENSE              GPL-3.0 text (project is GPL-3.0-or-later)
package.json         pnpm workspace root: scripts for the brand kit, the app and the core
rust-toolchain.toml  the Rust version everybody builds with
.node-version        the Node version the checks and releases use
```
