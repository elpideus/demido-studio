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
| `docs/research/` | Research reports behind the technical direction. |
| `brand/` | The brand kit: logo, tokens, fonts, reference components, voice. |
| `apps/web/` | The web UI. Today: the interactive preview with fake data. |
| `core/` | The core, in Rust: server, command line, settings, event log, hardware detection. See `core/README.md`. |
| `scripts/` | Putting a release archive together. |
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

The same UI opens, served by the real core. Setup then shows this computer's actual hardware; every other
screen is still the scripted preview and says so.

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
