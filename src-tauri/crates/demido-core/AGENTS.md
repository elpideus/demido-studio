# demido-core

Shared types, the error shape, and the composition root.

This crate is the bottom of the dependency graph. Every other crate in the
workspace may depend on it; it depends on none of them, and it knows nothing
about Tauri, about the frontend, or about any subsystem. If something here needs
to reach upwards, it does not belong here.

## What is in it

| Module | What |
|---|---|
| `error` | `Error` and `Result`: one error type, serialised as `{ kind, message }` so the window branches on a tag rather than parsing a sentence. |
| `wiring` | `Wiring`, the composition root. One implementation per trait, named in one place, per [`docs/rules/tiles.md`](../../../docs/rules/tiles.md). |

`VERSION` is read from the crate manifest rather than written out again, because
[`docs/rules/versioning.md`](../../../docs/rules/versioning.md) requires the tag,
`tauri.conf.json` and the Cargo manifests to agree and a fourth copy is a fourth
thing to forget.

## Traits and contract suites

None yet. `Wiring` is empty and says so: the first trait, `Backend`, arrives with
the next ticket of S1. When it does, its contract suite lands beside it in its
own crate, not here, and `Wiring` gains the one line that names its
implementation.

## What does not belong here

- Anything that talks to the filesystem, the network or a process. Those are
  subsystems, and a subsystem is a trait in its own crate.
- A type only one crate uses. Shared means shared; a type with one caller lives
  with its caller.
- Tauri commands. They live in the application package (`src-tauri/src/`), which
  is the only place allowed to know Tauri exists.
