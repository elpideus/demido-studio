# Provenance and licensing

**Enforced now.** `scripts/check-rules.mjs`, rule `provenance`.

Decided on wayfinder ticket
[#16](https://github.com/elpideus/demido-studio/issues/16). This is hard rules 2
and 5 of [`AGENTS.md`](../../AGENTS.md).

Brief B46:

> A licenses folder will also be needed

## The rule

1. A file containing code taken from somewhere else carries a **provenance
   header** naming the source, the commit and the license.
2. Every source named that way has its license text at
   `licenses/<owner>/<project>/LICENSE`.
3. Every source named that way has a row in
   [`THIRD_PARTY_NOTICES.md`](../../THIRD_PARTY_NOTICES.md), which is the index
   the in-app credits surface renders.
4. Nothing is copied from a project on the ruled-out list.

Rules 2 and 3 hold in both directions: a license on disk that nothing credits
fails, and a credited project with no license on disk fails.

## The header

The first comment of the file, in the file's own comment syntax, containing one
line in exactly this shape:

```rust
// Ported from opencode/opencode @ 4f2c1ab, MIT.
//
// Adapted: the session store is ours, only the stream parser is theirs.
```

`Ported from <owner>/<project> @ <commit>, <license>.` The commit is what makes
the header useful: "from opencode" is unfindable a year later, "from opencode at
4f2c1ab" is a diff. The license is the SPDX id, and it must match the text under
`licenses/`.

A file that is merely *inspired* by another gets no header, because the header
is a legal claim about copied expression rather than a reading list. When in
doubt, the test is whether the two files would diff usefully.

## Safe to port from

opencode, gemini-cli, jan, anything-llm, 9router, OmniRoute. All MIT or
Apache-2.0, all compatible with GPL-3.0-or-later in this direction.

## Ruled out

Two projects are readable for patterns and are never copied from. The check
rejects them by name, and the reason is legal rather than editorial:

| Project | Why |
|---|---|
| `open-webui` | Its license carries a branding-retention clause: removing Open WebUI branding above 50 users is forbidden, which our distribution cannot honour. |
| `openclaude` | It is a derivative of proprietary code, redistributed without authorization. Its own position is not one we can inherit. |

The ruled-out table in `THIRD_PARTY_NOTICES.md` is where the list lives and may
grow. It cannot shrink below these two: both are pinned in the checker, so
deleting a row does not lift a ban.

## Nothing third-party is bundled

Hard rule 3 of `AGENTS.md`. Runtimes and inference backends are fetched from
upstream onto the user's machine at first use, each stating its size before it is
fetched (`design/windows.md`). That rule is enforced by review rather than by
this checker: it needs an installer to check, and there is no installer yet. It
lands with the packaging work.

## What this does not check

- **Whether a header is telling the truth.** Nothing here compares the file with
  the upstream commit it names. The header is a claim by the author; the check
  is that the claim is complete and that the paperwork behind it exists.
- **Uncredited copying with no header at all.** A file that copies without
  saying so is invisible to a checker that reads headers. This is the reason
  port quarantine exists: a candidate crate enters through a slice a human drove,
  not through a commit nobody read.
