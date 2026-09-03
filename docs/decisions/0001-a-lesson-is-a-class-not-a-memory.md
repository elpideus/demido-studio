# 0001. A lesson is a class, not a memory

Status: accepted
Decided: [#13](https://github.com/elpideus/demido-studio/issues/13)

## Decision

The lesson engine generalises by **classifying into a closed, authored
vocabulary of ten failure classes**, not by learning categories from prose and
not by measuring similarity between failures. A lesson is `(class, surface,
signal, remedy)` plus provenance, at most one live per `(class, surface)` pair.

Retrieval is an exact match on `class`, with `surface` re-ranking and never
filtering. There is no embedding model. Injection happens only after a recorded
failure, as a hint before the retry, and never in the system prompt.

The full contract is [`docs/rules/lessons.md`](../rules/lessons.md).

## Consequences

Transfer across surfaces becomes mechanical, which is the only version of this
feature a 4B model can carry: mpv retrieves the PowerShell lesson because the
classifier put both in `version-skew`, not because anything compared them.

The store is bounded by construction at ten classes times the surfaces present
on the machine, so it can never grow into a context problem.

It forecloses relatedness the vocabulary does not name. Two genuinely similar
failures placed in different classes are unrelated to this system, and the only
repair is to change the vocabulary here and migrate the store. `unclassified` is
kept and counted so that the vocabulary being wrong is visible rather than
silent.

No embedding model competes for VRAM with the model the user wanted to run, and
retrieval reuses the FTS5 index the trace already ships.
