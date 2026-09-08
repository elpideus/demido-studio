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

**The supervisor is shared, not owned.** `Chat::new` takes an
`Arc<Supervisor<B>>`. The rule the supervisor enforces is that one model is
resident on the card, and a chat that made its own would make that one model per
conversation, on a card the whole design is sized against.

**A failure is reported, never fatal.** A model that will not start is a
`Presence::Failed` carrying the backend's own sentence, and the desk carries on
around it. A backend that died between two turns is found when the turn is
attempted, reported, and startable again.

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
| `tests/a_real_model.rs` | The model gate of `docs/rules/done.md`: a real model, on all three tiers. |

The live one is `#[ignore]`d and runs one at a time under the rig's process-wide
permit. See the live commands in the root `AGENTS.md`.
