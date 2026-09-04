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
