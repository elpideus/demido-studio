# Third-party notices

Demido Studio is GPL-3.0-or-later. It stands on other people's work, and that work is credited here,
in the source, and inside the application itself.

Full license texts live under `licenses/<owner>/<project>/LICENSE`. Both tables
below are read by `scripts/check-rules.mjs`: a credited project must have its
license on disk, a license on disk must be credited here, and a provenance header
naming a ruled-out project fails the build. See
[`docs/rules/provenance.md`](docs/rules/provenance.md).

| Project | Owner | License | Used for |
|---|---|---|---|
| _(populated as dependencies are accepted)_ | | | |

## Ruled out

These projects were read for patterns but no code may be copied from them:

| Project | Why |
|---|---|
| open-webui | License carries a branding-retention clause incompatible with our distribution. |
| openclaude | Derived from proprietary Claude Code without authorization to redistribute. |

## Chosen, not yet installed

Decided and not yet a dependency of anything, because there is no code to depend on them. Each lands
in the table above, with its license text under `licenses/`, on the commit that first installs it.

| Project | Owner | License | Chosen on |
|---|---|---|---|
| lucide | lucide-icons | ISC | [#7](https://github.com/elpideus/demido-studio/issues/7), [#9](https://github.com/elpideus/demido-studio/issues/9) |
| simple-icons | simple-icons | CC0-1.0 | [#7](https://github.com/elpideus/demido-studio/issues/7) |
| llama.cpp | ggml-org | MIT | [#19](https://github.com/elpideus/demido-studio/issues/19) |
| cuda-redistributables | nvidia | NVIDIA CUDA Toolkit EULA | [#19](https://github.com/elpideus/demido-studio/issues/19), [#27](https://github.com/elpideus/demido-studio/issues/27) |
| uv | astral-sh | MIT or Apache-2.0, at the user's option | [#21](https://github.com/elpideus/demido-studio/issues/21) |
| python-build-standalone | astral-sh | MPL-2.0 for the build, PSF-2.0 for the CPython it produces | [#21](https://github.com/elpideus/demido-studio/issues/21), [#27](https://github.com/elpideus/demido-studio/issues/27) |
| searxng | searxng | AGPL-3.0-or-later | [#21](https://github.com/elpideus/demido-studio/issues/21) |
| node | nodejs | MIT | [#21](https://github.com/elpideus/demido-studio/issues/21) |
| agent-browser | vercel-labs | Apache-2.0 | [#21](https://github.com/elpideus/demido-studio/issues/21) |
| chrome-for-testing | google | Google Chrome Terms of Service | [#21](https://github.com/elpideus/demido-studio/issues/21), [#28](https://github.com/elpideus/demido-studio/issues/28) |

**`chrome-for-testing` is the one row here that is conditional.**
[#28](https://github.com/elpideus/demido-studio/issues/28) made it a fetch that happens only on a
machine with no browser Demido can drive, so it is credited here on every install and present on
disk on some. Its entry at `licenses/google/chrome-for-testing/LICENSE` is written by hand and exists
now rather than on the commit that first fetches it, because there is no upstream text to unpack
later and the decision not to have one is itself the thing worth recording.

The rows below `llama.cpp` are what
[#21](https://github.com/elpideus/demido-studio/issues/21) added by moving every runtime fetch from
first use to set-up. They were written as **to confirm** rather than guessed until
[#27](https://github.com/elpideus/demido-studio/issues/27) read each one upstream, because a license
nobody has read is not a license and this file is what the in-app credits surface renders. Three of
them came back as something other than the placeholder said:

- **`chromium` was never Chromium.** `agent-browser` fetches Google's Chrome for Testing channel,
  which is Google Chrome under the [Google Chrome Terms of Service](https://www.google.com/chrome/terms/).
  The archive carries no license file at all, and Chrome's own credits are readable only from inside
  the running binary at `chrome://credits`, so `licenses/google/chrome-for-testing/LICENSE` has to be
  written by hand rather than unpacked, and it is:
  [`licenses/google/chrome-for-testing/LICENSE`](licenses/google/chrome-for-testing/LICENSE) records
  the terms URL, the fact that all 308 archive entries carry no license of any kind, and the
  `chrome://credits` route that is the only place a user can read Chrome's own credits.
  [#28](https://github.com/elpideus/demido-studio/issues/28) then made the fetch conditional: Demido
  reaches for it only when the machine has no Chrome, Brave, Vivaldi or Opera it can drive. Edge is
  explicitly not counted, for measured reasons in
  [`docs/rules/setup.md`](docs/rules/setup.md) section 6.
- **`cudart` is NVIDIA's, and it is the larger half of the required download.** The companion archive
  #19 pinned is three redistributable DLLs, not part of `llama.cpp`, so it is a row of its own under
  the CUDA Toolkit EULA rather than under ggml-org's MIT.
- **The Python interpreter is a second astral-sh project.** uv fetches it from
  `python-build-standalone`, whose build tooling is MPL-2.0 and whose output is CPython under
  PSF-2.0. The fetched archive carries `LICENSE.txt` at its root, so this one unpacks.

`agent-browser` is Apache-2.0 (Copyright 2025 Vercel Inc.) and bundles axe-core under MPL-2.0, whose
text ships beside it in the package as `LICENSE-axe-core.txt`.

SearXNG's AGPL-3.0-or-later is the only copyleft in the manifest. Its 39 wheels are not: they resolve
to MIT, BSD, Apache-2.0, MPL-2.0, ISC and PSF-2.0, and nothing among them is viral. Demido runs
SearXNG as a separate process that binds no socket, which is the shape v2 chose deliberately and the
reason the network clause is never reached.

`llama.cpp` is credited here rather than in the table above because nothing fetches it yet. It is
never bundled: hard rule 3 puts it on the user's disk from an upstream release, which is a fetch and
still a distribution, so its license lands under `licenses/ggml-org/llama.cpp/` on the commit that
first fetches it. The build the rig is pinned to is in
[`docs/rules/done.md`](docs/rules/done.md), and every other pin in this table is in
[`docs/rules/setup.md`](docs/rules/setup.md) section 4 with its measured size.
