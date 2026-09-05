# Offered, and permitted

Decided on wayfinder ticket
[#20](https://github.com/elpideus/demido-studio/issues/20).

Two axes, and the whole of this file is keeping them apart.

**Offered** is what is in the registry this turn: what the model can see, and
therefore what it can call. The **user** owns this, from a popover in the chat
input area.

**Permitted** is what runs without asking. The **mode** owns this, in the
capability matrix.

Neither is expressible in the other. A tool that is offered but not permitted
produces an approval prompt. A tool that is permitted but not offered produces
nothing at all, because the model never learns it exists. Confusing the two is
how a permission system turns into a system prompt that asks nicely.

## There is no Chat mode

Brief B13:

> Two main chat modes: Chat and Agent.

**Overruled.** There is one loop and it is the agent loop, always, the shape
Claude Code has. A conversation that calls no tool is not a different mode: it
is the same loop with nothing to do but answer.

The second half of that line binds unamended. Brief B57:

> There should be 3 agent modes: Cautious, Balanced, Autonomous.

v2 never built the split either, and it is worth being precise about that,
because the ticket opened believing otherwise. There is no `chat.mode` anywhere
in v2's schema and no second loop in `demido-chat`. The only thing that ever
resembled one was an accident, an empty `tools.workspace` meaning no tools at
all, and v2's own roadmap records it as already dead: *"No workspace used to
mean no tools at all, which was true for exactly as long as every tool needed
one. `read_skill` is the tool that justified the distinction."*

The reason to delete the split rather than build it is that everything the Chat
mode would have done, the user can now do better on the offered axis. "This
conversation has no tools" is a set with nothing in it, reached by a control the
user can also use to say "this conversation has files but no shell", which no
mode could express.

## Permitted: the matrix

Carried forward from v2's `demido-tools::permission`, which is good and stays.
One pure matrix, three presets over it, nothing hardcoded per mode: an `Intent`
goes in, a verdict comes out, and three modes are three rows in a table rather
than three code paths. The third one is always written in a hurry.

| | read | write | shell | network |
|---|---|---|---|---|
| Cautious | Allow | **Ask** | **Ask** | **Ask** |
| Balanced | Allow | Allow | **Ask** | **Ask** |
| Autonomous | Allow | Allow | Allow | Allow |

Three rules hold it up.

**Destructive always asks**, in every mode including the one that approves
everything else. It is not a preset, it is the floor under all of them, it
cannot be waived by `always_allow`, and a tool that is unsure should declare
itself destructive: erring towards asking costs a click, erring the other way
costs the thing.

**An unknown mode name resolves to Cautious.** A profile written by a newer
build naming a mode this one has never heard of must never be read as permission
to do more.

**A denial is information, not an error.** The model is told, and gets to do
something else.

### What the four abilities mean

Four and no more. A fifth axis is a fifth question a person has to answer about
a prompt they are seeing for the first time, probably annoyed, which is the
ground v2's ADR 0009 rejected `Remote` on. That argument holds, so the work goes
into the definitions instead.

- **Read** is the workspace, and only the workspace. Every path has already been
  through `Workspace::resolve`, so "outside the project" is not a thing that can
  be approved or refused, because it is not a thing that can happen.
- **Network** is anything off this machine, **including anything reaching an
  account**, whether or not it changes something. Reading the user's mailbox is
  `Network`, never `Read`. This closes a hole the matrix had until now: `Read`
  is documented as "looks at something and changes nothing" and is Allow in
  every mode, so under the old reading a Cautious model read the user's entire
  inbox without a prompt. Sending mail is `Write` **and** destructive, because a
  sent message cannot be put back.
- **Shell** is running a program somebody else wrote. An MCP call declares
  `Shell` and `destructive: false` (ADR 0009, carried forward), and so does
  `delegate_task`, for the reason below.
- The **browser** ([#10](https://github.com/elpideus/demido-studio/issues/10)) is
  `Network` when it fetches and `Shell` when the model drives a click. It is
  honestly two things and the matrix should say so.

### The mode gates permissions and nothing else

Not the step limit, not parallelism, not delegation depth. Those stay
independent settings, for three reasons that compound: the matrix's purity is
what let ADR 0009 be decided at all;
[#18](https://github.com/elpideus/demido-studio/issues/18) established that a
Nexus source can impose a **ceiling** on parallelism and depth, so a mode
setting them too would give one number three owners; and
[#19](https://github.com/elpideus/demido-studio/issues/19) found `--ctx-size` is
per slot, which makes parallelism a VRAM budget rather than a risk appetite.

The one thing the mode does own about agents: **`delegate_task` declares
`Ability::Shell`**, so Cautious and Balanced ask before the first delegation of
a turn and Autonomous does not. That answers "may the model delegate at all"
through the existing mechanism rather than a new axis, and it costs one prompt
per turn rather than one per sub-agent.

### The mode is never prose

It changes no system prompt, adds no adjective, and tells the model nothing. A
mode expressed in prose has an effect that depends on whether the model obeyed
it, which is the single property the matrix exists to eliminate.

So the mode is invisible during a conversation that calls no tool. That is
correct behaviour rather than a gap: a permission is meant to go unnoticed until
something asks for one. Its only standing visibility is the shield on Cautious
that [`design/shell.md`](../../design/shell.md) draws.

## Offered: the picker

Brief B15: "Tool calling and custom tools"

A popover in the chat input area, the way Claude and ChatGPT put it there, so
the set is changed **about the message you are about to send**. A round trip
through a settings window to switch what the model may touch is a round trip
nobody makes twice.

**Rows are groups, expandable in place to individual tools.** Demido's built-in
groups first (Files, Shell, Web, Browser, Delegation), then one row per
installed skill, each with its tool count. Expanding a group reveals its tools
with their own switches, and a group with some of its tools off renders as
partial rather than as on. Nothing is delegated to a settings page: the popover
is the whole surface. The grouping is not invented here, the registry already
has it, which is what `find_tools` opens.

The grain matters over time. A skill installing adds **one** row, not eleven,
and that is the property that keeps the popover readable in a year.

**Everything is on by default.** The popover is an escape hatch, not a setup
step: a fresh user who asks for a file and is refused by a control they have
never opened is a worse failure than a crowded registry.

**Disabled means absent.** A tool that is off is not sent to the model in any
form: not in the tool list, not held back behind `find_tools`, not mentioned.
This is a third case v2's rule never anticipated. That rule says *"a tool held
back is not a tool that is gone"*, and answers a call naming a held-back tool
with the group to ask for, which is right for progressive disclosure and exactly
wrong here: it would send the model to reopen something a person deliberately
closed.

So when a model names a disabled tool anyway, it is told **the user turned it
off**. Still not a lie, still actionable, and the action is the right one: the
model says it would need file access and that file access is off, and the user
reaches for the popover they already know about. A model that instead silently
works around a disabled tool is the outcome this prevents.

**Progressive disclosure survives underneath, unchanged.** `tools.shown` answers
*the registry is too large for the context*; the popover answers *I do not want
this model doing that*. Those are different problems that happen to look alike,
and one mechanism doing both would do neither.

### A skill is one switch

Brief B20: "Skills system. This is important because my requirements for it are complex."

Switching a skill off stops offering its page **and** does not start its server.
Not two switches, one.
[#14](https://github.com/elpideus/demido-studio/issues/14) made a skill's
`engine/` an MCP stdio server precisely so that one skill is one thing with one
approval path and one crash boundary; two independent switches would re-split
what that decision joined. It would also produce the worst available state, a
model reading `SKILL.md` for a skill whose tools are gone, spending context on
guidance that can only ever produce a failed call.

The consequence is deliberate and useful: the popover is the one place a user
can tell an unwanted server to go away.

## The offered set is an event in the log

Brief B07: "recorded in an append-only session log"

When the offered set changes, that is a `tools/offered` event carrying the set
and the layer that decided it, the same shape as any other setting change.

The assembly stays a faithful record of what was actually sent, so
[#8](https://github.com/elpideus/demido-studio/issues/8)'s rebuild needs no
change to be correct. But correct is not answerable: a user debugging *why did
it not use the file tools* opens the session monitor, sees no file tools in the
assembly, and cannot tell a deliberate absence from a dropped one. The event is
what distinguishes them. The monitor's source ledger gains a source that
explains an absence, chat export stays a projection of the log, and the cost is
one event per change rather than a field on every turn.

## A sub-agent inherits, and can only narrow

Brief B19: "Configurable Agents & sub-agents system."

A sub-agent's offered set is its parent's set. It may narrow it further; it can
never add to it, at any depth. The same applies to the mode: a sub-agent runs at
its parent's mode or stricter.

This is the load-bearing rule of the file. Without it both axes are advisory,
because both are escapable by one `delegate_task`: a user who switched the shell
off has a model that restores it by delegating, and Cautious means nothing to
anyone willing to spawn a helper. With it, both are ceilings, and the brief's
depth-3 chain can only shed permissions on the way down.

v2's `delegate.rs` already reasons this way about the matrix, in a doc comment
rather than a rule: a tool *"offered to a sub-agent's model is simply never on
offer"*. This makes it the rule.

## Where both settings resolve

The ordinary ladder from
[#8](https://github.com/elpideus/demido-studio/issues/8): global, then model,
then chat, then character, following-or-overridden, with no fourth shape
invented for either. v2 already resolves `tools.mode` this way and has a test
for it.

The one wrinkle is that the offered set is a **set** where every other row on
that ladder is a scalar, and **an override replaces rather than merges**. A
merging set has no way to express *off*: a character declaring "Files only"
would get Files plus everything global already had, which is the opposite of
what it said. Replacing also keeps the settings row readable in the form #8
specified, *"Files, Web, set by this character, overriding global"*.

That gives the conversational character its mechanism after all. A persona whose
job is talking declares an empty set and its chats open with no tools, without a
Chat mode existing anywhere.

## What v0.1 ships

All three modes, because they are three rows in one table and shipping two would
be more work than shipping three. **Cautious is the default**, matching
`Matrix::default()` and the unknown-name fallback, so the strictest answer wins
every ambiguous case.

Two scenarios land on [`done.md`](done.md)'s S2, and the **matrix ships at S2
rather than S4**: a tool call in Cautious is an approval prompt, so the gate has
to be real the first time a tool runs. S4 adds delegation to it, not the modes.

- **S2 must include a greeting.** S1 has no tools at all, so the full registry
  does not exist until S2. With it loaded, "hello" must
  produce an answer and no tool call, on all three tiers. This is the cost of
  deleting the Chat mode, and it is measured rather than assumed:
  [#19](https://github.com/elpideus/demido-studio/issues/19)'s probe had all four
  models on disk pick the right tool out of three and emit valid JSON, three
  probes out of three, so the expected degradation is a hypothesis and not a
  finding. Three tools is not twelve. If a tier fails, the fix is already sitting
  here in two forms, widening `tools.shown` to cover Demido's own tools, or the
  user switching a group off.
- **S2 must also include a denial**, not only an approval. *No, and the model does
  something else* is the path most likely to be broken and least likely to be
  exercised, because a small model handed a refusal typically retries the same
  call forever. `Bar: chose` on that scenario means it chose **something else**.
