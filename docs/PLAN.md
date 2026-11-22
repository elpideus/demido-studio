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
  Given a folder, it can read and change the files in it and run programs there, and nowhere else; it
  asks first where the session's level says so, and each change, a program's too, can be undone. Setup:
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
four permission levels decide when it asks, and every change can be undone. It can also run programs in
that folder, shut in a sandbox on Linux, with the folder saved first so that all a program did can be put
back (DECISIONS T). And it can read from the internet: search, and read a page from its address, with
rules in code for what may leave the computer (DECISIONS N). Left in item 5: the sandbox on macOS and
Windows, and tools other people write.

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
   on four models with real files. Then running programs: a sandbox on Linux (bubblewrap), the folder
   saved before each program, one Undo for all a program did. Then reading from the internet (DECISIONS N,
   research note 15): search that tries sources in turn (your own SearXNG, Exa, DuckDuckGo, Wikipedia with
   Mwmbl), reading a page as text, an address a model wrote
   itself always asks, only the public internet, and a change to a file asks once a page was read. Tried
   on four models: 5, 6, 7 and 7 of 8 everyday requests right; a page that gave orders changed nothing.
   Then the whole of what a program printed, kept, with a short view for the model and a way to read on;
   and what each change changed, as a diff (DECISIONS T18, T19; research note 16).
   The sandbox on macOS is built too (DECISIONS T20): Seatbelt, tried on the checks' Mac.
   Next inside it: the sandbox on Windows; tools other people write, with their browser and
   updater.
6. Models browser. Providers (Anthropic API, Gemini, OpenRouter, Claude Code, OpenCode). Usage.
   The models browser and downloader are built (DECISIONS H, research note 17): searching Hugging Face for
   GGUF models, whether each fits this computer, a quantization recommended, downloads checked and resumable,
   a vision part downloaded with its model. Settings for each model are built (DECISIONS K26 to K30, research
   note 19): each model is asked the way its maker recommends, thinking and not, and the person can set their
   own. Left in it: gated models (with the vault), deleting a model from the window, the context for each
   model.
   The vault and three providers are built (DECISIONS U, research note 18): keys in the system keyring (a
   file only the user can read where there is none), never shown again and never seen by a model; Anthropic,
   Gemini and OpenRouter in sessions, chosen by the person, with what each answer cost and a limit for the
   month. Tried against stand-ins; the first real request waits for the owner's key. Claude Code and OpenCode
   are built too (DECISIONS U15 to U31): Demido's own copies, downloaded from their makers on demand,
   checked, kept in its own folder with profiles of their own (nothing installed on the system), their
   tools shown, what they ask asked in Demido's window, the folder saved before each turn. Both tried for
   real. Several accounts at one provider are built too (DECISIONS U32 to U37, the owner's direction,
   2026-11-22): several keys at an API and several Claude Code sign-ins, each named, one the default, any of
   them chosen in a session, each one's cost counted on its own.
