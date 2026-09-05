# Characters

Decided on wayfinder ticket
[#32](https://github.com/elpideus/demido-studio/issues/32).

Brief B61:

> since I will need a character system implemented later down the road so users can give LLMs personalities and ways of doing things

Binds unamended, **including "later down the road"**. Nothing in this file ships
in v0.1. It is written now because
[#20](https://github.com/elpideus/demido-studio/issues/20) deleted Chat mode
partly on the strength of characters existing, and a mechanism another decision
leans on should not stay unwritten just because it is not next.

The short version: a character is **who the model is being**, and nothing else.
It is an identity plus one scope of the settings ladder. It binds no model, no
skills and no machine.

## v2 built this, and mostly got it right

`demido-characters` is 1,293 lines across four files, wired through
`src-tauri/src/commands/characters.rs` to `web/src/characters/api.ts`. The port
ledger ([#2](https://github.com/elpideus/demido-studio/issues/2)) left it in the
review bucket rather than the port-as-is list. Reviewed: it ports.

Its own module note states the position this file keeps:

> Characters deliberately bind no model, no skills and no agent mode. Those are
> properties of a machine and a session, not of a persona, and tying them
> together makes a character that cannot be shared.

The one place v3 departs from it is the offered set, and the departure is
narrower than it looks. See [What a character carries](#what-a-character-carries).

## What a character is

Two things, in two places, and the split is load-bearing.

**An identity**, owned by `demido-characters`: an id, a name, an optional
picture, and which card spec it was imported from. The id is stable for the
character's whole life including across a rename, because it is the key of the
settings scope and changing it would orphan every value the user had set.

**A behaviour**, owned by `demido-settings`: a scope in the character layer,
keyed by that id. The system prompt lives here, as the ordinary
`chat.system_prompt` setting, alongside temperature, caveman levels and
everything else the ladder carries.

`demido-characters` never reads or writes a settings file, which is why its
`import` hands the prompt back to the caller instead of storing it. One crate
decides what a request says. A character that could set its own generation
parameters directly would be a second one.

Deleting a character deletes its settings scope with it. v2's `characters_delete`
already does both.

## What a character carries

| Carried | Where it lives | Reaches the model |
|---|---|---|
| Id | `character.json` | No |
| Name | `character.json` | No |
| Picture | `picture.png` beside it | No |
| System prompt | Character scope, `chat.system_prompt` | Yes, first in the assembly |
| Generation params, caveman levels, thinking | Character scope | As settings always do |
| Offered set | Character scope | As a tools payload, never as prose |
| Agent mode | Character scope | No, it gates permissions only |

**The name and the picture never reach the model.** They are how a person picks
a persona out of a list. Demido injects no "you are Ada" sentence of its own: the
prompt says whatever it says, and v2's card importer already folds the card's own
name into the prompt text when the card wanted it there.

### The offered set is the one departure from v2

[#20](https://github.com/elpideus/demido-studio/issues/20) resolves the offered
set and the agent mode through the same ladder as every other setting, so a
character scope can carry them the moment the layer exists. That is the mechanism
`tools.md` leans on: a persona whose job is talking declares an empty set and its
chats open with no tools.

v2 refused exactly this, on a shareability argument that is correct and does not
actually conflict:

> a character that could set its own generation parameters would be a second
> place deciding what a request says

**A character may carry an offered set. A card never does.** The field is a
local setting like temperature; the interchange format has nowhere to put it, and
that is a fact about the format rather than a compromise. So a traded persona
never asserts anything about a machine it has not seen, and the failure mode v2
was guarding against, a card naming `market-analysis__market_quote` on a machine
with no such skill, cannot occur.

Cost, written down rather than argued away: **importing a card gives you a
persona with whatever tools are global.** The empty-set persona is something you
configure after import, not something you receive. A one-click conversational
character is therefore two clicks.

## Where the prompt sits

v2's `demido-chat/src/prompt.rs` composes the system prompt in three parts and
its ordering rule holds unchanged:

1. **What the model is.** The resolved `chat.system_prompt`, which is where a
   character's prompt arrives. First, because it is what the model is being.
2. **Context.** The workspace tree, when there is one. Context rather than a
   directive.
3. **Demido's own directives.** The caveman fragments. Last, nearest the
   generation, because a directive is the thing a small model loses in the middle
   of a long prompt.

Two consequences worth stating, because both invite a wrong guess.

**A chat-level system prompt replaces the character's, it does not append to
it.** It is a scalar on the ladder and follows the same following-or-overridden
shape as every other scalar. Nobody should expect concatenation.

**A skill's `when` text is not in here.** Per
[`skills.md`](skills.md) it reaches the model as a tool description in the
payload, so it never competes with a persona for position.

The record keeps its rule from v2: a fragment appears in the log only if it
appeared in the prompt, and each host fragment is named by id and hash rather
than repeated per turn.

## Which layer wins

**Chat outranks character.** The ladder is global, then model, then character,
then chat, following-or-overridden.

This reverses what
[`tools.md`](tools.md) and [`profiles.md`](profiles.md) said before this ticket,
both of which wrote it as global, model, chat, character. Both are amended. v2's
`demido-settings/src/stack.rs` already ordered it the way this file does, and
nobody had noticed the two files disagreeing with the code they described.

The reason is the offered set. Under character-wins, a persona with an empty set
makes [#20](https://github.com/elpideus/demido-studio/issues/20)'s composer
popover **inert**: the user clicks a control that cannot turn tools on for this
one conversation, and the only fix is to edit the persona for every chat that
uses it. A rule that renders a visible control dead is the worse failure.

What this gives up: a persona cannot **guarantee** it is tools-free, only default
to it. Opening a chat on that character still opens it with no tools, which is
all [#20](https://github.com/elpideus/demido-studio/issues/20) needed.

See [`docs/decisions/0007-a-chat-outranks-its-character.md`](../decisions/0007-a-chat-outranks-its-character.md).

## Switching mid-chat

Allowed, and v2's mechanism is the answer rather than a candidate for one. It is
built and tested in `demido-chat/src/derive.rs`.

**Which character a chat is using is not stored on the chat.** A switch writes a
`character/selected` event and the character in force is derived from the log, so
it is a recorded fact like everything else and resume, fork and replay all get it
right for free. Clearing the character is itself a switch, not the absence of
one, which is what stops a cleared chat from inheriting the last persona on
reload.

**Nothing is rewritten.** Turns already in the log keep the prompt they were
actually sent, and [#8](https://github.com/elpideus/demido-studio/issues/8)'s
monitor can rebuild the assembly at any event, so the seam is inspectable rather
than mysterious.

The cost, which v2 left implicit: the next assembly uses the new character's
prompt and the old one is **gone from it**, so the model's own history then
contains turns it would not have produced. That is what the user asked for by
switching, and the monitor is where it becomes visible.

## A character that was deleted

An old chat's log names an id whose prompt and offered set no longer exist.

**The chat carries on, and says so once.** Deletion resolves to no character, so
the next assembly falls back to the global prompt. Deleting a character writes a
`character/selected` clearing event into every open chat that was using it, which
makes it an ordinary switch rather than a silent change of what the model is.

Silence here would be exactly the invisible-reason failure that v2's card export
was built to avoid, and a transcript that changes voice with nothing in the log
to explain it is a bug report nobody can answer.

## Per profile

Characters are per profile, already stated in
[`profiles.md`](profiles.md)'s table. A persona is a person's, and a card is
shared by exporting a file rather than by two Windows users pointing at one
directory.

## The card is the distribution answer

A character does **not** wait on the skill registry, and defers nothing to it.
SillyTavern cards are an established interchange format with thousands already in
circulation, and v2 reads all three specs still traded: the original flat object,
`chara_card_v2` and `chara_card_v3`. Import and export are a file dialog.

Three rules carry over from v2 and stay rules:

1. **The original card is kept on disk untouched**, beside the folded prompt. It
   is what makes an import non-destructive.
2. **Import folds and names what it dropped.** A card spreads a persona over
   `description`, `personality`, `scenario` and its own `system_prompt`; Demido
   has one prompt, so import folds them in a fixed order and reports every field
   it could not use. Never silently dropped.
3. **Export collapses the folded fields** rather than leaving them stale.
   Exporting a card still carrying a `personality` Demido stopped reading would
   produce a character that behaves differently elsewhere for a reason nobody can
   see.

A picture must be a PNG, because a card **is** a PNG and a picture that is not
one is a character that could never be exported. Converting it would mean an
image library for a problem the user solves in a second.

## What Demido refuses to carry

A card can hold more than a persona. Demido takes the prompt and refuses the
rest, permanently rather than pending:

| Card field | Verdict |
|---|---|
| `first_mes`, `alternate_greetings` | Refused. A greeting is not a prompt, and Demido does not seed a chat with a message nobody sent. |
| `mes_example` | Refused. A prompt-assembly slot Demido deliberately has not got. |
| `post_history_instructions` | Refused, same reason. |
| `character_book` | Refused. A lorebook is a retrieval system, which is a much larger thing than a persona. |
| `assets` | Refused. One picture is the identity. |

Every one of them is still **named at import**, per v2's `unused` list. Refusing
to use a field and refusing to mention it are different things, and only the
second one is a lie.

## Which slice

**None.** Characters are not in v0.1, and
[`done.md`](done.md)'s four slices are unchanged.

The brief says "later down the road" and means it. The v0.1 case for pulling them
forward was that [#20](https://github.com/elpideus/demido-studio/issues/20)
deleted Chat mode and left the persona holding it, and that case does not survive
contact with this file: the composer popover gives per-chat tool control in S2 by
itself, and now that a chat outranks its character, the persona was only ever a
default. Shipping it in v0.1 would mean a settings section, a picker, a card
importer and a PNG text-chunk parser for a feature the brief itself deferred.

**Nothing in v0.1 forecloses it**, and this is the part that matters:

- The **settings ladder** ships in S1 with a character layer that has no scopes
  in it. Adding scopes later adds no shape.
- The **composed system prompt** ships in S1 and already reads
  `chat.system_prompt` off the resolved ladder, so a character prompt arrives
  through a path that already exists.
- The **offered set** ships in S2 resolving through the same ladder
  ([#20](https://github.com/elpideus/demido-studio/issues/20)).
- The **session log** is append-only and open, so a `character/selected` event
  costs nothing to add later and old chats simply have none.

The one thing v0.1 must not do is store the system prompt anywhere but the
settings ladder. That is the only decision here a slice could get wrong.

## What this does not decide

- **Whether a character can be enabled per project.** `demido-settings` carries a
  project layer and a project is a plausible place to pin a persona, but
  [#8](https://github.com/elpideus/demido-studio/issues/8)'s settings sections
  name a model, a chat and a character, not a project character. It is one row on
  the ladder and it can be added without reopening anything here.
- **What the character picker looks like.** It is a settings section and a
  control in the composer, both of which `system.md` already has components for,
  and neither is drawn.
- **Whether a character may pin a model.** v2 refused it on the same
  shareability argument as the offered set, and this file does not need to settle
  it: unlike the offered set, no other decision leans on the answer.
