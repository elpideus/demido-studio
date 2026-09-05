# The lesson corpus

The fixture [`docs/rules/lessons.md`](../../docs/rules/lessons.md) is measured
against. Collected on wayfinder ticket
[#22](https://github.com/elpideus/demido-studio/issues/22).

## What is here

| File | What it holds |
|---|---|
| `corpus.jsonl` | 137 rows, one per line, each hand labelled: 42 model-produced failures and 95 mined signatures standing for 248. |
| `pairs.jsonl` | 34 teach/test pairs: same class, different surface. |

A case is `(command, surface, exit code, verbatim error)` plus two gold labels.
**Replay is re-classification, never re-execution.** Nothing here depends on
this machine still being in the state that produced it, which is what lets the
fixture outlive the rig.

```
case_id        stable, derived from the command and the error
origin         model | mined
source         which model produced it, or "transcript"
surface        the lesson store's surface key
weight         how many real failures this row stands for. Mined rows are one
               per error signature and carry the count; model rows are 1.
gold_proposed  the hand label under the thirteen classes that ship today
gold_ten       the same case under the ten that shipped before #22, where
               everything the ten could not hold is unclassified
```

Both gold columns are kept deliberately. `gold_ten` is what makes the amendment
falsifiable: it is the only way to re-run the comparison that justified adding
`not-found`, and a later vocabulary change should add a third column rather than
overwrite either.

## Where it came from

Two producers, chosen so that no single set of habits decides the vocabulary.

- **`origin: model`**, 42 cases. The development and reference models in
  [`docs/rules/done.md`](../../docs/rules/done.md), driven at 95 everyday
  Windows tasks across three harnesses, every command executed for real.
- **`origin: mined`**, 95 signatures standing for 248 failures. Every failed
  shell command in the 185 agent transcripts on this machine, deduplicated by
  error signature. A different producer entirely, which is why the two agreeing
  on `not-found` is worth something.

The harnesses live outside the repo. They drive live models against a real
shell behind a denylist, and they are eval scaffolding rather than product.

## What it cannot do yet

**Seven of the thirteen classes appear on exactly one surface**, so the
teach/test pair the transfer eval is built on cannot be constructed for them.
That, and not size, is what the corpus needs next: failures for the thin classes
on a *second* surface. `rate-or-quota` has no cases at all and will not until
Nexus rung 0 ships.
