# 0008. A tool description is a prompt

Status: accepted
Decided: [#33](https://github.com/elpideus/demido-studio/issues/33)

## Decision

**If the model reads it and Demido wrote it, it has an id, a default file on
disk, a hash and an `Origin`.** That catches the 25 host tool descriptions and
the 33 `description` fields inside their parameter schemas, which v2 held as
string literals in `demido-tools` and treated as code.

They live in a second register in `demido-prompts`, keyed by tool name rather
than by prompt id, because the registry asks for them and the composer does not.
A tool is **one document**: its description and its parameter prose, edited
together and hashed together. The shape of a schema stays a contract with the
parser.

## Consequences

Easy: the brief's *"All prompts should be editable"* becomes true of the text
that matters most. v2's own trait doc calls a tool description *"the single
highest-leverage string in the crate: a small model picks tools almost entirely
on this"*, and it was the one category of model-facing text nobody could see or
change.

Hard: `tools/offered` changes shape. It carried `names: Vec<String>`; it now
carries a hash per tool, and each tool's document is stored in the log once per
session per hash. That is about 1,300 tokens of log per session, and it is what
makes [#8](https://github.com/elpideus/demido-studio/issues/8)'s promise to
rebuild the prompt as it stood at any event true rather than approximately true.
Without it the monitor renders today's wording against yesterday's reply, and it
does so silently.

Hard: a tool's prose stops being something a contributor edits in the file they
are already in. Changing `run_command`'s description means editing its default
file, and if a measurement is pinned to that wording, re-running the eval.

Foreclosed: a per-tool description generated freely at runtime. Only a declared
placeholder may vary, and only for a fact about the machine that cannot be typed
into a file, which is what `read_skill`'s list of installed skills is. Anything
the repo already owns is typed out as prose and bound by a contract test, so
that a hash over a template with a hole in it cannot hide a change to what fills
the hole.
