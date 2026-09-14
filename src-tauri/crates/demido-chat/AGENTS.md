# demido-chat

The turn loop: what is said, what is sent, and what comes back. It sits between
`demido-trace`, which is the source of truth, and `demido-inference`, which is
the seam a request goes through, and it owns the one thing neither of them can:
the order those two happen in.

Brief B06: "everything about what happens, from the input getting sent to the
model using the tools available up until the reply (and after, but we'll get to
this later), should be visible."

## The one thing to understand

**`Chat` holds no messages.** It holds a journal, a supervisor and the
cancellation token of whatever is generating right now. A transcript is
`Chat::history`, which is a projection of the log, so drawing the desk after a
restart is the same read as drawing it the first time. A chat that survives
closing the app does so because it was never anywhere but the log.

That absence is the point rather than an omission. v2 kept a message list beside
its log, and two stores over one conversation is two stores that can eventually
disagree about it.

## No trait, on purpose

`docs/rules/tiles.md` makes a trait a decision rather than a default, and this
crate needs none. It is generic over the two traits that already exist,
`Backend` and `Journal`, and there is no second implementation of a turn loop to
swap for: an agent mode narrows what a turn may offer a model, which is a value
passed in rather than a different loop.

Both generic parameters earn themselves twice. `Journal` because a session the
user asks not to keep is `demido_trace::Memory` and nothing else changes.
`Backend` because every rule this crate has is about *ordering*, and none of
them can be observed through a real `llama.cpp` without a card and several
gigabytes. `tests/a_turn.rs` is what that buys.

## The invariants

**Recorded, then sent, then recorded against.** `Turn::send` hands back the
request that recording produced, so a caller cannot send an assembly it did not
record. A crash between the two leaves a log that says what was about to happen
rather than a completion with nothing that produced it.

**A stop takes the same path as a completion.** Cancelling ends the stream with
one `Chunk::Done` carrying `FinishReason::Cancelled`, so the partial answer and
the stop are written by the line that writes a finished answer. There is no
second path, and therefore no second path to forget.

**History is carried by position, never by copy.** A turn carries earlier blocks
as sequence numbers (`Turn::carry`), so the log does not grow as the square of
the session and what reaches the model is what the log names.

**The log is opened by the first thing that needs it, not by the root.**
`Chat::new` takes an opener and does not call it. The composition root runs
before the window exists, so a root that opened a session log would make opening
a window a thing that can fail on a full or read-only disk, with nothing on
screen to report it on. Deferring moves that failure onto a desk that is already
drawn, where it is a sentence.

"Needs it" and not "writes to it": the desk asks for its transcript on mount, so
a profile that opens the app and says nothing still gets an empty log file. The
invariant that carries weight is the one `src-tauri/src/wiring.rs` has a test
for, that assembling the root touches no disk at all.

**Every value a turn carries comes off the ladder.** The temperature, the
system prompt and the context length are resolved from `demido-settings` per
load and per turn, through `Ladder::for_chat` built once from this chat's own
id. There is no `Options::default` here and no field holding a temperature: a
second place a value can come from is a settings page that says one thing while
the request says another. A value changed while the window is open therefore
takes effect on the next turn, which is what
`tests/a_turn.rs::the_temperature_sent_is_the_one_the_ladder_resolved` asserts.

The system prompt goes in first, before anything anybody said, and it is
recorded as `Source::Inject`: the taxonomy is about who put the text in the
window, Demido put it there without being asked this turn, and `Source::System`
is text Demido *wrote*. An empty one is left out of the assembly entirely rather
than sent as a blank message.

The context length is the one that reaches a **process** rather than a request,
through `Backend::with_context_length` in `Chat::load`. So changing it and
loading again really is a restart: `Supervisor::ensure` sees a different
configuration. Until the window asks for that load, the running server still has
the window it was started with, which is why `Setting::reloads` exists and why
the frontend calls `chat_load` after a change that carries it.

**The supervisor is shared, not owned.** `Chat::new` takes an
`Arc<Supervisor<B>>`. The rule the supervisor enforces is that one model is
resident on the card, and a chat that made its own would make that one model per
conversation, on a card the whole design is sized against.

**A failure is reported, never fatal.** A model that will not start is a
`Presence::Failed` carrying the backend's own sentence, and the desk carries on
around it. A backend that died between two turns is found when the turn is
attempted, reported, and startable again.

