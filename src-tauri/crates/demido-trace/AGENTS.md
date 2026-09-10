# demido-trace

The append-only session log: the `Journal` trait, its contract suite, the two
journals that implement it, and the projections everything else in the app reads
instead of keeping a copy.

Brief B07: "recorded in an append-only session log"

## The one thing to understand

**The log is the source of truth, and it can rebuild what was sent rather than
describe it.**

Both halves are load-bearing. There is no chat table anywhere: a transcript is
`Replay::history`, an export is a projection of the same events, and the Session
Monitor will be a third. And a fragment is recorded as *the hash of a paragraph
plus the values that filled it*, never as the text it produced, so replaying it
means filling the paragraph again. A log that stored the finished text would
pass every rebuild test by copying, and would still be unable to answer the
question `design/windows.md` asks it: what did the model actually see at that
moment, and what changed since the moment before.

## The trait and its contract suite

`Journal` (`src/journal.rs`) has two methods, because there are two things
anyone does to an append-only log. Nothing in the type can rewrite a line or
delete one.

**`src/contract.rs` is the trait's second file**, per
[`docs/rules/tiles.md`](../../../docs/rules/tiles.md). Call
`contract::assert_journal(|name| ...)` from your implementation's own test file.
It takes a factory rather than a journal because the promise that matters most
cannot be tested on one handle: a log has to come back after the process that
wrote it is gone.

| Implementation | Where | Runs the suite in |
|---|---|---|
| `JsonLines` | `src/jsonl.rs` | `tests/journal_contract.rs` |
| `Memory` | `src/memory.rs` | `tests/journal_contract.rs` |

`JsonLines` is what the app writes. `Memory` is what a test that is about a turn
rather than about a file uses, and what a session the user asks not to keep will
be.

## The two fields on every event

`design/windows.md` fixes both, and the reason they are here from the first line
of code rather than added when a screen needs them is that adding either later
means rewriting the slice that wrote the events without them.

**Source.** Exactly the eight `--src-*` tokens in `design/tokens.css`, with the
same names, so the enum and the monitor's ledger cannot come to disagree about
how many there are. `Source::Reasoning` is the model's own output, its reply as
well as its thinking: there is no ninth source because a ninth source is a ninth
colour, and that is a change to a frozen board.

**Weight.** Per event, never per request, and it says whether it was counted or
estimated. `Weigher` is a seam: the default `Estimate` needs nobody and marks
everything it produces as an estimate, and a weigher that asks the running
backend's own tokeniser is one line at the construction of a `Session`. The live
suite uses the second, which is how the counted path is held to the same shape
as the estimated one.

## Writing and reading

| Module | What |
|---|---|
| `record` | `Session` and `Turn`. A turn records itself and `Turn::send` hands back the request to send, so nothing can send an assembly it did not record. |
| | `Turn::carry` takes a **position**, never text, and resolves it from the log. Taking the text as well would be the one call left that could put words in the log's mouth, and it would look exactly like a correct one. |
| `replay` | `Replay`. History, the rebuild, the per-turn occupancy and the per-source ledger, all over the same events. |

An assembly refers to its blocks **by sequence number** rather than copying
them, which is what keeps a forty turn session from holding forty copies of its
own history, and what makes an eviction expressible as a block that the next
assembly does not name. See
[`docs/decisions/0009-an-assembly-refers-to-its-blocks.md`](../../../docs/decisions/0009-an-assembly-refers-to-its-blocks.md).

## What this crate depends on and why

`demido-inference`, for the shapes a turn is made of: a message, a parameter
set, a finish reason, a token count. Recording those means recording the
inference crate's own types rather than a second declaration of them that can
drift, which is the same argument that makes history a projection of this log
instead of a table beside it.

`demido-prompts`, because rebuilding a fragment is `fill` over a stored wording
and nothing here re-implements it.

## What is deliberately not here

**The Session Monitor's UI.** This is the one place in v0.1 where a slice ships
data ahead of its screen on purpose
([#41](https://github.com/elpideus/demido-studio/issues/41)), because history is
derived from the log and so the log cannot wait for the window that inspects it.
The monitor is [#57](https://github.com/elpideus/demido-studio/issues/57).

**Anything a later slice will record.** Tool calls, approvals, artifacts and
sub-agent scheduling are each a `Body` variant, a source that already exists, and
a rebuild case. Declaring them now would be declaring a shape nothing can be
held to, which is the same rule `demido-inference` applies to its own `Chunk`.

## The tests

`cargo test -p demido-trace` runs everything that needs no card, including
`tests/a_session.rs`, which replays the committed trace of a real live run out
of `tests/fixtures/`. That fixture was written by a different process on a
different day, which is the restart promise across a boundary no harness can
fake ([`docs/rules/done.md`](../../../docs/rules/done.md): "a pruned,
deterministic trace is exactly a replayable fixture").

`tests/a_real_model.rs` is the live suite, `#[ignore]`d and run by the live
command in [`AGENTS.md`](../../../AGENTS.md). It **fails rather than skips**
when the rig is absent. It runs on the development tier only, and that is
deliberate: what it asserts is that the log rebuilds the assembly, which is a
claim about Demido rather than about a model. The three tier ladder exists for
claims about what a model *does*, and those live in `demido-inference`.

Its assertion is not that two Rust values are equal. It feeds the rebuilt
assembly back through `llama.cpp`'s own chat template and compares the string
against the one the server built from what was sent, character for character. A
rebuild that dropped a paragraph, reordered two blocks or filled a placeholder
differently comes back as a different string from the server rather than as a
passing test.

**The string it is compared against is taken before the turn runs**, from the
value that is then handed to `Backend::generate`. Rendering both sides at the
end, after asserting the two requests equal, is a comparison that cannot fail on
its own: it proves the endpoint is a function and nothing about the log.
