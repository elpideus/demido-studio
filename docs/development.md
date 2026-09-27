# Development

## First run

```bash
pnpm install
pnpm dev:runtime    # once: llama.cpp for this machine, Node, uv, Python + packages → .dev/install
pnpm dev
```

`pnpm dev:runtime` is the installer's engine run from a terminal (`crates/setup-cli`). It detects
the hardware, picks the recommended runtime and writes `.dev/install/install.json`, which the
app reads in development exactly as it reads the real one next to an installed executable. It
skips the starter model: add a folder of GGUF files in Settings, Models, or download one there.
If it refuses `.dev/install` (an older build set that folder up without recording what it
created), delete `.dev/install` and run it again.

`pnpm dev` first assembles `.dev/resources` (the bundled market service and the default skills),
then starts the app with hot reload. The data folder is the real one,
`%LOCALAPPDATA%\Demido Studio`; set `DEMIDO_DATA_DIR` to use another.

Useful terminal commands:

```bash
cargo run -p demido-setup-cli -- --detect        # what the installer sees on this machine
pnpm dev:installer                               # the wizard (its "app files" step is empty in dev)
```

## Checks

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
pnpm typecheck
pnpm test:web            # window geometry (vitest) and the market service (node --test)
pnpm format:check
```

## Driving the running app

The webview accepts the Chrome DevTools Protocol when started with a debugging port:

```bash
WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222 pnpm dev
```

`scripts/drive.mjs` then clicks, hovers, types, drags, calls backend commands and takes
screenshots, in the order given (`--drag "selector|x,y|hold"` keeps the button down until
`--release`, to capture a drag in progress):

```bash
node scripts/drive.mjs --click '[aria-label="Settings"]' --wait 500 --screenshot settings.png
node scripts/drive.mjs --invoke market_status
node scripts/drive.mjs --eval "document.querySelectorAll('[data-window]').length"
```

`scripts/scenario.mjs` runs a whole conversation the way a person would, one prompt per turn,
with a screenshot after each and a list of the tools every turn called:

```bash
node scripts/scenario.mjs --out .dev/scenario --approve "Price of EUR/USD now?" "Chart gold, 4h, 3 months"
```

## The market data service

`sidecars/market` runs on its own with Node 22 (type stripping, no build step), speaking JSON lines
on stdin and stdout (`src/protocol.ts`):

```bash
DEMIDO_CACHE_DIR=/tmp/market-copy node sidecars/market/src/main.ts
{"id":1,"method":"bars.older","params":{"symbol":"FX:EURUSD","timeframe":"1h","before":1704067200,"count":500}}
```

Without `DEMIDO_CACHE_DIR` it keeps its store in the system temp folder. Point it at a **copy** of
`%LOCALAPPDATA%\Demido Studio\cache\market`, never the real one: the first start migrates the old
flat Dukascopy cache and TradingView files into the new layout and deletes the originals. Its tests
(`pnpm --filter @demido/market-sidecar test`) use temp folders and a fake fetcher; they never reach
Dukascopy. For a manual check against Dukascopy, keep to a few requests, spaced out: it answers
bursts with 429. To drive the bundled service end to end without any traffic, preload a module
that replaces `globalThis.fetch` with one answering the candle URLs locally (`node --import
<module> .dev/resources/sidecars/market.mjs`): the service captures `fetch` when it loads.

## Release

```bash
pnpm build    # → release/Demido-Studio-Setup-<version>.exe
```

`scripts/build-release.mjs` bundles the market service, copies the default skills, builds the
app, zips it (`setup-cli --pack`) and builds the installer with the zip compiled in
(`DEMIDO_PAYLOAD`). The version is the one in the root `package.json`; keep the Tauri configs
and `Cargo.toml` in step with it.

## Changing the logo

`design/logo.svg` is the mark. `pnpm icons` builds the app icon from it (`design/app-icon.svg`,
the mark on a dark disc, and `app-icon.png`) and regenerates the icon files of both apps. The
windows draw the mark with `Logo` from `@demido/ui`, which holds a copy of the same paths.

## Changing what gets installed

The runtimes, tools and starter models are pinned in `catalog/`. See [catalog.md](catalog.md)
for the format and how to move to a new llama.cpp build or model.

## Adding a tool

1. Describe it in `apps/studio/src-tauri/src/tools/mod.rs` (`TOOLS`): name, group, description,
   JSON schema, and whether it needs approval.
2. Implement it in the group's file (`tools/market.rs`, `tools/python.rs` ...) and route it in
   `tools::run`. A tool that must ask while it runs (the market tools before a long download)
   calls `ctx.request_approval(card)`, and `ctx.set_display(...)` shows progress on its card
   before it finishes.
3. Give it a label in `tools::describe` and, if its result deserves more than JSON, a view in
   `apps/studio/src/chat/ToolDisplay.tsx`.

## Adding a provider

`llm::Client` has one variant per wire format. A provider with an OpenAI-compatible endpoint
only needs an entry in `providers.rs` and a base URL; any other format needs a module beside
`llm/gemini.rs` that turns `ChatRequest` into its request body and its stream into
`StreamEvent`s.
