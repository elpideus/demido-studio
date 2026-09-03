# Demido Studio agent contract

An LLM harness that makes small models behave, and shows you why.

Read this file fully. It is deliberately short; everything else loads on demand.

---

## Read the brief. Every session.

[`docs/brief.md`](docs/brief.md) is Stefan's brief, copied verbatim. It is 196
lines. Read all of it. Never summarise it, never work from a summary of it, and
never resolve a ticket or write a line of code from one.

Then read [`docs/brief-map.md`](docs/brief-map.md): every requirement of the
brief as a row, what has been decided about it, what has been built, whether a
real model has ever driven it, and which lines Stefan has since overruled. Find
the rows your work touches. Fill in the cell you earn.

Cite the brief in every resolution and in anything you write into the repo, in
the form [`docs/rules/brief.md`](docs/rules/brief.md) fixes. CI checks that your
quote is really in the brief.

This is not ceremony. v2 was set aside for three reasons, and one of them was
that its milestones 10 and 11 were built from a summary of the brief instead of
the brief, discovered only by an audit milestone that re-read the original item
by item. v2's own contract file pointed every session at a 1817-line roadmap
under the words **"Read this first."** That roadmap was the summary. v3 has no
roadmap: the open issues are the sequence, the map is the index of what is
settled, and this file plus the brief are the contract.

## Hard rules

Enforced by CI (`node scripts/check-rules.mjs`). Violating one fails the build.

