# Decision references resolve

**Enforced now.** `scripts/check-rules.mjs`, rule `decisions`.

Decided on wayfinder ticket
[#16](https://github.com/elpideus/demido-studio/issues/16). This is hard rule 8
of [`AGENTS.md`](../../AGENTS.md). What a decision note *is*, and when one is
written at all, is [`docs/decisions/README.md`](../decisions/README.md).

## The rule

1. A `docs/decisions/NNNN-slug.md` reference anywhere in the repo points at a
   note that exists.
2. That note is not superseded. A note carries `Status: accepted` or
   `Status: superseded-by NNNN`, and a reference to the second kind fails.

## Why

A dangling reference is worse than no reference: it tells the reader an
explanation exists and then wastes their time proving it does not. A reference to
a superseded note is worse still, because it resolves, reads as current, and
hands them reasoning that has since been overturned. Both are silent failures
that only a checker catches, since neither breaks a build or a test.

Superseding a note means setting its `Status:` line and updating what points at
it. The check finds whatever was missed.

## What this does not check

That a note is *right*, or that the code referencing it still does what the note
says. `NNNN-slug.md` written literally, as a placeholder in prose about the
convention, is ignored: the pattern needs four digits.
