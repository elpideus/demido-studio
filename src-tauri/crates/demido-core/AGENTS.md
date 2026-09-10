# demido-core

Shared types and the error shape.

This crate is the bottom of the dependency graph. Every other crate in the
workspace may depend on it; it depends on none of them, and it knows nothing
about Tauri, about the frontend, or about any subsystem. If something here needs
to reach upwards, it does not belong here.

## What is in it

| Module | What |
|---|---|
| `error` | `Error` and `Result`: one error type, serialised as `{ kind, message }` so the window branches on a tag rather than parsing a sentence. |

`VERSION` is read from the crate manifest rather than written out again, because
[`docs/rules/versioning.md`](../../../docs/rules/versioning.md) requires the tag,
`tauri.conf.json` and the Cargo manifests to agree and a fourth copy is a fourth
thing to forget.

## The composition root is not here

`Wiring` lives in `src-tauri/src/wiring.rs`, in the application package. It
names one implementation per trait
([`docs/rules/tiles.md`](../../../docs/rules/tiles.md)), so it depends on every
crate that has one, and depending upwards is the one thing this crate does not
do. It started here on #38, when there were no traits to name and the direction
of the dependency was not yet visible.

## What does not belong here

- Anything that talks to the filesystem, the network or a process. Those are
  subsystems, and a subsystem is a trait in its own crate.
- A type only one crate uses. Shared means shared; a type with one caller lives
  with its caller.
- Tauri commands, and the composition root. Both live in the application
  package (`src-tauri/src/`), which is the only place allowed to know Tauri
  exists and the only place that may depend on every crate at once.