7. Agents (a model for each, chosen by the orchestrating model or by Demido's rule: DECISIONS, section O),
   lending, schedules, session messaging.
8. Files, artifacts, editor. Browser, search, fetch.
9. Voice. Email. Messaging. Markets.
10. Desktop shell, installer, updater, server mode hardening.

Checkpoints: one report per numbered item, more often where a decision is the owner's.

## What is left, piece by piece

One worker does all of it (the owner's direction, 2026-11-21; DECISIONS, section W). For a few hours on that
day the work was split between two models; the owner took that back before any piece was handed over. The
list made for the split stays, because it says what is left and what to watch in each piece.

| # | Remaining work | What to watch |
|---|---|---|
| 5 | ~~The sandbox on macOS (Seatbelt)~~ done (T20); the sandbox on Windows; a filter on what a program may ask of the kernel | Security, and it cannot be tried here: only on the checks' machines |
| 5 | The second look that Smart is to give, and the trial that says whether a small model can give it | A measurement that decides a security design |
| 5 | Tools other people write: the package, the WebAssembly runtime, what a tool may ask for, signatures | Runs other people's code. Everything later is built on it |
| 5 | The tools browser, the updater's window, editing a tool's guidance in the app; bringing in MCP servers and skills once the format stands | Building to a format that is already decided |
| 6 | ~~Settings for each model: the research, and applying what the makers recommend~~ done (DECISIONS K26 to K30, research note 19) | It meets `docs/LATER.md` 1 |
| 6 | ~~Where keys are kept (the vault) and how a provider is allowed to be used.~~ Done (DECISIONS U). Left from it: search services that need a key (Brave and others), which is what would make search good, and the Hugging Face key for gated models (H8) | Secrets and money |
| 6 | ~~Anthropic, Gemini and OpenRouter; usage shown; Claude Code and OpenCode.~~ Done (DECISIONS U), tested against stand-ins: the first real request waits for your key, and the first message to a program for your yes | Documented interfaces, one at a time; never touch a program's login |
| 7 | Agents with a model each, lending a loaded model, fitting several into memory | The hardest design left, and the product's main promise |
| 7 | Schedules and monitors that run with nobody there: what they may do | Nobody is there to say no |
| 7 | The schedules screen; messages between sessions shown | Window work on a decided design |
| 8 | Files, the viewer, the editor, artifacts | Window work with a clear edge |
| 8 | The browser a model drives: what it may see and do. With it: pages that only show their text after running scripts | Logged-in pages are the most private thing there is |
| 9 | Voice: the libraries, the screen | Research and building, little risk |
| 9 | Email and the messaging platforms: reading and sending in a person's name | Messages from outside are injected text; sending cannot be undone |
| 9 | Markets: charts, indicators, the Pine editor | Nothing here places an order |
| 10 | The desktop shell and the installer | Packaging, checked on three systems by the checks |
| 10 | The updater, signing releases, hardening server mode | A wrong update reaches everyone; an open server is an open computer |
| – | The system accent on Windows and macOS; the per-model context size | Small and bounded |

Done from this list on 2026-11-21: reading from the internet, and the facts for it (DECISIONS N). What it
left for later is in the rows it touches: search with a key waits for the vault, pages that need scripts
wait for the browser.

Also done on 2026-11-21: the whole of what a program printed, and what a change changed (DECISIONS T18,
T19).

Also done on 2026-11-21: the models browser and downloader (DECISIONS H).

Also done on 2026-11-21: the vault, and Anthropic, Gemini and OpenRouter with what they cost (DECISIONS U,
research note 18).

Also done on 2026-11-21: Claude Code and OpenCode (DECISIONS U15 to U25).

Also done on 2026-11-22: settings for each model, from what each maker recommends (DECISIONS K26 to K30).

Next, in this order unless the owner says otherwise: the sandbox on Windows (it needs the owner's view
first: note 05's first step guards only writes, and changes the folder's permissions on the person's own
disk); then plan item 7.

## Set aside for later

Plans the owner wants tried further down the road are in `docs/LATER.md`. So far: words chosen for each
model, from a short trial when a model is first used.

## Needs the owner before it can start

- **Before the first release:** did the previous Demido check `elpideus/demido-studio` for updates? That name
  now belongs to this repository, so an old installation that checks it would be offered this project's
  releases. I have not looked at the old code and will not. No release exists yet, so nothing is offered today.
- **Search, for your eye.** It now works as the previous version did, a ladder of sources (DECISIONS N6),
  with two things left out on purpose: a browser's name in place of Demido's, and Brave's and Bing's
  pages (N7). Say so if you want either otherwise.
- **Running Claude Code inside Demido, formally.** Anthropic's legal page says that running Claude Code in a
  product "requires agreeing to" its Commercial Terms and two conditions (the program unchanged with every way
  of signing in kept; each person signs in and is billed themselves). Demido meets the two conditions as
  built (DECISIONS U15). Whether the project accepts the Commercial Terms, and whether to ask Anthropic to
  confirm this design in writing, is yours to decide before a release offers it (note 06, open question 2).
- **A first real request to a provider.** Everything for Anthropic, Gemini and OpenRouter is built and
  tested against stand-ins that answer the way their documentation shows. A real request needs your own key,
  which I may not enter for you, and it costs money (OpenRouter's free models cost nothing). When you add a
  key on the Providers screen (or `demido keys add anthropic`), one short question in a session is the trial;
  tell me what it said if anything looks wrong.
- Signing the Windows and macOS builds. It costs money every year (DECISIONS R7).
- A usage policy for the name and logo, to replace the holding notice (DECISIONS A13).
- Whether to rerun the research sweep or let it be done directly (the earlier run stalled and has ended).
- The eight items in the kit's "For your review" section.
- What a voice interruption does to a running task (IDEAS 42).
- Whether sixteen places in the navigation is right, or services should appear only once connected (IDEAS 58).
