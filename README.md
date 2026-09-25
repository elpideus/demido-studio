# Demido Studio

An easy to use, powerful AI workspace that runs on your own computer, built for optimization,
agentic work and transparency.

- **Runs locally, set up for you.** The installer detects your graphics card, picks the right AI
  runtime (NVIDIA CUDA, AMD ROCm, Apple Silicon, Vulkan or CPU), and downloads a Qwen or Gemma
  model sized for your memory. No manual hunting for runtimes, models or plugins.
- **Agentic tools that work together.** Live and historical market data, Python for analysis and
  charts, workspace files, and skills the assistant can write for itself.
- **Transparent.** Every answer records exactly what was sent to the model and what came back,
  with token counts and speed. Open any answer in the Inspector.
- **Cloud when you want it.** Connect Google Gemini with an API key; its models appear next to
  the local ones.
- **A desktop, not a web page.** The chat is always there; Settings, Market and the Inspector
  are windows you can move, resize, maximize or pin to a side, a corner, the top or the bottom,
  like Windows 11's snap layouts.

> Demido Studio is under active development. AI models can be wrong or be manipulated by
> content they read. Review what the assistant does before letting it run code.

## Install

Download `Demido-Studio-Setup-<version>.exe` and run it. Setup walks you through:

1. **AI runtime.** The best option for your hardware is preselected, with a note on each choice.
2. **Just me or everyone.** A per-user install needs no administrator rights (default).
3. **Install folder** (optional).
4. **First model.** Picked for your GPU memory from unsloth's quantized Qwen and Gemma builds.
5. **Install.** Everything is downloaded from its official source and verified against a pinned
   SHA-256: the runtime, Python with data packages, Node.js, uv, and the model. Downloads resume
   if the connection drops.
6. **Run Demido Studio.**

Uninstall from Windows' Installed apps. Your chats and models are kept unless you choose to
delete them.

## What is inside

| Feature | Where |
|---|---|
| Chat with local or cloud models, streaming, thinking, tool calls | The main screen |
| Model picker and tools/skills picker | The composer |
| Providers (Gemini), models (enable, edit, download from Hugging Face), skills, general settings | Settings window |
| Live charts from TradingView, history from Dukascopy | Market window |
| Exact request and response of any answer | Inspector (from an answer's toolbar) |

**Market data.** Real-time data comes from TradingView through
[TradingView-API](https://github.com/Mathieu2301/Tradingview-API); sign in once in the window
Demido opens (your password never passes through Demido). History comes from Dukascopy's public
feed without any account. When both exist, older Dukascopy candles extend TradingView's history
without overlapping it.

**Skills** are folders with a `SKILL.md` (a short frontmatter with `name` and `description`,
then instructions) and any files it refers to. Enabled skills are part of every conversation.
Ask the assistant to "make this a skill" and it writes one, scripts included; it appears
immediately, without a restart.

## Develop

Requirements: Rust (stable), Node.js 22+, pnpm 10+. Windows is the primary platform today.

```bash
pnpm install
pnpm dev:runtime      # download llama.cpp, Python, Node and uv into .dev/install (once)
pnpm dev              # the app, with hot reload
pnpm dev:installer    # the setup wizard (runtimes only, no app payload in dev)
pnpm build            # release/Demido-Studio-Setup-<version>.exe
```

`pnpm dev:runtime` runs the same installation engine as the setup wizard. Point the app at a
folder of GGUF files from Settings, Models, Model folders (LM Studio's folder is detected), or
download a model from the Download tab.

Checks:

```bash
cargo test --workspace          # Rust unit tests
pnpm test:web                   # window geometry and market service tests
pnpm typecheck                  # TypeScript
```

Driving the running app (screenshots, scripted conversations) is documented in
[`docs/development.md`](docs/development.md).

## Layout

| Path | What |
|---|---|
| `apps/studio` | The app: React UI in `src/`, Rust backend in `src-tauri/` |
| `apps/installer` | The setup wizard (also the uninstaller) |
| `crates/core` | Types shared by installer and app: the install manifest, paths |
| `crates/hardware` | GPU, VRAM, CPU and memory detection |
| `crates/catalog` | What gets installed (`catalog/*.json`) and how it is chosen for a machine |
| `crates/fetch` | Resumable, verified downloads and archive extraction |
| `crates/provision` | The installation engine |
| `crates/setup-cli` | The engine from a terminal (`--detect`, `--dev`, `--pack`) |
| `packages/ui` | Design tokens and shared React components |
| `sidecars/market` | The market data service (Node): TradingView and Dukascopy |
| `skills` | Skills shipped with the app |
| `catalog` | Pinned runtimes and the model tiers, with checksums |
| `docs` | Architecture and development notes |

See [`docs/architecture.md`](docs/architecture.md) for how the pieces fit together.

## License

GPL-3.0-or-later. See [`LICENSE`](LICENSE) and [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md).
