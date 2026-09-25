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
lines over stdio: requests with an id, responses with the same id, and events for live streams.
TradingView-API supplies real-time quotes and candles; `dukascopy-node` supplies deep history
without an account. `candles.stitch` joins the two so Dukascopy extends TradingView's history
without overlapping it. The TradingView session is captured by `market::auth`: a separate
window with its own browser profile shows TradingView's sign-in page, the `sessionid` cookies
are read from that profile, checked by the sidecar, and saved in the credential store. When a
market tool needs a session and there is none, the sign-in window opens; if the person does
not sign in within the timeout, the tool result tells the model to ask them to.

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
| `chat` | Chat list, message list (markdown, math, code, tool cards, thinking), composer, model and tools pickers |
| `wm` | The window manager: `WindowFrame` (title bar, drag, resize edges, snap), `WindowLayer`, `TabbedLayout` (tab rail on the left). `geometry.ts` holds the pure math, unit tested |
| `settings` | Providers, Models (list, editor, download), Skills, General |
| `market` | Symbol search, live chart (Lightweight Charts), timeframes, history paging |
| `inspector` | A turn's traces: request, response, timings |
| `stores` | App state per area; `windows.ts` holds window geometry, focus order, docking |

**Windows.** A window floats, maximizes, or docks to the left or right edge. Docked windows
share the screen with the chat, which reflows beside them, and stay resizable along their inner
edge. Dragging a docked or maximized window's title bar tears it off at its floating size.
Windows cannot be minimized; closing is the way out.

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
| `logs/` | App log, rotated daily |

Secrets are never in files: the Gemini API key, a Hugging Face token and the TradingView session
are in Windows Credential Manager (service `Demido Studio`).
