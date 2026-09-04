# Every crate documents itself

**Enforced now.** `scripts/check-rules.mjs`, rule `crate-docs`. A no-op until
there are crates.

Decided on wayfinder ticket
[#16](https://github.com/elpideus/demido-studio/issues/16). This is hard rule 9
of [`AGENTS.md`](../../AGENTS.md).

## The rule

Every directory under `src-tauri/crates/` contains an `AGENTS.md`.

## Why

A session that opens one crate should learn what that crate is from inside it,
rather than by reading the tree or the root contract. The codebase is meant to
feel like a server rack: a unit you can pull out on its own is a unit that
explains itself on its own.

The check exists before the first crate does, so that it is already true when
they arrive rather than retrofitted across two dozen directories later. The
scaffold lands on
[#10](https://github.com/elpideus/demido-studio/issues/10).

## What a crate's AGENTS.md holds

What the crate is for, the trait it implements and where that trait's contract
suite lives ([`tiles.md`](tiles.md)), and anything a caller would otherwise have
to read the source to discover. Not a changelog, and not a copy of the root
contract.
