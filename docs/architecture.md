# Architecture

Demido Studio is two desktop programs built from one workspace: the **installer**, which puts a
working AI stack on the machine, and the **app**, which uses it. Nothing is installed from inside
the app; everything it needs is laid down by the installer (or by `pnpm dev:runtime` in
development) and described in `install.json`. Updates keep to that: the app only downloads the new
installer and hands over to it.

```
installer ──(catalog + hardware)──► install folder ──(install.json)──► app
                                        │                              │
                                        ├─ runtimes/llama.cpp          ├─ llama-server   (local models)
                                        ├─ tools/{node,uv,python}      ├─ market.mjs     (Node sidecar)
                                        ├─ resources/{sidecars,skills} ├─ python         (run_python tool)
                                        └─ demido-studio.exe           └─ Gemini API     (cloud models)
```

## The installer

`apps/installer` is a Tauri app with a React wizard. The installation itself is not in the
installer: it is the `demido-provision` crate, which the wizard, `demido-setup-cli --dev` and
the uninstaller all drive the same way.

1. **Hardware** (`crates/hardware`). NVIDIA GPUs come from `nvidia-smi` (driver version,
   compute capability, VRAM), every GPU from DXGI on Windows (vendor, dedicated memory), and
   `/sys` on Linux. Unified memory on Apple Silicon counts as VRAM.
2. **Choices** (`crates/catalog`). `catalog/runtimes.json` pins one llama.cpp release and every
   asset of it with its size and SHA-256; `catalog/models.json` lists model tiers by the VRAM
   they need, each with a Qwen and a Gemma pick from unsloth's quantized GGUFs, and the search
   models that find passages of attached files by meaning.
   `plan::backend_choices` rates every runtime for the machine (recommended, works, unsupported
   with a reason) and `plan::recommend_models` picks the tier. The CPU runtime is offered but
   never recommended; it is preselected only when no GPU runtime works on the machine.
3. **Plan** (`provision::plan`). The choices become an ordered list of steps: app files,
   runtime, Node, uv, Python (depends on uv), the starter model, the search model, shortcuts,
   the uninstall entry, and the manifest. App files and the manifest are critical; any other step can fail without
   losing the rest, and its error is shown on the last page.
4. **Run** (`provision::runner`). Each step reports progress as `SetupEvent`s. Downloads
   (`crates/fetch`) resume from a `.part` file and are verified against the pinned checksum
   before they are used. A log is written to `<install>/logs/install.log`.

The app itself travels inside the installer: `scripts/build-release.mjs` zips the built app and
its resources, and the installer compiles the zip in with `include_bytes!`. One file to
download, and the installer still fetches every large dependency from its official source.

Scope: a per-user install goes to `%LOCALAPPDATA%\Programs\Demido Studio` with no elevation. A
system-wide install relaunches the installer elevated (`--resume` with the choices in a file)
and goes to `%ProgramFiles%\Demido Studio`. An elevated installer launches the app through
Explorer, so the app never inherits administrator rights.

**The install folder only ever loses what setup put there** (`provision::folder`). Setup
refuses a folder that already holds other files or overlaps the data folder, in the wizard
and again in the engine. It records every top-level entry it creates, first in a
`.demido-setup` marker (so an interrupted run still has a record) and then in the `created`
list of `install.json`. Anything not on that record is someone else's: setup never claims,
overwrites or deletes it. Uninstall removes only the recorded entries, keeps `install.json`
until everything else is gone, and deletes the folder only when it ends up empty; the
uninstaller removes itself after it exits.

**Updating.** Setup also updates an installation to the app it carries, keeping every choice made
when it was installed (scope, runtime, models folder), which it reads back from `install.json`.
There are two ways in:

- A person runs Setup where an installation exists: the Welcome page offers **Update** instead of
  the full wizard. This is how installations older than 0.4.0, which have no updater, move on.
- The app starts it with the update command line (`demido_core::setup_args`):
  `--update --dir <install folder> --wait-pid <app pid> --relaunch`. Only a small "Updating Demido
  Studio" window shows.

