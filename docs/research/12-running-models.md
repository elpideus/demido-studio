# 12. Running models: what the runtime does by itself now

Written 2026-11-21, directly, before building the part of Demido that starts the runtime and loads models.
Narrow on purpose: what `llama-server` offers today for loading, unloading and fitting models into memory,
and what that means for Demido's own design.

**Read** means I read it myself, in llama.cpp's own server documentation at the stable build
(`tools/server/README.md` at tag `b12359`, 140 KB). Nothing here was tried on a real runtime: none is
installed yet (the first download waits for the owner). Every "how it behaves" below is the documentation's
claim until it has been tried.

## Why this note exists

Decision K8 said: Demido runs one `llama-server` process per loaded model, so that unloading is certain,
and Demido decides what fits in memory. I was about to build exactly that: a process keeper and a memory
budget. Reading the current documentation first changed the picture. The runtime now does most of it.

## What is true today

| Fact | Checked |
|---|---|
| **Router mode.** Started without a model, `llama-server` becomes a router: one address, and it loads and unloads models on request. Each loaded model is its own child `llama-server` process. | **Read** ("Using multiple models") |
| **Where models come from.** A folder of GGUF files (`--models-dir`), a preset file (`--models-preset`, an `.ini` with one section per model whose keys are the command-line arguments), or llama.cpp's own cache. A model with several files (a vision part, or shards) is a subfolder. | **Read** ("Model sources", "Model presets") |
| **Loading and unloading on request.** `POST /models/load` and `POST /models/unload` with the model's name. `GET /models` lists every model with its state. `GET /models/sse` streams state changes. | **Read** |
| **Loading by itself.** By default a request that names a model loads it. `--no-models-autoload` turns that off. | **Read** ("Routing requests") |
| **Fitting into memory.** `--models-max N` limits how many are loaded (default 4), least recently used out first. `--models-memory-margin N` goes further: the router measures what each model needs on each device, and before loading one it unloads idle models until the new one fits while leaving `N` MiB free. A model that is answering is never unloaded. | **Read** ("Memory-based unloading") |
| **It reports the numbers.** For every model: the memory it needs at the smallest context, at the largest that fits now, and what it really took once loaded, per device (`memory.min`, `max`, `effective` in `GET /models`). | **Read** |
| **Context is chosen to fit.** With `--fit` (on by default) a model takes the largest context that fits when it loads. A preset can pin the context instead. | **Read** |
| **Sleeping when idle.** `--sleep-idle-seconds` frees a model's memory after that long without work and reloads it on the next request. Health, properties and model-list requests do not count as work. | **Read** ("Sleeping on Idle") |
| **Health.** `GET /health` answers 503 "Loading model" while loading and 200 `{"status":"ok"}` when ready. It needs no key. | **Read** |
| **A key.** `--api-key` or `--api-key-file`. Listens on `127.0.0.1` by default; the default port is 9931. | **Read** ("Server-specific params") |
| **It can download models itself** (`POST /models`, from Hugging Face), with pause and cancel. | **Read** |
| **It can run tools itself** (`--tools`, marked experimental, "do not enable in untrusted environments"), and has a web UI of its own. | **Read** |

## What this means for Demido

The brief's theme is "load when needed, unload when not", with a memory budget and lending to agents.
Three of the four hard parts now exist in the runtime: a process per model, measuring what a model needs,
and making room by unloading the least recently used.

### Proposed: Demido runs one router, and steers it

1. **One `llama-server` in router mode**, started by Demido when a model is first needed and stopped when
   none has been loaded for a while. On this computer only (`127.0.0.1`), on a port Demido picks, with a key
   Demido makes for each start. Nothing but Demido talks to it.
2. **Models are Demido's.** Demido downloads them itself (same care as the runtime: publisher's address,
   checksum, resume) into its own folder, writes the preset file (one section per model, holding the
   values Demido computed for this computer), and tells the router to look again. The router's own
   downloader and its cache are not used: a second way in would skip Demido's checks.
3. **Demido decides when; the router decides how.** Autoload off. Demido asks for a load when a session,
   an agent or a schedule needs a model, and asks for an unload when its own idle timer runs out
   (the "Idle unload" setting). An unload ends the model's process, which is the certainty K8 wanted.
   Sleeping-when-idle is not used for that reason: it frees memory but keeps the process.
4. **"Memory kept free" becomes the router's margin.** The setting already exists in the preview; it maps
   to `--models-memory-margin`. The router then never loads past it.
5. **The memory picture comes from the router's numbers**, not from Demido's guesses: what each model
   needs, what it took, per device. That is what the budget bar in the UI should draw.
6. **Tools and the runtime's web UI stay off.** Demido has its own tools, permissions and sandbox; a
   second, unguarded tool runner inside the runtime would be a hole.

### What stays Demido's work

- When to load and unload: sessions, lending a loaded model to agents, schedules, queues.
- Memory that is not the runtime's: an embedding model, speech, the desktop itself.
- Per-model values: context size, layers on the card, the small-model techniques. Written into the preset.
- Everything about a model before it runs: finding, downloading, checking, storing.

### What it costs

- Demido depends on how the router behaves. If it proves too rigid (for example for lending, or for two
  copies of one model), the fallback is K8 as first written: Demido starts one `llama-server` per model
  itself. The installer and everything around it stay the same either way.
- Changing a model's values means rewriting the preset and asking the router to look again. Whether a
  loaded model must be reloaded for that is not known yet.

## Must be tried on the real runtime before this is decided

1. That an unload really ends the child process and gives the memory back, every time.
2. That `--models-memory-margin` works with CUDA on a 12 GB card, and what it does when a model cannot fit.
3. What `GET /models` and `/models/sse` really send (the shapes above are from examples in the text).
4. That a new file in the models folder is seen after `GET /models?reload=1`, without a restart.
   The text says a restart is needed for the cache; it does not say so for a folder.
5. How long the router takes to start, and to measure a model the first time.
6. That the web UI and the tools are off unless asked for, and how to make sure of it.
7. How it behaves when a child crashes while answering.

## Not looked at here

Which values to compute per model, and the small-model techniques (constrained output, repair, compaction).
That is the research owed before the chat loop (`docs/PLAN.md`).

## Source

- <https://github.com/ggml-org/llama.cpp/blob/b12359/tools/server/README.md>
