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
| uv | astral-sh | to confirm ([#27](https://github.com/elpideus/demido-studio/issues/27)) | [#21](https://github.com/elpideus/demido-studio/issues/21) |
| searxng | searxng | to confirm ([#27](https://github.com/elpideus/demido-studio/issues/27)) | [#21](https://github.com/elpideus/demido-studio/issues/21) |
| node | nodejs | to confirm ([#27](https://github.com/elpideus/demido-studio/issues/27)) | [#21](https://github.com/elpideus/demido-studio/issues/21) |
| agent-browser | to confirm | to confirm ([#27](https://github.com/elpideus/demido-studio/issues/27)) | [#21](https://github.com/elpideus/demido-studio/issues/21) |
| chromium | to confirm | to confirm ([#27](https://github.com/elpideus/demido-studio/issues/27)) | [#21](https://github.com/elpideus/demido-studio/issues/21) |

The five rows below `llama.cpp` are what
[#21](https://github.com/elpideus/demido-studio/issues/21) added by moving every runtime fetch from
first use to set-up. Their owners and licenses are written as **to confirm** rather than guessed:
[#27](https://github.com/elpideus/demido-studio/issues/27) reads each one upstream and replaces the
placeholder. A license nobody has read is not a license, and this file is what the in-app credits
surface renders.

`llama.cpp` is credited here rather than in the table above because nothing fetches it yet. It is
never bundled: hard rule 3 puts it on the user's disk from an upstream release, which is a fetch and
still a distribution, so its license lands under `licenses/ggml-org/llama.cpp/` on the commit that
first fetches it. The build the rig is pinned to is in
[`docs/rules/done.md`](docs/rules/done.md).
