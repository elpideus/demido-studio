# What Nexus promises, and what it is allowed to say

Nexus is the free models router. This file decides what a user with no model and
no key actually gets, in what order it is offered, and what the UI may claim
about it.

Decided on wayfinder ticket
[#18](https://github.com/elpideus/demido-studio/issues/18), on the live research
in [#4](https://github.com/elpideus/demido-studio/issues/4).

## The promise, kept literally

Brief B28:

> Free models system via OmniRoute/9Route-like system. I want the system to be called Nexus. It should be Demido Studio's own free models router system so that anyone can use Demido Studio for free without having to download a model.

That line binds as written and is **not amended**. A fresh install with no model,
no key and no account reaches a real answer from a real model, and it still does
so on the day every keyless source has gone dark, because the ladder decided on
[#4](https://github.com/elpideus/demido-studio/issues/4) degrades downward to the
user's own disk rather than to a paywall.

What this file adds is the part measurement forced. On 2026-09-03 the keyless
pool was two sources: OVHcloud, which supports tool calling at **2 requests per
minute per IP per model**, and AI Horde, which has no quota and **accepts a
`tools` array, ignores it, and returns confident prose**. One agent turn is 5 to
20 calls. So:

**Keyless can carry a first conversation. It cannot carry first work.** Every
rule below follows from that one sentence.

## 1. Nexus is the companion to the download, not the front door

The first screen offers a **local model sized to the detected hardware**. Nexus
is what answers while that download runs, and it stays available afterwards.

Rejected: leading with Nexus. A first impression of a chat that answers every
thirty seconds and stalls the moment the model reaches for a tool is a first
impression of the product, and tool calling is most of the product.

Also rejected: burying Nexus in the model picker. That is the brief's line
broken quietly, since a user who does not want to download 5 GB before seeing
anything would then see nothing.

## 2. The copy

The panel is named **Nexus**. Its subtitle is one true sentence:

> Chat for free on shared public endpoints. No account, no key. Slow, and tools
> are limited. Your own model is faster.

The words **"free models"** as a headline are banned. They promise plenty and
deliver two sources, one of which cannot call a tool. v2 shipped a page called
"Free models" with no free model on it; the fix is not better inventory, it is
copy that survives an empty rung.

Every rung states its cost in its own currency before it is chosen: rung 0 its
rate, rung 1 the minutes of signup, rung 2 the gigabytes.

## 3. Agent mode on rung 0: allowed, with the clock named

Switching a Nexus model into Agent mode is **permitted and priced, never
silently slow and never refused**. Before the first turn, the mode shows the
arithmetic:

> This source allows 2 requests per minute. A task taking 12 steps will take
> about 6 minutes.

On rung 0, **parallel agents and delegation depth are forced to 1**. The cap is
per IP, so delegation multiplies calls against a shared ceiling and turns six
minutes into thirty.

A source that cannot call tools at all leaves the candidate set for a tools
turn, which is [#4](https://github.com/elpideus/demido-studio/issues/4) rule 4
and removes AI Horde from agent work entirely. This section is about the source
that passes that filter and is merely slow.

Rejected: hard-refusing Agent mode on rung 0. Watching the agent loop actually
run is the demonstration that sells the product, and a user who has been told
the clock is not being deceived by it.

## 4. Rung 1 is offered at the point of pain, never before

The keyed free tier (Groq, Cerebras, Google AI Studio and similar) is real
capacity with tools and no card, but pasting a key is precisely the chore the
brief exists to abolish:

Brief B56:

> I need to create apps on developer dashboards, provide API keys, etc.

So rung 1 is **never volunteered on first run**. It appears as one inline,
permanently dismissible row at the moment the user is holding the problem it
solves: a rung 0 walk exhausted, or a rate limit hit twice in one session.

> Groq gives you a faster free key in about two minutes. No card.

with the link beside it. It remains available in Settings for anyone who goes
looking, and it is never a modal.

This is the rung [#4](https://github.com/elpideus/demido-studio/issues/4) warns
has already been deleted once for having no reason to exist. Its reason is this
moment.

## 5. A source dies quietly, and the user finds out honestly

No probing on launch. Startup never blocks, and a network call to a volunteer
commons on every start buys information the user usually does not need.

- The Nexus panel shows **`last answered <date>`** per source, written by real
  traffic rather than by a probe.
- A failed turn updates that date and says what happened.
- An **exhausted walk is a message, not an error**, naming what was tried and
  offering rungs 1 and 2 as buttons.
- Rot is a release gate, not a runtime one: a build whose `verified_on` is more
  than 90 days old fails to release
  ([#4](https://github.com/elpideus/demido-studio/issues/4) rule 6).

## 6. AI Horde is a commons, and Demido says who it is

AI Horde runs on volunteer GPUs and a kudos economy that anonymous callers do
not feed.

- **Required now**: every request carries
  `Client-Agent: DemidoStudio:<version>:<contact>`. v2 sent nothing, so Demido
  presented itself to a volunteer commons as `unknown:0:unknown`.
- **Offered**: one row in the Nexus panel to link an existing Horde account, so
  a user's own kudos raise their own priority.
- **Never**: Demido does not run a worker, does not contribute the user's GPU,
  and does not make an account a condition of anything.

## 7. The boundary with the first-launch wizard

This file owns **the offer**: which rungs appear on first run, in what order,
and in what words. Ticket
[#21](https://github.com/elpideus/demido-studio/issues/21) owns **the
mechanics** of the wizard that renders it:

Brief B11: "Guided set-up on first launch"

That is blocking or not, hardware detection, download sequencing, and what is
per profile rather than per machine. It inherits the offer above rather than
re-deciding it. Two tickets writing one screen is how v2's shell came to be
rebuilt three times in three days.

**Settled, in [`setup.md`](setup.md).** The wizard is front and centre on first
launch and every step has a way out; a user who leaves lands on the desk with
rung 0 already answering, which is the arrangement §1 above describes. Nexus is
what makes leaving survivable, so this file's offer and that file's door are one
mechanism seen from two sides.
