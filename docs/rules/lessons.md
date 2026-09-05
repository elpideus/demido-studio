# A lesson is a class, not a memory

Decided on wayfinder ticket
[#13](https://github.com/elpideus/demido-studio/issues/13), and amended against
a real corpus on [#22](https://github.com/elpideus/demido-studio/issues/22).

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

Thirteen values. Twelve that retrieve, and one that does not.

**Terminal** says whether the model can fix this itself. It decides how the
lesson is injected, not whether it is stored. See [Injection](#injection).

| Class | The mistake | Terminal |
|---|---|---|
| `version-skew` | The installed version is not the one the model's memory assumes. | no |
| `dialect` | Right idea, wrong shell or language for this machine. | no |
| `not-installed` | The binary, module or package is not on this machine at all. | **yes** |
| `path-shape` | A path that cannot exist as written: separators, quoting, case. | no |
| `not-found` | The file, directory, ref or named object is not there. The path is well formed and nothing is at it. | no |
| `permission` | Needs elevation, or a credential that is not held. | **yes** |
| `api-shape` | Arguments that do not match the schema the tool actually accepts. | no |
| `state-order` | The step is right, but something has to happen first. | no |
| `stale-assumption` | The target exists, but not in the state assumed: the content, line or field looked for is not in it. | no |
| `rate-or-quota` | Refused for volume, not for correctness. | **yes** |
| `encoding` | Bytes, newlines or code page. Not logic. | no |
| `timeout` | The command never returned and was killed. | no |
| `unclassified` | Fits none of the above. Recorded, counted, never retrieved. | n/a |

Adding a class is a change to this file and a migration of the store. It is
meant to be a decision, not a convenience.

### What the corpus changed, and what it disproved

The first ten were authored before anyone had looked at a real failure, and
[#22](https://github.com/elpideus/demido-studio/issues/22) went and looked: 46
failures driven live out of the development and reference models, and 248
labelled out of the 362 in the agent transcripts on this machine, two producers
with nothing in common.

**`not-found` was the largest real class and it did not exist.** First in both
corpora, 39% of the mined failures and 17% of the model-produced ones. It is not
`path-shape`, whose definition is a path that *cannot* exist; these paths are
well formed and empty. It is not `not-installed`, which is about binaries.
Adding it lifts agreement with hand labels from about 52% to about 80% on every
tier.

**`timeout` and `stale-assumption`** appear in both corpora and in the mined one
respectively, at a few per cent each. `timeout` has no error text of its own, so
the harness synthesises one; see the trigger below.

**`rate-or-quota` fired zero times** in 610 failures. It stays, on reasoning
rather than on evidence: rung 0 of Nexus is one source at two requests per
minute per IP ([#18](https://github.com/elpideus/demido-studio/issues/18)), so
it will fire constantly the moment that ships, and a class the corpus has not
reached yet is not the same as a class that is wrong.

**The paragraph that used to stand here was wrong, and this is what it said:**

> `unclassified` is the vocabulary's own test. A class list authored before
> seeing real failures is a guess, and the honest way to find out it was wrong
> is to count what it refuses to hold rather than to widen it until nothing
> falls through.

It does not refuse. Given no `not-found`, the reference model's most used class
became `path-shape`, 33 of 141 predictions, and 85 weighted cases of `not-found`
came back labelled `path-shape` confidently. **A missing class does not raise
the `unclassified` count. It produces a wrong neighbour, and wrong classes
retrieve.** The valve this file relied on to detect its own error is the one
thing that was measured not to work, so the check moved to the `signal` field
below.

## What a lesson is

```
Lesson {
  id
  class        one of the thirteen above
  surface      shell:<name> | binary:<name> | tool:<name> | lang:<ext> | host
  signal       one line of the error text, chosen by index, never composed
  remedy       one to three imperative sentences
  origin       derived | inferred | taught
  evidence     [{ session, seq }]
  confidence   integer, starts at 0
  hits, misses integers
  scope        { profile, project? }
  state        live | retired
  created, last_used
}
```

### `signal` is the gate, and it is structural

`signal` was specified as the verbatim fragment of the error that identified the
class, and nothing enforced it. Measured, it actually was one **32% to 41%** of
the time on the development model and **65% to 77%** on the reference model. The
rest were composed: the word "error", a paraphrase, a fragment that appears
nowhere in the text it claims to quote.

So the model no longer writes `signal`. **It selects a line of the error by
index**, which a grammar enforces exactly, and a lesson whose `signal` is not a
line of the error it came from is refused at write time.

That is now the check that replaces the `unclassified` count, and it works for
the reason the count did not: a class stretched onto a failure it does not fit
usually cannot quote the line that supposedly identified it. It is not free.
Line selection only reaches **53%** expressibility on the development model
against **85%** on the reference model, so with the smallest classifier roughly
half of failures will produce no lesson at all. That is the trade taken
deliberately: fewer lessons, and the ones written are grounded in text that
exists.

`evidence` holds **pointers into the session log, never copies**. A lesson is a
few hundred bytes and the trace stays the single source of truth, which is what
lets the Lessons panel show a user the exact failure a lesson came from without
the store growing a second copy of the conversation.

**At most one live lesson per `(class, surface)` pair.** A newer one supersedes
the older, which is retired rather than deleted and stays visible as history.
That is the only contradiction rule there is, because two remedies disagreeing
in prose is not mechanically detectable and pretending otherwise would be
theatre.

It also bounds the store by construction: thirteen classes times the surfaces this
machine actually has. A lesson store cannot grow into a context problem, which
is the failure mode every "the assistant remembers things" feature eventually
hits.

## The trigger

**No recorded failure, no lesson. Ever. And no signal, no lesson.**

Every lesson anchors to a `tool/result` event carrying `failed: true`. That
field already exists (`ToolResult.failed`, v2's `demido-chat/src/events.rs`), so
the substrate is not new work.

**`failed: true` is a noisy bit, and the corpus measured how noisy.** A third of
the recorded failures in the mined corpus, 33%, are not failures: a non-zero
exit with no diagnostic at all, almost every one of them `grep` finding no match
inside a compound command. The model-produced corpus, one command per turn, has
9%. Separately, the eval harness's own first attempt at a failure test counted a
non-empty stderr as failure and so called three successes failures, because
`ffmpeg` writes its banner to stderr and `curl` writes a progress meter there,
both exiting zero.

Two consequences, and the second is the rule.

- **Demido sets `failed` from the exit status, never from stderr.** A tool that
  wants to report failure another way says so; stderr is not an error channel.
- **A failure carrying no error text never becomes a lesson.** There is nothing
  to select a `signal` from, so the record is malformed by construction, and the
  cheapest place to say so is the trigger. `timeout` is the one class this would
  wrongly exclude, since a killed command says nothing, so the harness writes
  the line itself: `the command did not finish within <n> seconds and was
  killed`. That is a real line of the real error text, authored by the runner
  rather than the model, and it selects like any other.

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

The **task model**, grammar constrained to the enum, run once **after the
turn ends** and never on the hot path. Both this and the judgement above it are
task-model jobs, and
[#24](https://github.com/elpideus/demido-studio/issues/24) settled which weights
that means: the conversation model by default, another one if the user assigns
it and the card has room, falling back to whichever is already resident rather
than evicting it. [`docs/rules/tasks.md`](tasks.md) holds the role, the
scheduling and the failure behaviour; this file holds what the job is asked. v2 already records whether a turn was
grammar constrained (`tools/offered` carries it, precisely so that "the model
produced arguments the schema forbids" has one explanation rather than two), so
constraining a classifier's output is existing machinery rather than a new
dependency.

**Thinking is off.** The grammar binds the content and not the reasoning, so a
model that reasons first spends its budget thinking and emits nothing at all:
the reference model at a 300 token budget produced an empty string on every one
of the first 34 cases. Given room to think it costs **21 seconds a case against
1.4 seconds** with thinking disabled, a factor of 15, on a step that runs after
every failed turn. Classification is a lookup into a thirteen-valued enum and it
does not need deliberation.

A heavier model, or Nexus, is a **quality upgrade to the same pipeline**, not a
second pipeline. There is one code path and it degrades by getting a smaller
classifier, not by changing shape.

The user is never the author of a remedy; the model writes it. The user is the
**anchor**, the **editor** and the **auditor**, and
[#23](https://github.com/elpideus/demido-studio/issues/23) is where each of
those three is drawn.

## What the user sees

Decided on wayfinder ticket
[#23](https://github.com/elpideus/demido-studio/issues/23), against
`prototypes/lessons-board.html`, eight screens at 1280 by 800. That board is
throwaway and is not in this repo.

Everything above this line is invisible by construction. It runs after the turn
ends, it stores a few hundred bytes, and nothing on screen has to move for any
of it to work. That is the failure to avoid rather than the design to keep: the
brief names two ways a lesson is learned and one of them is a person, and a
person who cannot tell that their correction landed is a person for whom half
the feature does not exist.

Three ways a lesson is born, and the store now records which:

| `origin` | Born from |
|---|---|
| `derived` | The model fixed it itself. Trigger path 1, no user involved. |
| `inferred` | A message that followed a failure, judged a correction by the task model. Trigger path 2. |
| `taught` | The user pressed **Teach** on the failed row. |

The field is one word and it earns its place twice: the Lessons page cannot say
where a lesson came from without it, and a false positive out of `inferred` is
the one kind worth finding, which needs telling apart from something a person
typed on purpose.

### The affordance: Teach, on failed rows

A failed `tool/result` row carries a ghost **Teach** beside **Retry**. Pressing
it opens a one-line field **anchored to that failure**, with the quoted error
above it, and what is typed goes to the classifier rather than to the model as a
turn.

It exists for one reason, and it is not convenience. Path 2 asks a small model
"is this a correction?" about a message, and that is a judgement that fails in
both directions: a false positive writes a lesson nobody asked for, a false
negative silently drops the brief's second path. **A press is not a judgement.**
Down the `taught` path that question is never asked, and the model's work
collapses to the two things it is measurably good at, a thirteen-valued enum
under a grammar and a line selected by index.

Inference stays, unchanged and unweakened, because most people will simply type
the sentence. The affordance is the high-confidence route into the same engine,
not a second engine.

### The write moment: a card, once, then a strip

A lesson is text the model will be shown verbatim, and the brief wants
everything the model sees inspectable. So the write is visible **where it
happened**: a card under the failure carrying the class, the surface, the quoted
`signal` line and the remedy, with **Edit** and **Discard** on it. Once the turn
is over it collapses to a one-line strip, because a transcript that accumulates
cards stops being a transcript.

**There is no confirmation gate**, and the alternative was drawn (board 5)
rather than argued away. An unapproved lesson never fires, so a queue nobody
drains is the feature quietly not working, and it fails identically to having
learned nothing at all. It also overprices the mistake: injection happens only
after a failure, so a wrong lesson costs exactly one bad hint, is then
measurable, and three misses retire it.

**And not a toast.** It interrupts, it dismisses into nothing, and it points
away from the one place that carries the context.

### Editing at the moment, and what an edit costs

Yes, in the card, and again later in the page. The remedy is model-written prose
that will be injected word for word; the cheapest moment to fix it is while the
failure it came from is still on screen.

**A user edit resets `hits`, `misses` and `confidence` to zero** and is recorded
in the lesson's history. The counters measure how a remedy performed, and after
an edit it is not that remedy any more. It keeps its id, because it is still one
live lesson for its `(class, surface)` pair and superseding itself would be
theatre.

### Where the panel lives: Settings

**Lessons is a Settings page**, not a rail entry and not a tab in the session
monitor.

The rail was refused for the reason [#8](https://github.com/elpideus/demido-studio/issues/8)
refused it a Sub-agents entry: the rail is for places, and it is already six
deep. The monitor is the closer call, since every lesson's `evidence` points
into the log the monitor renders, and it loses on scope: the monitor is one
session and the store outlives all of them. The adjacency is kept without the
move, because the evidence row **opens the monitor at that event**.

The page groups Live and Retired, and a lesson's detail carries the quoted
signal, the remedy, the evidence pointer, `hits`, `misses`, `confidence`, the
surfaces it has actually reached, and the lesson it superseded. Retired stays
visible, greyed and restorable, as [Being wrong](#being-wrong) already requires.

### Wrong-but-succeeded, still out, and now it says so

The gap [#13](https://github.com/elpideus/demido-studio/issues/13) accepted is
unchanged: a command that exits zero and is still wrong teaches nothing. The
affordance does not quietly reach it. Two reasons, and the second is the deeper
one: there is no error text, so there is no line to select and the record is
malformed at write time, and **twelve of the thirteen classes describe a way a
command fails**. "It ran and did the wrong thing" is not one of them, and
inventing a fourteenth is a schema change with a migration behind it.

On a row that did not fail, **Teach is greyed rather than hidden**, and says why
in one line, offering the thing that does work: tell the model in the chat, this
turn. Greyed rather than hidden is not a fresh preference, it is
[`design/system.md`](../../design/system.md)'s own rule for a capability tag,
where an absent capability greys to `ink-4` and is never hidden. The board drew
it as a live button opening a refusal; that is amended here to the greyed form.

The alternative, letting the user select a line of **output** as the `signal`,
is written down and not built. It buys the missing half at the price of the
invariant the whole engine rests on, and nothing has yet shown the half is
wanted.

### One amendment to the design system

[`design/system.md`](../../design/system.md) puts keycaps "never" inside the
transcript, and in the same file gives the approval prompt always-visible Enter
and Escape caps inside it. The rule is right and its wording is short by one
clause: **never on anything a model produced**, and a control that takes a
decision from the person is the exception. There are now two of them, the
approval prompt and the Teach field, and both take Enter and Escape. The card at
rest takes no cap at all.

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

**Only after a failure.** Never the system prompt. **A hint before the retry
when the class is not terminal, and a sentence addressed to the user when it
is.**

The split is measured, not tidy. Replaying 34 teach/test pairs on the
development model, one further turn after the failure, under three conditions:
nothing injected, the lesson taught on a **different** surface of the same
class, and a lesson of a deliberately wrong class.

| | recovered | gave up |
|---|---|---|
| nothing injected | 15% (8/54) | 56% (30/54) |
| the right lesson | 17% (9/52) | **23%** (12/52) |
| a wrong class | 17% (9/53) | 51% (27/53) |

**A lesson does not measurably fix the failure.** Recovery moves 15% to 17%,
which at this n is nothing (z = 0.35). What it does is stop the model giving up:
give-ups more than halve (z = 3.42). The wrong-class arm is the control that
makes this worth believing, because a wrong lesson is text in the context too
and it leaves give-ups at 51%, indistinguishable from injecting nothing. The
content is doing the work, not the presence of a hint.

But turning a give-up into a retry is only a gain where a retry can win, and
per class it often cannot:

- `not-found`: recoveries 2 to 4, give-ups 10 to 7. The mechanism working.
- `permission`: give-ups 13 to 4, and **one recovery in every arm**. Twelve
  extra attempts, zero extra successes.
- `not-installed`: give-ups 4 to 0, and **zero recoveries in every arm**.

On a failure the model cannot fix, the lesson talks it out of a correct
surrender and buys nothing. Hence `terminal` on the class. A terminal class
still learns, stores and retrieves exactly as any other; what changes is that
its remedy is addressed to the person (`this needs an elevated shell`, `uv is
not installed, and set-up can fetch it`) instead of being offered to the model
as something to try again.

The honest residue: a wrong `terminal` flag suppresses a fix that would have
worked, and `permission` is genuinely recoverable sometimes. The flag sits on
the class rather than the lesson so that it is one decision in one table, and it
is revisited when a corpus exists in which elevation is actually possible; the
scratch shell these numbers came from can never elevate.

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

Stored **per profile**. Project is a rank boost, not a second store.

A lesson encodes facts about this machine and this user's habits, and the brief
wants the machine shared by families and college rooms.

Brief B12: "Multi-account system"

Lessons never cross the profile boundary. No sharing, no export, no sync in
v0.1: what a user's lessons reveal about what a user does is a privacy question
that deserves deciding rather than defaulting. A profile is a Windows profile
([#15](https://github.com/elpideus/demido-studio/issues/15)), so the lesson store
lives under `%LOCALAPPDATA%` and the operating system holds the boundary. Two
people at one Windows login share one lesson store, which is the cost that
ruling names.

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

### The gates, and the one that cannot be set yet

Retrieval here is an exact match on the class enum, so **a lesson retrieves for
a new failure exactly when the classifier gives that failure the same class**.
Transfer and poison are therefore both bounded by classifier agreement, and it
is the number worth gating on. Measured on 137 hand-labelled cases, real
failures only, grammar constrained, thinking off:

| Tier | The old ten | The thirteen | Weighted |
|---|---|---|---|
| Development, `gemma-4-E4B-it` Q8_0 | 55.7% | **80.7%** | 86.0% |
| Reference, `gemma-4-26B-A4B-it` IQ2_M | 51.8% | **76.4%** | 85.6% |
| Breadth, `Qwen3.8-27B` IQ2_M | 50.7% | **84.5%** | 92.3% |

The 4B classifies as well as the 26B, and the breadth model best of the three.
This file worried that "a vocabulary a 4B cannot classify into is the same
failure as a wrong vocabulary". On this corpus **the model was never the
constraint**; the vocabulary was.

Three gates, and they are set where the evidence reaches:

1. **Classifier agreement on real failures is at least 75% on every tier**,
   weighted by frequency at least 85%. All three tiers clear it today. This is
   the gate that bounds poison, because poison is misclassification.
2. **Give-ups fall by at least half** against the engine off, on the
   non-terminal classes. Measured 56% to 23%.
3. **Recovery has no threshold yet, deliberately.** The engine moved it 15% to
   17% on 34 pairs, which is not a result, and the corpus is dominated by
   classes no retry could have fixed. Setting a number here would be inventing
   one. It gets a threshold when a corpus exists that can carry it, and the
   corpus cannot: of the thirteen classes, **seven were seen on exactly one
   surface**, so the teach/test pair this section is built on cannot be
   constructed for them at all. **A class needs failures on at least two
   surfaces before it can be said to transfer**, and reaching that is the next
   piece of collection work, not a bigger corpus of the same shape.

### The corpus

`evals/lessons/` holds it, and it is captured as `(command, surface, exit code,
verbatim error)` per case. **Replay is re-classification, never re-execution**,
which is what makes it work offline and a year from now: nothing in the fixture
depends on the machine still being in the state that produced it. The harnesses
that produced it live outside the repo, because they drive real models against a
real shell and they are not part of the product.

## What this deliberately does not do

- **Nothing here ships in v0.1.** The four slices are chat, tools, delegation
  and the downloader ([#11](https://github.com/elpideus/demido-studio/issues/11)),
  and this is none of them. It is specified now so that v0.1 does not foreclose
  it, and it does not: the log already records `failed`, which is the only thing
  the trigger needs.
- **It does not learn from success.** Only failures teach.
- **It does not detect semantic wrongness.** Exit zero teaches nothing.
- **It does not merge lessons.** Supersede is the only combination rule.
- **It does not learn from a failure with no error text.** There is nothing to
  quote, so there is nothing to store.
- **It does not measure similarity.** If two failures are related and the
  classifier put them in different classes, they are unrelated as far as this
  system is concerned. That is the price of making the kinds authored instead of
  emergent, and it is the whole reason a small model can do this at all.
