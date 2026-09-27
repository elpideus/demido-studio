# Architecture

Demido Studio is two desktop programs built from one workspace: the **installer**, which puts a
working AI stack on the machine, and the **app**, which uses it. Nothing is installed from inside
the app; everything it needs is laid down by the installer (or by `pnpm dev:runtime` in
development) and described in `install.json`.

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
   they need, each with a Qwen and a Gemma pick from unsloth's quantized GGUFs.
   `plan::backend_choices` rates every runtime for the machine (recommended, works, unsupported
   with a reason) and `plan::recommend_models` picks the tier. The CPU runtime is offered but
   never recommended; it is preselected only when no GPU runtime works on the machine.
3. **Plan** (`provision::plan`). The choices become an ordered list of steps: app files,
   runtime, Node, uv, Python (depends on uv), the starter model, shortcuts, the uninstall entry,
   and the manifest. App files and the manifest are critical; any other step can fail without
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

## The app

### Backend (`apps/studio/src-tauri`)

`lib.rs` resolves paths, opens storage and starts the services. No service blocks startup; one
that fails is logged and the app opens anyway, showing what is missing where it matters.

| Module | Responsibility |
|---|---|
| `paths` | Install folder (from `install.json`), data folder (`%LOCALAPPDATA%\Demido Studio`) |
| `db` | SQLite (WAL): chats, messages, and a trace per model call. Numbered migrations |
| `settings`, `secrets` | `settings.json`; API keys and the TradingView session in the OS credential store |
| `runtime` | One `llama-server` process at a time, restarted only when the launch settings change |
| `models` | GGUF discovery (built-in folder, extra folders, LM Studio), GGUF metadata, cloud models, per-model overrides, Hugging Face search and downloads |
| `providers` | Gemini configuration and model listing |
| `llm` | Provider-neutral `ChatRequest` → llama.cpp (OpenAI-compatible SSE) or Gemini (native SSE); streams `StreamEvent`s and returns the exact request for the trace |
| `agent` | The turn loop: prompt → model → tool calls → results → model, until it answers |
| `tools` | Market data, Python, workspace files, skills. Grouped for the Tools menu |
| `skills` | Skill folders, enable/disable, and a file watcher that updates the UI live |
| `market` | The Node sidecar's lifecycle and protocol, TradingView sign-in |
| `commands` | The functions the UI calls, one file per area |

**A turn.** `agent::run_turn` builds the system prompt (identity, date, enabled skills, tool
guidance) and fits the history into the model's context window, newest first. The model's
answer streams to the UI as `ChatEvent`s while it is written to the database, so a crash loses
nothing that was shown. Tool calls run one by one; `run_python` asks for approval first unless
the person chose "always allow". Each model call is saved as a trace (exact request, response,
token counts, speed) that the Inspector window shows.

**Prompt cache.** The system prompt contains the date but not the time, and tools are listed in
a fixed order, so from one turn to the next the prompt prefix is identical and llama.cpp reuses
its cache instead of re-reading the conversation.

**Local runtime.** `llama-server` runs on `127.0.0.1` with a random free port, one slot,
`--jinja` for the model's own chat template, `--reasoning-format deepseek` to separate thinking
from the answer, and llama.cpp's automatic fitting to split layers between GPU and CPU when
the model does not fit in VRAM. On Windows every child
process is in a job object that kills it when the app exits, however the app exits.

**Gemini.** Answers that fail as "busy" (429 or 5xx, before anything was shown) are retried up
to four times with growing waits. A model the API key cannot use is turned off, with a note in
the chat, instead of failing every time; an empty or refused answer says why.

**Market data.** `sidecars/market` is a small Node program (bundled to one file) that speaks JSON
lines over stdio: requests with an id, responses with the same id, and events for live streams,
downloads and store changes. TradingView-API supplies real-time quotes and candles; Dukascopy's
public candle API supplies deep history without an account, for everything but stock and ETF
CFDs (not split-adjusted), whose history comes from TradingView. History lives in a local store
(`src/store/`) and is never requested twice:

- `dukascopy-store` keeps each raw API bucket gzipped, at three tiers (`m1` a UTC day of 1-minute
  candles, `h1` a month of hourly ones, `d1` a year of daily ones), with a manifest per instrument
  of what was fetched, what came back empty, what is provisional (a copy built before its bucket was
  complete, served meanwhile and fetched once more when a complete one can exist: CloudFront keeps
  a copy for 7 days) and what Dukascopy does not have.
  `tv-store` keeps TradingView bars per symbol and timeframe with the time ranges they cover.
