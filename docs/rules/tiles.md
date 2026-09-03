# A tile is replaced without touching the ceiling

Decided on wayfinder ticket [#10](https://github.com/elpideus/demido-studio/issues/10),
carrying v2's decision 0003 forward unchanged. `docs/stack.md` says what the
stack is; this says how a piece of it comes out.

The brief's central architectural demand:

> I want a codebase that feels like a server rack, a NAS or a drop-ceiling...
> you remove a broken tile, replace it with a new one...the ceiling didn't
> change, only the tile did.

The failure mode is answering that with one universal plugin abstraction, which
produces a plugin system for your own kernel: the opposite of simple. So there
are **two mechanisms at two layers**, and which one a thing belongs to is decided
by who ships it.

| | Compile-time tile | Runtime tile |
|---|---|---|
| Shipped by | Us | Anyone |
| Is | A Rust trait with implementations | A manifest folder |
| Swapped by | Changing one wiring line and recompiling | Adding or removing the folder |
| Runs | In process | Out of process |
| Proved by | A contract test suite | The host API version it declares |

## Compile-time tiles

A first-party subsystem is a **trait**, and its implementations are
interchangeable. Swapping one is a single wiring line at the composition root.

**The contract test suite is the trait's second file, and it is written before
the second implementation exists.** This is the rule that makes "swappable" true
rather than aspirational: a trait without a contract suite is a trait whose
implementations will quietly disagree, and the disagreement will be found by a
model at runtime rather than by CI.

Implementable shape, for a session starting one:

1. The trait lives in its own crate, or in the crate that owns the concept.
2. Beside it, a `contract` module exposing one function that takes any
   implementation and exercises the whole trait: every method, the error cases,
   and the ordering guarantees the trait claims.
3. Each implementation's test file calls that function with itself. An
   implementation that does not call it is not an implementation.
4. The composition root names exactly one implementation per trait, in one place.

Adding a first-party subsystem is a recompile, and that is fine. Adding a *skill*
is not, and skills are what users actually add.

## Runtime tiles

A runtime tile is a **folder with a manifest**, scanned at boot. Skills are the
reference shape; MCP servers, search engines and output filters use the same
frame.

Every manifest declares a **`host_api` semver**. The host refuses a tile whose
major does not match, and says so in a way the user can read, because a tile that
silently does not load is worse than one that visibly will not.

Anything executable runs **out of process**. Third-party code does not share the
address space of the inference supervisor, and a tile that crashes takes nothing
with it.

How a skill's tools are actually launched, supervised and killed is ticket
[#14](https://github.com/elpideus/demido-studio/issues/14), not this file. What
this file fixes is the frame those answers have to fit.

### A runtime tile does not contribute UI

**In v3 a tile contributes tools, slash commands, prompts and MCP entries. It
does not contribute a panel, a window, or any component.**

Three reasons, and only the first is about security:

1. A third-party panel could hide the approval prompt, the occupancy bar or the
   browser's "who is driving" line. That is a hole straight through this
   application's one promise, that everything the model sees is inspectable.
2. It cannot be held to the theme contract in `design/system.md`, that a theme
   repaints and never relayouts. One screenshot staying valid across every theme
   is a guarantee a foreign component breaks.
3. It is the largest surface a third party could take, in a product whose whole
   third-party story is that a skill brings its own tools so the user does not
   have to.

**When a tile needs to show something, it emits an artifact.** The artifact
system is the sanctioned render path, which has the useful side effect of giving
it a real caller: v2 built it across six types and no live model ever reached for
one.

**This is a "not yet", not a "never".** Stefan decided on 2026-09-03 that skill
UI arrives in a later release, together with a provenance ledger showing who
added, removed or changed what. Two consequences bind now:

- The manifest format must be able to grow a UI section **without a `host_api`
  major bump**, so reserve the key and refuse it with a clear message rather than
  leaving it undefined.
- Whatever renders skill UI later must be able to attribute every element on
  screen to the tile that put it there. A design that cannot answer "who drew
  this" is not a candidate.

## Where the line falls

A thing is a compile-time tile if it is ours and swapping it is a decision. It is
a runtime tile if somebody else can ship it. Two mechanisms means two ways in, and
the wrong instinct is to unify them: paying IPC on every call into our own
inference supervisor is absurd, and letting third-party code into the process is
the crash isolation and the security boundary given away at once.
