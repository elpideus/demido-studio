# The task model

Decided on wayfinder ticket
[#24](https://github.com/elpideus/demido-studio/issues/24).

Brief B24:

> Task-model functionality. User should be able to set a model (the conversation one by default) that does simple tasks around Demido Studio, like renaming conversation every N messages based on context, and other such things.

Binds unamended. This file is what "other such things" turned out to be, and the
answer is smaller than the phrase suggests: four jobs, one seam, and a scheduler
whose only rule is that housekeeping never costs the user their model.

## A role, not a model

The task model is not a slot in a config file that some code reads. It is a
**shape of call**, and a job qualifies when all four of these hold:

1. It runs in a **scratch context**: no conversation history, no character, no
   tools, its own sampling.
2. Its output is **never rendered as a turn**.
3. Its result is **discardable**.
4. The caller is **correct when it is discarded**.

v2 already enforced all four, by hand, in one place. `demido-chat`'s
`agent/economy.rs` calls itself "a second job for the same weights" and refuses
the conversation's prompt on the grounds that "a model told it is a pirate
summarises like one". What that file did for one job, this file does for the
role.

So **a task-model job** is one such call, and **the task model** is whichever
weights answer it. The two are separate words on purpose: the second can change
between two dispatches of the first.

## The jobs

Four today. A fifth arrives by satisfying the four properties above, not by
being small or unimportant.

| Job | Trace event | Timing | What it is asked |
|---|---|---|---|
| Name a chat | `chat/named` | `deferred` | A short label for a conversation, from its opening exchange. |
| Judge a correction | `lesson/judged` | `deferred` | Is this message, which followed a failure, a correction? |
| Write a lesson | `lesson/classified` | `deferred` | A class from the enum, a signal line index, a two sentence remedy. |
| Shorten tool output | `economy/compressed` | `blocking` | A summary of output stage one could not shorten. |

The middle two are [`docs/rules/lessons.md`](lessons.md)'s, and they are two jobs
rather than one because they fail differently: a wrong judgement writes a lesson
that should not exist, a wrong classification writes the wrong lesson. Both run
grammar constrained with **thinking off**, which
[#22](https://github.com/elpideus/demido-studio/issues/22) measured at 1.4
seconds a case against 21 with a thinking budget, on a step that runs after every
failed turn.

## Scheduling: a job never displaces the conversation model

The rule Stefan set, and everything below is a consequence of it.

**A task-model job either runs simultaneously with the conversation model, when
resources allow it, or synchronously after the main model has finished.** It
never evicts, and it never queues the conversation behind itself.

That kills v2's plan on the way past. Its settled decision gave a small task
model "its own permanent allocation", and an allocation that is permanent cannot
be conditional on resources.
[#19](https://github.com/elpideus/demido-studio/issues/19) is why it had to go:
the breadth model at 32k leaves **271 MiB** on a 12 GB card, so a permanent
allocation is a model that fails to load rather than a job that waits.

It also gives `demido-vram`'s `Ledger` its first consumer. That crate is built,
tested, and dormant precisely because nothing ever asked it a question. This
asks it one, per dispatch.

### Never during generation

Three of the four jobs run when the turn ends and nobody is waiting.
Compression does not: it sits between a tool result and the next step, so the
conversation model is idle at that instant, but the turn cannot continue until
the summary comes back.

The timing rule is therefore **never during generation** rather than "after the
turn", and a job carries which kind it is:

- **`deferred`**: it queues, and the turn's own next step always outranks it.
- **`blocking`**: the turn waits for it.

**Compression is the only `blocking` job**, and a new one has to argue for it.
The argument it wins on is already priced: `economy.rs`'s summary can only ever
be an improvement, so "the worst this stage can do is cost a round trip".

### What "resources allow" measures

The **driver's free VRAM, read at dispatch**. Not a mode, not a startup
decision, and not a number the user typed: the same install answers yes beside
the development model and no beside breadth, which is a fact about today's
conversation rather than about the machine.

v2 refused a VRAM budget in the wizard for the same reason and said it well:
"the card's free memory is not what a settings page saw when it was drawn."
That refusal stands, and it is now load bearing rather than merely tidy.

**Cheapest simultaneity first.** There are two ways to run beside the
conversation model and they differ by an order of magnitude:

| Form | Costs | Measured on the rig |
|---|---|---|
| Same weights, second `llama.cpp` slot | one extra KV reservation | 563 MiB on the development model at 32k, 1129 MiB on the reference model |
| A separately assigned model | its whole weights, plus a slot | 4.9 GB for the smallest model on this rig |

The second slot is the default parallel path, and on a 12 GB card with a real
model loaded it is the only one reachable at all. A separately assigned model is
attempted only when its weights **and** its slot measure free.

One cost is stated rather than hidden. #19 ruled that `--ctx-size` is per slot
and that "the number the user sees has to be the one they get", so a task slot's
KV is part of the context arithmetic: **either it is shown, or the task slot is
not opened**.

## Which weights answer

The conversation model by default, per the brief, and the user may assign
another. The assignment is a **preference, never a guarantee**: if answering on
the assigned model would evict the resident one, the job runs on the resident
one instead, and the trace records which answered.

That is v2's instinct, "work that does not fit falls back to the primary rather
than queueing, because a stalled agent is worse than a slightly dumber one",
applied to housekeeping, where the case is stronger still: a chat title is worth
no user's second.

It also settles a contradiction inside `lessons.md`, which named "the task
model" in its trigger section and "the local chat model" in its "Who writes it"
section. The trigger section was right. The chat model is the **default**, not
the rule, and a heavier model or Nexus stays what that file already called it:
"a quality upgrade to the same pipeline, not a second pipeline."

## Naming a chat, and when to stop

The brief says "every N messages". Taken literally that is a title that never
settles, and a rail entry that moves under the cursor of the person trying to
click it.

- **First rename at the first exchange**, one user message and one answer. A
  blank rail entry is the worst state a chat can be in and it is cheapest to fix
  immediately. `SessionHeader.title` in v2 is `Option<String>`, documented as
  "Blank until the task model names it", so the seam exists and this is what
  fills it.
- **At most one more**, when the conversation has roughly tripled in length and
  the subject may genuinely have moved. Not a standing every-N loop.
- **A title the user typed is final.** The header gains a companion fact
  recording whether the name is the model's or the person's, and the job reads
  it and declines. This is
  [#23](https://github.com/elpideus/demido-studio/issues/23)'s rule in a second
  place: a person's edit is authoritative over a model's guess, and the same
  fact is what a per-chat "do not rename this" switch writes.

## Failure

**A task-model job is best effort, and its failure is indistinguishable from
never having run.** No rename, no lesson, stage one's text. There is no error
for the user to see, because nothing they have is worse than it was.

- **No retries and no queue on failure.** A job that could not run is not owed a
  second attempt: the state it would have improved is still correct.
- **Every attempt is logged, used or abandoned.** v2's disclosure rule for
  compression generalises to the role, and for its original reason: "the model
  would not answer" and "the model answered with something useless" are the same
  outcome and completely different problems.

The one job with a named non-answer is classification, where `unclassified` is
in the enum. #22 is the reason that is not treated as a general pattern: a
missing class showed up as a confident wrong neighbour rather than as a rising
`unclassified` count, so a refusal a model can express is worth having and is
never evidence that the vocabulary fits.

## Settings

Three values, and per [#8](https://github.com/elpideus/demido-studio/issues/8)
settings hold global values only, with a model, chat or character overriding in
its own section.

| Value | Scope |
|---|---|
| Which model answers task jobs | Global only. |
| Which jobs are on | Global only. |
| Rename cadence | Global, plus a per chat off switch. |

**No per-character override, at all.** A character is a persona for the
conversation, and a task-model job runs in a scratch context that never sees it.
A character section offering a task-model field would offer a setting the
mechanism is built to refuse to read.

The per chat exception earns its place by being the same switch as "I named this
one myself": one fact, written by an edit or by a toggle, read by one job.

## What this does not decide

- **Which small model to recommend** as a dedicated task model, and whether the
  set-up manifest offers one. That is a size and a license, which is
  [#27](https://github.com/elpideus/demido-studio/issues/27)'s shape of work,
  and it cannot be asked before there is a candidate.
- **Whether a fifth job exists.** The four properties are the test; nothing on
  the map currently passes them and is not listed here.
