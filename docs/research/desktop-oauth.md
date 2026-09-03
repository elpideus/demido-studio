# Desktop OAuth for a GPL-3.0 app

Research for issue #5. Resolves the blocker on the accounts / mail / calendar / contacts subsystem
described in `docs/brief.md` ("Accounts management system for things like: Google Account OAuth for
things like Email, Calendar and Contacts integration" and "Email, Calendar and Contacts
implementation + tools provided to LLMs to use them (with multi-account support)").

All claims below are cited to primary sources: Google's current OAuth 2.0 and API Services User Data
Policy documentation, Google's product API docs, and RFC 8252. Cost figures that Google does not
publish are marked explicitly as third-party estimates.

Date of research: 2026-09-03.

---

## Recommendation, in one paragraph

**Ship a bring-your-own-credentials OAuth flow.** Demido Studio implements the RFC 8252 installed-app
flow — PKCE, loopback redirect, system browser — but does **not** ship an OAuth client. Each user
creates their own Google Cloud project and desktop OAuth client, and pastes the client ID and client
secret into the Accounts panel. A `wizard` walks them through it. This is the only option that
survives contact with the facts: it costs Stefan nothing, has no user cap, needs no verification, no
CASA assessment, no privacy policy on a domain he owns, and no annual recertification. It also fits
GPL-3.0 honestly: there is no secret in the repo because there is no shared client at all.

**Plain IMAP/SMTP with app passwords is not the pragmatic answer for Google**, and it is important to
say why rather than assume it: Google's CalDAV and CardDAV endpoints require OAuth 2.0 and reject
Basic authentication outright (§4 below). App passwords would cover mail and nothing else — calendar
and contacts, both named in the brief, would still need OAuth. App passwords remain the right
mechanism for the *generic SMTP/IMAP* account type in the brief, and as a mail-only fallback.

---

## 1. The installed-app flow: what Google actually requires

### 1.1 The client secret exists, and Google says plainly that it is not a secret

Google's OAuth 2.0 overview, describing the Installed-application flow:

> The process results in a client ID and, in some cases, a client secret, **which you embed in the
> source code of your application. (In this context, the client secret is obviously not treated as a
> secret.)**
>
> — <https://developers.google.com/identity/protocols/oauth2>

In the desktop-app guide the `client_secret` parameter on the token exchange is documented as
**"(Optional)"**, and Google notes the secret "is not applicable to requests from clients registered
as Android, iOS, or Chrome applications." Google also states that installed applications "cannot keep
secrets" because they are "distributed to individual devices."
(<https://developers.google.com/identity/protocols/oauth2/native-app>)

This matches RFC 8252 §8.5, which classifies native apps as public clients and states that "secrets
that are statically included as part of an app distributed to multiple users should not be treated as
confidential secrets."
(<https://datatracker.ietf.org/doc/html/rfc8252>)

**So: publishing the client secret in the GPL-3.0 repo is not a policy violation.** The problem with
a shipped client is not confidentiality — it is the 100-user cap and the verification bill attached
to that one client identity (§2, §3). Anyone who reads the repo can also impersonate the app to
Google's consent screen, and any abuse from a forked binary lands on Stefan's project.

Two operational notes on the secret: since June 2025 Google shows the full client secret **only once,
at creation time**, and masks it afterwards — it must be downloaded then or the client re-created
(<https://developers.googleblog.com/usability-and-safety-updates-to-google-auth-platform/>,
<https://support.google.com/cloud/answer/15549257>). And OAuth clients unused for 6 months are
automatically deleted, with a 30-day restore window — relevant for a user who sets up an account and
then leaves it idle.

### 1.2 PKCE and loopback are the required shape

- RFC 8252 §8.1: "Public native app clients **MUST** implement the Proof Key for Code Exchange
  (PKCE) ... and authorization servers **MUST** support PKCE for such clients."
- RFC 8252 §7.3 / §8.3: for loopback redirects the server "MUST allow any port to be specified at the
  time of the request," so the app binds an ephemeral port rather than a fixed one.
- RFC 8252 §8.12: "native apps MUST use an external user-agent to perform OAuth authorization
  requests" — the system browser, never an embedded webview. This matters for Demido Studio because
  the brief specifies an "Integrated basic Web Browser"; that browser must **not** be used for the
  Google consent screen.
- Google describes PKCE as supported and recommended for the installed-app flow, with a verifier of
  43–128 characters (<https://developers.google.com/identity/protocols/oauth2/native-app>). Use S256.

Two Google-side deprecations to respect:

- **Custom URI schemes are gone** on desktop: "Custom URI schemes are no longer supported due to the
  risk of app impersonation."
- **The out-of-band copy/paste flow is gone**: "The manual copy/paste option, also referred to as an
  out of band (OOB) redirect method, is no longer supported."

Loopback (`http://127.0.0.1:<ephemeral port>`) is the only supported desktop redirect. That is what
we implement.

### 1.3 Scope classification for what the brief needs

| Need | Scope | Class |
|---|---|---|
| Read mail | `gmail.readonly` | **Restricted** |
| Read + label + send mail | `gmail.modify` | **Restricted** |
| Full mailbox incl. permanent delete | `https://mail.google.com/` | **Restricted** |
| Send only | `gmail.send` | Sensitive |
| Calendar read/write | `calendar`, `calendar.events` | Sensitive |
| Contacts read/write | `contacts` | Sensitive |

Gmail classification is from Google's own table at
<https://developers.google.com/workspace/gmail/api/auth/scopes>, which lists `mail.google.com`,
`gmail.readonly`, `gmail.compose`, `gmail.insert`, `gmail.modify`, `gmail.metadata`,
`gmail.settings.basic` and `gmail.settings.sharing` as **restricted**, and `gmail.send` plus the
add-on message scopes as **sensitive**. Calendar and Contacts scopes are sensitive, not restricted;
restricted scopes are the smaller set enumerated per-product (Gmail, Drive, and a few others)
(<https://support.google.com/cloud/answer/13463817>).

The consequence is unavoidable: **the brief's "read and send emails" requirement lands in restricted
scope territory**, which is the tier that triggers the CASA security assessment (§3). There is no
narrower Gmail scope that lets you read a message body. `gmail.send` alone (sensitive, no CASA) would
give a send-only mail integration; reading the inbox cannot avoid restricted.

---

## 2. Verification and the unverified-app screen

### 2.1 The two publishing states, and their caps

From <https://support.google.com/cloud/answer/15549945>:

- **Testing**: "limited to up to 100 test users listed in the OAuth consent screen." Crucially,
  "Authorizations by a test user will expire seven days from the time of consent," and when using
  offline access "that token will also expire" — i.e. **refresh tokens die after 7 days in Testing
  mode**. A harness that has to re-prompt every user for consent weekly is unusable. Testing mode is
  a development state, not a shipping state.
- **In production, unverified**: Google "will display an Unverified apps warning message if your
  project's OAuth clients request authorization of scopes considered sensitive or restricted." The
  app is capped at **100 new users in total** after the unverified screen appears, and that quota
  "cannot be reset or changed." Refresh tokens do not carry the 7-day expiry.

So an unverified but *published* client works indefinitely for up to 100 Google accounts. Past that,
Google's FAQ says users hit "the Unverified App Screen or 'Sign-in with Google temporarily disabled'"
(<https://support.google.com/cloud/answer/13463817>).

**100 accounts is the ceiling on any shared-client design.** Stefan alone has "multiple Google
accounts"; a public GPL harness would burn through 100 in days and then be permanently broken for
everyone else, with no reset.

### 2.2 When verification is not needed

Google lists explicit exceptions (<https://support.google.com/cloud/answer/13464323>):

1. **Personal-use apps** — "If the app is for your personal use (fewer than 100 users)." The
   unverified warning still shows; the user clicks through Advanced.
2. Development / testing / staging apps.
3. Apps using a service account against their own data only.
4. **Internal-only** apps, restricted to one Google Workspace or Cloud Identity organisation.
5. Workspace-admin-trusted or Marketplace-installed apps.

Exception 1 is exactly what a bring-your-own-credentials design produces: every user's client is a
personal-use client with one user. This is the load-bearing fact behind the recommendation.

All apps, verified or not, still must comply with the API Services User Data Policy
(<https://developers.google.com/terms/api-services-user-data-policy>).

### 2.3 What Stefan would personally have to do to get verified

Two separate reviews, in order.

**a) Brand verification** (<https://developers.google.com/identity/protocols/oauth2/production-readiness/brand-verification>)

- App must be External + Published.
- **Verify domain ownership via Google Search Console** — so a real domain Stefan controls.
- Accurate app name, logo, privacy policy URL, terms of service URL.
- Automated check "typically takes a few minutes"; manual review "usually takes 2-3 business days."
- "You must have a published branding status before you can request verification for data access."

**b) Sensitive / restricted scope verification**
(<https://developers.google.com/identity/protocols/oauth2/production-readiness/sensitive-scope-verification>)

- **Privacy policy** that "discloses the manner in which your application accesses, uses, stores, or
  shares Google user data," **hosted on the same domain as the app homepage**, linked from the
  consent screen.
- **Homepage**: publicly accessible, not login-gated, clearly relevant to the app; a GitHub repo page
  is borderline and a Play Store / social link is explicitly not acceptable.
- **Demo video**: unlisted YouTube, in English, showing the OAuth consent flow, the correct app name
  on the consent screen, **the OAuth client ID visible in the browser address bar**, and how each
  sensitive scope is actually used in the app.
- **Per-scope justification** explaining why a narrower scope will not do.
- Google states sensitive-scope review "typically takes 3-5 business days," while the restricted-scope
  page says the overall process "can potentially take several weeks to complete"
  (<https://developers.google.com/identity/protocols/oauth2/production-readiness/restricted-scope-verification>).

Concretely, for Stefan: buy and hold a domain, stand up a real homepage and a privacy policy on it,
verify it in Search Console, record and narrate a screencast in English of Demido Studio reading
Gmail / editing Calendar / editing Contacts, write scope justifications, and then wait — days if it
goes cleanly, weeks if Google comes back with questions. Every time the app's scopes change, the
approved app changes and re-review is required.

---

## 3. The CASA security assessment (restricted scopes)

Because the brief needs to *read* mail, Gmail restricted scopes apply, and restricted scopes add a
third gate on top of §2.

From <https://developers.google.com/identity/protocols/oauth2/production-readiness/restricted-scope-verification>:

- Apps must use the **App Defense Alliance** Cloud Application Security Assessment (**CASA**)
  framework. It applies to "every app that requests access to Google users' restricted data and has
  the ability to access data from or through a third-party server."
- "Apps must be reverified for compliance and complete a security assessment **at least every 12
  months** after your assessor's Letter of Assessment (LOA) approval date."

From <https://support.google.com/cloud/answer/13465431>: assessment is "a risk-based, multi-tiered
approach" with assurance levels **AL1 and AL2**, assigned by user count, requested scopes and other
signals; the level "is dynamic and may increase based on changes in your user base"; it is "the final
step of the restricted scopes review process."

From the User Data Policy: "applications must pass an annual security assessment and obtain a Letter
of Assessment from a Google-designated third party."

**Cost.** Google does not publish a price and does not charge one itself; the assessment is performed
by independent CASA-authorised assessors whom the developer pays directly. Google's own pages carry
no figure, so the following is **third-party estimate, not a primary source**: published assessor
pricing in 2025–2026 clusters around **US$900–1,500 for a lab-validated Tier 2 scan and up to roughly
US$4,500 for higher-assurance work**, recurring annually
(<https://www.switchlabs.dev/post/casa-tier-2-tier-3-security-review-providers-pricing-and-the-cheapest-option>,
<https://deepstrike.io/blog/google-casa-security-assessment-2025>). Treat these as an order of
magnitude to be confirmed with an assessor, not as a quote.

**Total honest cost of the "one verified Demido Studio app" path, per year, out of Stefan's pocket:**
domain and hosting (~US$15–50/yr) + roughly US$1,000–4,500/yr of CASA assessment + the unpaid labour
of a homepage, a privacy policy, a demo video, scope justifications, and an annual recertification
cycle — before a single user is served, and repeated forever.

**And it does not even hold up for a GPL app.** A fork that rebuilds the binary with the same client
ID is, from Google's side, the verified app; from a policy side, Stefan has attested to data-handling
practices for code he no longer controls. The verified-single-client model assumes one publisher of
one binary. That assumption is false for GPL-3.0 software.

### 3.1 Limited Use, which applies regardless

The API Services User Data Policy's Limited Use rules bind us whichever path we take: data may only
be used "for providing or improving user-facing features that are prominent in the requesting
application's user interface"; transfers to third parties are prohibited except for narrow listed
cases; selling data or using it for ads is prohibited.

This has a real design consequence for an LLM harness. **Sending a user's Gmail body text to a remote
model provider is a transfer of Google user data to a third party.** Local models (the brief's
primary target — Qwen, Gemma, GPT-OSS running on the user's machine) do not transfer anything. The
Nexus free-model router and any cloud model do. Demido Studio must therefore:

- keep mail/calendar/contacts tool output out of prompts sent to remote providers unless the user has
  consented for that account, with a clear per-account setting;
- never use this data to train or improve anything;
- surface the consent in the UI, not bury it.

This is worth a ticket of its own on the accounts subsystem spec.

---

## 4. The alternatives, judged

### 4.1 Bring-your-own-credentials — **RECOMMENDED**

Each user creates a Google Cloud project, enables the Gmail / Calendar / People APIs, creates a
**Desktop app** OAuth client, and pastes the client ID + secret into Demido Studio. Demido Studio
runs the PKCE + loopback flow against those credentials.

- **Verification**: not needed. Personal-use exception, §2.2 item 1. The user clicks through their
  own unverified-app screen for their own client.
- **Caps**: the 100-user cap is per client, and each client has one user. Effectively no cap.
- **Refresh tokens**: publish the client to "In production" and the 7-day Testing expiry does not
  apply. **The wizard must tell the user to do this** — leaving it in Testing is the single most
  likely way for a user's account to silently break after a week.
- **Cost to Stefan**: zero. Ongoing: zero.
- **Cost to the user**: a Google Cloud account (free tier, no billing card needed for these APIs) and
  roughly 10 minutes of dashboard clicking, once per Google identity they want to connect.
- **Quota**: each user gets their own per-project API quota rather than sharing one project's — a
  genuine advantage, since a shared client would throttle everyone.
- **GPL fit**: perfect. Nothing secret in the repo; forks inherit the same model.
- **Cost**: the onboarding friction. This is exactly the friction the brief complains about — "I have
  to go around looking for MCPs, Skills, Plugins, then manually install and configure them one by
  one. I need to create apps on developer dashboards, provide API keys." That objection is
  acknowledged and answered by making the wizard excellent, not by paying US$4,500/yr for a shared
  client that caps at 100 users anyway.

### 4.2 Ship a shared client in the repo — **REJECTED**

Legal and policy-clean per §1.1, and technically it works. But: hard-capped at 100 Google accounts
ever, unresettable; drags in the entire §2/§3 cost stack if you try to lift the cap; any fork
inherits Stefan's app identity and his compliance attestation; abuse by one fork revokes the client
for everybody. Not viable.

### 4.3 A Stefan-hosted broker holding the secret — **REJECTED**

Keeps the secret off the client, but: it makes Stefan the operator of a service that handles other
people's Gmail, which is precisely the "ability to access data from or through a third-party server"
that triggers CASA; it needs hosting, uptime, and an incident-response story; it is a permanent
personal liability and running cost for a desktop app that should have no server; and it contradicts
the brief's local-first, no-server posture. Worse on every axis than 4.1.

### 4.4 App passwords with IMAP/SMTP + CalDAV/CardDAV — **PARTIALLY REJECTED, and this is the
important finding**

The tempting v1 shortcut does not work for Google, for a specific documented reason:

- **CalDAV**: "The CalDAV server refuses to authenticate a request unless it arrives over HTTPS with
  OAuth 2.0 authentication of a Google Account. Attempting to connect over HTTP or using Basic
  Authentication results in an HTTP `401 Unauthorized` status code."
  (<https://developers.google.com/workspace/calendar/caldav/v2/guide>)
- **CardDAV**: "Google's CardDAV interface requires OAuth 2.0." "Google does not support any other
  authentication method." Basic auth yields 401.
  (<https://developers.google.com/people/carddav>)

So calendar and contacts — two of the three things the brief asks for — cannot be done with app
passwords at all. There is no app-password path to Google Calendar or Google Contacts.

Mail is different. App passwords still exist, still work for Gmail IMAP/SMTP, and require only that
the account has 2-Step Verification on. Google's page says they "aren't recommended and are
unnecessary in most cases" but has not deprecated them; they are **unavailable** for accounts using
security-keys-only 2SV, accounts under Advanced Protection, and work/school/organisational accounts
(<https://support.google.com/accounts/answer/185833>). Separately, plain-password "less secure apps"
access was turned off for all Google accounts on **March 14, 2025**, affecting CalDAV, CardDAV, IMAP,
SMTP and POP — app passwords survived that shutdown, plain passwords did not
(<https://knowledge.workspace.google.com/admin/sync/transition-from-less-secure-apps-to-oauth>).

**Verdict**: app passwords are not the v1 answer for Google, because they solve one third of the
requirement. They *are* the correct mechanism for the brief's "simple SMTP" account type, and a
reasonable optional fallback for Gmail-mail-only for users who refuse the Cloud Console. The account
model should therefore support both credential kinds per provider rather than assuming OAuth
everywhere. Note the Workspace exclusion: many of Stefan's likely users on work accounts cannot use
app passwords at all.

---

## 5. Does this generalise to the other services in the brief?

| Service | Answer | Notes |
|---|---|---|
| **Google (Gmail/Calendar/Contacts)** | BYO OAuth client, PKCE + loopback | §4.1 |
| **Generic SMTP/IMAP** | User-supplied host, port, username, password/app password | No OAuth, no registration, no cap. Straightforward. |
| **Zoom** | BYO OAuth client, PKCE, same flow shape | Zoom explicitly supports the public-client PKCE flow: "Use PKCE when you don't have a backend server for user authorization" and "Unlike the confidential client flow, PKCE does not use an `Authorization` header" — i.e. no client secret at token exchange. Public distribution requires Marketplace review; a user-created app for their own account avoids it. (<https://developers.zoom.us/docs/integrations/oauth/>) |
| **Bybit / Binance / KuCoin** | User-generated API key + secret, entered in Accounts | No OAuth, no app registration by Stefan, no verification. Same credential-vault storage as everything else. Read-only key permissions should be enforced/encouraged, since the brief says "No market operations initially, just market data." |
| **TradingView** | Cookie capture from a real login window | The brief already specifies this: a window opens on the TradingView login page and Demido Studio captures the cookies Tradingview-API needs. This is the one place an embedded browser is correct — there is no OAuth to violate RFC 8252 over. It is also fragile by nature and should be isolated behind the same account-provider interface so it can be replaced without touching anything else. |

**The generalisation holds.** Every provider reduces to: *the user supplies their own credential, of
one of three kinds — an OAuth client the user registered, a static secret the user generated, or a
captured session.* That is the seam the accounts subsystem should be built on. Nothing in the design
should assume Demido Studio itself is a registered party anywhere.

This is also the right answer for the brief's modularity goal ("a codebase that feels like a server
rack, a NAS or a drop-ceiling"): a provider is a tile. Adding Fastmail, Microsoft 365 or Proton later
means adding a tile, not changing the auth core.

---

## 6. What this means for the accounts subsystem spec

1. One credential-vault abstraction, three credential kinds: `oauth_byo` (client id + secret + tokens),
   `static_secret` (username/password, API key/secret), `captured_session` (cookies). OS-keychain
   backed, never plaintext on disk.
2. One RFC 8252 OAuth engine shared by Google and Zoom: system browser, ephemeral loopback port,
   PKCE S256, `state` checked, no embedded webview.
3. Per-account provider config, so multi-account works by construction — the brief requires "LLMs
   should use the default account (the first one added or configured from the settings for each of
   the services), unless user doesn't explicitly specify another."
4. Token refresh with graceful re-consent: refresh tokens can be revoked at any time
   (<https://developers.google.com/identity/protocols/oauth2/policies>), and a user who left their
   client in Testing mode will break on day 7. Detect and explain, do not just fail.
5. A Limited Use boundary in the tool layer: mail/calendar/contact content must not leave the machine
   to a remote model provider without explicit per-account consent (§3.1).
6. Scope minimisation: request `gmail.modify` rather than `https://mail.google.com/` (no permanent
   delete), `calendar` and `contacts`, and request them lazily per feature rather than all at first
   launch.

---

## 7. Steps only Stefan or the user can perform on a dashboard

These belong in a follow-up ticket that turns them into a `wizard`. For **each** Google account the
user connects:

1. Go to the Google Cloud Console and create a project (or reuse one).
2. Enable the **Gmail API**, **Google Calendar API**, and **People API** on it.
3. Configure the Google Auth Platform branding: app name, user support email, developer email.
4. Set audience to **External**.
5. Add the scopes the user wants Demido Studio to have.
6. **Set publishing status to "In production."** Not Testing — Testing kills refresh tokens after
   7 days.
7. Create an OAuth client of type **Desktop app**.
8. **Download the client secret JSON immediately** — Google shows the secret only once.
9. Paste the client ID + secret into Demido Studio, or point it at the downloaded JSON.
10. Complete the browser consent, clicking through **Advanced → Go to (app) (unsafe)** on the
    unverified-app screen; this is expected and correct for a personal-use client.

For Zoom: create a **General App** on the Zoom Marketplace, enable the PKCE / public-client flow, set
the redirect URI to the loopback URL Demido Studio shows, and keep it unpublished (personal use).

For the exchanges: generate a **read-only** API key in the exchange's own security settings.

The wizard should detect the "left it in Testing" mistake and the "secret not downloaded" mistake,
because those are the two that produce confusing failures a week later.

---

## Sources

Primary:

- Google, *Using OAuth 2.0 to Access Google APIs* — <https://developers.google.com/identity/protocols/oauth2>
- Google, *OAuth 2.0 for Mobile & Desktop Apps* — <https://developers.google.com/identity/protocols/oauth2/native-app>
- Google, *OAuth 2.0 Policies* — <https://developers.google.com/identity/protocols/oauth2/policies>
- Google, *Brand verification* — <https://developers.google.com/identity/protocols/oauth2/production-readiness/brand-verification>
- Google, *Sensitive scope verification* — <https://developers.google.com/identity/protocols/oauth2/production-readiness/sensitive-scope-verification>
- Google, *Restricted scope verification* — <https://developers.google.com/identity/protocols/oauth2/production-readiness/restricted-scope-verification>
- Google, *Security assessment* — <https://support.google.com/cloud/answer/13465431>
- Google, *When verification is not needed* — <https://support.google.com/cloud/answer/13464323>
- Google, *Managing app publishing status / audience* — <https://support.google.com/cloud/answer/15549945>
- Google, *OAuth API verification FAQ* — <https://support.google.com/cloud/answer/13463817>
- Google, *Manage OAuth clients* — <https://support.google.com/cloud/answer/15549257>
- Google, *API Services User Data Policy* — <https://developers.google.com/terms/api-services-user-data-policy>
- Google, *Gmail API scopes* — <https://developers.google.com/workspace/gmail/api/auth/scopes>
- Google, *Gmail IMAP/SMTP (XOAUTH2)* — <https://developers.google.com/workspace/gmail/imap/imap-smtp>
- Google, *CalDAV API developer's guide* — <https://developers.google.com/workspace/calendar/caldav/v2/guide>
- Google, *CardDAV API* — <https://developers.google.com/people/carddav>
- Google, *Sign in with app passwords* — <https://support.google.com/accounts/answer/185833>
- Google Workspace, *Transition from less secure apps to OAuth* — <https://knowledge.workspace.google.com/admin/sync/transition-from-less-secure-apps-to-oauth>
- Google for Developers Blog, *Usability and safety updates to the Google Auth Platform* — <https://developers.googleblog.com/usability-and-safety-updates-to-google-auth-platform/>
- Zoom, *OAuth* — <https://developers.zoom.us/docs/integrations/oauth/>
- IETF, *RFC 8252 — OAuth 2.0 for Native Apps* — <https://datatracker.ietf.org/doc/html/rfc8252>

Secondary, used only for the CASA price range and clearly marked as estimates:

- <https://www.switchlabs.dev/post/casa-tier-2-tier-3-security-review-providers-pricing-and-the-cheapest-option>
- <https://deepstrike.io/blog/google-casa-security-assessment-2025>
