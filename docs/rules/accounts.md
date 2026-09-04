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

- Whether mail bodies may leave the machine. Google's Limited Use terms allow
  the data only for prominent user-facing features and forbid transfer to a
  third party, and sending a message to a remote model is a transfer while a
  local model is not. That is
  [#26](https://github.com/elpideus/demido-studio/issues/26).
- Which account answers when a profile has several, beyond the brief's own
  rule that it is the default one unless the user names another.
