# demido-inference

The inference seam: the `Backend` trait, its contract suite, and the
`llama.cpp` server Demido starts and owns.

## The trait and its contract suite

`Backend` (`src/backend.rs`) covers the whole life of a backend: start against a
model and a set of parameters, report readiness, say what is loaded and how much
context the slot got, stream a completion, cancel one in flight, and stop.

**`src/contract.rs` is the trait's second file**, per
[`docs/rules/tiles.md`](../../../docs/rules/tiles.md). It was written before the
second implementation existed, which is the whole reason "swappable" is a claim
worth making. Call `contract::run::<YourBackend>(config, model)` from your
implementation's own test file. An implementation that does not call it is not
an implementation.

It takes a `Config` rather than a started backend, because starting and stopping
are half of what the trait promises and a suite handed a live one could not test
either. The suite applies its own context length through
`Backend::with_context_length`, which is the writer paired with
`Backend::context_length`: the settings ladder resolves a number, the writer is
how that number reaches a process whose `Config` the ladder cannot know the
shape of, and the reader is how the case checks it arrived.

A slot now has a word on the trait too. `Backend::with_slots` and
`Backend::slots` are the same writer-and-reader pair one layer along: a
sub-agent runs the conversation's own weights on a second slot, so the slot
count is a VRAM decision (`demido_vram::admit`) rather than a preference, and
the reader is what makes **the number of slots shown to the user the number
actually opened** ([#65](https://github.com/elpideus/demido-studio/issues/65)).
The suite runs every case at two slots for the reason it asks for 3072 rather
than 4096: one is the number a backend that ignored the writer would report
anyway, and at two the context case measures something. `LlamaCpp` reads both
off `/props`, which is the server's own account.

What the contract still cannot ask is what a second slot **costs**: that is a
number on a card, not a promise of the trait. The measurement lives in
[`docs/rules/done.md`](../../../docs/rules/done.md), which is also where
`--ctx-size` being the whole KV pool was measured: a build that passed the
user's number through raw would hand back a fraction of it and every one-slot
test would still be green.

What the trait does say is **which file a configuration loads**,
`Backend::model_file`, because the cost is read from that file's header
(`demido_models::slot`, [#105](https://github.com/elpideus/demido-studio/issues/105)).
It defaults to `None`, a backend with no file to price a slot from, whose slots
above the first are then unmeasured; `LlamaCpp` names `--model`.

| Implementation | Where | Runs the suite in |
|---|---|---|
| `LlamaCpp` | `src/llamacpp.rs` | `tests/llamacpp_contract.rs` |
| `Scripted` | `src/scripted.rs` | `tests/scripted_contract.rs` |

`Scripted` is a backend that is entirely bookkeeping: each generation replies
with the next scripted steps (text, reasoning, a whole call), and the last reply
repeats. It is what drives the turn loop with no card in the machine
([#54](https://github.com/elpideus/demido-studio/issues/54)), and it is held to
the contract rather than trusted, so a loop proved against it is proved against
the promises `llama.cpp` keeps. It ships in the library, like the contract, for
the tests of the crates above this one.

A reply can also be **addressed** rather than positional, with `Script::when`
([#66](https://github.com/elpideus/demido-studio/issues/66)). The key is the
first user message of the request, which for a sub-agent is the task it was
handed and never changes as its turn takes steps. That is what makes a
concurrent run assertable: above the default parallelism a sub-agent generates
beside the turn that asked for it, so the order two of them reach a positional
script is the scheduler's rather than the test's. Addressing a reply to who is
asking is the alternative to holding a clock still, and it is why nothing above
this crate has to.

The second implementation the suite was written for is an OpenAI-compatible
endpoint. It does not ship in S1. For such a backend `start` connects and `stop`
is a no-op, which the contract already allows: it never asks that stopping kill
anything, only that a stopped backend stop claiming to be ready.

## The supervisor

`Supervisor<B>` (`src/supervisor.rs`) owns the lifetime, so that chat reuses the
server that answered the last message instead of loading the weights again.
Three rules: at most one resident, the old one stops before the new one starts,
and a dead backend is not a running one. It is generic because that is the only
way those rules can be tested at all: every one of them is about *when* a
process is started, and none is observable through a real `llama.cpp` without a
card and several gigabytes.

## Tool calls, added the way this file said they would be

S1 had no tools and declared none of their shapes. S2 added them additively
([#54](https://github.com/elpideus/demido-studio/issues/54)):
`Request::tools`, `Role::Tool` with `Message::answers` naming the call a
result is for, `Message::calls` on an assistant message, `Chunk::Call`, and
`FinishReason::ToolCalls`. The contract case that goes with them,
`a_call_arrives_whole_and_before_done`, sends a request carrying an answered
call and holds whatever comes back to three things a loop depends on: every
call has an id and a name, every call comes before `Done`, and `Done` says
`ToolCalls` exactly when there were calls. It asserts nothing about whether a
model chooses to call.

`llama.cpp` streams a call's arguments a few characters per frame. The decoder
assembles each call and hands it on whole once the stream ends, and a cancelled
stream hands on none: half an argument list is not a call.

Still not here: grammars and `Capabilities`, for the same reason tools were not
in S1.

## The tests

`cargo test -p demido-inference` runs everything that needs no card: the wire
shape, a call decoded from streamed frames, the command line, the startup
failures, the supervisor's ordering rules against a fake, and the whole contract
suite against `Scripted`.

The two suites that need a real model are `#[ignore]`d and run by the live
command in [`AGENTS.md`](../../../AGENTS.md). They **fail rather than skip**
when the rig is absent, because a live suite that quietly passes on a machine
with no models is the "built but never driven" failure this project was restarted
to avoid. `tests/rig.rs` says which environment variables point at the rig.
