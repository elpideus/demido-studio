# A lesson is a class, not a memory

Decided on wayfinder ticket
[#13](https://github.com/elpideus/demido-studio/issues/13).

## The requirement

Brief B05:

> NOT this specific issue, but this specific KIND of issue

Brief B04: "get the most out of even smaller models"

The brief's example is a PowerShell 5.1 flag that PowerShell 7 renamed. The
specific lesson is "use `-TargetName`, not `-ComputerName`". The transferable
lesson is closer to "when a command rejects a flag, check the installed version
before trusting your memory of its syntax". The brief then asks for that second
lesson to fire on mpv and on hyprland, which share nothing with PowerShell
except the shape of the mistake.

Nothing in v1 or v2 implements any of this.

## The move that makes it possible

Getting from the first lesson to the second is a hard inference **only if the
kinds are emergent**. A 4B model asked to invent the category "version skew"
from one PowerShell failure, in prose, and then to recognise that an mpv failure
belongs to the same invented category, will not do it reliably.

So the kinds are not emergent. **The class vocabulary is closed and authored
here**, and the model's job collapses from open-ended generalisation to
classification into a fixed enum plus a short remedy. Transfer then costs
nothing: mpv and hyprland retrieve the PowerShell lesson because the classifier
put all three in `version-skew`, not because anything measured their similarity.

The generalisation is a schema decision, not a model capability.

## The classes

Ten values. Nine that retrieve, and one that does not.

| Class | The mistake |
|---|---|
| `version-skew` | The installed version is not the one the model's memory assumes. |
| `dialect` | Right idea, wrong shell or language for this machine. |
| `not-installed` | The binary, module or package is not on this machine at all. |
| `path-shape` | A path that cannot exist as written: separators, quoting, case. |
| `permission` | Needs elevation, or a credential that is not held. |
| `api-shape` | Arguments that do not match the schema the tool actually accepts. |
| `state-order` | The step is right, but something has to happen first. |
| `rate-or-quota` | Refused for volume, not for correctness. |
| `encoding` | Bytes, newlines or code page. Not logic. |
| `unclassified` | Fits none of the above. Recorded, counted, never retrieved. |

`unclassified` is the vocabulary's own test. A class list authored before seeing
real failures is a guess, and the honest way to find out it was wrong is to
count what it refuses to hold rather than to widen it until nothing falls
through.

Adding a class is a change to this file and a migration of the store. It is
meant to be a decision, not a convenience.

## What a lesson is

```
Lesson {
  id
  class        one of the ten above
  surface      shell:<name> | binary:<name> | tool:<name> | lang:<ext> | host
  signal       the verbatim fragment of the error text that identified the class
  remedy       one to three imperative sentences
  evidence     [{ session, seq }]
  confidence   integer, starts at 0
  hits, misses integers
  scope        { account, project? }
  state        live | retired
  created, last_used
}
```

`evidence` holds **pointers into the session log, never copies**. A lesson is a
few hundred bytes and the trace stays the single source of truth, which is what
lets the Lessons panel show a user the exact failure a lesson came from without
the store growing a second copy of the conversation.

**At most one live lesson per `(class, surface)` pair.** A newer one supersedes
the older, which is retired rather than deleted and stays visible as history.
That is the only contradiction rule there is, because two remedies disagreeing
in prose is not mechanically detectable and pretending otherwise would be
theatre.

It also bounds the store by construction: ten classes times the surfaces this
machine actually has. A lesson store cannot grow into a context problem, which
is the failure mode every "the assistant remembers things" feature eventually
hits.

## The trigger

**No recorded failure, no lesson. Ever.**

Every lesson anchors to a `tool/result` event carrying `failed: true`. That
field already exists (`ToolResult.failed`, v2's `demido-chat/src/events.rs`), so
the substrate is not new work.

Two paths reach a lesson, and they are the two the brief names.

1. **The model dealt with it.** A failed result, then a later result from the
   same tool that did not fail, with materially different arguments, inside the
   same turn. v2's `stuck.rs` already derives exactly this shape of consecutive
   call reasoning from the log, with no counters to reset and no state a resumed
   session can lose.
2. **The user dealt with it.** A user message that directly follows a failed
   result. Only then, and never otherwise, the task model is asked the narrow
   question "is this a correction?".

Path 2 is tractable *because* it is anchored. Free-floating "did the user just
correct me?" over every message is a classification a small model gets wrong in
both directions, and each false positive writes a lesson that then fires
forever. Anchoring turns an open judgement into a closed one.

The cost, accepted: a command that exits zero and is still wrong teaches
nothing. That is a real gap and it is not covered here.

## Who writes it

The **local chat model**, grammar constrained to the enum, run once **after the
turn ends** and never on the hot path. v2 already records whether a turn was
grammar constrained (`tools/offered` carries it, precisely so that "the model
produced arguments the schema forbids" has one explanation rather than two), so
constraining a classifier's output is existing machinery rather than a new
dependency.

A heavier model, or Nexus, is a **quality upgrade to the same pipeline**, not a
second pipeline. There is one code path and it degrades by getting a smaller
classifier, not by changing shape.

The user is never the author. The user is the auditor.

## Retrieval

**No embeddings.** An embedding model is resident weights competing for VRAM
with the model the user actually wanted to run, on the single card the three
test models already fill. Against that, `demido-trace` already stands up an
FTS5 table with a BM25 ranker for session search, so keyword retrieval is a
dependency that ships, works and has been exercised.

More to the point, the class does most of the work, and the class is an exact
match on a ten-valued enum. There is very little left for a similarity measure
to contribute.

Retrieval, in order:

1. Exact match on `class`.
2. `surface` **re-ranks, and never filters.** This is where transfer physically
   happens: a `version-skew` lesson learned on `binary:pwsh` is retrieved for a
   `version-skew` failure on `binary:mpv` because the class matched. Filtering
   by surface would delete the entire feature.
3. BM25 over `signal` and `remedy` as a tiebreak, then confidence, then recency.

**Budget: at most three lessons, and a hard ceiling of 400 tokens.** Over
budget, the lowest ranked lesson is dropped whole. A truncated remedy is worse
than a missing one, because it reads as complete.

## Injection

**Only after a failure, as a hint before the retry.** Never the system prompt.

Three reasons, in order of weight. It is the only point where the retrieval key,
the actual error text, exists. A wrong lesson costs exactly one bad hint and is
then measurable, rather than quietly poisoning every turn of every session. And
it is free on the turns where nothing went wrong, which is most of them.

The honest cost: a lesson never *prevents* the first failure, so the user pays
one failed call each time. The brief asks that the model "now knows how to solve
those too", not that it never errs, so this satisfies the requirement as
written. A pre-emptive slot before a tool call is deferred until the hit rates
below say it would earn its context.

Injection is a logged event. `lesson/injected` carries the lesson id and its
token weight, so it appears in the session monitor as a context injection like
any other and the prompt can still be rebuilt as it stood at any event
([#8](https://github.com/elpideus/demido-studio/issues/8)).

Brief B06: "All prompts should be editable. Requests monitorable."

## Being wrong

A lesson learned once and wrong is applied forever unless something stops it.

- **Hit**: injected, and the next call of that tool did not fail. `confidence + 1`.
- **Miss**: injected, and it failed the same way again. `confidence - 1`.
- **Retired** when `misses >= 3` and `misses >= hits + 2`, or when it has not
  been used in 90 days and `confidence <= 0`.
- Retired lessons are kept, shown greyed, and restorable. Nothing is deleted
  behind the user's back.

Every lesson is visible, editable and deletable in a Lessons panel. That is not
a nicety: an injected lesson is something the model sees, and the brief wants
everything the model sees inspectable.

Brief B07: "recorded in an append-only session log"

## Scope

Stored **per account**. Project is a rank boost, not a second store.

A lesson encodes facts about this machine and this user's habits, and the brief
wants the machine shared by families and college rooms.

Brief B12: "Multi-account system"

Lessons never cross the account boundary. No sharing, no export, no sync in
v0.1: what a user's lessons reveal about what a user does is a privacy question
that deserves deciding rather than defaulting. This follows whatever boundary
[#15](https://github.com/elpideus/demido-studio/issues/15) settles, and does not
weaken it.

## How we know it transfers

Without this the feature is a vibe, and a vibe is how v2 shipped twelve things
nobody had driven.

`evals/lessons/` holds a fixture corpus. Every case is a **pair**: a `teach`
failure, and a `test` failure of the same class on a **different surface**. The
PowerShell case teaches on `binary:pwsh` and tests on `binary:mpv`.

Two numbers, and the second matters as much as the first:

- **Transfer rate.** Taught on A, does the lesson retrieve and fix B?
- **Poison rate.** How often does a lesson fire on a case of a different class?

Both measured against the same corpus with the engine off, and run across the
three tiers in [`done.md`](done.md), not against one model. A red on breadth
alone is a note, not a defect.

`Bar: chose`.

## What this deliberately does not do

- **Nothing here ships in v0.1.** The four slices are chat, tools, delegation
  and the downloader ([#11](https://github.com/elpideus/demido-studio/issues/11)),
  and this is none of them. It is specified now so that v0.1 does not foreclose
  it, and it does not: the log already records `failed`, which is the only thing
  the trigger needs.
- **It does not learn from success.** Only failures teach.
- **It does not detect semantic wrongness.** Exit zero teaches nothing.
- **It does not merge lessons.** Supersede is the only combination rule.
- **It does not measure similarity.** If two failures are related and the
  classifier put them in different classes, they are unrelated as far as this
  system is concerned. That is the price of making the kinds authored instead of
  emergent, and it is the whole reason a small model can do this at all.
