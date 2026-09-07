# demido-prompts

Every string Demido wrote that a model reads, identified by content and editable
on disk.

This is hard rule 10 of [`AGENTS.md`](../../../AGENTS.md), and the rule is one
sentence: **if the model reads it and Demido wrote it, it has an id, a default
file on disk, a hash and an `Origin`.** Never a string literal. The reasoning is
[`docs/rules/prompts.md`](../../../docs/rules/prompts.md) and
[`0008-a-tool-description-is-a-prompt.md`](../../../docs/decisions/0008-a-tool-description-is-a-prompt.md);
the crate is the half of it that runs.

## Two registers, one crate

`prompts.md` splits host text by who asks for it.

| Register | Holds | Asked for by | Ships |
|---|---|---|---|
| **Paragraphs** | Text composed into a prompt: the caveman levels, the project tree, the failure classifier. | The composer, by id | S1, here |
| **Tools** | One document per host tool: its description and the prose in its parameter schema. | The registry, by tool name | S2 |

Two registers rather than one list because a tool description is not a paragraph
the composer picks up, it is a field on a struct the registry builds, and
forcing them into one namespace makes `id` mean two things. One crate rather
than two because the versioning, the log record, the edit path and the editor
are the same in both cases, and the failure this crate exists to prevent is
exactly the one that follows from two places deciding what a prompt says.

**The tool register is not here yet, and that is deliberate.** S1 has no tool
call in it, and twenty five entries nothing sends is the failure this crate's
own rule warns about: a prompt nothing sends is worse than no prompt, because
the editor offers to change something that cannot matter.

Three paragraphs `prompts.md` names are held back for the same reason, and they
are the ones whose askers do not exist yet: the **lesson injection wrapper**,
the **parameter asking sentence** (a skill's, so S2 with the tool register) and
the **four task-model jobs**. What ships is what S1 sends or measures: the six
caveman levels, the project tree, and the failure classifier, which is here
because a pinned wording with no default file is a release gate that cannot
fire.

## The seam

`Paragraphs::open(dir)` and then one of four verbs:

| Call | For |
|---|---|
| `get(id)` | one entry, for whoever is about to send it |
| `all()` | the editor's list |
| `set(id, text)` | an edit |
| `reset(id)` | back to what this build ships |

`catalog` says what exists and what it says when nobody has touched it;
`register` decides what it says right now. Nobody else invents default text,
builds a path into the prompts directory, or computes a version.

It holds no loaded state. Every call reads the directory again, which is what
makes hot reload a property of the design rather than a feature somebody has to
remember to wire up.

## Nothing is read-only

The brief says "All prompts should be editable", and `prompts.md` refused a
read-only flag in writing. What an entry carries instead is its **dependants**,
rendered above the field in the editor:

> A measurement in `evals/lessons/` was taken against this wording.

`Dependency` has two kinds and only one of them has an entry today.
`Measured` is the classifier's. `Shared` is the asking sentence's, which arrives
with the tool register: it is declared now rather than later because the editor
renders both sentences side by side, and a kind added when its first entry
appears would be a schema change to a seam the window is already reading.

An edit **suppresses the claim rather than being refused**. `Prompt::suppressed`
is what the window renders: a user may degrade their own classifier,
`Origin::Edited` records that they did, and nothing detects that it hurt. That
cost is written down rather than glossed.

The one thing `set` refuses is a placeholder nothing will fill, which is not a
judgement about the wording: it is a name that could never expand, and the
failure is invisible until a model is handed a literal pair of braces.

## Invariants

- **An entry nobody edited has no file.** An edit whose text equals the built-in
  default resets instead of writing one, so a user who types the default out by
  hand still receives a later build's improved wording.
- **The default text is a file, not a string literal.** `defaults/*.md` is what
  `include_str!` ships and what an edit writes back, so a change to a default
  reads as a prose diff in review, "reset" restores something a reviewer can
  compare against, and `scripts/check-rules.mjs` can hash it against a pin.
- **An id that is not in the register has no path.** That is what keeps an id
  arriving from the window layer from naming a file elsewhere on the disk.
- **Line endings do not change an entry's identity.** Text is normalised to `\n`
  before it is hashed and before it is sent. The rule checker normalises the
  same way, or a Windows clone would report an edit nobody made.
- **A placeholder may be dropped by an edit, never invented.**
- **An edit records the base hash it was made from**, in `<id>.base` beside
  `<id>.md`. When the built-in wording has since changed, `note` says so. It
  stays a note: it never blocks a turn, and it never becomes a prompt to act.
  Beside the text rather than inside it, so `<id>.md` stays exactly the wording
  and nothing has to be stripped before it is hashed or sent.
- **An unreadable edit falls back to the built-in text and says so.** A prompt
  is not something a conversation may refuse to start over, and the substitution
  reaches the user rather than only the log.
- **The register is English.** One text per id. A translation is an edit like
  any other, recorded as `Origin::Edited`, carrying no special status. There is
  no locale axis and no reserved field for one; `prompts.md` gives the two
  measurements that decided it, and `a_paragraph_id_carries_no_locale_tag` is
  the guard against a later i18n effort landing one here.

## Versions, and what a change to a default costs

`Prompt::hash` is `sha256:` and the digest of the text with its placeholders
still standing, so it identifies the **wording** and not the string one turn
built out of it. `fill` does not recompute it: two turns that filled the same
entry differently point at the same stored text. The session log
([#41](https://github.com/elpideus/demido-studio/issues/41)) writes the full
text once per session per hash and names it by hash afterwards.

A default a measurement was taken against is **pinned**, and changing it is a
release gate rather than an edit. `scripts/check-rules.mjs`, rule `prompts`,
compares `evals/lessons/AGENTS.md`'s digest against
`defaults/lessons.classify.md` and fails until the eval is re-run and the pin
replaced, or the wording reverted. It binds defaults only, never a user's
machine.

The limit, written down: the gate proves the **wording** is unchanged. It cannot
prove the numbers are still right, because the models, the `llama.cpp` pin and
the sampling move underneath it too.

## Adding a paragraph

One entry in `CATALOG` and one file in `defaults/`. Give it a consumer in the
same commit.

**The register's own first commit is the exception, and it is the last one.**
[#40](https://github.com/elpideus/demido-studio/issues/40) lands eight entries
that nothing sends yet, because it is the ticket that has to land before
anything assembles a prompt: a turn loop that composes a system prompt from a
string literal violates hard rule 10 at the first line of assembly and is paid
for twice. The composer arrives in the same slice, and the entries here are the
ones it will ask for. Every entry after that owes a consumer, which is why the
three paragraphs above are held back rather than declared early.

If it is chosen by a setting, the mapping from setting value to id belongs in
the crate that composes the prompt, with a contract test asserting every value
names an entry that exists. Neither crate can keep that promise on its own.

## Tests

`cargo test -p demido-prompts`.

The suite checks the declaration against its own default text in both
directions, which is the failure most likely to ship: a placeholder renamed in
the prose and not in the table expands to nothing and is never noticed until a
model is asked to write about `{{target}}`.

`tests/class_table.rs` is the other kind, and `prompts.md` requires it: a block
a hash alone would not catch. The classifier's thirteen classes are prose in its
default file rather than a `{{classes}}` hole, because a hash over a frame with
a hole in it does not change when a class is added, and the test binds that
table to the vocabulary `docs/rules/lessons.md` owns in both directions. It
reads the rule file directly, because the vocabulary has one owner and a copy of
it in here to test against would be a third place the classes are written down.
