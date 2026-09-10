# demido-settings

The settings ladder: the schema, the four tiers, the `Store` trait with its
contract suite and two implementations, and `Settings`, which is the only thing
in the workspace that collapses a ladder into an answer.

Brief B09: "both at a model/global level as well as at a per-chat level"

## The one thing to understand

**The chat is the last word, and the system prompt is a ladder value.**

Precedence is the declaration order of `Tier` and nothing else: global, model,
character, chat, ascending, last writer wins. That order is
[`docs/decisions/0007-a-chat-outranks-its-character.md`](../../../docs/decisions/0007-a-chat-outranks-its-character.md),
and `stack::tests::the_chat_is_the_last_word` is where it is asserted rather
than described. Two rule files once described it backwards; a test cannot.

The system prompt is a setting like the temperature is. Not a field on a chat,
not a constant, not a file beside the session log. A character is a persona, a
persona is a system prompt plus a set of tools, and a system prompt stored
anywhere but here is one the character tier could never override. That is the
single thing S1 could have done that no later slice could undo
([#32](https://github.com/elpideus/demido-studio/issues/32)).

## Two tiers are live and four are stored

`document.rs` is the file shape, and it carries `global`, `models`,
`characters` and `chats` from the first commit. Nothing in v0.1 writes the
middle two: there is no model picker and no character system, so nothing names a
subject to resolve them against, and `Ladder::for_chat` is deliberately global
and chat alone rather than four scopes of which two are empty.

The room is there because the alternative is a migration of every profile on the
day the character system lands. The contract suite asserts it
(`every_tier_survives_the_round_trip`), so a store that quietly dropped the tiers
nothing uses would fail rather than be discovered later.

## The schema is three settings

`schema.rs` holds `conversation.system_prompt`, `conversation.temperature` and
`conversation.context_length`, and nothing else. v2 declared twenty four
samplers before anything sent one. A setting added later is one entry in
`SCHEMA`; a setting declared before something resolves and sends it is a
contract nothing can be held to.

The prefix is the **section**, not the tier. Any of the three can be set on any
tier, and `conversation` is the page they are drawn on.

Two fields on `Setting` are worth knowing about:

- **`kind`** carries the control, its range and its default, so a setting cannot
  be declared with a default its own range would reject, and so one component
  can draw it in two hosts (the settings page and the set-up wizard).
- **`reloads`** says whether changing it costs a restart of the server. Only the
  context length does: it is a flag on the process, and the other two are fields
  of a request. The window reads it to decide whether to load the model again.

The titles and summaries live here rather than in the frontend so that one
declaration serves the settings page, the wizard and (later) the Navigator. They
are UI copy and no model reads them, which is what the `// not-a-prompt:`
markers in that file account for (hard rule 10).

## Off is not zero

`Kind::Amount` is a switch and a field. `null` is off, which is a request that
does not carry the parameter at all and leaves the choice to the server; `0.0`
is a request that carries zero. On `llama.cpp` those are different generations.
v2 stored both as zero and could express neither, which is why this is a kind
rather than a convention.

## The store, and how it differs from the desk's

`Store` (`src/store.rs`) reads and replaces one document per profile.
`src/contract.rs` is the trait's second file, per
[`docs/rules/tiles.md`](../../../docs/rules/tiles.md); call
`contract::assert_store(|name| ...)` from an implementation's own test file.

| Implementation | Where | Runs the suite in |
|---|---|---|
| `Files` | `src/file.rs` | `tests/store_contract.rs` |
| `Memory` | `src/memory.rs` | `tests/store_contract.rs` |

**Both methods return a `Result`, and that is the difference from
`demido_shell::Store`.** A desk arrangement that will not load is discarded
silently, because nobody can act on the report and remaking it costs one
gesture. Settings are typed by a person: a file that will not load is their
work, so `Files` moves it to `settings.json.unreadable` and says where it went,
and the next write starts clean rather than landing on top of it.

`Settings::open` still cannot fail. Startup never blocks
([`AGENTS.md`](../../../AGENTS.md)), a settings file is not a reason a window
does not open, and the file has already been kept by the time the warning is
logged.

## What is deliberately not here

**Debouncing.** A settings change is a deliberate act, not the residue of a
drag, so it is written through. `demido-shell` has a debouncer because a seam
being dragged reports sixty times a second; a person typing in a field does not.

**Provenance on the session log.** `Resolved` knows which tier won, and the log
records the `Options` a turn was sent with rather than which tier chose them.
That is the Session Monitor's question and it needs a `Body` variant, which is a
change to the log's shape and belongs with the monitor.

**A settings history.** There is none, and a value that was changed is simply
changed.

**Anything the window draws.** `design/windows.md` owns the settings page and
`web/src/settings/` implements it. The one thing this crate owes that surface is
`Row`, which says what is in force, whether this tier is the one saying so, and
what reverting would leave behind.

## The tests

`cargo test -p demido-settings` runs everything. There is no live suite of this
crate's own: what a real model does with a system prompt and a temperature is
asserted where the turn is, in `demido-chat/tests/a_real_model.rs`, and that a
context length is really reserved is asserted against a running server by
`demido-inference`'s contract suite.
