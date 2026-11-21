# Plan

The order of the first three steps is the owner's (brief section 0). Everything after is proposed.

## Where we are

**Step 1, brand kit: approved** (2026-11-20), with the accent changed to Demido green and made a setting.

**Step 2, interactive preview: approved** (2026-11-21), after four rounds of changes.

**Step 3, real feature work: started.** The first slice of the core is built and tested (DECISIONS, section K):

- `demido serve` runs the server; `demido` is also the command line (`status`, `system`, `settings`,
  `events`, `open`).
- Storage, the settings registry, the event log with its live stream, the key and the browser link, the
  API with its own description.
- It reads this computer's real hardware and recommends the model runtime build to download.
- Served by the core, the Setup screen shows that real hardware, and the Settings screen keeps Appearance,
  the listen address and the log's length in the core. Every other screen is still the scripted preview,
  and says so.
- The project is public at `github.com/elpideus/demido-studio`. Every change is checked on Linux, Windows
  and macOS; a version tag builds six archives (DECISIONS, section R).

Run it: `pnpm core:serve`, then `cargo run --manifest-path core/Cargo.toml -- open`.

What the preview rounds changed, for the record:

Changed on 2026-11-20, first round: voice mode as a screen of its own (DECISIONS V); Markets with green and
red candles, indicators and a Pine editor (DECISIONS M).

Changed on 2026-11-20, second round:

- **Markets**: TradingView sign-in behind a warning, then automatic (favourites, scripts, chart layouts,
  colours), modelled on the previous version; indicators by TradingView or by Demido; symbol search, more
  timeframes, chart types, zoom, drawing tools, alerts. DECISIONS M10 to M14.
- **Sessions**: groups; a session folder with `@` for its files; a shield per permission level; the right
  panel can be closed. DECISIONS S.
- **Models**: a list with rising models and pages, and a model card. **Tools**: every file of a tool can be
  edited. **Providers**: several accounts each. **Updates**: version label, update screen. DECISIONS X.
- **Browser**: annotation tools; an ad blocker that models know about. DECISIONS X3, X4.
- **Email**: account switch, folder icons, search, filters, date and time range, sorting. DECISIONS X5.
- **Messaging**: WhatsApp, Telegram and Discord with your own account (after a warning), and bots for
  Telegram and Discord. DECISIONS G.

Changed on 2026-11-20, third round:

- **Messaging**: a conversation with the model beside the open chat (it can type into the message box);
  "Draft" continues from what you typed; "Improve" with tones or your own instruction; Discord laid out as
  Discord is. DECISIONS G8 to G10.
- **Email**: write, reply, reply to everyone, forward, drafts, undo send, file, label, star, delete, and
  acting on several messages at once. DECISIONS X5a.

Changed on 2026-11-20, fourth round:

- **Email**: "Ask Demido about this" opens a panel beside the message instead of leaving the screen; a
  switch to always load images and styles. DECISIONS X5b, X5c.
- **WhatsApp and Telegram**: profile pictures in the list and at the top; pressing the name opens details
  with media, links and docs. DECISIONS G11.

## Next

### Research still owed

Done directly and narrowly, each before the feature that needs it. None of the four early reports (01, 05,
06, 08) has been fact-checked; each is checked when its feature starts.

1. Model loading and tuning, and the small-model techniques: before the chat loop.
2. Search engines and the named third-party projects (licences, look-alike repos): before search and fetch.
3. Voice, email and messaging libraries; a local Pine Script engine; price feeds: before each of those.

### Step 3. Real feature work

The order. Items 1 and 2 are done; item 3 is under way.

1. Repository on GitHub: confirm the account, rename the old repo, create the new one, release CI on tag.
   Done on 2026-11-21 (DECISIONS, section R). The old repository is now `demido-studio-fourth`.
2. Core skeleton: daemon, API, storage, settings system, event log, CLI. Includes the Accent setting and
   reading the system accent (Linux portal first).
   Done on 2026-11-21 (DECISIONS, section K). Left open inside it: the system accent on Windows and macOS.
3. Local inference: runtime install, hardware detection, load / unload, memory budget.
   Hardware detection and the installer are built (DECISIONS K8, K9, K15 to K21). The installer is tested
   against a stand-in publisher; the first real download waits for the owner. Next: start the runtime
   with a model, stop it when idle, and the memory budget. Reading the runtime's current documentation
   first showed that it now does much of this itself (research note 12, DECISIONS K22, proposed); that
   has to be tried on the real runtime before anything is built on it.
4. Chat loop with the small-model techniques: constrained tool calls, repair, task list, compaction.
5. Tools system, permission levels, sandbox, injection defence.
6. Models browser. Providers (Anthropic API, Gemini, OpenRouter, Claude Code, OpenCode). Usage.
7. Agents, lending, schedules, session messaging.
8. Files, artifacts, editor. Browser, search, fetch.
9. Voice. Email. Messaging. Markets.
10. Desktop shell, installer, updater, server mode hardening.

Checkpoints: one report per numbered item, more often where a decision is the owner's.

## Needs the owner before it can start

- **The first real runtime download.** Everything is built and tested without it. The recommended build for
  this computer is 170 MB (llama.cpp, CUDA 13.5, from `github.com/ggml-org/llama.cpp`); the processor-only
  build is 22 MB. It is a download and a program that gets run, so it waits for a yes.
- **Before the first release:** did the previous Demido check `elpideus/demido-studio` for updates? That name
  now belongs to this repository, so an old installation that checks it would be offered this project's
  releases. I have not looked at the old code and will not. No release exists yet, so nothing is offered today.
- Signing the Windows and macOS builds. It costs money every year (DECISIONS R7).
- A usage policy for the name and logo, to replace the holding notice (DECISIONS A13).
- Whether to rerun the research sweep or let it be done directly (the earlier run stalled and has ended).
- The eight items in the kit's "For your review" section.
- What a voice interruption does to a running task (IDEAS 42).
- Whether sixteen places in the navigation is right, or services should appear only once connected (IDEAS 58).
