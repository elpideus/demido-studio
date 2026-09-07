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
either.

| Implementation | Where | Runs the suite in |
|---|---|---|
| `LlamaCpp` | `src/llamacpp.rs` | `tests/llamacpp_contract.rs` |

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

## What S1 leaves out, and why it is additive

No tools, no grammar, no `Capabilities`. S1 has no tools
([`docs/rules/done.md`](../../../docs/rules/done.md)), so the shapes they need
are not declared here yet: each is a `Chunk` variant, a `Request` field, and a
contract case saying what a backend must do with it. Declaring them now would be
declaring a contract nothing can be held to.

## The tests

`cargo test -p demido-inference` runs everything that needs no card: the wire
shape, the command line, the startup failures, and the supervisor's ordering
rules against a fake.

The two suites that need a real model are `#[ignore]`d and run by the live
command in [`AGENTS.md`](../../../AGENTS.md). They **fail rather than skip**
when the rig is absent, because a live suite that quietly passes on a machine
with no models is the "built but never driven" failure this project was restarted
to avoid. `tests/rig.rs` says which environment variables point at the rig.
