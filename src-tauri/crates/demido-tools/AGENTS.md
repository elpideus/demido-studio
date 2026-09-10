# demido-tools

What a model can do, and where it may do it: the tool trait, the registry, the
workspace every path goes through, and the Files group.

Brief B15: "Tool calling and custom tools like run_command, read_file, write_file, delete_file, list_dir, etc."

## The two things to understand

**A tool decides nothing about permission and nothing about rendering.** Its
whole surface is `name`, `parameters`, `intent` and `run`. The capability matrix
([#53](https://github.com/elpideus/demido-studio/issues/53)) reads `Intent` and
decides; the transcript
([#55](https://github.com/elpideus/demido-studio/issues/55)) reads the same
declarations and draws; neither asks the tool, and a tool has no way to ask
either of them. The enforceable half of that sentence is the dependency list in
`Cargo.toml`: this crate knows nothing about a mode, a session, a chat or a
window, and a `use` that changed that would be the violation.

That is also where `destructive` lives, and the only place it lives.
`DeleteFile::intent` sets it, and nothing keeps a list of destructive tool
names, infers it from an ability, or special-cases the word "delete". A
destructive call asks in every mode including Autonomous and cannot be waived by
*always for this tool* ([`docs/rules/tools.md`](../../../docs/rules/tools.md)),
and that one field is the whole reason it happens.

**Every path a tool acts on has been through `Workspace::resolve`.** A tool is
handed arguments and a `Context`, and the only route from a string in the
arguments to something on disk is `Context::resolve`, which answers with a
`Resolved` or refuses. That is what makes the matrix's Read row honest: it is
Allow in every mode, which is only defensible because *outside the project* is
not a state that can be approved or refused, because it never reaches the point
of asking.

`Resolved` makes the correct route the shortest one. It is not a capability: a
tool determined to call `std::fs` with a raw string still can. So the property
is asserted rather than assumed, in the contract suite below.

## The contract suite

`src/contract.rs`, beside the trait, per
[`docs/rules/tiles.md`](../../../docs/rules/tiles.md). `contract::holds_for`
takes any implementation and a `Rig` (a real project, and a real directory
outside it with something worth stealing in it) and holds it to three promises:

1. **A schema is a shape and carries no prose**, at any depth (hard rule 10, ADR
   0008).
2. **A schema closes itself**, which is what lets `arguments::faults` refuse an
   invented property rather than pass it on.
3. **No path reaches the filesystem without `Workspace::resolve`**: every
   spelling of somewhere else is refused *as confinement* rather than by
   accident, an `Intent` never claims to touch a path it could not reach, and
   nothing outside is read, written or taken away.

`tests/confinement.rs` calls it for every tool in `files()`. **An implementation
that does not call it is not an implementation**, which is the rule's own
wording and is what `run_command`
([#51](https://github.com/elpideus/demido-studio/issues/51)) and any tool named
by a server Demido did not write inherit by calling it.

`Tool` is an unusual trait to hold this way, because its implementations are not
interchangeable: nobody swaps `read_file` for `write_file`, so it is not a tile
in [`tiles.md`](../../../docs/rules/tiles.md)'s sense of *swapping it is a
decision*. What its implementations share is not a job, it is a set of promises,
and those are exactly the promises the rest of Demido has to be able to make
about a tool it did not write.

It was checked by mutation on #50 rather than assumed: a `description` typed
back into a schema, and a tool that calls `std::fs` with the string it was
handed instead of `Context::resolve`, both fail it, each naming the tool.

## What is in it

| Module | What |
|---|---|
| `workspace` | `Workspace`, `Resolved`, and the confinement rules. The most important file here. |
| `tool` | The trait, `Ability`, `Intent`, `Failure`, `Context`. |
| `contract` | The trait's contract suite. Every implementation calls it. |
| `registry` | Which tools exist, and a call turned into an outcome. `plan` stops one step short of running. |
| `arguments` | Everything provably wrong with a call's arguments, all at once. |
| `files` | `read_file`, `write_file`, `delete_file`. |
| `listing` | `list_directory`. |
| `search` | `search_files`, first-party rather than whatever `grep` is on the machine. |

## These tools deliberately get no seam

`read_file` against a real temporary directory is a better test than `read_file`
against a filesystem trait, and workspace confinement is only interesting
against real paths, real symlinks and the real path shapes Windows accepts. Every
test here stands on a `tempfile::TempDir`. There is no `Filesystem` trait and
adding one would make the suite prove less.

**The symlink case is not in the contract**, and is `#[ignore]`d on Windows,
for one reason: creating a symlink there needs Developer Mode or elevation, so
the contract's callers cannot all be assumed to manage it and a suite that fails
on an ordinary profile is a suite people turn off. It lives in
`tests/confinement.rs` and in `workspace.rs`'s own tests instead. Those three
are not optional: run them, and record that you did.

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p demido-tools -- --ignored
```

They were run and passed on
[#50](https://github.com/elpideus/demido-studio/issues/50), on a Windows profile
with Developer Mode on.

## Where a tool's prose is, and is not

Not here. A tool's description and the `description` fields inside its parameter
schema are host-authored prompt text with an id, a default file, a hash and an
`Origin`, per hard rule 10 and
[`0008`](../../../docs/decisions/0008-a-tool-description-is-a-prompt.md). They
live in `demido-prompts`' tool register and are merged onto the shape declared
here when a payload is assembled
([#52](https://github.com/elpideus/demido-studio/issues/52)). What a
`parameters()` body carries is the half that is a contract with the parser: the
property names, their types, which are required, and that the schema closes
itself. The contract suite is what keeps a description from being typed back
in, at any depth.

A **failure message is not in the register.** It is a sentence about a path or an
error, assembled with `format!`, and
[`prompts.md`](../../../docs/rules/prompts.md) reads interpolated prose as a
message rather than a prompt. It is still the most rewritten text in the crate:
"src/mian.rs does not exist. src contains: main.rs" is what a model recovers
from, and "No such file or directory (os error 2)" is what it gives up on.

## The port review

`demido-tools` is a **port with review**
([#2](https://github.com/elpideus/demido-studio/issues/2)), and this is the
review, recorded on [#50](https://github.com/elpideus/demido-studio/issues/50).
v2's crate was 12,751 lines across 28 files. What entered v3 is eight.

### Carried over, largely as it stood

- **`workspace.rs`**: `open`, `resolve`, `split_at_existing`, `relative`, and
  their tests. It was already right about the two things that matter, that the
  comparison is against the real path and that a path which does not exist yet
  still has to be checked, and the comment about the trailing separator records
  a bug that cost v2 two platforms.
- **`arguments.rs`**: whole, including its posture. It refuses only what it can
  prove wrong, and a call it accepts is not thereby proved right.
- **`registry.rs`**: `plan` and `run`, the four failures answered as sentences,
  and `shape`, which says a schema in one line for a model that has just
  ignored the schema.
- **The Files tools' behaviour and wording**: line numbering, the sibling list
  under a missing name, the coordinate a search answers with, the exact counts,
  the numbered listing. Most of these were rewritten by v2's live runs, and the
  comments saying which run and what it watched came with them.

### Rewritten, and why

- **The descriptions and the parameter prose are gone.** Hard rule 10 and ADR
  0008: v2 held 25 of them as string literals and its own trait doc called a
  description *"the single highest-leverage string in the crate"*. They arrive
  with the register on #52.
- **`Room` and the Context Economy are gone.** v2's `read_file`,
  `list_directory` and `search_files` each cut their output to the room left in
  the conversation. None of that machinery is in v3 and none of it is in this
  slice, so porting the room-shaped half would have been dead code shaped like a
  feature. The constant ceilings underneath it stayed, because a listing of
  fifty thousand entries has to stop somewhere whether or not anybody counted.
- **`Needs` is gone.** v2 had two variants and one tool used the second. All
  five here need somewhere to stand, so a registry with no workspace offers
  nothing, which is what v2's own registry comment said the rule should be until
  a tool arrived to justify the distinction.
- **The contract suite is new.** v2's `Tool` had no `contract` module, so what
  every tool promises was kept by each tool remembering it, and v2's own
  `search::sweep` is what that costs (below). `AGENTS.md`'s design rules and
  [`tiles.md`](../../../docs/rules/tiles.md) both require one, and it is written
  now rather than when `run_command` arrives, which is the rule's own timing.
- **`Resolved` is new.** v2's `resolve` answered with a `PathBuf`, so nothing
  in a signature distinguished a path that had been checked from one that had
  not, and every tool kept that rule by remembering it.
- **`Workspace::confine` is new, and it closes a real hole.** v2's
  `search::sweep` descended with `path.is_dir()`, which follows a symlink
  without saying so, so a link inside the project pointing at the rest of the
  disk was walked and its contents were returned as search results. Every entry
  a walk keeps now goes through the workspace exactly as a model's path does.
- **The Windows path shapes are refused from the path, not from the
  filesystem.** v2 left a UNC share, a device namespace, a drive-relative
  `C:tmp` and a driveless `\Windows` to `canonicalize`, which answers with an
  error about a machine nobody mentioned, or (for the drive-relative case) joins
  a colon into a filename under the workspace. They are decided in `resolve`
  now, and `every_windows_spelling_of_somewhere_else_is_refused` is the table.
- **`list_directory` was rewritten.** v2 read through `browse::read`, shared
  with the file explorer so a window and a model could not describe a folder
  differently. There is no file explorer in v3, so the tool reads the directory
  itself. The wording, which is the part v2's live runs rewrote three times,
  came over unchanged.
- **`delete_file` is new.** v2 never had it. The brief names it, and it is the
  only tool in this slice that exercises the destructive floor: without it that
  rule ships untested and the first tool to test it would be one that deletes
  something.
- **`write_file` stages and renames inline** instead of calling
  `demido_core::atomic::write`. That module is not in v3 yet and `demido-core`'s
  own review belongs to
  [#51](https://github.com/elpideus/demido-studio/issues/51); `demido-settings`
  already writes a profile the same way, inline, so this is the second instance
  rather than a third pattern. If a third appears, that is when it moves down
  into `demido-core`.

### Not ported, and not missing

`filtered`, `in_a_row`, `group`, `find_tools` and the whole of `disclosure`:
their consumers are the Context Economy and the tool picker, and neither is in
this ticket. Progressive disclosure arrives with the picker
([#56](https://github.com/elpideus/demido-studio/issues/56)) and answers a
different question from the picker's, which
[`tools.md`](../../../docs/rules/tools.md) is careful about.
`Context::for_call` went with `delegate_task`, which is S4.
`permission.rs` is [#53](https://github.com/elpideus/demido-studio/issues/53)'s
and `command.rs` is [#51](https://github.com/elpideus/demido-studio/issues/51)'s,
deliberately: `run_command` is one tool and most of a crate, and reviewing it
alongside `demido-core`'s job objects is what keeps the Windows-shaped half in
one place.

Port quarantine still applies. Nothing here has been driven live yet; that is
[#59](https://github.com/elpideus/demido-studio/issues/59).
