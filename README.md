# Demido Studio

A local-first, open-source AI harness that makes small models behave.

**Status: pre-alpha.** Nothing runs models yet. The repository holds the brief, the brand kit, the research,
an interactive preview of the app on fake data, and the first slice of the real core.

## What is here

| Path | What |
|---|---|
| `docs/BRIEF.md` | What Demido Studio is meant to be. |
| `docs/PLAN.md` | The order of work and where it stands. |
| `docs/DECISIONS.md` | Every decision, its reason and its status. |
| `docs/IDEAS.md` | Ideas and pushback, logged as they come up. |
| `docs/LATER.md` | Plans the owner has set aside to try later, written out far enough to pick up cold. |
| `docs/research/` | Research reports behind the technical direction. |
| `brand/` | The brand kit: logo, tokens, fonts, reference components, voice. |
| `apps/web/` | The web UI. Today: the interactive preview with fake data. |
| `core/` | The core, in Rust: server, command line, settings, event log, hardware detection. See `core/README.md`. |
| `scripts/` | Putting a release archive together; checking every dependency's licence. |
| `.github/workflows/` | The checks on every change, and the release builds. |

## Try the interactive preview

```bash
pnpm install
```

```bash
pnpm app:dev
```

Then open <http://127.0.0.1:5173>. Every screen is clickable. Answers are scripted; no model runs.

## Run the core

```bash
pnpm core:serve
```

Then, in another terminal:

```bash
cargo run --manifest-path core/Cargo.toml -- open
```

The same UI opens, served by the real core. Five screens are real then (Sessions, Models, Providers, Setup
and part of Settings); every other screen is still the scripted preview and says so. In a session the model can read
the clock, calculate, and do work as a list of tasks. Give the session a folder and it can read and change
the files in it and run programs there, and nowhere else, asking first where the session's level says so.
On Linux the programs run shut in a sandbox; everything a model changed can be undone. It can also
search the internet without an account (it tries Exa, DuckDuckGo, and Wikipedia with Mwmbl in turn) and
read a page from its address: it only
reads there, and it asks before opening an address it wrote itself. To talk to a model, the core needs
a model runtime (Setup installs it) and a GGUF model: Models › Discover finds and downloads one from
Hugging Face, or `demido models download <owner/name>`. Or a model elsewhere, with your own key: Providers
keeps an Anthropic, Gemini or OpenRouter key in your system's keyring (or `demido keys add anthropic`), you
choose the models a session may use, and Demido counts what each answer costs and stops at the month's limit
you set (`demido usage`). Or your own Claude Code or OpenCode, if they are installed: Demido runs them as
they are, shows what they do, and asks you what they ask.

## See the brand kit

```bash
pnpm brand:serve
```

Then open <http://127.0.0.1:4173/kit/>. Or open `brand/kit/index.html` straight from disk.

Rebuild after editing tokens, components or sections:

```bash
node brand/build.mjs
```

## Releases

Pushing a version tag, or publishing a release on GitHub, builds Demido Studio for Linux, Windows and macOS
(x64 and arm64 each) and attaches the archives and their checksums to the release.

```bash
git tag v0.1.0
```

```bash
git push origin v0.1.0
```

The version comes from the tag. A tag with a hyphen (`v0.1.0-alpha.1`) becomes a pre-release.
To try the whole build without publishing anything, start the "Release" workflow by hand on GitHub: it keeps
the files with the run.

What a release holds today: the core and the web UI, as one archive to unpack and start. There is no desktop
window and no installer yet, and the builds are not signed. The note inside each archive says the same.

## Licence

GPL-3.0-or-later. See `LICENSE`. Bundled fonts and icons keep their own licences (`brand/fonts`, `brand/icons/ui`).

The Demido name and logo are the owner's. A usage policy for them is not written yet; until it is, do not
assume the GPL covers them.
