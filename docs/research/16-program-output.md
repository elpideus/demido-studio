# 16. What a program printed: shortened for the model, kept whole

Written 2026-11-21, while building brief 4.18 ("CLI output compression"): "An RTK-like system, or rtk itself
if there's no better option. It compresses CLI outputs while always keeping a way to see the entire output
if it's really needed."

How sure each line is: **Tried** (run on the owner's computer today), **Read** (read today in the source
linked).

## rtk, checked

| Fact | Source |
|---|---|
| The real one is `rtk-ai/rtk`, as the brief says, and the warning in the brief holds: "Another project named \"rtk\" exists on crates.io". Made 2026-01-22; 87,371 stars; release v1.2.1 of 2026-11-19 | Read: GitHub's record of the repository and its readme |
| **It is no longer MIT, as the brief says.** It ships as three downloads: `rtk` (Apache-2.0 **and BUSL-1.1**: "Hook core, every command filter, and the pipeline engine"), `rtk-oss` (Apache-2.0 only: "Hook core and every command filter. No engine."), and `rtk-proxy` (BUSL-1.1). BUSL-1.1 is not a free-software licence and does not fit the GPL. Only `rtk-oss` could ever be used with Demido, and only as a separate program (it is one; it is not on crates.io under that name) | Read: its readme, "Editions and licence"; its release files |
| What it is: a program that sits between an agent and the shell. A hook rewrites the agent's commands (`git log` becomes `rtk git log`), and rtk runs the command and gives back a shorter output, with "100+ commands" each given a filter of its own: test runners show the failures, linters group by rule, `ls` and `grep` are grouped. "Never blocks. If a filter fails, you get the raw output." "Output that is cut says where the full version is" | Read: its readme |
| Its numbers are its own: "up to 90% less command output", measured by its makers on their commands | Read: its readme. Not tried |

## What was built instead, and why

Demido does not hand a model a shell: it runs one command for it, in the sandbox, and reads all of what it
printed itself (`sandbox.rs`). So Demido already stands where rtk's hook stands, and the general part of
what rtk does can be done there without a second program:

- **The whole is kept** (`chat`): as a terminal would show it once the program had ended, without colour
  codes, with a progress line that kept rewriting itself as its last state. Up to 2 MB of it, the newest 50
  outputs of a session. The window has "All it printed" beside the step.
- **The model is given a short view** (`tools/output.rs`), at most about 6,000 characters, chosen by rules:
  the lines that speak of something going wrong first (with the line after each, which often says where),
  then the ending, then the beginning. A run of lines that differ only in their numbers, or that start with
  the same word ("Compiling …"), is folded into its first and its last line and a count. A run never
  swallows a line that tells of a failure. What was left out is marked where it was, by line numbers.
- **The model can read on** (`read_output(from_line)`), right after a command whose output was shortened,
  once.

What rtk has and this does not: a filter for each program, written against real output. That is a great
deal of work by many people, and it is the part that makes rtk's numbers. Bringing in `rtk-oss` as a
separate program, downloaded from its makers and run inside the sandbox, would add it; it would also mean
rewriting every command the model writes. Not done: the general rules came first because they cover
every program, and nobody has measured on Demido's models what the specific filters add.

## Tried

| What | What happened |
|---|---|
| The rules on a made-up test run: 1,409 lines (300 "Compiling", 300 different lines, 800 passing tests, one failure with its panic, a summary) | Given in under 1,700 characters: the failure, its panic and assertion, the summary, the folded "Compiling" lines; what was left out marked by line numbers. A unit test now |
| A real model (Qwen3 4B) ran a script that printed 2,502 lines and failed | It was given the error in its first look. Offered `read_output` for as long as the command was in the request, it then read on five times in a row, page after page, though the error was already in front of it. Since then `read_output` is offered only right after the command, once |
| The window, with a real file change and a real program run | A change opens to its diff ("+1 −1", the line that went and the line that came); a program run that changed nothing says "nothing changed"; "All it printed" opens the whole output |

Not tried: what the short view does to how often small models get a programming task right. That needs the
program trial (`core/dev/`) re-run with failing builds, and has not been done.
