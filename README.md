# Demido Studio

An easy to use, powerful AI workspace that runs on your own computer, built for optimization,
agentic work and transparency.

- **Runs locally, set up for you.** The installer detects your graphics card, picks the right AI
  runtime (NVIDIA CUDA, AMD ROCm, Apple Silicon, Vulkan or CPU), and downloads a Qwen or Gemma
  model sized for your memory. No manual hunting for runtimes, models or plugins.
- **Agentic tools that work together.** Live and historical market data, your email, Python for
  analysis and charts, the programs already on your computer (yt-dlp, ffmpeg, git, ping ...) through your own
  PowerShell, workspace files, and skills the assistant can write for itself.
- **Transparent.** Every answer records exactly what was sent to the model and what came back,
  with token counts and speed. Open any answer in the Inspector.
- **Cloud when you want it.** Connect Google Gemini with an API key; its models appear next to
  the local ones.
- **A desktop, not a web page.** The chat is always there; Settings, Market, Mail and the
  Inspector are windows you can move, resize, maximize or pin to a side, a corner, the top or the bottom,
  like Windows 11's snap layouts.

> Demido Studio is under active development. AI models can be wrong or be manipulated by
> content they read. Review what the assistant does before letting it run code.

## Install

Download `Demido-Studio-Setup-<version>.exe` from
[Releases](https://github.com/elpideus/demido-studio/releases) and run it. Setup walks you
through:

1. **AI runtime.** The best option for your hardware is preselected, with a note on each choice.
2. **Just me or everyone.** A per-user install needs no administrator rights (default).
3. **Install folder** (optional).
4. **First model.** Picked for your GPU memory from unsloth's quantized Qwen and Gemma builds.
5. **Install.** Everything is downloaded from its official source and verified against a pinned
   SHA-256: the runtime, Python with data packages, Node.js, uv, and the model. Downloads resume
   if the connection drops.
6. **Run Demido Studio.**

Run Setup where Demido Studio is already installed and it offers **Update** instead of the
wizard: it replaces the app in place and keeps every choice made when it was installed (just me
or everyone, the runtime, the models folder), along with your chats, models and settings. This is
also how an installation older than 0.4.0 gets a new version.

Uninstall from Windows' Installed apps. Your chats and models are kept unless you choose to
delete them.

**Updates.** From 0.4.0 on, Demido Studio updates itself. In Settings, Updates you pick the
channel (Release, or Pre-release for early versions) and whether it updates automatically. That is
on by default and works like Discord: a new version downloads in the background and is installed
the next time the app starts, or at once with **Restart and update**. With it off, **Check for
updates**, then **Update** downloads the new version and restarts the app to install it, asking
first if a reply is still being written. Every download is checked against the release's signature
before it runs. If an automatic update fails, Setup says so and **Open Demido Studio** opens the
app without trying it again; a later start tries once more, and after that the update waits for a
click in Settings, Updates.

## What is inside

| Feature | Where |
|---|---|
| Chat with local or cloud models, streaming, thinking, tool calls | The main screen |
| Model picker and tools/skills picker | The composer |
| Long chats compact themselves: near the end of the model's context window, the earlier conversation becomes a summary the model reads instead (when, in Settings, General) | The chat; `/compact` to do it now |
| Slash commands: `/compact`, `/autocompact 12k` (or `off`, `auto`, `12.5k`, `twelve thousand`), and the commands skills bring, such as `/analyze` | Type `/` in the composer |
| Send files with a message: images, PDFs, Word, PowerPoint, spreadsheets, text and code. Short ones are read in full, long ones are searched for the passages your question needs (RAG), by meaning as well as by words | The composer: **+**, drag and drop, or paste |
| Commands in your own shell (PowerShell 7 when installed, else Windows PowerShell), shown live like a terminal with a Stop button; each one asks first | The chat (Coding in the Tools menu) |
| What each model can do, as icons: vision, audio, tools, thinking (hover for what each means) | Model picker, Settings, Models |
| Providers (Gemini), models (enable, edit, download from Hugging Face), skills, general settings | Settings window |
| Live charts from TradingView, history from Dukascopy | Market window, Chart tab |
| Indicators on the chart, computed by TradingView: your Favorites and your own scripts, the indicators of your saved TradingView chart layouts, TradingView's built-ins and community scripts; signed out, SMA, EMA, Bollinger Bands and RSI are computed on your computer | Chart tab, **Indicators** |
| Each indicator's settings as in TradingView: Inputs, Style (colors, opacity, widths, line styles, plot types, levels, precision) and Visibility (per timeframe) | The gear on the indicator's legend |
| Pine scripts: write, compile and run your own, or open one of your TradingView scripts; save them to your TradingView account to use them on tradingview.com | Chart tab, **Pine Editor**; Indicators, Pine scripts |
| The assistant writes and tests Pine indicators, puts indicators on your chart, and draws on it (levels, zones, markers) | The chat |
| What history is stored per market (one timeline: every timeframe reads the same 1-minute history), where it came from, and what is missing; download missing parts, delete | Market window, Data tab |
| History downloads with a progress bar you can pause and resume, from a chart, the Data tab or the chat | Chart, Data tab, chat cards |
| Ask before long downloads the assistant wants to start (30 s to 5 min) | Settings, General |
| Your Gmail (or another IMAP mailbox): folders, messages and attachments, read only; new mail shows up on its own while the window is open | Mail window |
| The assistant lists, searches and reads your email, exports hundreds of emails to a file for analysis, and saves attachments to its workspace | The chat (Email in the Tools menu) |
| Updates: Release or Pre-release channel, automatic in the background or checked by hand | Settings, Updates |
| Exact request and response of any answer | Inspector (from an answer's toolbar) |

**Market data.** Real-time data comes from TradingView through
[TradingView-API](https://github.com/Mathieu2301/Tradingview-API); sign in once in the window
Demido opens (your password never passes through Demido). History comes from Dukascopy's public
feed without any account (forex, metals, indices, commodities, crypto; stocks and ETFs come from
TradingView, since Dukascopy's are not split-adjusted). Downloaded history is kept on your
computer and never fetched twice. It is always 1-minute candles, whatever timeframe the chart shows:
every timeframe is built from them, so one download serves them all and no timeframe ever needs a
download of its own. Downloads run in the background, resume after a restart and slow down on their
own before Dukascopy would refuse them. Charts page back through what is stored and offer to
download more where it ends: one more month or year than the stored 1-minute history, everything
from a date, or all of it, each up to today, so one download leaves the whole stretch stored. Where
TradingView's candles are stored they are shown, and Dukascopy's fill the rest. The assistant
downloads what it needs the same way, and asks first when the estimate is longer than your limit.

**Indicators.** Signed in, the chart's indicators are TradingView's own: the Indicators menu lists
your Favorites and your scripts as TradingView's Indicators menu does, its built-ins, and the
community library (search). It also brings over the indicators of a chart layout saved on your
account, with their settings and colors. TradingView computes them on the live chart, so their
values match tradingview.com, and your plan's limit of indicators per chart applies. Lines,
histograms, shapes, and the labels, lines, boxes and tables scripts draw are shown, each pane
with its own legend, eye, settings and remove buttons. The settings dialog has TradingView's
three tabs: Inputs, Style (each plot's visibility, color and opacity, width, line style and plot
type, the levels, the value precision and what the price scale and status line show) and
Visibility (the timeframes it shows on). Fills, background and bar colors are not drawn yet, and
strategies are not supported. Signed out, SMA, EMA, Bollinger Bands and RSI are computed on your
computer from the chart's bars with TradingView's formulas and settings. Other indicators stay
listed until you sign in. Each chart window keeps its indicators across restarts.

**Pine scripts.** The Pine Editor (Chart tab) writes Pine Script indicators kept in Demido's own
library. TradingView compiles them as you type, with errors and warnings on their lines, and runs
them on the chart like any other indicator (Ctrl+S saves, Ctrl+Enter adds to the chart). **Save
to TradingView** puts a script on your TradingView account, private under My scripts, so it is in
tradingview.com's Indicators menu and Pine Editor too; saving again updates the same script. Your
own TradingView scripts open in the editor from the Indicators menu (a copy, for someone else's).
The assistant uses the same library: it writes a script, reads TradingView's compiler errors
and warnings, fixes them one change at a time (even in a long script, and with small local
models), runs it on any market and timeframe to check its values (every bar's values go to a CSV in the
workspace), and puts it on your chart. It asks before saving anything to your TradingView
account. For a quick look that needs no indicator, it draws on the chart instead: levels, zones,
trend lines, markers and labels, kept per market until you remove them from the legend.

**Email.** The Mail window connects Gmail with an
[app password](https://myaccount.google.com/apppasswords) (it needs 2-Step Verification on the
Google account; work and school accounts can have app passwords turned off by their admin), or
any mailbox that offers IMAP over TLS on port 993. Demido only reads: opening a message never marks
it as read, and nothing is moved or deleted. Messages are kept on your computer once downloaded, and
each check asks the server only for what changed since the last one; while the window is open the
inbox is watched, so new mail arrives without asking. The assistant reads the same mail, treated as
the sender's words and never as instructions. With several accounts, give each a name (Work,
Personal) when you connect it or from the account menu, and ask the assistant about any of them by
name or address; when you name none, it reads the first. To analyse many emails it exports them, text
included, to a file in its workspace and works on that with Python; up to a thousand at a time,
about a hundred per request to the server, and every email it downloads stays cached for the next
time. Remote images in emails stay hidden until you show
them, since loading them tells the sender you read the message.

**Skills** are folders with a `SKILL.md` (a short frontmatter with `name` and `description`,
then instructions) and any files it refers to. Enabled skills are part of every conversation.
Ask the assistant to "make this a skill" and it writes one, scripts included; it appears
immediately, without a restart. A `commands.json` beside `SKILL.md` adds slash commands:

```json
{
  "commands": [
    {
      "name": "analyze",
      "description": "Price, trend, key levels and a chart for one market",
      "args": "<symbol> [timeframe]",
      "prompt": "Analyze this market: $ARGUMENTS."
    }
  ]
}
```

`$ARGUMENTS` is what you type after the command, `$1` to `$9` its words. Commands cost no
context until you use one, which keeps small models' prompts small.

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
pnpm test:scripts               # release scripts and workflows
pnpm typecheck                  # TypeScript
```

Driving the running app (screenshots, scripted conversations), cutting a release
(`pnpm release <version>`), CI and the update signing key are documented in
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
| `crates/extract` | Reading attached files: kind, text, passages, a model-ready copy of an image |
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
