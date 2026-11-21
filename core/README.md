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
| `…/auth.rs` | The key every request needs, and the one-use link that lets a browser in. |
| `…/api.rs` | The API, its guard, its own description, and the server. |
| `…/ui.rs` | Serving the web UI from the same address. |
| `crates/demido/src/main.rs` | The `demido` program. |

The API describes itself at `/api/v1/openapi.json`.

## Test it

```bash
cargo test --manifest-path core/Cargo.toml
```

## Not built yet

Downloading and starting the model runtime. Sessions, tools, models, providers. Carrying the web UI inside
the binary (a release keeps it in a folder named `ui` beside the program, and the program finds it there).
Use from other devices (encryption, passkeys). Windows compiles when cross-compiled here; its tests, and macOS, run
only in the checks on GitHub.
