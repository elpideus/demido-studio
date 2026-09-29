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
`%LOCALAPPDATA%\Demido Studio`; set `DEMIDO_DATA_DIR` to use another. Downloaded updates are the
exception: a development build keeps them in `.dev/updates`, away from the installed app's.

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
pnpm test:scripts        # the release scripts, against throwaway git repositories, and the workflows
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

## Releases

### Versions

The version is written in sixteen places: the npm packages, both Tauri configs, the Cargo
workspace and its crates' entries in `Cargo.lock`. `scripts/version.mjs` keeps them in step, with
the root `package.json` as the one the others follow:

```bash
pnpm set-version                   # print it
pnpm set-version 0.5.0             # set it everywhere (0.5.0-beta.1 for a pre-release)
node scripts/version.mjs --check   # fail when a file disagrees (CI does this)
```

Versions are semantic versions without build metadata and without a `v`; tags add the `v`
(`v0.5.0`). A number in a pre-release has no leading zero (`0.5.0-beta.1`, never `beta.01`):
Cargo cannot read such a version, so the scripts refuse it before anything is written.

### Cutting a release

```bash
pnpm release 0.4.1               # a release
pnpm release 0.5.0-beta.1        # a pre-release: any version with a "-"
pnpm release 0.4.1 --no-push     # commit and tag only; it prints the push command for later
```

The repository must be public: installed apps read the releases without signing in, and the
publish job refuses to release from a private repository.

`scripts/release.mjs` refuses unless the working tree is clean, the branch is `main` and it is not
behind `origin/main` (it fetches first), and when the tag already exists here or on GitHub. It
sets the version, commits the changed files as "Release v0.4.1" (signed, when your git config
signs commits), makes the annotated tag `v0.4.1` and pushes the commit and the tag together. When
the version is already the one asked for, it tags the current commit instead of making an empty
one.

The tag starts `.github/workflows/release.yml`:

1. **Checks**: all of CI, on the tag.
2. **Build**, on Windows at the same time: it stops unless the tag matches the version in the
   repository, runs `pnpm build` and keeps the unsigned `Demido-Studio-Setup-<version>.exe` as a
   workflow artifact, under its exact name (the signature names the file it was made for). The
   run's summary shows the installer's SHA-256. This job never sees the signing key.
3. **Sign**, on a fresh Linux runner once the build is done: it checks nothing out, installs
   nothing and has no token permissions. It downloads the installer, signs it with
   `tauri signer sign --app-version` from the exact `@tauri-apps/cli` version in `pnpm-lock.yaml`
   (fetched by `npx` into an empty directory, no install scripts), checks the `.sig` is not empty
   and keeps the installer and its `.exe.sig` as the artifact Publish takes.
4. **Publish**, once Checks and Sign have passed: a GitHub release "Demido Studio <version>" with
   exactly those two files and the notes from `scripts/release-notes.mjs` (every commit subject
   since the previous release, without the "Release v..." ones, then how to install). A
   pre-release counts from the previous `v*` tag of either kind; a stable release counts from the
   previous stable tag, so it also lists what its pre-releases shipped, which people on the Release
   channel have not seen. A version with a `-` becomes a pre-release and is never marked as the
   latest release. Running the workflow again for the same tag replaces the release's files and
   notes.

Actions, Release, **Run workflow** builds the installer from any branch or tag and keeps it as an
artifact, unsigned, publishing nothing: a build to test. Only a pushed `v*` tag is signed.

Every action in both workflows is pinned to a commit, with its tag in a comment. To move one to a
newer version, look up the commit of the new tag (`gh api repos/<owner>/<repo>/git/ref/tags/<tag>`;
for an annotated tag, whose object type is `tag`, follow it with
`gh api repos/<owner>/<repo>/git/tags/<sha>`) and change both. The signer's version,
`TAURI_CLI_VERSION` in the sign job, follows `pnpm-lock.yaml`: `pnpm test:scripts` fails when the
two differ.

### CI