1. **Attribution.** Every commit is authored by
   `Stefan Cucoranu <elpideus@gmail.com>` and signed. Never add
   `Co-Authored-By`, and never name an AI in a commit message, a PR body, an
   issue, or any other repo metadata. *(check lands on
   [#16](https://github.com/elpideus/demido-studio/issues/16))*
2. **No code from `open-webui`**: its license forbids removing Open WebUI
   branding above 50 users. **No code from `openclaude`**: it is derived from
   proprietary Claude Code without authorization. Read them for patterns; never
   copy. Safe to port from: opencode, gemini-cli, jan, anything-llm, 9router,
   OmniRoute. *(#16)*
3. **Nothing third-party is bundled into the installer.** Runtimes, inference
   backends and anything else are fetched from upstream onto the user's machine
   at first use. Nothing is pre-installed, and each thing states its size before
   it is fetched. *(#16)*
4. **Arbitrary values live in `design/tokens.css`** and nowhere else: colour,
   type, space, radius, motion. Every theme's contrast is recomputed rather than
   claimed. See [`docs/rules/no-raw-values.md`](docs/rules/no-raw-values.md).
   **Enforced now.**
5. **Ported code carries a provenance header** (source repo, commit, license)
   and a matching entry under `licenses/<owner>/<repo>/LICENSE`. *(#16)*
6. **No em dashes.** Anywhere: code, comments, docs, UI copy, issues, commit
   messages. Use a colon, a comma, parentheses, a semicolon or a full stop.
   *(#16)*
7. **The brief stays canonical.** Anchors verbatim, every bullet covered, every
   citation quoting rather than paraphrasing. See
   [`docs/rules/brief.md`](docs/rules/brief.md). **Enforced now.**
8. **Decision references resolve.** A `docs/decisions/NNNN-slug.md` reference in
   code or docs points at a note that exists and is not superseded. *(#16)*
9. **Every crate documents itself.** Each directory under `src-tauri/crates/`
   contains an `AGENTS.md`. *(#16, once there are crates)*

Two of v2's eight rules are not here. Its colour rule is rule 4, widened from
one family to five on
[#9](https://github.com/elpideus/demido-studio/issues/9). Its dev-only MCP
bridge rule waits until there is a bridge.

The asymmetry that justifies all of this was measured in v2, in one codebase, by
one author, in one year: the **enforced** colour rule held perfectly across 214
commits, while the **unenforced** type rule grew to 21 distinct font sizes. A
rule an agent can break without CI noticing is a rule that will be broken.

## Design rules

- **Driven live, or it is not done.** A ticket closes on two gates that fail
  differently: a live-model scenario, and a screenshot of the running window.
  See [`docs/rules/done.md`](docs/rules/done.md). Twelve v2 features were built,
  unit-tested and never once operated; the first live run found a bug in the
  newest of them within one question.
- **Port quarantine.** A v2 crate is a *candidate*, never a port. It enters v3
  inside a vertical slice, and only once that slice is driven live.
- **Startup never blocks.** A subsystem that fails is reported and skipped; the
  app still reaches a usable state. Never trap the user on a boot screen.
- **Islands are separated by gaps, not borders.** The gap is the border. A
  hairline is allowed only in the two cases named in
  [`docs/rules/gaps-and-hairlines.md`](docs/rules/gaps-and-hairlines.md).
- **Every surface has a declared role**, decided in
  [`docs/rules/surfaces.md`](docs/rules/surfaces.md), not at the component. A
  component that wants a surface the table has no row for is asking for a row.
- **Icons come from a pack** (Lucide, Simple Icons). Never drawn by hand.
- **Tiles are swappable.** Every trait has a contract test suite that any
  implementation must pass, written before the second implementation. See
  [`docs/rules/tiles.md`](docs/rules/tiles.md).
- **Prefer deleting to adding.** This codebase should feel like a server rack:
  pull a unit out and the rest keeps running.

## Where things are

| Path | What |
|---|---|
| `docs/brief.md` | The brief, verbatim. **Read this first.** |
| `docs/brief-map.md` | Every requirement, its state, and the amendments. |
| `docs/rules/` | One file per rule. The CI-enforced ones say so at the top. |
| `docs/decisions/` | Why things are the way they are. Short notes, one per load-bearing choice, each linking the ticket that holds the reasoning. |
| `docs/agents/` | Configuration the installed engineering skills read. |
| `design/` | The design system: `tokens.css` owns every arbitrary value, and `system.md`, `shell.md`, `windows.md` are the frozen boards. |
| `licenses/` | One `LICENSE` per ported source, mirroring `<owner>/<repo>`. |
| `scripts/check-rules.mjs` | The hard rules above. No dependencies, on purpose. |

There is no roadmap file and no history file. v2's reached 1817 and 1364 lines,
had to be split, and duplicated state GitHub already holds. The issues are the
sequence, a release tag is the history.

## Decision notes

`docs/decisions/NNNN-slug.md`, and only for a choice that is hard to reverse,
surprising without context, and the result of a real trade-off. Ordinary
features and bugfixes do not get one.

A note is **short**: the decision stated in a few lines, plus a link to the
ticket or session that holds the reasoning. It exists so that code can reference
it and so a reader offline can still find out why. The reasoning lives in
exactly one place, and it is not the note. A wayfinder resolution earns a note
only when code will point at it.

Reference notes from the code they govern. When superseding one, set
`Status: superseded-by NNNN` and update the references.

## Commands

Not yet. The stack is decided
([#10](https://github.com/elpideus/demido-studio/issues/10): Tauri 2, Rust,
React 19, Radix, CSS Modules, no Tailwind and no shadcn) but nothing is
scaffolded. Until then the only gate is:

```bash
node scripts/check-rules.mjs          # the hard rules above
node scripts/check-rules.mjs --report # print the contrast measurements
```

The frontend and Rust gates, the live-model harness invocation and the versioning
scheme arrive with the scaffold on
[#16](https://github.com/elpideus/demido-studio/issues/16) and the first slice on
[#11](https://github.com/elpideus/demido-studio/issues/11).

## Agent skills

Configuration the installed engineering skills read lives in `docs/agents/`.

- **Issue tracker.** GitHub issues on `elpideus/demido-studio`, driven by the
  `gh` CLI. See [`docs/agents/issue-tracker.md`](docs/agents/issue-tracker.md).
  The foundation for v3 is being charted as a wayfinder map,
  [#1](https://github.com/elpideus/demido-studio/issues/1).
- **Domain docs.** Single context. Vocabulary in `docs/glossary.md` when there
  is code to name; decision notes in `docs/decisions/`.
