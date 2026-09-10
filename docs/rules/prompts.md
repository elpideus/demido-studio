# Host-authored prompt text

Where the strings Demido itself puts in front of the model live, who may edit
one, and what changing one costs.

Decided on wayfinder ticket
[#33](https://github.com/elpideus/demido-studio/issues/33).

Brief B06: "All prompts should be editable. Requests monitorable."

## What the ticket found before it decided anything

The ticket opened believing this was a question about `demido-prompts`, the
catalog v2 already built. It is not, or not mainly. Two measurements moved it.

**The catalog is not where most of the host's model-facing text is.** v2's
`demido-tools` carries 25 hand-written `fn description(&self) -> String` bodies,
5,215 characters, roughly 1,300 tokens, sent on **every turn**. Beside them sit
33 `"description"` fields inside the parameter schemas. The whole
`demido-prompts` catalog is 4,638 characters across seven entries, of which at
most one caveman level and the project tree ever ship on a given turn. The
trait's own doc comment says what is at stake: *"This is the single
highest-leverage string in the crate: a small model picks tools almost entirely
on this."* None of it is editable, versioned, hashed or logged.

**The rebuild promise is already broken there.** `tools/offered` carries
`names: Vec<String>` and nothing else.
[#8](https://github.com/elpideus/demido-studio/issues/8) promised the session
monitor can rebuild the prompt as it stood at any event. It cannot rebuild the
largest host-authored block in the payload, and it fails silently: the monitor
would render today's wording against a reply produced by yesterday's.

Two smaller ones. v2's `demido-chat/src/agent/economy.rs` holds a model-facing
`JOB` string hard-coded outside the catalog, which is one of the four task-model
jobs [#24](https://github.com/elpideus/demido-studio/issues/24) named. And the
classifier prompt that produced
[#22](https://github.com/elpideus/demido-studio/issues/22)'s numbers lived in a
throwaway harness outside the repo, built by splicing the class table that
[`lessons.md`](lessons.md) owns into a frame.

## The rule

**If the model reads it and Demido wrote it, it has an id, a default file on
disk, a hash and an `Origin`.**

That is the whole boundary, and it is
[`docs/decisions/0008-a-tool-description-is-a-prompt.md`](../decisions/0008-a-tool-description-is-a-prompt.md).
What decides *where* it lives is who asks for it.

| Register | Holds | Asked for by |
|---|---|---|
| **Paragraphs** | Text composed into a prompt: the caveman levels, the project tree, the lesson injection wrapper, the parameter asking sentence, the four task-model jobs, the failure classifier. | The composer, by id. |
| **Tools** | One document per host tool: its description and the prose in its parameter schema. | The registry, by tool name. |

Both registers live in `demido-prompts`, and every mechanism below applies
identically to both. Two registers rather than one list because a tool
description is not a paragraph the composer picks up, it is a field on a struct
the registry builds, and forcing them into one namespace makes `id` mean two
things. One crate rather than two because the versioning, the log record, the
edit path and the editor are the same in both cases, and the failure this file
exists to prevent is exactly the one that follows from two places deciding what
a prompt says.

**A tool is one document.** Its description and its parameter prose are edited
together and hashed together, so one hash covers everything about that tool the
model reads. The *shape* of a schema stays a contract with the parser and is not
editable; the `description` fields inside it are prose aimed at a 4B model and
fail the way a tool description fails.

### What is not host text

- **An MCP server's own tool descriptions.** The server wrote them. They are
  captured in the approval digest ([`skills.md`](skills.md)) and that is the
  right place for them.
- **A skill's re-description of its server's tool.** That is the skill author's
  file, and [`skills.md`](skills.md) already built it: the name is the join key,
  the model is shown the skill's wording, and a server that rewords its own tool
  changes what is disclosed and asks again.
- **A skill cannot reword a host tool.** [`skills.md`](skills.md): a skill's
  `tools` array is its own contributions and nothing else. Brief B62 binds the
  MCP path only.
- **The user's own system prompt and a character's.** Those are settings on the
  ladder ([`characters.md`](characters.md)), recorded in full beside every
  request, and they are the user's words rather than Demido's.

## Editable, when a string is load-bearing

The brief says all prompts are editable. It does not say some of them are
harmless to edit.

[`skills.md`](skills.md) needs its parameter asking sentence *identical across
every command in every skill*, so a user who edits it has changed every command
at once. [#22](https://github.com/elpideus/demido-studio/issues/22) measured the
failure classifier at about 80 per cent agreement with hand labels against
specific wording.

**Everything is editable, and an entry declares what it is load-bearing for.**
No entry is read-only. What an entry carries instead is its dependants, rendered
above the field in the editor:

> Every slash command in every skill uses this wording.

> A measurement in `evals/lessons/` was taken against this wording.

An edited entry **suppresses the claim rather than being refused**. A user may
degrade their own classifier, `Origin::Edited` records that they did, and
nothing detects that it hurt. That cost is written down rather than glossed, and
it is the cost [`lessons.md`](lessons.md) already accepts for a user-edited
remedy.

Refusing the edit was drawn and rejected. It contradicts the brief on the four
strings most worth editing, and the honest version of the concern is not
*forbid* but *say what breaks*, which is the move [`runtimes.md`](runtimes.md)
made with verify-then-delete and [`setup.md`](setup.md) made with stating a size
before a fetch.

## Versions, the log, and the rebuild

v2's mechanism carries forward unchanged and is good: `Prompt::hash` is
`sha256:` and the digest of the text with its placeholders still standing, so it
identifies the **wording** and not the string one turn built out of it. The full
text goes into the session log once per session per hash as `prompt/version`,
and every request afterwards names it by hash. A forty-turn session holds one
copy of a paragraph, an edit mid-session writes a second entry, and a reply from
before the edit stays explainable.

**The tool register is recorded the same way.** `tools/offered` gains a hash per
tool: `Vec<(name, hash)>` rather than `Vec<String>`, with each tool's document
stored under the same once-per-session-per-hash rule. That costs about 1,300
tokens of log once per session and makes
[#8](https://github.com/elpideus/demido-studio/issues/8)'s rebuild true.

Per tool rather than one hash over the whole block. The offered set is per turn
and per sub-agent ([`tools.md`](tools.md) lets a child narrow it), so a block
hash changes when the **set** changes and not when the **wording** does, and it
could never answer "was this reply produced under the old `read_file` wording".
Per tool also composes with the tool picker for free: a disabled tool is absent
from the payload and therefore absent from the record.

## Prose or value: what may be generated

The classifier prompt was not fixed prose. The harness spliced the thirteen-row
class table into a frame, and [`lessons.md`](lessons.md) owns that table.
`read_skill`'s description ends *"The skills installed are:"* and then lists what
is installed, which is why v2's trait returns `String` rather than
`&'static str`.

The two look identical and are opposites. The test that separates them:

**Could a reviewer diff this block in a pull request?**

- **Yes: it is prose.** It lives in the default file, typed out in full, and a
  contract test binds it to whatever the repo already owns. The class table is
  prose in the classifier's default file, and a test asserts it matches the
  class enum row for row, in both directions, the way the catalog suite already
  checks placeholders in both directions. It is not a placeholder, because a
  hash over a frame with a `{{classes}}` hole in it does not change when a class
  is added, which is the change most likely to move the numbers, and the gate
  below would sleep through it.
- **No: it is a value.** It is a declared placeholder filled per turn, and the
  hash covers the template as everywhere else. `read_skill` takes a `{{skills}}`
  placeholder, because what is installed is a fact about this machine that
  cannot be typed into a file at all.

Stated as one rule: **no block generated from something the repo already owns.**
A per-machine fact is not that.

## What a version change costs against a pinned measurement

[#22](https://github.com/elpideus/demido-studio/issues/22)'s numbers were taken
against one exact wording, now pinned as
[`evals/lessons/classifier.md`](../../evals/lessons/classifier.md) with its
digest recorded beside the corpus.

**A change to a shipped default is a release gate.** `check-rules.mjs` compares
the pinned digest against the register's default. A mismatch fails until either
the eval is re-run and the fixture re-pinned with the new numbers, or the change
is reverted. Re-pinning costs one command; the alternative is
[`lessons.md`](lessons.md) quoting an agreement rate no shipping build has ever
produced, which is the drift this whole map was chartered against.

It binds **defaults only**, never a user's machine. A user edit suppresses the
claim, per the section above, and gates nothing.

The limit, written down: the gate proves the **wording** is unchanged. It cannot
prove the numbers are still right, because the models, the `llama.cpp` pin and
the sampling move underneath it too. It is the same kind of check as
[`brief.md`](brief.md)'s verbatim anchors, and it fails in the same useful
direction.

## An edit that predates a changed default

v2 stores an edit as a bare `<id>.md` holding the text. It records nothing about
which default it was made from, so a later build improving a default is
invisible to anyone who has edited that entry, and with the gate above that user
is running a build whose measured claims do not describe their app.

**An edit records the base hash it was made from**, and when the built-in
wording has since changed, Demido says so: the `note` already on `Prompt`, which
is the channel `AGENTS.md` rule 6 requires and which v2 already uses when an
edited file cannot be read. The editor shows the same thing with a diff and a
reset.

It stays a note. It never blocks a turn, and it never becomes a prompt to act.

## Locale

**A prompt is English. One text per id. A translation is an edit like any
other**, recorded as `Origin::Edited` and carrying no special status.

Refused in writing rather than left open, because this is the file where a later
i18n effort would land it and saying so now is cheaper than discovering it. Two
reasons. Every measurement this project has was taken on English wording at 4B:
[#22](https://github.com/elpideus/demido-studio/issues/22)'s agreement rates,
[#19](https://github.com/elpideus/demido-studio/issues/19)'s tool-selection
probes, [`skills.md`](skills.md)'s fixed-words argument. A locale axis multiplies
that eval surface by the locale count. And the UI language and the prompt
language are not the same question: a user reading Italian menus may still want
the model prompted in the language it was trained hardest on.

The catalog carries no locale axis and no reserved field for one. Adding one
later is a schema change, which is cheap; pretending to support one now is not.

## Which slice

| | Register | Editor |
|---|---|---|
| **Paragraphs** | S1 | S2 |
| **Tools** | S2 | S3 |

The paragraph register earns S1 on the caveman fragments and the project tree,
both of which ship in the first slice, and it is what makes S1's `request/sent`
honest.

**The tool register waits for S2 because S1 has no tool call in it**
([`done.md`](done.md)). Twenty-five entries nothing sends is the exact failure
`demido-prompts`' own crate doc warns about: *"a prompt nothing sends is worse
than no prompt, because the editor offers to change something that cannot
matter."*

The editor follows its register by one slice in each case, because declaring
what an entry is load-bearing for needs the dependants to exist before it can
name them. Prompt editing is a Settings page
([#8](https://github.com/elpideus/demido-studio/issues/8)), and per
[`setup.md`](setup.md) any set-up step that touches one renders the same control
Settings renders.

## What is enforced

`scripts/check-rules.mjs`, rule `prompts`.

1. **The eval pin is self-consistent.** `evals/lessons/AGENTS.md` records a
   digest for `evals/lessons/classifier.md`, and it must be the digest of that
   file, whitespace normalised to `\n`. **Enforced now.**
2. **A shipped default matches its pin.**
   `src-tauri/crates/demido-prompts/defaults/<id>.md` must exist for a pinned id
   once the register ships, and its normalised text must hash to the pinned
   digest. **Enforced now**, and real since the register landed on
   [#40](https://github.com/elpideus/demido-studio/issues/40).
3. **A default is a file.** Every `default:` in the register's declaration is an
   `include_str!`. **Enforced now.** It is what makes a change to a default read
   in review as a prose diff, and it is what leaves check 2 something to hash.
4. **Host prose is not a literal.** A string in `src-tauri/` that reads as prose,
   six words or more, fails unless it is a diagnostic or a `// not-a-prompt:`
   comment accounts for it in one sentence. **Enforced now.**

Check 4 is the rule itself rather than its pins, and it is shaped the way rule 4
is: everything that looks like prose is refused, and the places prose
legitimately lives are named rather than guessed at. Three limits, written down
because a check nobody trusts is a check somebody disables.

- **Diagnostics are exempt.** A log line, an error's own sentence, a test's
  failure message, an attribute's own prose and the macros that read a file at
  compile time are all prose for a person and never reach a payload. `format!`
  is on that list and it is the one entry that is a judgement rather than a
  fact: every sentence Demido builds for a person interpolates the path or the
  error it is about, and a prompt never does, because a prompt's holes are
  declared placeholders that `Prompt::fill` puts values in. Prose assembled by
  `format!` is therefore read as a message. A system prompt smuggled through
  `format!` would pass, and nothing but review catches it.
- **Tests are exempt.** A `tests/` file and a `#[cfg(test)]` item are one long
  assertion about text, and a suite that quotes a paragraph in order to check it
  sends nothing. The contract probe in `demido-inference`'s contract module is
  the exception that proves it: it is in `src/`, it really is sent to a model,
  and it carries a `// not-a-prompt:` line saying a test sends it and a turn
  never does.
- **`web/` is not scanned.** The payload is assembled in Rust. Scanning UI copy
  would drown the signal, and a frontend that composed a prompt would be a
  different violation than this one.
- **Six words is a threshold, not a boundary.** It was measured against the
  workspace as it stood: the only literal that long in it was one `tracing::info!`
  message. A five word prompt fragment passes, and the register is what makes
  writing one pointless rather than the check. Twelve characters is the same
  threshold for a script that puts no spaces between words, because the register
  ships three wenyan paragraphs and counting words would find none in them.

Two things this file specifies that CI cannot check: the class table against the
vocabulary, and the placeholder declaration in both directions. Both are contract
tests in the crate, `tests/class_table.rs` and `catalog`'s own suite. The first
reads `docs/rules/lessons.md` directly, because the vocabulary has one owner and
a copy of it inside the crate to test against would be a third place the thirteen
classes are written down.

## What this deliberately does not do

- **No read-only entries.** See above: the brief is unambiguous and the concern
  is answered by disclosure instead.
- **No second identity per prompt.** One hash, over the text as stored, on every
  entry with no exceptions. A filled-text digest beside it would be correct and
  would serve exactly one entry.
- **No `default` value for a skill's parameter, and no host override key for the
  asking sentence.** [`skills.md`](skills.md) settled both, and neither is needed
  yet.
- **Nothing about the *user's* prompts.** Those are settings, and the ladder is
  [`characters.md`](characters.md)'s.