## A turn with tools in it

[#54](https://github.com/elpideus/demido-studio/issues/54). One loop, always,
per [`docs/rules/tools.md`](../../../docs/rules/tools.md): a turn that offers
nothing is the same loop with nothing to do but answer.

**Generate, answer every call, send again.** `Chat::ask` records the assembly
with the `Toolbox`'s offered set in it, streams one generation, records the
completion and then each call, and answers each call in order. What came back
goes on the end of the same turn's assembly through `Session::step`, which is
recorded before it is sent, exactly as the first one was.

**Each call is answered, always, and in one of two ways.** A `tool/result` for
a call that was attempted, `failed` taken from the tool's own outcome. A
`tool/refusal` for one that was not: declined, stopped, or past the step limit,
in a paragraph's wording (hard rule 10). Every call needs one, because a next
request carrying an answer's calls without what came back for them is one a
server refuses.

**The order inside a call is fixed.** The registry plans it, and a call it
cannot understand is a failed result. A call identical, by name and parsed
arguments, to one the person declined this turn is refused without asking
again. Then the matrix rules, and the person is asked only when it says `Ask`.

**The approval is a callback, not a trait.** `ask` takes
`FnMut(Asking) -> impl Future<Output = Decision>`. The window is the only real
implementation (#55), and a trait would buy a second one that exists only in
tests (`tiles.md`); `Asking` is the interface it would have. Every decision is
a `tool/decision` event, so the log says which of allow, deny and always
happened. *Always* is read back off the log at the start of each turn, so it
holds on later messages; #55 moves where it is written to the ladder's chat
tier.

**The mode is the matrix's, and the step limit is the ladder's.** Both are
resolved off the ladder once per message, `tools.mode` and `tools.step_limit`,
like the temperature, so a mode changed between two messages rules the second.
The mode's stored name becomes a `Mode` handed to `demido_permission::verdict`
per call, and nothing else here reads it. `tests/a_tool.rs` runs the same
runaway script under all three modes and gets the same count.

**Offered is a set on the ladder, and absent means absent.**
[#56](https://github.com/elpideus/demido-studio/issues/56). `tools.offered` is
resolved per message and the registry is narrowed to it before anything else
happens, so the log's `tools/offered`, the request's `tools` and the registry a
call is planned against are one list. The event carries the layer that decided
it: `registry` when nobody named a set, otherwise the tier that did. A call
naming a registered tool that is not in the set is refused with `tools.off`
before planning, so the model is told the user turned it off rather than that
the name is not a tool. `tests/offered.rs` asserts all of it against what the
scripted backend received.

**A stop reaches whatever the turn is doing.** One cancellation token for the
whole turn. Mid generation, the backend ends the stream with what it had and
hands on no half-built call; a call that had already arrived is answered as
stopped. Waiting on the person or running a tool, the wait is dropped, which
for `run_command` kills the process tree, and that call and every one after it
are answered as stopped. The answer comes back `Cancelled` either way, and the
next message can be sent.

**The step limit ends the turn as a failure.** The calls past it are refused,
a `turn/failure` with kind `step-limit` is written, and `ask` returns
`Error::StepLimit`.

## Presence carries the fact, never the wording

`Presence` says whether there is anything to talk to. The sentence the composer
shows is the window's (`design/shell.md` writes them), and the only string this
crate passes up is `Presence::Failed`'s detail, which is what `llama.cpp` said
before it died. That string is the difference between "inference did not start"
and a fix, and it is not something a frontend can derive from a tag.

## The two suites

| Suite | What it proves |
|---|---|
| `tests/a_turn.rs` | The ordering rules, against a scripted backend. |
| `tests/a_tool.rs` | The loop with tools in it: dispatch, the matrix, the approval, the step limit, and what a stop leaves. Against the same scripted backend. |
| `tests/offered.rs` | What reaches the payload: the offered set and the mode off the ladder, a switched-off tool absent and refused as off, and one chat's set reaching no other. |

The scripted backend is `demido_inference::scripted`, which passes the
`Backend` contract suite, rather than a fake written here: a loop proved against
a fake that keeps its own promises is proved against the wrong ones.
| `tests/a_real_model.rs` | The model gate of `docs/rules/done.md`: a real model, on all three tiers. |

The live one is `#[ignore]`d and runs one at a time under the rig's process-wide
permit. See the live commands in the root `AGENTS.md`.
