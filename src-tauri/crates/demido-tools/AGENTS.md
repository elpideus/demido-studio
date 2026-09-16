# demido-tools

What a model can do, and where it may do it: the tool trait, the registry, the
workspace every path goes through, the Files group, and the Shell group.

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

`tests/confinement.rs` calls it for every tool in `files()` and `shell()`. **An
implementation that does not call it is not an implementation**, which is the
rule's own wording, and it is what any tool named by a server Demido did not
write inherits by calling it.

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
| `command` | `run_command`, the Shell group. |
| `delegate` | `delegate_task`, the Delegation group, and the callback that carries a task out. |
| `tree` | A job object: killing a call kills everything the call started. |

## delegate_task

The Delegation group, one tool,
[#61](https://github.com/elpideus/demido-studio/issues/61). It is here rather
than in `demido-chat` for the reason every other tool is: what a person switches
off in the picker, what the matrix rules on and what the log records is a
registry entry, and a delegation that arrived beside that machinery would be a
second answer to all three questions.

It declares `Ability::Shell` and adds nothing to the matrix, per
[`tools.md`](../../../docs/rules/tools.md). Two things follow from that and only
that: Cautious and Balanced ask, Autonomous does not. **One prompt per turn
rather than one per sub-agent** is `demido_permission::answers_for_the_turn`, not
here: a tool decides nothing about permission, and this one is not allowed to be
the exception.

**What a sub-agent actually is, is not here.** A child session, its log, the
depth limit and the pool need a session, a backend and a settings ladder, and a
tool may know about none of the three. So the tool holds a `Delegating`: a
callback taking the task and answering with an `Outcome`. A callback rather than
a trait, for the reason the approval is one
([`tiles.md`](../../../docs/rules/tiles.md)): there is one real implementation,
and a trait would buy a second that exists only in tests.

The real one landed with the child session
([#63](https://github.com/elpideus/demido-studio/issues/63)), and it is
`demido_chat::delegations`: one end of a pair, whose other end the turn loop
reads beside the call it is running. It is a pair rather than a plain closure
because a registry entry outlives every turn it is offered in, and a sub-agent
needs the turn's sink, the turn's person and the turn's cancellation. That is
also when `src-tauri/src/wiring.rs` began registering the group: until a
delegation could succeed, offering it would have been the one thing `registry.rs`
is explicit about, a tool a model will call and a call that cannot succeed
however it is written.

## run_command

The Shell group, one tool, [#51](https://github.com/elpideus/demido-studio/issues/51).
It declares `Ability::Shell`, and four things about it are worth knowing before
touching it.

**A shell is confined where it starts and nowhere else.** `cwd` goes through
`Context::resolve_dir` like every other path, which is what the contract holds
it to, and a running program can then reach whatever the user can. That is the
whole reason it is `Shell` and the cautious modes ask.

**A command that ran failed by its exit status, never by stderr.** The field is
the `Outcome` arm, and the turn loop
([#54](https://github.com/elpideus/demido-studio/issues/54)) records a
`tool/result` whose `failed` is exactly that arm and nothing else: a command
that exits non-zero, or is killed at its deadline,
is `Err`, and one that exits zero is `Ok` however much it wrote to stderr, per
[`lessons.md`](../../../docs/rules/lessons.md). A call refused before anything
ran (no command, a `cwd` outside the workspace, `cmd.exe` not starting) is
`Err` as well, because it is a failed call like any other tool's. What
`lessons.md` rules out is stderr as a trigger, and nothing here uses it; when
the record arrives, whether a lesson may anchor to a refusal is that ticket's to
decide, and the message says which kind it was. A killed command gets
the runner's own line, `the command did not finish within <n> seconds and was
killed`, because a failure with no text never becomes a lesson.

**The call owns its tree.** The child goes into a job object on the line after
the spawn, and the job is killed when the child exits, when the deadline
passes, and when the call's future is dropped, which is what stopping a
generation mid call does. Nothing a command started outlives the call that
started it, including a server somebody meant to leave running. That is
deliberate for now: a background process nobody in the window can see or stop
is the leak this exists to prevent, and the day a model needs to start one on
purpose is the day that becomes a tool of its own. On Unix `tree` is a no-op.

**`destructive` is decided per call, from the command.** `looks_destructive`
is a net and not a wall: it recognises a program whose purpose is to remove
something (`rm`, `del`, `taskkill`) and a handful of phrases where the flag is
the danger (`reset --hard`, `push --force`), and it will miss `xargs rm`. It
lives in `RunCommand::intent`, which is the declaration the matrix reads, so it
is still the tool saying what one call is about to do rather than anybody
keeping a list of names.

**This departs from [`tools.md`](../../../docs/rules/tools.md), deliberately.**
That file says a tool that is unsure should declare itself destructive, and a
shell is unsure about every command it does not recognise. Following it
literally makes every command destructive, so Autonomous asks before each one
and the matrix's Autonomous Shell cell, which is Allow, could never be reached.
The two rules cannot both hold for a shell, and the table is the more specific
of them, so the net stands, as it did in v2: in Autonomous an unrecognised
destructive command runs without asking, which is what choosing Autonomous
accepts. The matrix ([#53](https://github.com/elpideus/demido-studio/issues/53))
is where that is exercised, and the place to revisit it.

### What is not tested automatically, and why

**There is no automated test of a hung child.** What is tested, against real
`cmd.exe` and a real grandchild started with `start /b`: that a dropped call,
a call past its deadline and a call that returned all leave nothing running,
proved by a marker the grandchild would have written had it lived. What is not:

- a child that hangs in a way the job cannot end, such as one stuck in a driver
  call, or one that broke away from its job on purpose;
- the window between spawning the child and putting it in the job (`tree.rs`
  says why it is open);
- stopping a real generation from the window while a command runs, which needs
  the approval and the turn loop of
  [#55](https://github.com/elpideus/demido-studio/issues/55) before anything can
  press Stop on a tool call.

The last one is owed to the window gate: the teardown is exercised by hand there
and the result recorded in the closing comment.

**Exercised by hand on #51**, outside the test harness, with a throwaway driver
that ran `RunCommand` through the `Tool` trait and read the operating system's
process table (`Win32_Process`, by command line) before and after. The tree was
`cmd.exe` running a script that started `node` and `powershell` in the
background with `start /b`, then an hour-long `ping` in the foreground: four
real processes, none of which would ever end on its own.

| How the call ended | Running before | Running 2 s after |
|---|---|---|
| Dropped mid-run, as Stop drops it | 4 | 0 |
| Killed at a 10 s deadline, answering with the `lessons.md` line | 4 | 0 |
| Child exited zero with `node` and `powershell` still running | 3 | 0 |

The window gate on [#59](https://github.com/elpideus/demido-studio/issues/59)
repeats this through a real Stop, which is the part a driver cannot stand in for.

The ticket said there would be no automated test of a long-running child at
all, and there are three, so this narrows its promise rather than keeping it.
They turned out cheap and deterministic: the grandchild is a planted script that
writes a marker after three seconds, so nothing waits on a process that might
not end. What stays manual is exactly what cannot be made deterministic.

Three smaller gaps, recorded rather than closed:

- If the job object cannot be created or the child cannot be put in one,
  `Tree::around` falls back to killing the child alone, and says nothing. The
  crate has no logger to say it with, and refusing to run the command would be
  worse than cleaning up after it less well.
- A command killed at its deadline loses what it had printed; only the runner's
  line comes back. That line is what `lessons.md` selects on, so the lesson
  engine loses nothing, but a model does.
- The working directory is written without `\\?\`, so a workspace path longer
  than `MAX_PATH` cannot be one. `cmd.exe` could not use it with the prefix
  either. The others are recorded here
because a test that waits on a process the operating system cannot end is a
test that hangs CI, and a suite that sometimes hangs is a suite people turn off.

**Whether a bare name is found in the working directory is the user's call.**
`cmd.exe` searches the current directory before `PATH` unless
`NoDefaultCurrentDirectoryInExePath` is set, and the session that wrote this
had it set, so `greet` failed where `.\greet` ran. `run_command` passes the
environment through rather than overriding a setting somebody chose, and the
tests name a planted script as `.\name`, which PATHEXT and the quoting apply to
just the same.

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
live in `demido-prompts`' tool register, one document per tool, and
`Registry::offered(&Tools)` merges them onto the shape declared here
([#52](https://github.com/elpideus/demido-studio/issues/52)). What a
`parameters()` body carries is the half that is a contract with the parser: the
property names, their types, which are required, and that the schema closes
itself. The contract suite is what keeps a description from being typed back
in, at any depth, and `scripts/check-rules.mjs` refuses a `"description"` key
here too.

A `Spec` carries the `Document` it was described from, so whoever records the
offered set records the name and hash of exactly the wording that was merged.

**`tests/documents.rs` is the contract between the two halves.** Every host
tool has a document and every document names a host tool; a document declares
exactly its schema's properties; and what is offered is the shape with prose
added and nothing else, even for a document written by hand that gives prose to
a property the tool does not take. It is not in `contract::holds_for`, because
that suite is also what a tool Demido did not write is held to, and such a tool
has no host document.

A registered tool with no document is **not offered**. For a host tool that is a
state the test above keeps anything from reaching, not a fallback: a tool
offered with no words on it is the one a small model picks worst. **It is wrong
for a tool Demido did not write**, which has no host document and would vanish
from the set with no event saying why, exactly the dropped absence `tools.md`
records `tools/offered` to explain. No such tool exists in v3 yet. The ticket
that registers the first one (an MCP tool, whose wording is the server's and is
captured in the approval digest) replaces this filter with that tool's own
wording, and is the place to decide it.

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
`permission.rs` did not come here either. The matrix is
[#53](https://github.com/elpideus/demido-studio/issues/53)'s and lives in
`demido-permission`, which depends on this crate, so the dependency list above
still says a tool knows nothing about a mode.

## The run_command and demido-core review

Recorded on [#51](https://github.com/elpideus/demido-studio/issues/51).
`run_command` depends on three things v2 kept in `demido-core`: the job
object, `PATHEXT`, and the `cmd.exe` quoting. They were reviewed together, as
the ticket asked, so that the Windows-shaped half is decided in one place.

### Carried over, largely as it stood

- **`command.rs`'s `shell`**: `cmd /d /s /c` with the whole command line put on
  verbatim through `raw_arg`, and the comment about the model that found it.
  `a_quoted_argument_reaches_the_program_the_way_it_was_typed` and the test with
  a space in the program's name were checked by mutation: with a plain
  `.arg(command)` both fail.
- **`looks_destructive`**, with all three of its lists (programs, wrapper words,
  phrases) and both of its test tables. `pwsh` joined the wrappers, and a
  program named with its extension or its path is now recognised.
- **The deadline**: 60 seconds unless asked, never more than 600.
- **`demido-core::tree`**, into `tree.rs`: the job object, `KILL_ON_JOB_CLOSE`,
  and the reasoning about why killing the child is not killing the work.

### Rewritten, and why

- **`tree` lives here, not in `demido-core`.** v3's `demido-core` says in its
  own `AGENTS.md` that nothing which talks to a process belongs in it, and that
  a type with one caller lives with its caller. In v2 two crates started trees
  (MCP servers and the market sidecar); in v3 `run_command` is the only one.
  If a second caller arrives, the move is to its own crate rather than down
  into the bottom of the graph.
- **The call owns its tree.** v2's `run_command` never used `tree` at all: it
  relied on `kill_on_drop`, which kills `cmd.exe` and leaves whatever `cmd.exe`
  started, and `output()`, which waits for every holder of the pipes. A
  grandchild started with `start /b` therefore held a v2 call open until it
  finished, and survived a stopped one. v3 kills the job when the child exits,
  at the deadline and on drop.
- **`failed` follows the exit status.** v2 answered a non-zero exit with `Ok`
  and an `(exit code N)` line, so a failed command and a successful one were the
  same arm and `failed` had to be inferred from prose. It is `Err` now, and a
  zero exit with stderr is `Ok` (`lessons.md`).
- **The deadline's message is the runner's line from `lessons.md`**, rather
  than v2's wording, because the lesson engine selects on it.
- **`cwd` is new.** v2 always ran in the workspace root. Taking a directory
  gives the contract's confinement half something to hold `run_command` to,
  and costs a model nothing it had.
- **Output is cut at a ceiling**, per stream, and the pipes are drained past it
  so a chatty child never blocks on a full pipe. v2's description promised
  shortening and nothing did it.
- **The description and the parameter prose are gone**, to the register on
  [#52](https://github.com/elpideus/demido-studio/issues/52), as for the Files
  group.
- **The working directory is written without `\\?\`.** A canonical workspace
  root carries the prefix on Windows, and `cmd.exe` refuses it as a current
  directory and quietly substitutes the Windows directory.

### Not ported, and not missing

- **`program.rs`** (`PATHEXT` resolved by hand). `CreateProcess` does not read
  `PATHEXT`, which is why v2 needed it to spawn `npx` directly. `run_command`
  spawns `cmd.exe`, which reads `PATHEXT` itself in its own order, and
  `a_bare_name_finds_its_extension_the_way_a_terminal_would` holds it to that.
  Its two v2 callers, MCP and the web fetch ladder, are not in v3.
- **`child.rs` and `noise.rs`**: a long-lived child talking over its pipes.
  `run_command`'s child is not long-lived and is not talked to. Their callers
  are MCP and the market sidecar.
- **`cancel.rs`**: v3 already has `demido-inference`'s `Cancel`, and a tool is
  stopped by dropping its future, which is what the tests do.
- **`atomic.rs`**: `write_file` and `demido-prompts` each stage and rename
  inline, as #50 recorded. Two instances, not three.
- **`paths.rs` and v2's `error.rs`**: v3's `demido-core` and `demido-settings`
  already own what they did.

Port quarantine still applies. Nothing here has been driven live yet; that is
[#59](https://github.com/elpideus/demido-studio/issues/59).
