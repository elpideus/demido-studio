# No em dashes

**Enforced now.** `scripts/check-rules.mjs`, rule `text`, and the
`.githooks/commit-msg` hook.

Decided on wayfinder ticket
[#16](https://github.com/elpideus/demido-studio/issues/16). This is hard rule 6
of [`AGENTS.md`](../../AGENTS.md).

## The rule

The em dash (U+2014) appears nowhere: not in code, comments, documentation, UI
copy, issue bodies or commit messages. Use a colon, a comma, parentheses, a
semicolon or a full stop.

## Why it is checked

It is the smallest rule in the repo and the least important on its own, which is
exactly what makes it worth enforcing. Stefan asked for it, an agent produces the
character by reflex several times a page, and nobody would notice it drifting
back. That is the shape of every rule this project has lost before: v2's colour
rule was enforced and held perfectly across 214 commits, its type rule was not
and grew to 21 distinct font sizes in the same codebase.

Enforcing it costs one regular expression. Not enforcing it means the house style
is whatever the last session felt like.

## What is excluded

- [`docs/brief.md`](../brief.md), which is reproduced verbatim and is never
  edited. If the brief ever contains the character, it stays.
- `licenses/`, which holds other people's license texts word for word.

Binary files are not read at all. Everything else under `docs/`, `design/`,
`scripts/`, `.github/`, `.githooks/`, the source tree and the three root
Markdown files is checked line by line, and the violation names the line.