- `series` is the one read path, from the store only. A timeframe is built from the coarsest tier
  with data for each stretch of time (a fetched-empty coarse bucket is not coverage while the finer
  tier there is unfetched), merged and aggregated once; TradingView wins wherever it has coverage.
  Reads return the bars, `spans` (which source each run of bars came from), and whether more is
  stored before the oldest bar, or a gap to download. `reads` fetches what a read needs first: the
  newest bars walk back over a closed market (a weekend, a holiday) to the last trading ones, and a
  tool read re-fetches the provisional copies it would show once complete ones exist; a sweep does
  the same for every due copy at start-up and every half hour.
- `fetcher` is the single queue every Dukascopy request goes through: deduplicated per bucket,
  lanes (chart and tool reads, then buckets a tool waits on, then background downloads), a token
  bucket that adapts to 429s and remembers the rate that was safe (a 429 at or below it lowers
  both), a circuit breaker for outages. An answer is stored only under the bucket it belongs to, and
  "From time is too late" just after a close is retried, not recorded as missing.
- `planner` estimates a download (requests, bytes, time) from what is missing, counting a due
  provisional copy as missing; `jobs` runs downloads of every tier for a range, coarsest tier first
  and newest first, resumable across restarts and shared between overlapping requests. Every planned
  file stored while a job runs or waits paused counts for it with its bytes, whoever fetched it, so
  its total holds; files the source has nothing for (before a learned start) are counted apart as
  `skipped`. A start is learned from an empty stretch only once the coarser tier is known empty down
  to its own start. A TradingView download that reached the start fills the holes between stored
  runs afterwards. The old flat cache is migrated once by `migrate` before anything is served.

The backend's `commands::market` passes the store's calls through for the chart and the Data tab.
The assistant's market tools share `ensure_downloaded`: plan what is missing, ask the person when
the estimate is over their limit (`ToolContext::request_approval`, with a smaller "only this
timeframe" choice, and an earlier "only" job is reused without asking again), start or join a job,
show its progress on the tool's card and wait up to 90 s before letting it finish in the background.
A wait for one timeframe pushes that timeframe's files ahead of the rest (`download.wait` with
`boost`); a wait for the whole job only watches. When the person pauses or cancels, or the job
fails, the tool result says so plainly with the job's own counts. At launch the service starts
early when a download was left running; on exit it is asked to flush and stop. The TradingView
session is captured by `market::auth`: a separate window with its own browser profile shows
TradingView's sign-in page, the `sessionid` cookies are read from that profile, checked by the
sidecar, and saved in the credential store. When a market tool needs a session and there is none,
the sign-in window opens; if the person does not sign in within the timeout, the tool result tells
the model to ask them to.

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
| `chat` | Chat list, message list (markdown, math, code, tool cards, thinking; runs of file calls fold into one card, `steps.ts`), composer, model and tools pickers |
| `wm` | The window manager: `WindowFrame` (title bar, drag, resize edges, snap), `SnapLayouts` (the pinning flyout), `WindowLayer`, `TabbedLayout` (tab rail on the left, icons only in narrow windows). `geometry.ts` holds the pure math, unit tested |
| `settings` | Providers, Models (list, editor, download), Skills, General |
| `market` | The Market window's tabs. Chart: symbol search, live chart (Lightweight Charts), timeframes, paging back through stored history, the download popup where it ends. Data (`data/`): what is stored per market, source and detail level on a timeline, downloads, delete. `DownloadProgress` is the progress bar the chart, the Data tab and chat cards share |
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

## Data

Everything a person makes is in the data folder, never in the install folder:

| Path | What |
|---|---|
| `demido.db` | Chats, messages, traces |
| `settings.json`, `models.json`, `providers.json`, `skills.json` | Preferences and overrides |
| `skills/` | Skills (default ones are copied here on first run) |
| `models/` | Downloaded models (per-user installs) |
| `workspaces/<chat>/` | Files tools produce for a chat: data CSVs, charts, scripts |
| `avatars/` | Model pictures |
| `webview-tradingview/` | The TradingView sign-in browser profile |
| `cache/market/dukascopy/` | Downloaded Dukascopy history: `<instrument>/<tier>/<year>/<bucket>.json.gz` and a `manifest.json` per instrument; `stats.json` holds the learned request rate |
| `cache/market/tradingview/` | TradingView bars stored from charts and downloads, one file per symbol and timeframe, and `index.json` with what they cover |
| `cache/market/jobs/` | One file per history download, so downloads resume after a restart and chat cards find them |
| `logs/` | App log, rotated daily |

Secrets are never in files: the Gemini API key, a Hugging Face token and the TradingView session
are in Windows Credential Manager (service `Demido Studio`).
