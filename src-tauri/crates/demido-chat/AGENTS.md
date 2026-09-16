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
`Chat::transcript`, which is a projection of the log, so drawing the desk after
a restart is the same read as drawing it the first time. A chat that survives
closing the app does so because it was never anywhere but the log.

It carries the **calls** as well as the messages
([#55](https://github.com/elpideus/demido-studio/issues/55)), because a call and
its result are drawn in the transcript at the point in the turn where they
happened rather than in a window somebody has to know to open. The pairing of a
call with what came back is `demido_trace::Replay::transcript`'s, over the same
events the session monitor reads as two rows: a row on screen answers "what did
this call do", and a reader who has to match two rows by a sequence number is a
reader doing a join by hand.

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
implementation, and it landed on #55 as `src-tauri/src/chat.rs`'s `Approvals`: an
event out, a `chat_decide` command back, a `oneshot` between them. A trait would
buy a second one that exists only in tests (`tiles.md`); `Asking` is the
interface it would have.

Every decision is a `tool/decision` event, so the log says which of allow, deny
and always happened.

***Always for this tool* is a setting, and it is the chat's.** #55 moved it off
the log and onto the ladder's `tools.always`, written with `Scope::chat` and
never at the global tier: a person answering about one call in one conversation
did not consent for every conversation they will ever open. The log still says
what they answered, because that is what happened; what is in force next turn is
resolved with the mode and the step limit, once per message, like every other
value.

**A delegation is asked about once per turn, not once per sub-agent.**
[#61](https://github.com/elpideus/demido-studio/issues/61) and
`docs/rules/tools.md`. An allowed call to a tool
`demido_permission::answers_for_the_turn` names, which is `delegate_task` and
nothing else, pushes that name onto the turn's **own copy** of `always`, so the
second delegation of the turn runs and the first of the next message asks again.
It is never written to the ladder, which is the whole difference from *always
for this tool*: one is an answer read twice in one turn, the other is a standing
grant a person made deliberately. A denial pushes nothing, so a person who said
no is asked about the next one. `tests/a_tool.rs` has all three.

The **floor is held here rather than in the window**: an *always* about a
destructive call runs that call and is not remembered, whatever a frontend sent.
`docs/rules/tools.md` says such a call asks every time and that *always* cannot
waive it, and a floor that only held while the window agreed with it is a floor
a second window steps through.

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

## A delegation is a child session

[#63](https://github.com/elpideus/demido-studio/issues/63). **A delegated task
runs the whole agent loop again**, in a child session rather than as a second
kind of step interleaved into the first one's. `Agent` is that run: the
conversation's own, or one sub-agent's. It borrows the backend, the ladder, the
register and the log, because a sub-agent with a supervisor of its own would be
a second model resident on a card the whole design is sized against, and one
with a log of its own would be the second store
[`0013`](../../../docs/decisions/0013-a-sub-agent-is-a-scope-on-one-log.md)
refuses.

**The child's transcript is durable, and its context is clean.** It composes its
own assembly out of the system prompt and the task and nothing else
(`Carrying::Nothing`), and every event of its run is on the conversation's log
under its own agent. Clean is not the same as hidden: what the conversation does
not carry is the child's messages into its next request, and that is the whole
difference. It is also what makes the monitor's scope possible at all.

**The tool is a pair, and the turn is what carries a task out.** `delegate_task`
holds a `Delegating` (#61), and a registry entry outlives every turn it is
offered in, so it cannot hold the turn's sink, the turn's person or the turn's
cancellation. `src/delegation.rs` makes both ends together: the tool asks on one,
the loop answers on the other, beside the call it is running. The child therefore
runs on the stack of the turn that asked for it, with everything that turn has,
and *"at the default parallelism the call blocks and the answer is the tool's
result"* is the only shape that pair has rather than a rule anybody keeps. Both
ends are made in one line of the composition root and split between the registry
and the chat in the two that follow, because a tool wired to one conversation's
loop and registered on another's is a delegation that answers in the wrong
session.

**The inheritance rule is called on the way into every child, at every depth.**
`demido_permission::inherit`, over the parent's `Resolution` and a `Request`, and
nothing else below the root mints one. `Rules` holds that resolution rather than
a `Mode`, so the matrix a child's call goes through is the matrix its parent's
went through, with the child's mode in it.

**A tool failure inside a child is a result, not an error.** The child answers
its own calls the way this agent does, and a turn that ended badly comes back to
the parent as a failed tool result it can act on. Only a journal error stops a
run, at any depth: a child whose events cannot be written is a child nothing can
say happened.

**The parent's Stop is the child's, at every depth.** One token, shared down the
chain, so the stop reaches whichever agent is talking to the model and ends its
generation with the `Done` every ending takes. Dropping the child instead would
be a second path to a stop, and this crate does not have one. A cancel that left
a sub-agent generating against a model nobody is waiting for is the next
question's VRAM, which is why the cancel ships here rather than in a ticket of
its own.

The **depth** is a constant in `src/chat.rs` until
[#64](https://github.com/elpideus/demido-studio/issues/64) puts it on the ladder;
what is here is that `inherit` is the only thing that decrements it and that a
chain ends when it runs out.

## What the monitor reads

[#57](https://github.com/elpideus/demido-studio/issues/57). Two questions, and
neither of them assembles anything: `Chat::log` is the events, whole and in
order, and `Chat::assembly` is `Replay::rebuild` with one thing added that the
log cannot answer on its own.

**A tool group missing from an assembly says whether it was switched off or
dropped.** The log records tool *names*; only a registry knows which group a
name is in, which is why `src/monitor.rs` is here and not in `demido-trace`. The
rule is the layer on `tools/offered`: a set a tier of the ladder named is a
person's choice, so an absence in it is **switched off** and the control that
made it is the tool picker; a set nobody named is everything the registry had,
so an absence is **dropped**, which today means no workspace. `docs/rules/tools.md`
is explicit that this is what the event exists for.

One answer per assembly rather than one per group, and that is the registry's
property rather than a shortcut: it drops all of itself or none of it, because
what it drops for is having nowhere to act. A set with anything in it therefore
proves the registry was not the reason, and every absence in it is the ladder's.

**An empty set claims neither.** Both roads end there: a person can switch every
group off, and a registry with no workspace offers nothing whatever the picker
says. The layer says who chose the set, never why it came out empty, so the
standing is `Nothing` and the monitor says the one thing that is certain instead
of sending somebody to a control that may not be the one that is wrong.
`tests/monitored.rs` holds all of it apart against the scripted backend,
including the crossed case: a set named by the chat, on a conversation with no
workspace.

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
| `tests/a_tool.rs` | The loop with tools in it: dispatch, the matrix, the approval, the step limit, what a stop leaves, what the transcript draws for a call, and which tier an *always* is written to. Against the same scripted backend. |
| `tests/offered.rs` | What reaches the payload: the offered set and the mode off the ladder, a switched-off tool absent and refused as off, and one chat's set reaching no other. |
| `tests/monitored.rs` | What the session monitor reads: the assembly at an event, and a group switched off in the picker told apart from one nothing ever offered. |
| `tests/delegated.rs` | The child session: the store it shares, the clean context and the durable record, the call that blocks, the ceiling at every depth, a failed call inside a child, and a Stop asserted at depth 2. |

The scripted backend is `demido_inference::scripted`, which passes the
`Backend` contract suite, rather than a fake written here: a loop proved against
a fake that keeps its own promises is proved against the wrong ones.
| `tests/a_real_model.rs` | The model gate of `docs/rules/done.md`: a real model, on all three tiers. |

The live one is `#[ignore]`d and runs one at a time under the rig's process-wide
permit. See the live commands in the root `AGENTS.md`.