`.github/workflows/ci.yml` runs on every push to `main` and every pull request, on a Windows
runner: the version check, `pnpm format:check`, both frontends' builds (the Tauri crates embed
them when they compile, so clippy and the Rust tests need them), `pnpm typecheck`,
`pnpm test:web`, `pnpm test:scripts`, clippy with warnings as errors and the Rust tests, with
`--locked`. A newer push to a pull request cancels its run in progress.

### Building by hand

```bash
pnpm build    # → release/Demido-Studio-Setup-<version>.exe
```

`scripts/build-release.mjs` bundles the market service, copies the default skills, builds the
app, zips it (`setup-cli --pack`) and builds the installer with the zip compiled in
(`DEMIDO_PAYLOAD`). The version is the one in the root `package.json`. With
`TAURI_SIGNING_PRIVATE_KEY` (the key's text) or `TAURI_SIGNING_PRIVATE_KEY_PATH` (its file) in the
environment it signs the installer the way the release workflow does, next to it as `.exe.sig`,
and asks for the key's password unless `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` holds it. Without a
key it says the installer is unsigned.

### The signing key

The updater installs only an installer signed with the project's update key (a minisign key, in
the format of Tauri's signer). The public key is compiled into the app and into Setup
(`demido_core::signature`). The private key and its password are the secrets
`TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` of the repository's `release`
environment (Settings, Environments), whose deployment rule admits only `v*` tags; only the release
workflow's sign job uses that environment. The job runs on a runner of its own, so no code from the
build (a dependency's build script, say) runs where the key is. That keeps the key safe, not the
installer: whatever the build made is signed as it is. The lead keeps a backup of both outside the
repository. Never commit either.

A signature names the file and the version it was made for (`--app-version` puts the version in
its trusted comment, which the signature covers), and the app accepts it only for the file and
version it was offered.

**Losing the private key or its password** means no release can be signed for the installed apps
any more: they cannot verify, and so never install, another update. Moving to a new key takes a
release carrying the new public key, and every existing installation would have to run that
Setup by hand.

### Testing an update locally

Only an installation made by Setup from a release build installs updates. A development build
checks, downloads and verifies, into `.dev/updates` in the repository, never into the installed
app's `updates` folder (the data folder is shared), and never installs. To try the whole path:

1. Build and install a lower version of the current code: `pnpm set-version 0.3.9`, `pnpm build`,
   run `release/Demido-Studio-Setup-0.3.9.exe`, then `git restore .` to put the version back.
2. Build the version to update to, signed with the release key (the lead's backup):

   ```bash
   TAURI_SIGNING_PRIVATE_KEY_PATH=<key file> TAURI_SIGNING_PRIVATE_KEY_PASSWORD=<password> pnpm build
   ```

3. Serve a releases list that looks like GitHub's (`GET <base>/releases`), with asset links to
   your own server: a custom feed may link to any http(s) address. For example, a folder holding
   the installer, its `.sig` and a file named `releases`, served with `python -m http.server 8000`
   (each `size` is that file's size in bytes):

   ```json
   [
     {
       "tag_name": "v0.4.0",
       "draft": false,
       "prerelease": false,
       "html_url": "http://127.0.0.1:8000/",
       "assets": [
         {
           "name": "Demido-Studio-Setup-0.4.0.exe",
           "browser_download_url": "http://127.0.0.1:8000/Demido-Studio-Setup-0.4.0.exe",
           "size": 25000000
         },
         {
           "name": "Demido-Studio-Setup-0.4.0.exe.sig",
           "browser_download_url": "http://127.0.0.1:8000/Demido-Studio-Setup-0.4.0.exe.sig",
           "size": 432
         }
       ]
     }
   ]
   ```

4. Start the installed app with the feed's base URL in `DEMIDO_UPDATE_FEED`, then Settings,
   Updates, **Check for updates**:

   ```bash
   DEMIDO_UPDATE_FEED=http://127.0.0.1:8000 "$LOCALAPPDATA/Programs/Demido Studio/demido-studio.exe"
   ```

The app leaves `DEMIDO_UPDATE_FEED` out of the installer's environment, so the app the update
starts again at the end runs without it, per-user and machine-wide alike, and checks GitHub from
then on. Start it with the variable again to keep testing against your feed.

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
