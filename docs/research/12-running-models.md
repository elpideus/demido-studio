# 12. Running models: what the runtime does by itself now

Written 2026-11-21, directly, before building the part of Demido that starts the runtime and loads models.
Narrow on purpose: what `llama-server` offers today for loading, unloading and fitting models into memory,
and what that means for Demido's own design.

**Read** means I read it myself, in llama.cpp's own server documentation at the stable build
(`tools/server/README.md` at tag `b12359`, 140 KB). Nothing here was tried on a real runtime: none is
installed yet when this was written. The trial came later the same day and is at the end; where it
changed or sharpened something, the section says so.

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

### Decided after the trial below: Demido runs one router, and steers it

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

## Tried on the real runtime (2026-11-21)

On the owner's computer: llama.cpp build `b12359`, CUDA 13.5, NVIDIA RTX 3060 with 12 GB, the system's own
CUDA 13.4 libraries. Two test models from their maker's pages, both Apache-2.0, checked against the SHA-256
the pages list: Qwen3 0.6B (Q8, 610 MB) and Qwen3 1.7B (Q8, 1.7 GB). The script that did it is
`core/dev/router-trial.py`; it can be run again. The router was started with a key,
without its web UI, with autoload off. Every line below was seen, not read.

| Question | What happened |
|---|---|
| 1. Does an unload end the model's process and give the memory back? | **Yes.** Unloaded in 0.25 s; the child process was gone; graphics memory in use went back to exactly what it was before (811 MiB). |
| 2. Does the memory margin work on this card? | **Yes.** With about 2.6 GB left usable: the small model loaded; asking for the larger one unloaded the small one by itself and loaded the larger; asking for the small one again did the reverse. Each swap took under a second. |
| 2b. And when a model cannot fit even alone? | It is loaded anyway, with a line in the runtime's log ("projected to be short by 423 MiB … loading anyway"). So Demido must check before asking, not rely on a refusal. |
| 3. What do the list and the event stream really send? | The list: one entry per file, named after the file, with its state (`unloaded`, `loading`, `loaded`), the command line it would get, and the model's own context length. The stream: `model_memory` (the measurement), `model_status` and `status_change` with loading progress, `loaded` with details, `unloaded` with the child's exit code. |
| 4. Is a new file seen without a restart? | **Yes**, with `GET /models?reload=1`. Not without it. A removed file disappears the same way. |
| 5. How long does it take to start? | The router: 0.2 s. Loading the 0.6B model: 0.8 s. Its first answer: 0.07 s, at 135 tokens a second. |
| 6. Are the web UI and the tools off? | With `--no-webui`, the root address answers 404. Tools are off unless `--tools` is given, and it is not. Without the key everything answers 401, except `/health`. |
| 7. What if a child crashes? | Killed on purpose: within half a second the model is listed as `unloaded`, its memory is free, the router keeps running, and the next request for it is refused with "model is not loaded". |
| Does the CUDA 13.5 build answer with the system's CUDA 13.4 libraries? | **Yes.** |
| Stopping the router with a model loaded | Ended in 1 s, no child left, memory back. |

Three things the trial showed that the documentation did not make plain:

1. **Without a pinned context a model takes the largest that fits.** The 0.6B model (610 MB on disk) took
   5.3 GB of graphics memory when loaded with nothing set. With the context pinned to 4096 it took 1.2 GB.
   So Demido must always set the context for each model. Leaving it to the runtime defeats the budget.
2. **The memory numbers exist only when a margin is set.** With `--models-memory-margin 0` nothing is
   measured and the `memory` field stays empty. Demido always sets a margin (its "Memory kept free"
   setting), so it always gets the numbers.
3. **A request for a model that is not loaded is refused** (400, "model is not loaded") when autoload is off.
   That is what Demido wants: nothing loads unless Demido asked for it.

What the runtime measured, for the budget (context 4096): the 0.6B model needs 1360 MiB (633 model, 470
context, 324 working memory); the 1.7B model needs 2508 MiB.

Built on this the same day: `core/crates/demido-core/src/engine/` (DECISIONS K23).

Not tried: several requests at once to one model; a model with a vision part; two graphics cards; Windows
and macOS; what a long-running router does to memory over days.

## Not looked at here

Which values to compute per model, and the small-model techniques (constrained output, repair, compaction).
That is the research owed before the chat loop (`docs/PLAN.md`).

## Source

- <https://github.com/ggml-org/llama.cpp/blob/b12359/tools/server/README.md>
