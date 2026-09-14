# demido-shell

The remembered shell layout: the `Store` trait, its contract suite, the two
stores that implement it, and the debouncer that decides when a gesture becomes
a file.

Brief B42: "a VSCode-like Icons-only sidebar"

## The one thing to understand

**A layout is not a setting, and a layout that will not load is not an error.**

Both halves are load-bearing, and both are recorded in
[`docs/decisions/0010-the-desk-remembers-itself.md`](../../../docs/decisions/0010-the-desk-remembers-itself.md).

A setting is typed by a person and belongs on the ladder from
[#8](https://github.com/elpideus/demido-studio/issues/8). A layout is the
residue of gestures nobody typed, and putting it in the settings schema gives
every drag of a seam a settings row, a settings default and a settings history
for something that already has a window to be seen in.

And a desk arrangement is the one piece of state whose failure has no useful
report. Nobody can act on "your window arrangement was corrupt", remaking it
costs one gesture, and startup never blocks
([`AGENTS.md`](../../../AGENTS.md)). So `Store::read` returns an `Option` and
never a `Result`: the discard rule is in the type, not in a comment asking every
caller to keep it.

## The trait and its contract suite

`Store` (`src/store.rs`) has two methods: read the arrangement, replace it.
There is one layout per profile, so a write is a replacement rather than an
append.

**`src/contract.rs` is the trait's second file**, per
[`docs/rules/tiles.md`](../../../docs/rules/tiles.md). Call
`contract::assert_store(|name| ...)` from your implementation's own test file.
It takes a factory rather than a store because the promise that matters most
cannot be tested on one handle: an arrangement has to come back after the
process that arranged it is gone.

| Implementation | Where | Runs the suite in |
|---|---|---|
| `Files` | `src/file.rs` | `tests/store_contract.rs` |
| `Memory` | `src/memory.rs` | `tests/store_contract.rs` |

`Files` is what the app writes. `Memory` is what every test about the debouncer
rather than about a file uses.

## The generation number

`shell.json` carries the generation that wrote it. A file whose generation is
not `layout::GENERATION` is dropped and the default desk is drawn. There is no
migration path, and that is the point: growing the layout is a bump, and it
costs each user one rearrangement rather than costing this crate a migration to
write, debug and keep.

The number is a fact about the **file** and the window is never told it. A
frontend that could read it would eventually branch on it, and then the discard
rule would have a second implementation living in TypeScript.

## The debouncer

`Debounced` (`src/debounce.rs`) coalesces writes: the window reports what the
desk looks like as often as it likes, and the file is written once
`SETTLES_AFTER` the last report. Dropping the handle flushes whatever is
pending, so quitting inside the quiet window saves the arrangement rather than
discarding it. That is the case a naive debouncer gets wrong and the one a user
actually hits: move the rail, then close the window.

Reads pass through it, so the desk reads back what it last said even before that
has reached the disk.

**It is deliberately not a `Store`**, and so the contract suite is deliberately
never run against it. A `Store` promises that what was written comes back from
storage; a debouncer's whole job is that a write has *not* happened yet, so a
`Debounced` that implemented the trait would pass the suite only by flushing on
every call, which is the thing it exists to avoid. It is a policy wrapped around
a store rather than a second kind of one, which is why `Wiring`'s `Desk` alias
names `Files` as the tile and `Debounced` as what surrounds it.

## Per profile is the operating system's doing

[`docs/rules/profiles.md`](../../../docs/rules/profiles.md) rules that a Demido
profile is a Windows profile. The directory handed to `Files::in_profile` is
already inside `%LOCALAPPDATA%` and already carries the ACL that keeps a second
Windows user out, so this crate adds no boundary of its own. It does not resolve
that directory either: the application package does, from Tauri's path
resolver, because knowing where a profile lives is the ceiling's job.

## What is deliberately not here

**Panels.** Floating and pinned geometry, the seam positions and the previous
geometry an unpinned panel returns to are all layout, and all of them belong in
`Shell` when panels exist. They are not declared now, because a shape nothing
can be held to is the same mistake `demido-trace` refuses for its own `Body`
variants. `GENERATION` is what makes adding them cheap.

**The rail's order.** Editing the navbar is its own gesture and its own ticket.
The rail's *edge* is here because it is the one thing this slice can arrange.

**Anything the window draws.** This crate has no opinion about what a rail looks
like. `design/shell.md` and `design/system.md` own that, and `web/src/shell/`
implements it.

## The tests

`cargo test -p demido-shell` runs everything. There is no live suite: nothing in
here is a claim about what a model does.
