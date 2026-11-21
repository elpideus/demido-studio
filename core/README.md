# Demido Studio, the core

Everything that is not a window: storage, settings, the log of what happened, reading the computer's
hardware, and the one API every client talks to. Written in Rust.

**Status: the first slice.** It runs, it is tested, and it does little yet. No model runs through it.

## Run it

```bash
cargo run --manifest-path core/Cargo.toml -- serve --ui-dir apps/web/dist
```

Build the web UI first (`pnpm app:build`), or leave `--ui-dir` out to get the API alone.
Then, in another terminal:

```bash
cargo run --manifest-path core/Cargo.toml -- open
```

That opens the UI in your browser, signed in. The Setup screen now shows this computer as the core reads it.

## The command line

| Command | What it does |
|---|---|
| `demido serve` | Runs the server. `--listen 127.0.0.1:7341`, `--ui-dir <folder>`. |
| `demido status` | Says whether a server is running, and where. |
| `demido system` | This computer: processor, memory, graphics cards, and which model runtime suits it. `--json`. |
| `demido settings list` | Every setting with its value. Also `get <id>`, `set <id> <value>`, `reset <id>`. |
| `demido events` | The log of what happened. `--after <n>`, `--limit <n>`, `--json`. |
| `demido ask <question>` | Asks a model and prints its answer as it is written. `--session <id>` goes on in a session; `--model <id>` picks the model for a new one; `--think` and `--no-think` turn thinking on and off for the session; `--plan` has the model list the steps first and do them one at a time, as tasks; `--folder <path>` gives the session the one folder the model may read and change files in (`.` for where you are); `--level cautious|balanced|smart|autonomous` sets how much it may do without asking, and when it asks, you answer at the terminal. With a folder the model can also run programs there, shut in a sandbox where this computer has one; every level but Autonomous asks before each program. The tools the model uses and the tasks are printed as they happen. Ctrl+C stops the answer. Needs a running server. |
| `demido sessions` | The conversations kept on this computer. `show <id>` prints one, with how full the model is; `shorten <id>` replaces the older part with a short account for the model (the messages stay); `undo <id> [change]` puts a file back as it was before a model changed it; `delete <id>` removes one. |
| `demido models` | The models in Demido's folder and what each is doing. `load <id>` loads one and waits until it is ready; `unload <id>` ends its process and frees its memory. Loading needs a running server. |
| `demido runtime` | The program that runs models: what is installed. `available` asks its publisher what suits this computer; `install [id]` downloads, checks, unpacks and tries one; `check <id>` tries an installed one again; `stop`; `remove <id>`. |
| `demido open` | Opens the web UI in your browser, signed in. `--print` only prints the link. |

With a server running, the command line asks the server. Without one, it works on the files directly.

## What is in it

| File | What |
|---|---|
| `crates/demido-core/src/paths.rs` | Where Demido keeps its files. `DEMIDO_HOME` overrides it. |
| `…/db.rs` | The one SQLite file and its numbered migrations. |
| `…/events.rs` | The append-only log, and its live stream. |
| `…/settings.rs` | The registry of every setting, and their values. Add a setting here and it appears everywhere. |
| `…/system.rs` | Reading the hardware, and the advice on which runtime build to download. |
| `…/desktop.rs` | What the desktop looks like: its accent colour (Linux, through the desktop's portal). |
| `…/sessions.rs` | Sessions and what was said in them, kept in the one SQLite file. |
| `…/chat.rs` | Running a turn: what was said goes to the model, its answer is streamed as news and stored whole. Also making a long conversation shorter: the account, when it is written, and what stays word for word. |
| `…/chat/turn.rs` | What a turn does before its answer: asking whether a tool is needed, using it, and walking a plan task by task. Its first lines say what was measured to shape it. |
| `…/tools.rs` | The tools a model can ask for, how they are listed for it, how a request is read, mended or refused, and the cleaning of text from outside. `tools/calculate.rs` is the calculator; `tools/files.rs` reads and changes files inside a session's folder and nowhere else. |
| `…/permissions.rs` | Whether a use of a tool needs the person's yes: decided from the tool's reach and the session's level, never by a model. |
| `…/sandbox.rs` | Running a program a model asked for where it can do little harm: on Linux inside bubblewrap, with the folder, the system and no network. `cargo run --example sandbox_try -- <folder> <command>` tries it by hand. |
| `…/checkpoint.rs` | Saving a folder before a program runs in it, and putting it back, so that all a program did can be undone. |
| `…/web.rs` | Reading from the internet, the one way a model reaches the network: one page as text, or a search (`web/search.rs`: Wikipedia and Mwmbl). `web/guard.rs` decides where a request may go (only the public internet) and which addresses a model may open unasked (those it was given, not those it wrote); `web/robots.rs` follows a site's rules for programs; `web/page.rs` turns a page into text and picks the parts that match what was asked. `cargo run --example web_try -- read <address>` or `-- search <words>` tries it by hand, on real sites. |
| `…/engine/` | Running models: starting the runtime as a router (`router.rs`), loading and unloading, unloading what is idle, ending a runtime that was left behind (`mod.rs`), and keeping what is learned about each model (`known.rs`). |
| `…/runtime/` | The model runtime: what its publisher offers (`catalog.rs`), downloading with a check (`fetch.rs`), unpacking without trust (`unpack.rs`), and installing, listing and removing (`mod.rs`). |
| `…/auth.rs` | The key every request needs, and the one-use link that lets a browser in. |
| `…/api.rs` | The API, its guard, its own description, and the server. |
| `…/ui.rs` | Serving the web UI from the same address, so that a browser is never sent the same file twice. |
| `…/kept.rs` | Keeping an answer for a few seconds, so a burst of the same question costs one answer. |
| `crates/demido/src/main.rs` | The `demido` program. |

The API describes itself at `/api/v1/openapi.json`.

## Check the runtime's own behaviour

Demido leans on how llama.cpp's server loads, unloads and fits models. This is how that was checked, and how
to check it again when the runtime changes (about a minute; needs an installed runtime and two models):

```bash
python3 core/dev/router-trial.py
```

Four more scripts measure how to word things for small models, and what words cannot do (the last one: a file that forges a turn of the conversation). Their results are in research note 13, and
they can be run again when a model or the runtime changes. They use the models on this computer and
download nothing. `TRIAL_MODELS=4` takes the four smallest models in the folder instead of two:

```bash
python3 core/dev/tool-call-trial.py
```

```bash
python3 core/dev/tool-turn-trial.py
```

```bash
python3 core/dev/forged-turn-trial.py
```

```bash
python3 core/dev/account-trial.py
```

Two trials go through the whole program instead, with `demido ask`, in a scratch data folder and on a port
of their own (they need a build, and the real models). The first asks real sites, about forty requests.
The second serves two pages from this computer that give the model orders, and says at the end whether
any file changed and what the site was asked for:

```bash
core/dev/web-trial.sh
```

```bash
core/dev/page-orders-trial.sh
```

## Test it

```bash
cargo test --manifest-path core/Cargo.toml
```

## Not built yet

The sandbox on macOS and Windows, tools other people write. Of the internet: pages that need their scripts run, PDFs, and search with a key. Downloading models from within Demido. Providers. Carrying the web UI inside
the binary (a release keeps it in a folder named `ui` beside the program, and the program finds it there).
Use from other devices (encryption, passkeys). Windows and macOS are tested by the checks on GitHub, on machines
without a graphics card; no real Windows or Mac has run it yet.
