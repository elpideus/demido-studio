# Connecting an account, and what setup Demido may promise

What a user has to do before Demido can read their mail, their calendar or their
market data, what the UI is allowed to claim about that, and why Google costs
more than everything else.

Decided on wayfinder ticket
[#17](https://github.com/elpideus/demido-studio/issues/17), on the research in
[#5](https://github.com/elpideus/demido-studio/issues/5).

## The promise

Brief B02: "a harness that integrates all the tools I need seamlessly"

The brief's complaint is the sentence before that one:

Brief B56:

> I need to create apps on developer dashboards, provide API keys, etc.

Demido cannot keep that promise for Google, at any price worth paying. What it
keeps instead is a promise for **two audiences**, because Demido has two:

> **No setup on the common path, and every credential visible and replaceable
> underneath.**

The first half is for the person who wants a companion on their PC and should
never meet the word *client id*. The second half is for the person who wants the
whole pipeline in their hands, and it is the same half that serves a fork, a
locked-down Workspace account, and anyone who does not want to trust a
credential somebody else registered.

The promise is **not** "zero setup". It is also not "setup once, guided", which
was the first draft of this line and undersells the common path: for most
providers there is no setup at all beyond a password the user already has.

## The word

[`profiles.md`](profiles.md) split **profile** (a person on this machine) from
**account** (a login at somebody else's service). Two more, both from the shape
[#5](https://github.com/elpideus/demido-studio/issues/5) found:

- **Default path.** What a first-time user is shown for an account type. One
  path per type, chosen because it is the cheapest thing that works.
- **Own-credentials path.** The same account type, reached through a
  disclosure, taking a credential the user registered themselves.

Every credential Demido ever holds is one of three **kinds**: a client the user
registered, a static secret the user generated, or a session Demido captured in
a browser window. Nothing else. A provider is a tile.

Three more, from [#26](https://github.com/elpideus/demido-studio/issues/26):

- **Sensitivity class.** What an account's data is, `personal` or
  `operational`. A property of the account, never of a tool and never of a chat.
- **Marked.** An event that carries the result of a `personal` account, or is
  derived from one.
- **Endpoint class.** Where a model runs and what that place is permitted to do
  with what it is sent. Not the same question as local against remote.

## What each account costs the user

| Account | Default path | What the user actually does | Own-credentials path |
|---|---|---|---|
| Gmail, mail only | App password over IMAP and SMTP | Turns on 2-Step Verification, generates an app password, pastes it | Their own OAuth client |
| Google Calendar | Their own OAuth client | About ten guided minutes in the Google Cloud console | The same, it is the only path |
| Google Contacts | Their own OAuth client | The same trip, reusing the same client | The same, it is the only path |
| Generic IMAP and SMTP | Host, user, password | What every mail client asks for | n/a |
| Generic CalDAV and CardDAV | URL, user, password | The same | n/a |
| Exchange market data | Read-only API key | Generates a read-only key at the exchange | n/a |
| TradingView | Captured session | Logs in, in a Demido window, once | n/a |

Two things follow from that table and both are load bearing.

**Mail is one click and calendar is not**, on the same Google account, and the
UI has to say so without sounding broken. Google's CalDAV server "refuses to
authenticate a request unless it arrives over HTTPS with OAuth 2.0
authentication", answering `401` to Basic auth, and "Google's CardDAV interface
requires OAuth 2.0" with no other method supported. So an app password, which
solves mail outright, buys nothing at all for the other two thirds of

Brief B30: "Email, Calendar and Contacts implementation"

**Google is the hard case, not the shape of the feature.** The contract the
mail, calendar and contacts tiles implement is written from the protocol side,
IMAP and SMTP and CalDAV and CardDAV, and Google is one provider behind it. It
ships first because Stefan uses it daily and v1's working PKCE code is inherited
([#3](https://github.com/elpideus/demido-studio/issues/3)), but the hardest
provider does not get to define the interface. Per
[`tiles.md`](tiles.md), the contract suite exists before the second
implementation, and here the second implementation is a generic CalDAV host,
which is also the one that needs no setup at all.

## Why Demido ships no OAuth client of its own

A Demido-registered Google client would give exactly the experience users
expect, the one Mailspring gives: a "Sign in with Google" button, the system
browser, done. It is not shipped, and the reasons are worth writing down because
they will be re-argued otherwise.

- An **unverified** published client is capped at **100 new Google accounts,
  for the lifetime of the project**, and the quota cannot be reset or changed.
  Stefan alone has several accounts.
- Leaving it in Testing status instead is worse: consent and refresh tokens
  expire after **seven days**.
- **Verification needs a domain.** Google requires a top private domain the
  publisher owns and has verified in Search Console, hosting both a public
  homepage and a privacy policy. A `github.io` address does not qualify:
  Google does not treat GitHub Pages as a first-party domain even though Search
  Console will verify it. There is no free path.
- Gmail's read and modify scopes are **restricted**, which adds an annual
  third-party security assessment on top, at roughly **US$1,000 to $4,500 a
  year, recurring**. Calendar and Contacts are only **sensitive**, which needs
  the domain, the homepage, the privacy policy, a demo video and per-scope
  justifications, but no assessment and no annual fee.
- A hosted broker, the other half of how Mailspring does it, makes the operator
  a party to other people's mail. That is the explicit trigger for the annual
  assessment, and it puts a server in a desktop app that should have none.

So the cheap two thirds are cheap only relative to the expensive third: they
still cost a domain Demido does not have. The decision is not "verification is
never worth it", it is "not yet", and the condition is written down below.

## The rules

1. **Every account type has both paths**, and the own-credentials path is never
   removed. It is what a fork uses, what a Workspace account with app passwords
   disabled uses, and what everyone falls back to if a shipped credential ever
   lapses.
2. **Nothing in the accounts subsystem assumes Demido is a registered party
   anywhere.** No code path is conditional on a Demido-owned client existing.
3. **If a shipped client ever exists it is a pre-filled default in the same
   field**, never a second code path. That is what makes the trigger below a
   configuration change rather than a rewrite.
4. **The guided walkthrough is part of the account type**, not a separate
   feature, and it never asks twice. An account that is already connected does
   not show its wizard again.
5. **The known failure modes are detected, not documented.** A client left in
   Testing status loses its tokens after seven days: the connect flow warns
   about it and the refresh path recognises it rather than reporting a generic
   auth error. An account requesting readonly scopes while the tool layer
   performs writes is the defect v1 shipped
   ([#3](https://github.com/elpideus/demido-studio/issues/3)) and it is fixed on
   the way in, not inherited.
6. **Cost is stated before the work starts**, in the same spirit as a runtime
   download naming its size first: an account type that needs ten minutes in a
   console says so on the button, not after the click.

## What the UI may say

May say: *set up once, in one place*. *Demido never asks you for this twice.*
*This one needs about ten minutes and a Google Cloud account, here is the
walkthrough.*

May not say: *no setup*. *zero configuration*. *just sign in* on any surface
that then opens a console walkthrough. The promise above is the ceiling on
marketing copy as much as on button labels, and a screen that overclaims is a
defect, reviewable like any other.

Every connect surface names which of the two paths the user is on. A user who
pasted their own client id is entitled to know that is what happened.

## Where account data is allowed to go

Decided on wayfinder ticket
[#26](https://github.com/elpideus/demido-studio/issues/26).

Brief B30: "Email, Calendar and Contacts implementation"

Brief: silent

The brief asks for the integration and asks for transparency about everything
the model sees. It never says where the data may then travel, which is the
question a harness has to answer before it reads anybody's mail.

### What the policy actually forbids

Three findings, because the obvious reading of Google's terms is wrong in a way
that matters.

**Transfer is not flatly prohibited.** The
[API Services User Data Policy](https://developers.google.com/terms/api-services-user-data-policy)
prohibits "Transferring or selling user data to third parties like advertising
platforms, data brokers, or any information resellers". An inference provider is
not one of those. The clause with teeth is the
[Workspace policy](https://developers.google.com/workspace/workspace-api-user-data-developer-policy):
Google user data may not be used to "create, train, or improve a machine
learning or artificial intelligence model, including foundational models", nor
"be stored in conjunction with foundational models", and is permitted "only for
a personalized model to execute a user-facing feature".

**Human review is a separate prohibition.** A human may read the data only where
the developer "first obtained the user's affirmative agreement to view specific
messages, files, or other data", or for security. No user consent reaches a
stranger at the other end of an anonymous endpoint.

**The policy does not govern the default path.** It addresses OAuth API access
and says nothing about IMAP or SMTP with an app password, which is exactly the
path [#17](https://github.com/elpideus/demido-studio/issues/17) chose for Gmail.
The same rule binds both transports anyway, because the policy is a proxy for
the real question, which is the user's correspondence leaving their machine. A
rule that binds the credential the user registered and exempts the one Google
handed them is a rule about paperwork, and it would exempt the path most users
are on.

So the boundary is **not local against remote**. It is whether the receiving
endpoint retains what it is sent, and whether a person at the other end can read
it. A local model is simply the case where both answers are no.

### The mark

An **account** carries a **sensitivity class**, `personal` or `operational`. A
new account tile defaults to `personal`, so a provider nobody has classified
fails safe rather than fails open.

A **tool result** carries the id of the account it came from. A result from a
`personal` account is **marked**. The mark **propagates to anything derived from
it**: a model turn produced while a marked event was in the window is marked, and
so is a compaction summary built from marked events.

The mark is a property of an **event**. The gate is a property of an
**assembly**, recomputed at every dispatch from what is actually in the window.
Pruning therefore lifts the mark honestly, and nothing lifts it dishonestly.

Rejected: marking the **tool**, which marks every turn that ever offered
`mail_search` whether or not it was called. Rejected: marking the **account**,
which marks the whole chat for the same reason.

The result of a search with no hits is still marked, and that is deliberate
over-approximation. Demido does not read a payload to decide whether it contains
anything personal, because a rule that inspects content is a rule that is wrong
quietly. The account a result came from is a fact; what is in it is a judgement.

### Endpoint classes

Marked data is allowed to reach an endpoint according to one field on the
provider entry, `training`, shipped populated for the known providers, each value
carrying a link to the clause it rests on.

| Endpoint | Marked data |
|---|---|
| Local weights on this machine | Allowed, silent |
| Keyed provider, `training: bars` | Allowed, recorded |
| Keyed provider, `training: unknown` | Asked once for that provider, the answer recorded |
| Keyed provider, `training: does-not` | Refused |
| Keyless, Nexus rung 0 | Refused |

`unknown` **asks rather than refuses**, because a custom OpenAI-compatible base
URL is the user's own server on their own network as often as it is a stranger's,
and refusing that is simply wrong. Allowing it silently is also wrong. This is
rule 6 above applied to a provider instead of an account: the cost is stated
before the work starts.

Nexus rung 0 is **refused and not asked**. Nothing in either keyless source's
terms bars retention, and AI Horde dispatches to anonymous volunteer workers who
can read the prompt, which is the human review prohibition rather than the
training one. [#18](https://github.com/elpideus/demido-studio/issues/18) already
made terms a routing gate rather than a badge; this is that gate carrying one
more input.

### Enforced in two places

1. **The offered set.** An account-backed tool on a `personal` account is **not
   offered** when the conversation model sits on a refused endpoint
   ([`tools.md`](tools.md)). This prevents the dead end rather than producing
   one: a user on Nexus never gets mail into a context that then cannot be sent
   anywhere.
2. **The dispatch gate.** This is the actual rule, and it exists because the
   first one is not sufficient: the user can change models mid-chat, and
   [`tools.md`](tools.md)'s rule that a sub-agent inherits its parent's set and
   can only narrow it has to hold for the gate too.

No fifth permission axis. `Network` already means anything reaching an account.

### The task model is bound by the same gate

All four jobs in [`tasks.md`](tasks.md) read conversation content. Naming a chat
sends the first exchange somewhere. So a job dispatches only if the endpoint it
would run on passes the gate for the assembly that job carries.

A job **may always run on the conversation model's own endpoint**, which has by
definition already received the assembly. That keeps the one `blocking` job,
compression, from ever being blocked by this rule: it falls back rather than
stalling the turn. Every other job is `deferred` and simply does not run, which
`tasks.md` already requires a caller to survive.

### What the log records

Three fields, and no judgement anywhere:

- a **tool-result** event carries the **account id** it came from;
- an **assembly-dispatch** event carries the destination's **endpoint class**
  and the **gate verdict**.

Brief B07: "recorded in an append-only session log"

That makes "did any Google data ever reach a remote endpoint" a **query over the
log** rather than an audit of it, and it composes with
[#8](https://github.com/elpideus/demido-studio/issues/8)'s requirement that the
log can rebuild any assembly: the gate is recomputable after the fact from the
same events. Per
[#22](https://github.com/elpideus/demido-studio/issues/22)'s discipline these are
facts written at the time, never judgements written later.

### There is no override

A refusal offers no confirmation dialog. The tool is absent, the line says which
models can see the data, and the switch is beside it. Refuse with a door, not a
confirm.

[#23](https://github.com/elpideus/demido-studio/issues/23) already argued the
shape: a gate that is always answered *yes* is the feature quietly not working.
What settles it here is irreversibility. A wrong lesson is priced at one bad
hint; a mail body handed to an anonymous worker cannot be recalled.

This is the one place in the design where Demido overrules the user, and it is
worth writing down as such rather than gliding past it. The user is the
registered party and may consent to a transfer, but they cannot consent on
Google's behalf to training, and they cannot consent for the stranger who reads
the prompt.

### None of this is Google-specific

The mechanism is one field on an account and one field on a provider. An
`operational` account, a read-only exchange key returning candles, is not marked
and meets no gate. If a provider ever needs the stricter treatment it changes
class, which is a value rather than a code path.

Brief B49: "Any other accounts added in the future."

A third sensitivity class costs nothing to add and inventing one now would be the
over-engineering Brief B08 warns about.

The shape of the mark and the reason there is no override are
[`docs/decisions/0003-account-data-carries-a-mark.md`](../decisions/0003-account-data-carries-a-mark.md).

## When this gets reopened

**When Demido reaches 20 to 50 stars on GitHub and a domain is affordable**,
reopen the shipped-client question, sensitive scopes first: a verified Demido
client covering Calendar and Contacts, which needs the domain but no annual
assessment, and would move two of the three services to one click. Gmail comes
after that and only if the restricted-scope assessment can be avoided, which
turns on a question nobody has answered yet: the requirement names an app that
"has the ability to access data from or through a third-party server", and
Demido has no server, but no Google documentation says whether a purely local
desktop app is therefore exempt. That question is worth asking Google directly
before any money is spent, and it is worthless to ask before the domain exists,
because restricted verification needs the domain either way.

Until then, rule 2 keeps the door open at no structural cost.

## What this does not decide

- Which account answers when a profile has several, beyond the brief's own
  rule that it is the default one unless the user names another.
