# demido-permission

Whether a call runs without asking: the permission matrix, and nothing else.

Brief B57: "There should be 3 agent modes: Cautious, Balanced, Autonomous."

## The one thing to understand

**`verdict` is the whole crate.** An `Intent`, a `Mode` and the tools the person
said *always* about go in; `Allow` or `Ask` comes out. Nothing is read from
disk, nobody is asked, nothing is logged. The three modes are three rows in
`ROWS`, so adding a mode is adding a row, and the table is the one
[`docs/rules/tools.md`](../../../docs/rules/tools.md) draws.

Three rules hold it up, each asserted in `tests/the_matrix.rs`:

- **A destructive call asks**, in every mode including Autonomous, whatever
  ability it declares. It is checked before `always` and before any row, so
  neither can waive it. What is destructive is the tool's declaration
  (`Intent::destructive`), never a list kept here.
- **An unknown mode name is Cautious**, matched exactly, so a capitalised or
  padded spelling is unknown too. The first row is the default and the
  fallback, which is why it has to stay the strictest.
- **`always` covers the tool it names**, and only its ordinary calls.

## Why a crate and not `demido-tools::permission`

v2 kept this in `demido-tools`. v3's `demido-tools` says, and enforces through
its dependency list, that it knows nothing about a mode. Putting the matrix
there would have made that sentence false in the one file it was about, so the
matrix depends on the tools and never the other way round.

## Nothing but the matrix reads the mode

`Mode` is opaque: no accessor, no `PartialEq`, no `Debug`, no variant to match.
The only thing a caller can do with one is pass it to `verdict`. The three
`compile_fail` doctests in `src/lib.rs` are the assertion, so the step limit
([#54](https://github.com/elpideus/demido-studio/issues/54)), parallelism and
delegation depth ([#64](https://github.com/elpideus/demido-studio/issues/64),
[#65](https://github.com/elpideus/demido-studio/issues/65)) have nothing to
branch on. The one route left is probing `verdict` with a made-up intent to
work out which row is in force, which is reading the matrix and is refused in
review. Do not add a derive or an accessor to make a test or a screen easier:
the mode control holds the stored *name*, from `Mode::names`, never a `Mode`.

## The mode is never prose

Asserted twice. `no_prompt_demido_ships_names_a_mode` holds every paragraph in
the catalog and every tool document to naming no mode, and
`demido-chat`'s `nothing_sent_to_the_model_names_a_mode` holds the assembled
payload to the same, at the backend seam.

## What is not here

- **Asking.** The approval enters the turn loop as a callback on
  [#54](https://github.com/elpideus/demido-studio/issues/54), not as a trait:
  the window is the only real implementation.
- **Where the mode and `always` are stored.** Both resolve through the settings
  ladder on [#56](https://github.com/elpideus/demido-studio/issues/56), and
  `always` writes to the chat tier only.
- **v2's `Matrix` with public fields, and its `Rule`.** The fields let a caller
  build a row nobody declared, which is a fourth mode by the back door, and a
  row holds `Verdict`s directly, so a second two-variant enum bought nothing.
