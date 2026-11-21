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
- Served by the core, four things are real. Sessions: a conversation with a model on this computer, its
  answer written as you watch, kept by the core (`demido ask` does the same from a terminal); the model
  can read the clock and calculate, and work can be split into tasks that are ticked off as you watch.
  Given a folder, it can read and change the files in it, and nowhere else; it asks first where the
  session's level says so, and each change can be undone. Setup:
  this computer's hardware, and installing the model runtime. Models › On this computer: the models in
  Demido's folder, loaded and unloaded for real, with the indicator in the corner following. Settings:
  Appearance, Models and memory, Context and compaction, Tools and tasks, Permission levels, the listen address and the log's length. Everything else is still the
  scripted preview, and says so.
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

1. ~~The small-model techniques: before the chat loop.~~ Done on 2026-11-21: research note 13, with a
   measurement on the two test models. Still owed from this line: settings per model (temperature and the
   rest, brief 4.2), before the models work.
2. Search engines and the named third-party projects (licences, look-alike repos): before search and fetch.
3. Voice, email and messaging libraries; a local Pine Script engine; price feeds: before each of those.
4. How well small models choose among models, and in what words to tell them about the others (DECISIONS
   O2, O8): before agents get to pick their own model.

### Step 3. Real feature work

The order. Items 1 and 2 are done; item 3 is nearly done; item 4 (the chat loop) is under way: its research
is done (note 13, DECISIONS L) and plain chat works for real. Item 4 (the chat loop) is built: plain chat,
the switch for thinking, making a long conversation shorter, turns with tools, and the task list. Item 5
is under way: a session can have a folder, a model can read and change files in it and nowhere else, the
four permission levels decide when it asks, and every change can be undone (DECISIONS T). Left in item 5:
running programs in a sandbox, tools that reach the network, and tools other people write.

1. Repository on GitHub: confirm the account, rename the old repo, create the new one, release CI on tag.
   Done on 2026-11-21 (DECISIONS, section R). The old repository is now `demido-studio-fourth`.
2. Core skeleton: daemon, API, storage, settings system, event log, CLI. Includes the Accent setting and
   reading the system accent (Linux portal first).
   Done on 2026-11-21 (DECISIONS, section K). Left open inside it: the system accent on Windows and macOS.
3. Local inference: runtime install, hardware detection, load / unload, memory budget.
   Hardware detection and the installer are built (DECISIONS K8, K9, K15 to K21), and the real runtime is
   installed on the owner's computer (CUDA 13.5, 170 MB; it starts and sees the card). Loading, unloading
   and unloading when idle are built too (DECISIONS K22, K23): the runtime's own router does the fitting
   into memory, tried on the real card before it was relied on. The indicator in the corner shows the
   real graphics memory. Left in this item: a context chosen per model (it belongs with the research
   on model tuning).
4. Chat loop with the small-model techniques: constrained tool calls, repair, task list, compaction.
   Built on 2026-11-21 (DECISIONS L): plain chat, the thinking switch with "Answer now", compaction, turns
   with tools (the clock, the calculator, a plan), the task list that Demido walks, stopping a model that
   goes round. Measured on four models and tried on them for real (note 13): 4, 5, 6 and 7 of 8 everyday
   requests came out right. Left open inside it: the wording of the tool question chosen per model (no
   one wording suits two families; set aside by the owner, `docs/LATER.md`); chains of two tools, which did not once come about by themselves; a
   check that an account is a good one; a default compaction size worked out per model; showing a plan
   for approval before it is walked.
5. Tools system, permission levels, sandbox, injection defence.
   First part built on 2026-11-21 (DECISIONS T, research note 14): a folder for a session, tools that read
   and change files in it, the four levels, asking first, undo, and cleaning of text from outside. Tried
   on four models with real files. Next inside it, in this order: running programs in a sandbox; tools
   that reach the network, with rules for what may leave the computer; tools other people write, with
   their browser and updater.
6. Models browser. Providers (Anthropic API, Gemini, OpenRouter, Claude Code, OpenCode). Usage.
7. Agents (a model for each, chosen by the orchestrating model or by Demido's rule: DECISIONS, section O),
   lending, schedules, session messaging.
8. Files, artifacts, editor. Browser, search, fetch.
9. Voice. Email. Messaging. Markets.
10. Desktop shell, installer, updater, server mode hardening.

Checkpoints: one report per numbered item, more often where a decision is the owner's.

## Set aside for later

Plans the owner wants tried further down the road are in `docs/LATER.md`. So far: words chosen for each
model, from a short trial when a model is first used.

## Needs the owner before it can start

- **Before the first release:** did the previous Demido check `elpideus/demido-studio` for updates? That name
  now belongs to this repository, so an old installation that checks it would be offered this project's
  releases. I have not looked at the old code and will not. No release exists yet, so nothing is offered today.
- Signing the Windows and macOS builds. It costs money every year (DECISIONS R7).
- A usage policy for the name and logo, to replace the holding notice (DECISIONS A13).
- Whether to rerun the research sweep or let it be done directly (the earlier run stalled and has ended).
- The eight items in the kit's "For your review" section.
- What a voice interruption does to a running task (IDEAS 42).
- Whether sixteen places in the navigation is right, or services should appear only once connected (IDEAS 58).
