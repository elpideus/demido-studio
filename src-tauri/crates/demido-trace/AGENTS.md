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

## The three fields on every event

`design/windows.md` fixes both, and the reason they are here from the first line
of code rather than added when a screen needs them is that adding either later
means rewriting the slice that wrote the events without them.

**Source.** Exactly the eight `--src-*` tokens in `design/tokens.css`, with the
same names, so the enum and the monitor's ledger cannot come to disagree about
how many there are. `Source::Reasoning` is the model's own output, its reply as
well as its thinking: there is no ninth source because a ninth source is a ninth
colour, and that is a change to a frozen board.

**Agent.** Who produced the line: `main` for the conversation, or the sub-agent
that did. A delegated task is a child `Session` over the **same journal**, with
its own agent, its own depth and its own bookkeeping, so the monitor's agent
scope is a filter over one stream rather than a second stream
([`docs/decisions/0013-a-sub-agent-is-a-scope-on-one-log.md`](../../../docs/decisions/0013-a-sub-agent-is-a-scope-on-one-log.md)).
Three consequences are worth knowing before you write against this crate:

- **A turn number is an agent's own.** A sub-agent runs the agent loop again and
  counts its exchanges from one, so anything read by turn is read through an
  agent. `Replay` is scoped to the conversation and `Replay::scoped_to` is how
  you read one child's.
- **A child writes out every wording its own assemblies name**, including ones
  its parent already wrote. That is what makes a child's assembly rebuild out of
  the child's own events, which an export of one agent depends on and which a
  search across the whole log would pass without.
- **Resolution by position is not scoped** (a position names one event whoever
  wrote it), a search anchored at a position is scoped to **that** event's agent
  (a wording, a document), and an enumeration is scoped to the replay's.

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
| | `Turn::offer` records the tools on offer as `tools/offered`, a name and a hash per tool and the `Layer` that decided the set, only when the set changes. Each tool's document is a `tool/version`, once per session per hash, the rule `prompt/version` keeps ([#52](https://github.com/elpideus/demido-studio/issues/52)). |
| | A tool call is its own events ([#54](https://github.com/elpideus/demido-studio/issues/54)): `tool/call` names the completion that asked for it, `tool/decision` is the person's allow, deny or always, `tool/result` is what came back verbatim with `failed`, and `tool/refusal` is Demido's answer to a call it did not run, recorded by paragraph hash and values like a fragment. `Session::step` sends the same turn again carrying a step's blocks, so a turn that used a tool has one assembly per step. |
| | The shape of each offered tool (its schema with no prose) is on `tools/offered` beside the hash, and an assembly names the offered set it was sent with, so `request.tools` rebuilds too. |
| | `Session::delegate` opens a sub-agent: the `agent/delegated` event and the child's recorder come out together, so there is no child that is not in the log. `Session::folded_in` writes `agent/returned`, and **only** where a background answer is folded in at a step boundary ([#66](https://github.com/elpideus/demido-studio/issues/66)): a delegation that blocked was answered as its call's own result. `Session::framed` is what puts the block beside it, the step-boundary sibling of `Turn::fragment`: a background answer arrives when no `Turn` is open, and the frame has to be an event before `Session::step` can name it. |
| `replay` | `Replay`. History, the rebuild, the per-turn occupancy and the per-source ledger, all over the same events.
| | `Replay::rebuild` is what the monitor draws ([#57](https://github.com/elpideus/demido-studio/issues/57)): the assembly in force at **any** event, for the agent that event belongs to and carrying that agent's name, block by block, diffed against the assembly before it, with each block's own source and weight on it and the offered set in the wording it was sent in. The diff is over what each block put in front of the model rather than over positions on the log, because the composer records a fresh system paragraph every turn and a diff by position would report an injection every single turn, which is an injection signal nobody reads. `Replay::offered` is the set in force at any event, in the wording it was offered in, so an edit made later cannot rewrite the record of an earlier reply. `Replay::request(seq)` rebuilds any one assembly, and `Replay::assembly(turn)` is a turn's last. `Replay::conversation` is what a later turn carries: `history` plus the calls and what came back for them, plus one kind of fragment. A host paragraph is carried only when a **tool** put it there, which today means a background sub-agent's answer folded in at a step boundary ([#66](https://github.com/elpideus/demido-studio/issues/66)): the system prompt is a fragment too and is re-resolved off the ladder at the head of every turn, so carrying it would send it twice and would send yesterday's wording beside today's. The source is what tells the two apart, which is what the source taxonomy is for. |
| | `Replay::agents` is the monitor's left column: the conversation first, then every sub-agent in the order it was opened, with the call that opened it and the depth that is its indent. All of it read off the `agent/delegated` events, so the chain is the log's own record rather than a shape kept beside it. |
| | `Replay::transcript` is what the chat draws ([#55](https://github.com/elpideus/demido-studio/issues/55)): **literally** `history`, with each call put back in at its own position and paired with the decision and the result or refusal that answered it. The messages are `history`'s own answer rather than the same filter written twice, so the two cannot come to disagree about what is a bubble. The pairing is the transcript's alone: on the log a call and its result stay two events, each with its own source and weight, which is what the monitor reads. |

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

**The Session Monitor's UI.** The window is `web/src/monitor/`, and it computes
none of what it draws: the stream is `Chat::log`, the rebuild is
`Replay::rebuild`, and which group a tool name belongs to is the registry's
because only a registry knows that
([#57](https://github.com/elpideus/demido-studio/issues/57)). This crate shipped
its data one slice ahead of that screen on purpose
([#41](https://github.com/elpideus/demido-studio/issues/41)), because history is
derived from the log and so the log could not wait for the window that inspects
it.

**Anything a later slice will record.** Artifacts are a `Body` variant, a source
that already exists, and a rebuild case, added the way tool calls were on
[#54](https://github.com/elpideus/demido-studio/issues/54). Declaring one now
would be declaring a shape nothing can be held to, which is the same rule
`demido-inference` applies to its own `Chunk`. Sub-agent scheduling was that
until [#62](https://github.com/elpideus/demido-studio/issues/62), and it came
first rather than last for the one reason the rest of this list does not apply
to it: the agent is a field on **every** event, so a slice that added it later
would be a slice rewriting the one before it.

**The framed message a folded-in answer becomes.** `agent/returned` names the
child's completion; what the parent's model is then shown is a paragraph filled
with it, which is a fragment like any other and is
[#66](https://github.com/elpideus/demido-studio/issues/66)'s.

## The tests

`tests/rebuilt.rs` is the rebuild the monitor draws, asserted on the log rather
than on a screen: #57 gives it no seam of its own, because it is a projection.
The case worth knowing is
`a_rebuild_carries_the_tool_wording_that_was_actually_sent`, which edits a tool
document after a reply was produced and asserts the earlier assembly still
rebuilds in the wording it was sent in. That is the per-tool hash of
`docs/rules/tools.md` earning its keep, and the failure it prevents is silent:
today's wording rendered against yesterday's answer.

`tests/agents.rs` is the agent field and what it makes possible: the main
session's events named as well as a child's, a delegation carrying the call and
the depth, a child writing its own offered set and rebuilding its assembly out
of its own events, and the scope that keeps a parent's transcript the parent's.

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