Setup first waits for the app to exit, and for anything else still running from the install
folder. The update plan then runs the app files, the runtime and the tools (each skipped when the
version the catalog pins is already installed, so an update downloads only what changed; a
runtime build the catalog no longer has is replaced by the same backend's current build), the
entry in Installed apps (new version and size) and the manifest. It downloads no starter model and
makes no shortcut again, so one the person deleted stays deleted. It downloads the search model
when the installation has none (it came after the first releases), and keeps the one it has. The app files are swapped, never
overwritten in place: the payload is unpacked into a scratch folder inside the install folder,
then each top-level entry takes the place of the old one, which moves into a backup folder. When a
move fails, everything moves back, so an update that cannot finish leaves the previous version
whole rather than a mix of both. A machine-wide installation relaunches Setup elevated, as a
system-wide install does. With `--relaunch`, the updated app starts again at the end.

Setup also copies itself into the install folder as the uninstaller. The copy, not the file it
came from, is checked against the release signature the app keeps next to every installer it
stages (`<installer>.sig`), whenever one is there. Setup running elevated for an update the app
started requires it: it then runs from the app's updates folder, which the user can write to, and
puts its copy where only administrators can, so a copy that does not match the signature stops
the update before anything is swapped ("The setup file changed while it ran").

When an update fails, the window says what happened, and whether the previous version is still
whole (a swap that could not move everything back says so; running Setup again finishes it).
**Open Demido Studio** starts the app with `--skip-update` (`setup_args::SKIP_UPDATE`), so it
opens instead of installing the same update again at once (see [Updates](#updates)). Closing the
window never ends Setup halfway through a run: the window hides and the process ends with the run.
A run the person started stops between steps, as with Cancel; an update the app started finishes
out of sight and starts the updated app, as its window would have.

**One setup at a time.** From the moment it starts changing an installation until that run ends,
Setup holds a lock (`brand::SETUP_LOCK`, a named mutex, `provision::system::SetupLock`). A second
Setup refuses to install or update while it is held (a second `--update` just exits: the first one
opens the app when it is done), and the app exits at startup while it is held
(`updater::setup_is_running`), so it never opens from files that are being replaced.

## The app

### Backend (`apps/studio/src-tauri`)

`lib.rs` resolves paths, opens storage and starts the services. No service blocks startup; one
that fails is logged and the app opens anyway, showing what is missing where it matters.

| Module | Responsibility |
|---|---|
| `paths` | Install folder (from `install.json`), data folder (`%LOCALAPPDATA%\Demido Studio`) |
| `db` | SQLite (WAL): chats, messages, attached files, their passages (FTS5) and the passages' vectors, and a trace per model call. Numbered migrations |
| `settings`, `secrets` | `settings.json`; API keys and the TradingView session in the OS credential store |
| `runtime` | One `llama-server` process at a time, restarted only when the launch settings change |
| `models` | GGUF discovery (built-in folder, extra folders, LM Studio), GGUF metadata, cloud models, per-model overrides, what each model can do, Hugging Face search and downloads |
| `providers` | Gemini configuration and model listing |
| `llm` | Provider-neutral `ChatRequest` → llama.cpp (OpenAI-compatible SSE) or Gemini (native SSE); streams `StreamEvent`s and returns the exact request for the trace |
| `agent` | The turn loop: prompt → model → tool calls → results → model, until it answers |
| `attachments` | Files attached to messages: staging, moving them into the chat's workspace, what the model reads of them (full text, passages, images and sound), passage search |
| `tools` | Market data, Python, terminal commands, workspace and attached files, skills. Grouped for the Tools menu |
| `shell` | The person's own shell, found once; a command run in a pseudo-terminal and read back as the screen shows it |
| `skills` | Skill folders, enable/disable, and a file watcher that updates the UI live |
| `market` | The Node sidecar's lifecycle and protocol, TradingView sign-in |
| `updater` | The releases feed, downloading and verifying a new installer, staging it, handing over to it (see [Updates](#updates)) |
| `commands` | The functions the UI calls, one file per area |

**A turn.** `agent::run_turn` builds the system prompt (identity, date, enabled skills, tool
guidance) and fits the history into the model's context window, newest first. The model's
answer streams to the UI as `ChatEvent`s while it is written to the database, so a crash loses
nothing that was shown. Tool calls run one by one; `run_python` and `run_command` ask for approval first
unless the person chose "always allow". Each model call is saved as a trace (exact request, response,
token counts, speed) that the Inspector window shows. Two things are shortened, so a chat with
files does not store them again for every model call: images and sound are replaced by their type
and size (`llm::redact_media`), and an attached file's text is kept whole only in the first call of
the turn that sends it (see Attached files, below).

**Prompt cache.** The system prompt contains the date but not the time, and tools are listed in
a fixed order, so from one turn to the next the prompt prefix is identical and llama.cpp reuses
its cache instead of re-reading the conversation.

**Local runtime.** `llama-server` runs on `127.0.0.1` with a random free port, one slot,
`--jinja` for the model's own chat template, `--reasoning-format deepseek` to separate thinking
from the answer, and llama.cpp's automatic fitting to split layers between GPU and CPU when
the model does not fit in VRAM. On Windows every child
process is in a job object that kills it when the app exits, however the app exits.

**What models can do.** Every model carries its capabilities (`models/capabilities.rs`): vision,
audio, tools and thinking, each yes, no or unknown. The UI shows them as icons in the model picker
and the Models tab, the full list in the model editor, and the Tools picker says when the chosen
model cannot use tools. Nothing is guessed from names or templates:

- A local model is asked of llama.cpp once it is on disk (`models/probe.rs`). A background task
  starts `llama-server` with the model and its projector on the CPU only (`--device none`, no
  warmup, no prefetch of the file, below-normal priority), reads `modalities` and
  `chat_template_caps` from `/props` and the thinking verdict llama.cpp logs at start, then stops
  it: a few seconds per model, one model at a time, no GPU memory. The answer is kept in
  `cache/capabilities.json` for that file, projector and llama.cpp build, so it is asked again
  only when one of them changes; a model that does not load is asked again at the next start.
  Deleting a model pauses the checks first, since Windows refuses to delete a file llama.cpp has
  open. A chat loads a model's projector (`--mmproj`) once llama.cpp has said the model reads
  images or sound with it, so attached pictures reach it; a projector llama.cpp refused is never
  loaded, and the model starts without it. With a projector, llama.cpp decodes a picture in one
  micro-batch, so the micro-batch is 2,048 tokens and so is the most a picture may take: a
  1380×880 screenshot is 545 tokens for Gemma 4 26B, more than the default 512, and stopped the
  server.
- A Gemini model's thinking comes from Google's model list; tools and image and audio input come
  from [models.dev](https://models.dev), an open database of model specifications. Its Google
  entries are kept in `cache/models-dev.json`, refreshed weekly (and when a provider is added or
  its models refreshed), with an ETag so an unchanged catalog is not downloaded again. A model
  models.dev does not list keeps only what Google says.

**Gemini.** Answers that fail as "busy" (429 or 5xx, before anything was shown) are retried up
to four times with growing waits. A model the API key cannot use is turned off, with a note in
the chat, instead of failing every time; an empty or refused answer says why.

**Market data.** `sidecars/market` is a small Node program (bundled to one file) that speaks JSON
lines over stdio: requests with an id, responses with the same id, and events for live streams,
downloads and store changes. TradingView-API supplies real-time quotes and candles; Dukascopy's
public candle API supplies deep history without an account, for everything but stock and ETF
CFDs (not split-adjusted), whose history comes from TradingView. History lives in a local store
(`src/store/`) and is never requested twice:

- History is 1-minute candles, whatever timeframe asks: every timeframe is built from them, so one
  download serves them all and every timeframe shows the same coverage and the same gaps.
  `dukascopy-store` keeps each raw API bucket gzipped (`m1`: a UTC day of 1-minute candles; the
  store can hold `h1` and `d1` buckets too, but nothing fetches or reads them, and the ones earlier
  versions downloaded are deleted at start), with a manifest per instrument of what was fetched,
  what came back empty, what is provisional (a copy built before its day was complete, served
  meanwhile and fetched once more when a complete one can exist: CloudFront keeps a copy for 7 days)
  and what Dukascopy does not have.
  `tv-store` keeps TradingView bars per symbol and timeframe with the time ranges they cover.
- `series` is the one read path, from the store only. A timeframe is built by aggregating the
  stretch's 1-minute candles once; TradingView wins wherever it has coverage for that timeframe.
  Reads return the bars, `spans` (which source each run of bars came from), and whether more is
  stored before the oldest bar, or a gap to download. `reads` fetches what a chart shows by itself:
  the same recent stretch whatever the timeframe (from the newest stored data up to now, and at least
  the last week; anything older is an explicit download), and a tool read re-fetches the provisional
  copies it would show once complete ones exist; a sweep does the same for every due copy at
  start-up and every half hour.
- `fetcher` is the single queue every Dukascopy request goes through: deduplicated per bucket,
  lanes (chart and tool reads, then buckets a tool waits on, then background downloads), a token
  bucket that adapts to 429s and remembers the rate that was safe (a 429 at or below it lowers
  both), a circuit breaker for outages. An answer is stored only under the bucket it belongs to, and
  "From time is too late" just after a close is retried, not recorded as missing.
- `planner` estimates a download (requests, bytes, time) from the 1-minute days that are missing,
  counting a due provisional copy as missing; a range starts at a date, at the history's start, or
  `back` a month or a year before the stored 1-minute history (the chart's "1 more month / year"),
  and reaches now unless it names an end. `jobs` runs downloads newest first, resumable across
  restarts and shared between overlapping requests. Every planned day stored while a job runs or
  waits paused counts for it with its bytes, whoever fetched it, so its total holds; days the
  source has nothing for (before a learned start) are counted apart as `skipped`. After 20 empty
  weekdays a job probes one weekday a month; if the probes reach the history's start without data,
  that start is learned and older days are skipped. A TradingView download (stocks and ETFs, which
  TradingView serves per timeframe) pages every timeframe back to its start and fills the holes
  between stored runs afterwards. The old flat cache is migrated once by `migrate` before anything
  is served.

The backend's `commands::market` passes the store's calls through for the chart and the Data tab.
The assistant's market tools share `ensure_downloaded`: plan the 1-minute history that is missing,
ask the person when the estimate is over their limit (`ToolContext::request_approval`), start or
join a job, show its progress on the tool's card and wait up to 90 s before letting it finish in the
background.
A wait for one timeframe pushes that timeframe's files ahead of the rest (`download.wait` with
`boost`); a wait for the whole job only watches. When the person pauses or cancels, or the job
fails, the tool result says so plainly with the job's own counts. At launch the service starts
early when a download was left running; on exit it is asked to flush and stop. The TradingView
session is captured by `market::auth`: a separate window with its own browser profile shows
TradingView's sign-in page, the `sessionid` cookies are read from that profile, checked by the
sidecar, and saved in the credential store. When a market tool needs a session and there is none,
the sign-in window opens; if the person does not sign in within the timeout, the tool result tells
the model to ask them to.

**Commands.** `run_command` runs a command line in the person's own shell (`shell/`): PowerShell
7 when it is installed (on the PATH, where the Microsoft Store puts an app execution alias, or in
Program Files; each candidate is asked its version once, at startup), Windows PowerShell 5.1
otherwise, the login shell on macOS and Linux. The environment is the one a new terminal window
gets, read from the registry, so a program installed while the app runs is found. The command runs
in a 160×40 pseudo-terminal (ConPTY through `portable-pty`), so programs print what they print for
a person, and a terminal emulator (`avt`) turns the output into what the screen shows: a redrawn
progress bar is its last state, colours are gone, a full-screen program such as btop is its current
screen. The model reads that text (clipped, keeping both ends) and the exit code; the chat shows it
live, with a Stop button (`stop_tool`) that ends the command and lets the turn go on with what it
printed. PowerShell gets the command base64-encoded (`-EncodedCommand`, with `-NoProfile
-NonInteractive -ExecutionPolicy Bypass`) after one line that makes its output UTF-8, and exits
with the failing program's own exit code. A command ends at its timeout (120 s unless the model
asks for up to an hour); ending it ends every process it started (a job object on Windows, the
process group elsewhere), while a command that finishes by itself leaves a program it opened in a
window of its own running. It starts in the chat's workspace unless the model names a folder, and
files it creates there show on its card.

**Attached files.** The composer's **+** button, drag and drop onto the window, and pasting add
files to a message (at most 20, 100 MB each). The file is copied to `staging/<id>/` and read once
by `crates/extract`, two files at a time: its kind (from its content, then its name), the text a
model can read (PDF pages, Word, PowerPoint slides, OpenDocument, EPUB, RTF, spreadsheet sheets
as CSV, web pages, text and code, with a `--- Page N ---` line at each page, slide or sheet), that
text split into passages of about 650 tokens, and what a model is given of a picture (a JPEG or
PNG at most 1568 pixels on its long side, upright, since llama.cpp cannot decode WebP) or of a
sound (the recording itself, up to 10 MB). All of it is stored with the attachment, so it never
changes whatever later happens to the file. Passages live in `attachment_passages`, indexed by
the FTS5 table `passages_fts` (BM25, Porter stemming). The chip then shows pages and tokens, or
why the file cannot be read (a scanned PDF has no text). A pasted file is marked as coming from
the internet (Windows' Mark of the Web), as a download would be. Sending checks every file first,
then moves each into the chat's workspace, `uploads/<name>`, where Python, commands and
`read_file` can use it too, and links it to the message in the order it was attached. A name the
chat used before is never reused. Files left in the composer are forgotten at the next start.

What the model reads is decided when the prompt is built (`attachments::context`). Each user
message with files gets an `<attachments>` block before its text, one `<file>` per file with its
name, workspace path and type. Files with text are included in full, smallest first, while they
fit in three fifths of the room the history has (the context window less the system prompt, the
tools and the answer's reserve; at most 100,000 tokens), next to the message's own text and
pictures. The rest show only passages: the best matches for the message (see Search by meaning,
below) and each file's opening passage, within a quarter of that room, and at least one passage
per file however small the room is; a message with nothing to search for ("summarise this") gets
each file's opening. A later message is shown passages of those long files when they are about
what it asks, or at least two of its words match them, so a follow-up question about a long
document finds its answer and small talk pulls nothing in. The passages a message is shown are
chosen the first time its prompt is built and stored (`message_passages`): BM25's statistics
change whenever any file is added, the search model can change, and a message must keep showing
what the model answered from. A regenerate on another model only adapts the
choice: it is cut to that model's room in the order it was made, keeping a passage of every file,
and a file too long for it but not for the one that chose gets passages of its own, stored too.
Everything else depends only on the stored messages, their files and the model, so the prompt of
one step is a prefix of the next and llama.cpp keeps its cache. The model searches further with
`search_files` and reads with `read_file`, by pages where a file has them (PDFs, presentations,
spreadsheets) and by lines otherwise. `read_file` reads any binary file through `crates/extract`,
attached or not: the text stored at attach time while the file keeps its size, otherwise read
again. A long table shows its first lines and points at `run_python`.

**Search by meaning.** Words alone miss a question asked in other words or another language:
"How much money a year does Mr. Darcy have?" shares no word with "ten thousand a year". A search
model (`attachments::meaning`) turns every passage into a vector, and the question into another;
a passage whose vector points where the question's does is about what it asks. It is a small
embedding model from `catalog/models.json`: Qwen3 Embedding 0.6B when the GPU the runtime uses
has 10 GB, EmbeddingGemma 300M otherwise and on the CPU. Setup downloads it; an installation
without one gets it with its next update, and searches by words until then. The app finds it in
the model folders by its `<repo>/<file>` and keeps embedding models out of the model list (a GGUF
with `<arch>.pooling_type` cannot chat). It runs in a `llama-server` of its own (`--embedding`,
one 2048-token slot, the micro-batch the catalog gives, no prompt cache), started when there is
something to embed and stopped after three idle minutes; its log is `logs/search-server.log`.
The chat model comes first: before a local one loads, the search model finishes the request in
hand and stops, and it cannot start again until the chat model has loaded, so the chat model gets
the GPU memory it would get alone. Started again when next needed, the search model is fitted by
llama.cpp into what is left (`--fit`, which keeps 1 GiB free), on the CPU when nothing is. Which
search model is used does not change with the chat model, since that would index every file again. An
indexer embeds passages in the background, the newest file's first, in batches of eight, with
the document prompt the model card asks for, into `passage_vectors` (little-endian f32s, one row
per passage and model: vectors of one model do not compare with another's, so a change of model
indexes again). A passage the model cannot take is cut shorter, and given an empty vector, which
matches nothing, when even 200 characters fail. When the passages of a message are chosen, the
question is embedded with the model's query prompt, after waiting up to 30 seconds for the chat's
files to be indexed. Passages are then ranked by their cosine similarity, scaled to 0–1 between
the least and the most similar, weighed 0.85 against their BM25 score over the best one's: on
fourteen questions with known answers in a novel and a paper, words alone found 7 answers in the
first three passages, this mix 10. A passage matches when its similarity reaches the model's
`relevance` (0.5 for Qwen, 0.52 for Gemma; unrelated questions measured 0.26–0.45, related ones
0.62–0.73) or it has words of the question. An earlier message's files show only passages that
match. The message's own files, and `search_files`, show the best passages once any matches:
asked in Italian about an English paper, the passage that answers scored 0.47 and came second,
while "summarise this" matches nothing and gets the openings. While any passage searched has no
vector, and without a search model, BM25 alone ranks them.

**Reading files safely.** Attached files come from anywhere, and some parser failures cannot be
caught inside a process: a PDF whose form draws itself overflows pdf-extract's stack, and a
parser can loop or ask for more memory than there is. So the app reads every file in a child
process: itself, started with `--demido-extract <file>` (`main.rs` handles that before anything of
the app starts), tied to the app's job object, and given three minutes. A child that dies or runs
out of time costs only that file its text; the chip says it could not be read. Inside the reader,
every parser also runs behind `catch_unwind`, Excel sheets are read cell by cell (calamine's
ranges are dense grids, so one value in a sheet's far corner would ask for gigabytes), one file's
ZIP parts may inflate to 1 GB in all, and a part is read once however often a file lists it.

Pictures go to models that see (`image_url` data URLs for llama.cpp, `inlineData` for Gemini),
counted by their size in 28-pixel patches (local models) or 768-pixel tiles (Gemini); sound goes
to models that hear (WAV and MP3 for llama.cpp), counted at 32 tokens a second, the rate Google
documents. One request carries at most 15 MB of them, the newest first. Any other model is told
the file is in the workspace. When the history does not fit, older messages' files shrink to a
line naming each file before whole turns are dropped.

File text is material, not the person's words: the system prompt says so, and a document that
contains `</file>` or `</attachments>` cannot close the block, since those tags in file text are
escaped. For the same reason the assistant's Markdown never loads a picture from the web by
itself (a file could make the model write one whose address carries the conversation away): it
shows a link that opens in the browser. A trace keeps a file's text whole only in the first model
call of the turn that sends it; later calls note how much was left out.

**Skills.** A skill is a folder with `SKILL.md` (frontmatter `name`, `description`) and any
files it mentions. Enabled skills' instructions go into the system prompt; their other files are
read with `read_skill_file` or run with `run_python` (`skill:<id>/<file>`). `create_skill`
writes a new folder, copies the scripts it references from the chat's workspace, and rewrites
references to point at its final id. The watcher (debounced `notify`) picks it up and the UI
updates without a refresh.

### Frontend (`apps/studio/src`)

React with zustand stores, CSS Modules and the tokens in `packages/ui`.

| Folder | What |
|---|---|
| `shell` | Activity bar (Chats, Market, Settings), the safety notice, toasts |
| `chat` | Chat list, message list (markdown, math, code, tool cards, thinking; runs of file calls fold into one card, `steps.ts`), composer, model and tools pickers, attached files (`Attachments.tsx`: the composer's tray and a sent message's files; `attachmentView.ts`: what a chip says) |
| `wm` | The window manager: `WindowFrame` (title bar, drag, resize edges, snap), `SnapLayouts` (the pinning flyout), `WindowLayer`, `TabbedLayout` (tab rail on the left, icons only in narrow windows). `geometry.ts` holds the pure math, unit tested |
| `settings` | Providers, Models (list, editor, download; `Capabilities` draws what a model can do, here and in the model picker), Skills, General, Updates |
| `market` | The Market window's tabs. Chart: symbol search, live chart (Lightweight Charts), timeframes, paging back through stored history, the download popup where it ends. Data (`data/`): what is stored per market on one timeline coloured by source (every timeframe reads the same 1-minute history), downloads, delete. `DownloadProgress` is the progress bar the chart, the Data tab and chat cards share |
| `inspector` | A turn's traces: request, response, timings |
| `stores` | App state per area; `windows.ts` holds window geometry, focus order, pinning |

**Windows.** A window floats, maximizes, or is pinned to a slot, like Windows 11's snap layouts
(hover Maximize, drag to an edge or corner, or right-click the title bar). A slot is a column on
the left or right, whole or split into a top and a bottom half, or a row above or below the chat
between the columns. Pinned windows share the screen with the chat, which reflows around them;
they stay resizable along the edges that face the chat or their partner, and both halves of a
column resize together. An empty half offers the other windows (snap assist). Maximizing a pinned
window keeps its slot, and Restore puts it back there unless another window has taken it since.
Dragging a pinned or maximized window's title bar tears it off at its floating size. Each kind of
window has a maximum size (`WINDOW_SPECS`), past which its content only stretches apart: it limits
floating resizes and the draggable edge of a pinned one, not Maximize. What is on screen is always
the geometry each window was given (`WindowState.wanted`) fitted into the desktop, so a desktop that
is smaller for a while, such as the app window before it is maximized or snapped, squeezes windows
without losing their size, and the saved layout keeps what they were given. When two pinned columns
(or rows) no longer both fit, the one sized most recently keeps its size. Windows cannot be minimized; the
navigation rail's button closes a window that is in plain view and brings a covered one forward.
A closed window reopens where it was, pinned or not.

### Updates

The app updates itself from the GitHub Releases of `elpideus/demido-studio`
(`src-tauri/src/updater`, the Updates tab in Settings). Every release carries two files: the
installer people download, `Demido-Studio-Setup-<version>.exe`, and its signature, the same name
plus `.sig`. The release workflow builds the installer and signs it in a separate job (see
[development.md](development.md#releases)).

- **Feed and channels.** The updater reads the list of releases from GitHub's API
  (`DEMIDO_UPDATE_FEED` names another base URL, for testing; the app leaves it out of the
  installer's environment, so the app the installer starts again is back on GitHub's feed). The
  Release channel offers the newest stable release newer than the running version; Pre-release
  offers the newest of stable releases and pre-releases. A release counts only with both files at a
  GitHub address; the running version and older ones are never offered. Moving to the Release
  channel drops a pre-release that was found, is downloading or is staged, and a pre-release never
  installs on the Release channel, however it came to be staged.
- **Verification.** The `.sig` is a minisign signature made with the project's update key, whose
  public half is compiled into the app and into Setup (`demido_core::signature`). Its trusted
  comment, which the signature covers, names the
  file and the version (`tauri signer sign --app-version`), and the app accepts a signature only
  for the exact file and version it was offered, so a validly signed older installer cannot be
  passed off as a newer one. When GitHub lists an asset's SHA-256, the download is checked against
  it too. A staged installer is verified again right before it runs.
- **Staging.** A download goes into the data folder's `updates/`, resuming from a `.part` file
  after an interruption. Once the signature checks out, it is written next to the installer as
  `<installer>.sig` (setup checks the uninstaller it copies against it, see
  [Updating](#the-installer)) and `pending.json` records the update as ready. At every launch the
  app tidies the folder: an update that is now the running version has its files removed, and one
  that is no longer newer, or no longer verifies, is deleted; an installer and its `.sig` stay or
  go together. A development build shares the data folder with the installed app but keeps its
  updates in `<repo>/.dev/updates`, so it never tidies, fills or deletes the installed app's.
- **Automatic mode** (the default) works like Discord: a quiet check shortly after launch and then
  every hour, the download straight away, and the install at the next launch, before the window
  opens, or at once with **Restart and update**. Only per-user installations install at launch: a
  machine-wide one would raise a Windows permission prompt out of nowhere, so it waits for a
  click. Each launch that starts the installer counts as an attempt. When the update fails,
  setup's **Open Demido Studio** starts the app with `--skip-update`, and that launch installs
  nothing, so the app opens. A later ordinary launch may try once more; after two attempts in all
  the update waits for a click, so a broken installer cannot keep the app from opening.
- **Manual mode.** **Check for updates**, then **Update** downloads and verifies the new version;
  once it is ready, the app restarts to install it, asking first while a reply is still being
  written. Declining leaves the update ready for a click.
- **Installing** starts the verified installer with the update command line from
  `demido_core::setup_args` and exits; the installer takes it from there (see
  [Updating](#the-installer) above):

  ```
  Demido-Studio-Setup-<version>.exe --update --dir "<install folder>" --wait-pid <app pid> --relaunch
  ```

  Only an installation made by Setup can update itself: the installer needs its `install.json`,
  and the running executable must be the one in that installation. A development build checks and
  downloads (into `.dev/updates`) but never installs.

## Data

Everything a person makes is in the data folder, never in the install folder:

| Path | What |
|---|---|
| `demido.db` | Chats, messages, traces |
| `settings.json`, `models.json`, `providers.json`, `skills.json` | Preferences and overrides |
| `skills/` | Skills (default ones are copied here on first run) |
| `models/` | Downloaded models (per-user installs) |
| `workspaces/<chat>/` | Files tools produce for a chat: data CSVs, charts, scripts, what commands download; `uploads/` holds the files sent with its messages |
| `staging/` | Files added in the composer and not sent yet; emptied at every start |
| `avatars/` | Model pictures |
| `webview-tradingview/` | The TradingView sign-in browser profile |
| `cache/market/dukascopy/` | Downloaded Dukascopy history, 1-minute candles: `<instrument>/m1/<year>/<day>.json.gz` and a `manifest.json` per instrument; `stats.json` holds the learned request rate |
| `cache/market/tradingview/` | TradingView bars stored from charts and downloads, one file per symbol and timeframe, and `index.json` with what they cover |
| `cache/market/jobs/` | One file per history download, so downloads resume after a restart and chat cards find them |
| `updates/` | A downloaded update: the installer (a `.part` file while it downloads), then its `.sig` and `pending.json` once its signature is verified. Emptied once the update is installed. A development build uses `<repo>/.dev/updates` instead |
| `logs/` | App log, rotated daily |

Secrets are never in files: the Gemini API key, a Hugging Face token and the TradingView session
are in Windows Credential Manager (service `Demido Studio`).
