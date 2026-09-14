# 0009. An assembly refers to its blocks

Status: accepted
Decided: [#41](https://github.com/elpideus/demido-studio/issues/41)

Brief B07: "recorded in an append-only session log"

## Decision

**A `turn/assembly` event carries the sequence numbers of the blocks that were
sent, in order, and never their text.** A block is resolved by reading the event
at that position: a message by its own text, a paragraph by refilling the
wording the log stored under its hash, an answer by the completion that carried
it.

So nothing in a session is written twice. A question asked at turn 1 and still
in the window at turn 40 is one event, named by thirty nine assemblies.

## Consequences

Easy: the log stays linear in the length of the conversation rather than
quadratic. Forty turns of a growing context would otherwise hold roughly eight
hundred copies of the earlier ones.

Easy: **eviction becomes expressible without an eviction event.** A block that
one assembly names and the next does not has been evicted, and the two lists are
already the diff `design/windows.md` renders as an inserted block you can read
and a struck-out one with its cost beside it. Nothing has to remember to record
a dropping.

Easy: an edit to a paragraph mid session cannot rewrite what an earlier turn was
sent, because a fragment names a **hash** and a new wording is a new hash. This
is the mechanism [`docs/rules/prompts.md`](../rules/prompts.md) already
specifies, and it only works because the block is a reference.

Hard: the log has to resolve its own references, so a pruned export is not free.
Cutting an event that a later assembly still names produces a log that cannot
rebuild that turn, and `Replay` reports it as a dangling reference rather than
quietly producing a shorter prompt. Any pruning that ships has to prune whole
assemblies.

Foreclosed: reading one line of the log and knowing what was sent. A person in
the raw JSON tab sees positions, and the monitor is what turns them back into
text. That is the cost of not having a second copy of the conversation, and the
second copy is the thing this product exists to not have.
