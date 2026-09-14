# One version, in three files that must agree

**Enforced at the tag.** `scripts/check-release.mjs`, per
[`releases.md`](releases.md) section 6: "the tag matches the version in
`tauri.conf.json` and `Cargo.toml`".

Written down on [#38](https://github.com/elpideus/demido-studio/issues/38), the
ticket that created the manifests it governs.

## The scheme

**`MAJOR.MINOR.PATCH`, and a release is a signed `vX.Y.Z` tag on `main`**
([`releases.md`](releases.md) section 0). Pre-1.0, the minor is the slice
programme: `0.1.0` is the four slices of v0.1 in
[`done.md`](done.md), `0.2.0` is whatever the next programme turns out to be,
and a patch is a fix to a version that has already reached a machine.

There is no pre-release suffix and no build metadata. Tauri's NSIS target and
its updater both want a plain three-part version, and a suffix would be a fourth
thing for `latest.json`, the installer and the tag to disagree about.

## What carries it

| File | Field | Read by |
|---|---|---|
| `src-tauri/Cargo.toml` | `[workspace.package] version` | Every crate, through `version.workspace = true`. |
| `src-tauri/tauri.conf.json` | `version` | The installer, the updater, and the window. |
| the tag | `vX.Y.Z` | GitHub, and the release workflow. |

Three files rather than one because three different tools insist on owning it.
Nothing else may write it down. `demido-core::VERSION` reads
`CARGO_PKG_VERSION`, and `vite.config.ts` reads `tauri.conf.json`, so the window
and the frontend bundle carry the same number without being a fourth and fifth
copy of it.

`package.json` and `web/package.json` carry a version because npm's format
demands one. They are `private: true` and nothing publishes them, so their
version is decoration; the release check does not read it and neither should
anything else.

## The manifests carry the version being worked towards

`0.1.0` is in the manifests today and nothing has shipped. That is deliberate: a
build says which release it is part of, and the tag is what makes that release
real. The alternative, bumping at the tag, means every commit before it claims
to be the previous release, and the evidence attached to a ticket then names a
version that does not contain the thing the evidence is about.

So: **the minor moves when a programme of slices opens**, in its own commit, and
the tag closes it.
