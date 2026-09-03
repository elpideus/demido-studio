# Nexus: which keyless model sources actually work

Date of measurement: **2026-09-03**. Every status code below was produced on that date by a
request from this machine, with no credential except each upstream's own documented anonymous
one. Nothing here is quoted from a list.

Resolves issue [#4](https://github.com/elpideus/demido-studio/issues/4).

Brief reference: `docs/brief.md`, the Nexus bullet, quoted in full because the whole policy
below is an attempt to satisfy exactly this sentence and nothing more:

> Free models system via OmniRoute/9Route-like system. I want the system to be called Nexus. It
> should be Demido Studio's own free models router system so that anyone can use Demido Studio
> for free without having to download a model.

Two other lines of the brief bind this file. The licence line: "Demido Studio will be released
under GPL v3, so build it in a way that does not go against that. Furthermore all technologies,
frameworks and libraries used will need to be credited both in the source code and inside the
program itself." And the attribution line: Stefan Cucoranu is the sole contributor. A source
whose terms are breached is breached under one named person's name, published, with the source
code showing exactly how. That is the reason section 3 is written the way it is.

---

## 0. The trap this file is trying not to fall into, in either direction

v2 checked four endpoints on one afternoon (Hack Club, GitHub Models, GaiaNet, Pollinations),
found them gone or cached, and wrote
`docs/decisions/0026-nexus-is-a-key-the-user-fetches.md`, whose central claim was:

> There is, today, no legitimate OpenAI-compatible endpoint that will generate a fresh token for
> a stranger.

Two days later `docs/decisions/0034-keyless-is-not-dead.md` reversed it, having found six working
anonymous upstreams already sitting in the project's own earlier code, and recorded the lesson:
"Four endpoints checked once is not a fact about the web."

Both notes are half right, and the failure mode is symmetric. 0026 generalised from four dead
endpoints to a dead web. 0034 generalised from six live endpoints to a shippable feature, and
never asked the second question at all: **may we?** Six hosts answered. That is a fact about
sockets. It is not a fact about terms, and 0034 contains no terms analysis of any kind.

So this file separates three questions that v2 collapsed into one:

1. **Does it answer?** (measured, section 1)
2. **May we ship it answering?** (read from each provider's own terms, section 3)
3. **What happens when the answer is no to both, everywhere?** (section 6, the deliverable)

The honest headline, stated once, up front, so nothing below can be quoted out of it:

> **Keyless inference is not dead, and it cannot carry Demido Studio.** Three of v2's six
> upstreams still answer anonymously. Of those three, two are usable under their own published
> terms, and neither of those two can carry an agent turn. The two that could carry an agent turn
> are the two whose terms forbid it. Nexus must therefore be built as a ladder that expects its
> top rung to be empty, not as a list that expects to be refreshed.

---

## 1. Live verification, 2026-09-03

v2's catalogue is `src-tauri/crates/demido-nexus/src/keyless.rs` in
`demido-studio-second-version`. It holds six upstreams. All six were asked for `GET /models` and
then for a real `POST /chat/completions`, because listing is not the test: v2's own file says so
in its header comment ("One that answers `/models` and refuses `/chat/completions` is not
keyless, it is a signup form"), and then its live test `a_real_keyless` checks only the listing.
That gap is why 0034's table says "six of six answer" and this one does not.

### 1.1 Results

| Upstream | `GET /models` | Listed | Free by v2's filter | `POST /chat/completions` | Verdict |
|---|---|---|---|---|---|
| OpenCode Zen | `200` | 66 | 8 | **`200`, real completion** | Alive |
| Kilo Gateway | `200` | 367 | 18 | **`200`, real completion** | Alive |
| OVHcloud AI Endpoints | `200` | 25 | 25 | **`200`, but `429` on 2 of 3 tries** | Alive, throttled |
| Pollinations | `200` | 374 | 13 | **`401`** | **Dead to a stranger** |
| g4f (Pollinations mirror) | `200` | 9 | **1** (`sana`, an image model) | **`402`** | **Dead to a stranger** |
| AI Horde | `200` | 25 | 25 | **`200`, real completion** | Alive |

**Three of six.** Not six of six, and not zero.

### 1.2 What each one actually said

**OpenCode Zen** (`https://opencode.ai/zen/v1`, no `Authorization` header at all):

- `nemotron-3-ultra-free` returns a genuine completion.
- `deepseek-v4-flash-free`, the **first entry in v2's hardcoded fallback list**, returns
  `{"error":{"type":"server_error","message":"Error from provider (Console): Upstream request
  failed: Model is unavailable."}}`. v2's fallback constant exists precisely to be the thing that
  works before discovery has run, and its first element does not work.
- `north-mini-code-free`, also in v2's fallback list, is **no longer in the catalogue at all**.
- Tool calling works: a request with a `tools` array came back `finish_reason: "tool_calls"` with
  a well-formed call.
- **The status code lies.** In a burst of five identical requests, two returned `HTTP 200` with an
  error object in the body (`Upstream request failed: [502]`). A relay that classifies on the HTTP
  status will count those as successes, stream nothing, and never fail over. This is a concrete
  defect in the shape v2 built.

**Kilo Gateway** (`https://api.kilo.ai/api/openrouter`, `Authorization: Bearer anonymous`,
`X-KILOCODE-EDITORNAME: DemidoStudio`):

- Both of v2's fallbacks (`openrouter/free`, `cohere/north-mini-code:free`) return real
  completions. `openrouter/free` resolved to `liquid/lfm-2.5-2.6b:free` on the day.
- Tool calling works (`finish_reason: "tool_call…"`, provider `Cohere`).
- v2's `FreeMarked` suffix filter and Kilo's own `isFree` boolean agree **exactly**: 18 models
  each, zero disagreement in either direction. 9router reads `isFree` from
  `https://api.kilo.ai/api/gateway/models` instead (`src/app/api/providers/kilo/free-models/route.js`),
  which is the better field to read because it is the provider's own assertion rather than a
  spelling, but on today's data the two are the same set.
- **Every one of the 18 free models reports `mayTrainOnYourPrompts: true`.** Kilo publishes this
  as a field in its own catalogue. It is a disclosure obligation, covered in section 4.

**OVHcloud AI Endpoints** (`https://oai.endpoints.kepler.ai.cloud.ovh.net/v1`, no header):

- Three attempts spaced twelve seconds apart from one machine: `429`, `200`, `429`.
- The `200` was a real completion from `Qwen3.5-9B` including a `reasoning` field and an empty
  `tool_calls` array, so the OpenAI tool shape is understood.
- This is not a blip. It is the documented limit, quoted in section 3.

**Pollinations** (`https://gen.pollinations.ai/v1`):

- Still lists 374 models. All thirteen ids on v2's hand-checked allowlist are still listed.
- A completion returns
  `{"success":false,"error":{"message":"A valid API key is required. Get one at
  https://enter.pollinations.ai/keys","code":"UNAUTHORIZED"},"status":401}`.
- This is a **state change** since 0034, and it is a change in kind, not degree. 0034 recorded
  Pollinations as gated by a near-zero "pollen" credit balance, where a short reply worked and an
  ordinary turn did not. It is now a flat demand for a key. It is no longer a keyless source at
  all, and v2's `Filter::Allow` list of thirteen ids is now thirteen ids that cannot be reached.

**g4f (Pollinations mirror)** (`https://g4f.space/api/pollinations/v1`):

- The catalogue has collapsed from the ten v2 measured to nine, and the shape inverted. v2's
  `Filter::TopLevel` (accept ids with no `/`) was built because short ids were free and
  namespaced ones were paid. Today the entire list is namespaced except one entry, `sana`, which
  is an image model. **The filter now yields exactly one model, and it is not a chat model.**
- A completion returns `402`: `"No cake credits. Bake proof-of-work cakes at g4f.dev/chat to earn
  anonymous usage, or sign up at g4f.dev/members.html."` A new gate, absent from every prior
  measurement.
- Separately and more importantly, see section 3.6: this entry should never have shipped,
  because v2's own hard line names it.

**AI Horde** (`https://oai.aihorde.net/v1`, `Authorization: Bearer 0000000000`):

- Real completion, returned promptly, from `koboldcpp/Llama-3.2-3B`.
- v2's hardcoded fallback `koboldcpp/L3-8B-Stheno-v3.2` is **not in today's list**. This is
  expected and structural rather than rot: the catalogue is whatever volunteer workers happen to
  have loaded, so any hardcoded AI Horde model id is guaranteed to go stale. A fallback constant
  is the wrong mechanism for this one source.
- **Tools are accepted and silently ignored.** v2 marks it `tools: false` and is right, but the
  measured behaviour is worse than a refusal: given a `tools` array, it returned `HTTP 200` and a
  chatty prose hallucination ("`python3 Weather.py`\n\n### Description:\nThis tool will fetch the
  weather information from the OpenWeatherMap API.") rather than a tool call or an error. If this
  source is ever offered for a turn that carries tools, the model appears to work and produces
  garbage. It must be excluded at routing time, not left to fail.

### 1.3 Two candidates checked so nobody re-adds them

- **`ai.hackclub.com/model`**: `404`. Still gone, as 0026 said.
- **GitHub Models (`models.inference.ai.azure.com`)**: does not resolve. Still gone.
- **`api.llm7.io`**, a keyless source OmniRoute lists and this file checked because it is the
  obvious "new source" candidate: lists 45 models, and a completion returns
  `{"error":{"message":"Missing API key.","code":"missing_api_key"}}`. **No longer keyless.** Its
  catalogue also advertises `claude-opus-5`, `claude-sonnet-5` and `gemini-3.8-flash-high`, which
  is the signature of a proxied or reverse-engineered front end and puts it behind the hard line
  regardless.

The search for a replacement was therefore conducted and came back empty. That is worth writing
down, because it is the load-bearing input to section 6: the keyless pool is not being replenished
as fast as it is draining.

---

## 2. Rate limits, quality, and the first-run question

The ticket asks whether any of these can carry "someone who downloaded no model at all". Answered
per source, against what Demido actually does with a model, which per the brief is tool-using
agent work with small models, not single-shot chat.

| Source | Throughput to a stranger | Tools | Can it carry first run? |
|---|---|---|---|
| OVHcloud | **2 req/min per IP per model**, documented. Measured 1 success in 3 at 12s spacing. | Yes | **No.** One agent turn is 5 to 20 calls. |
| AI Horde | No quota, but a shared volunteer queue; lowest priority for anonymous users. v2 sets a 600s timeout for this reason. | **No** | **No.** No tools means no agent mode at all. |
| OpenCode Zen | Fast, undocumented quota (see 3.1). Body-level `502`s under a five-request burst. | Yes | Technically yes. See section 3. |
| Kilo Gateway | Fast, no published cap. | Yes | Technically yes. See section 3. |

**Conclusion, and it is the uncomfortable one.** The two sources that could carry a first-run
agent experience are exactly the two Demido Studio may not use (section 3). The two that are
clean cannot carry it: OVHcloud runs out of requests inside the first agent turn, and AI Horde
cannot call a tool.

Therefore: **keyless can carry a first conversation. It cannot carry first work.** Nexus should
be scoped to the first of those and never sold as the second, and the first-run flow must not
depend on it. That single sentence drives section 6.

A note on quality, since it is easy to overrate: the models that are free here are good. Kilo's
free set includes `nvidia/nemotron-3-ultra-550b-a55b:free` and `minimax/minimax-m3:free` at
1M context; OVHcloud serves `Qwen3.5-397B-A17B`, `gpt-oss-120b` and `Qwen3.5-9B`. Quality is not
the constraint. Rate, tools and terms are.

---

## 3. Terms of service, per source

Read from each provider's own published terms. "Probably fine" is not an answer here, so each
entry states the clause, the reading, and a verdict from a three-value scale:

- **Permitted**: the provider documents the anonymous access we are making.
- **Undocumented**: the endpoint answers, but no published term grants the access. Absence of a
  prohibition is not a grant.
- **Prohibited**: a published clause covers what we would be doing, and forbids it.

### 3.1 OpenCode Zen: **Prohibited**

Three independent findings, any one of which is disqualifying.

**The documented access path requires an account and billing details.** The Zen docs
(`https://opencode.ai/docs/zen/`) describe exactly one way in: "You sign in to OpenCode Zen, add
your billing details, and copy your API key." There is **no documented anonymous or keyless
access** to Zen, for free models or any others. Our `200` is an undocumented door.

**The Terms of Service restrict use to the accepting party's own internal use.** From
`https://opencode.ai/legal/terms-of-service` (Anomaly Innovations, Inc.):

> you will only use the Services for your own internal use, and not on behalf of or for the
> benefit of any third party

Demido Studio shipping a hardcoded Zen endpoint, on by default, to every person who installs it,
is use for the benefit of third parties by construction. It is worth being precise about the
counter-argument, because it is the tempting one: one could say each end user is making their own
request for their own internal use, and Demido is only a local client, not a proxy. That argument
fails on the first finding: the terms bind a party who accepted them by registering, and a
keyless caller has registered nothing and accepted nothing, so there is no licence to use the
service at all. There is no version of this where an unregistered caller has "own internal use"
rights.

The same terms also prohibit anything that "automatically or programmatically extracts data or
Output", which is a fair description of an agent loop.

**Zen is defined in those terms as a paid product**: "certain of these models are provided
directly by us if you use the OpenCode Zen **paid offering** ('Zen')". The free models sit inside
a paid product, gated by an undocumented quota. A public bug report,
[anomalyco/opencode#16844](https://github.com/anomalyco/opencode/issues/16844), asks for the
specific free-usage terms (how much per day, tokens or requests, when the backing model may be
substituted) and reports hitting `Free usage exceeded, add credits ... [retrying in 20h 44m]`
mid-session with no warning. The issue was closed by a bot for using the wrong template. **The
terms of the free tier are, as of today, unpublished and unanswerable.**

**Corroboration.** OmniRoute reached the same verdict independently and recorded it in
`open-sse/config/freeTierCatalog.ts` as `opencode: "avoid"`, with the note in
`docs/reference/FREE_TIERS.md`: "ToS (Anomaly Innovations, Inc.) explicitly restricts use to
'your own internal use, and not on behalf of or for the benefit of any third party'". Two
independent readings, one conclusion.

This is v2's **rank 0, on by default** source.

### 3.2 Kilo Gateway: **Prohibited**

From Kilo's Terms of Service:

> To access most features of the Service, you must register for an account ("Account").

and:

> [You may not use] any spider, crawler, scraper or other automatic device, process or software
> that intercepts, mines, scrapes, extracts or otherwise accesses the Service.

Registration is required; we register nothing. The literal string `anonymous` is not a documented
anonymous credential in the way AI Horde's `0000000000` is: no Kilo document publishes it, and it
is best understood as an unauthenticated code path rather than a granted tier.

Worse, and this is the part that cannot be argued around: v2 sends
`X-KILOCODE-EDITORNAME: DemidoStudio`. That header exists so Kilo can identify **its own editor
clients**. Sending it from a third-party application that is not a Kilo client, in order to
satisfy a gateway that "refuses a request that does not name its client" (0034's own wording), is
supplying an identifier to pass a check we do not pass honestly. Whatever the legal
characterisation, it is not something to ship under a named person's name in a source-visible
GPL-3.0 application, where the header is right there in the code for Kilo to read.

This is v2's **rank 1, on by default** source.

### 3.3 OVHcloud AI Endpoints: **Permitted**

The one unambiguous case, and the reason this file does not conclude that keyless is dead.

OVHcloud documents anonymous, keyless access as a product feature, with a published limit. From
`https://docs.ovhcloud.com/en/guides/public-cloud/ai-machine-learning/ai-endpoints-getting-started`:

- Anonymous access: **"2 requests per minute, per IP and per model."**
- Authenticated access: **"400 requests per minute, per PCI project and per model."**

Exceeding the limit returns `429`, which is exactly what was measured. There is no gap between
what the documentation describes and what the endpoint does. This is a deliberately provisioned
free trial lane, and using it as documented, at the documented rate, is the intended use.

**Verdict: ship it, on by default, with the rate limit surfaced honestly in the UI** rather than
presented as a general-purpose model source.

### 3.4 AI Horde: **Permitted**, with one obligation currently unmet

Also unambiguous, and the terms are unusually welcoming.

- The anonymous key is documented, by that literal value: **"Use API key `0000000000`"** for
  access without registration.
- The consequences are documented: **"Lowest priority in generation queues"**, and
  **"Service may be restricted for anonymous users during high load."**
- Third-party clients are explicitly invited: "If you want to build an integration to the AI
  Horde (Bot, application, scripts etc), please consult our Integration Readme."

**The obligation v2 does not meet.** AI Horde's API (`https://aihorde.net/api/swagger.json`)
defines a `Client-Agent` header on 82 operations, described as "The client name and version",
defaulting to `unknown:0:unknown`. It is how a volunteer-powered commons attributes load to the
software causing it. v2's `keyless.rs` sends **no headers at all** for AI Horde, so Demido
currently presents as `unknown:0:unknown`.

**Action, non-optional if this source ships**: send
`Client-Agent: DemidoStudio:<version>:<contact>`. It costs one line and it is the difference
between participating in a commons and free-riding on it anonymously.

AI Horde is GPL-licensed (Haidra-Org) and community funded. There is also a kudos economy; an
anonymous caller contributes none. Section 6 revisits whether Demido should offer to.

### 3.5 Pollinations: moot

`401` with a link to a key page. Not keyless, so no terms question arises. If it is ever re-added
it re-enters as a **keyed** free tier (rung 1, section 6) and needs its own terms read at that
time.

### 3.6 g4f: **Prohibited**, and it was always prohibited

This is the finding that most needs writing down, because it is not a change in the world. It is
an inconsistency v2 shipped.

Decision 0026 states the hard line, inherited from the roadmap since M0:

> **Reverse-engineered web-chat endpoints** (the `g4f` family, the aggregators in front of them).
> This is where "keyless" still exists, and it is where the original note's hard line was aimed.
> Unchanged: a treadmill, and somebody else's terms.

Decision 0034 then ported `g4f.space` into the keyless catalogue and shipped it, while explicitly
claiming that "Decision 0026 keeps its other half. No reverse-engineered web-chat endpoints ...
both stand." Both statements are in the same repository. The catalogue entry names the hard line's
own example.

It is dead now anyway (`402`, proof-of-work credits, and a filter that yields one image model),
so removing it costs nothing. But the reason to remove it is the rule, not the `402`, and that
distinction matters: if it starts answering again next month, it still does not come back.

### 3.7 Summary

| Source | Answers keyless? | Terms | Ship it? |
|---|---|---|---|
| OVHcloud AI Endpoints | Yes, at 2/min/IP | **Permitted**, documented anonymous tier | **Yes, on by default** |
| AI Horde | Yes | **Permitted**, documented anonymous key | **Yes**, with `Client-Agent`; never for tool turns |
| OpenCode Zen | Yes | **Prohibited**: no anonymous path, internal-use-only, paid product | **No** |
| Kilo Gateway | Yes | **Prohibited**: account required, anti-automation clause, client-identifying header | **No** |
| Pollinations | No (`401`) | n/a | No |
| g4f mirror | No (`402`) | **Prohibited** by the standing hard line | No, permanently |

**Two sources survive both tests.** That is the real size of keyless inference available to this
project today, and both of v2's default-on sources are removed by it.

---

## 4. Disclosure, which is not optional in a GPL-3.0 app

Every free model in Kilo's catalogue carries `mayTrainOnYourPrompts: true`, published by Kilo
itself as a field. OpenCode's Zen docs say free models permit data collection "to improve the
model", the NVIDIA free models carry "Trial use only, do not submit personal or confidential
data", and the Muse Spark models are explicitly priced in training rights: "heavily discounted
token pricing in exchange for permission to use your prompts and completions to train future
Meta models". AI Horde routes prompts through volunteer-operated machines belonging to strangers.

Demido Studio is a coding harness. Prompts contain source code, file paths, and whatever is in
the user's repository.

**Rule: no free or keyless source sends a first token until the user has been shown, in one
sentence, that their prompts leave the machine and may be used for training, and by whom.** Once
per source, not once per turn. This is the same commitment the brief already makes about
transparency ("everything about what happens, from the input getting sent to the model ... should
be visible"), applied at the one moment where it has real consequences.

---

## 5. The reference routers, and whether their claims survive contact

Both are MIT, both readable, both were read.

### 5.1 OmniRoute's free-token budget: partly survives

`docs/reference/FREE_TIERS.md` documents a pool-deduped budget headlined at ~1.51B free tokens
per month across 42 pools. The ticket asks whether it survives contact with reality. Findings:

**What is genuinely good, and worth copying.**

- **The deduplication is real and the corrections are honest.** The document narrates its own
  markdowns in public: Gemini 462M to 60M once the Flash variants were recognised as one pool,
  Cloudflare 122M to 30M, `longcat` reclassified from a recurring grant to a one-time signup
  credit, `doubao` likewise. A document that publishes its own downward revisions is a document
  doing the job.
- **The refusal to sum uncapped providers.** Rate-limit-only providers are held in a separate
  "permanently free, no published cap" row and never added to the headline, with the reasoning
  stated: "counting them at RPM x 24/7 is the inflation we reject". This is the correct
  discipline and it is exactly the discipline this file applied to OVHcloud in section 2.
- **The number is CI-gated.** `check:docs-counts` (`scripts/check/check-docs-counts-sync.mjs`,
  wired into `package.json`) fails the build if the documented figure drifts from
  `computeFreeModelTotals()`. A number that a build can falsify is a different kind of number.

**What does not survive.**

- **The per-provider catalogue has rotted while the doc was corrected.**
  `open-sse/config/freeTierCatalog.ts` still carries `"cloudflare-ai": 122_000_000`, the exact
  figure `FREE_TIERS.md` says was corrected to 30M. The aggregator over it is marked
  `@deprecated`. Two sources of truth, one gated and one not, and the ungated one is wrong.
- **The ToS verdicts are decorative.** This is the important one. `FREE_TIERS.md` states it
  plainly: "**ToS flag is advisory, not a routing gate.** Providers marked `tos` are still
  included in routing and combo/fallback by default; the flag only surfaces on
  `/dashboard/free-tiers`". The `excludeTosAvoid` parameter affects a summary view, not routing.
  So OmniRoute correctly identifies `opencode: "avoid"`, writes the reasoning down, and then
  routes to it anyway.

**The lesson Demido takes from it:** the research method is excellent and the enforcement is
absent. Copy the first. In Nexus a terms verdict is a **routing gate**, not a badge (section 6,
rule 4).

### 5.2 9router: one concrete idea worth taking

9router is smaller and makes no aggregate claims. Its Kilo integration
(`src/app/api/providers/kilo/free-models/route.js`) is the useful part:

- It reads `isFree === true` from the provider's own catalogue rather than pattern-matching an id
  suffix, which is the provider's assertion instead of a spelling. Verified today: on Kilo the two
  agree exactly, 18 and 18. Prefer the field anyway, because a suffix is a convention and a field
  is a contract.
- It caches the model list for one hour **and serves the stale cache when the upstream fails**,
  returning `{cached: true, warning}` rather than an error. Degrade to stale data with a note
  attached, never to an empty list. That behaviour goes straight into rule 5 below.

---

## 6. The failure policy

This is the deliverable. The question is what Nexus does the day every keyless source is gone,
and the answer must be designed now, because after section 3 that day is closer than a source
list suggests: **Nexus ships with two sources, not six.**

### 6.1 The shape: a ladder whose top rung is expected to be empty

Nexus is not a list of free models. Nexus is the component that answers one question:

> A fresh install, no model downloaded, no account anywhere. Does this person reach a first
> answer?

There are exactly three ways to reach one, and Nexus owns all three as rungs of one ladder:

- **Rung 0, keyless.** Someone else's compute, no account. Today: OVHcloud and AI Horde.
- **Rung 1, keyed free tier.** A key the user fetches, no card up front. This is v2's decision
  0026 catalogue, which is still correct and must not be deleted a second time.
- **Rung 2, local.** Download a model sized to the detected hardware, through the managed
  llama.cpp route. Works offline, forever, with no third party and no terms.

**Every rung ships in every build. The ladder is the feature; the rungs are inventory.** v2 got
into trouble twice by treating one rung as the feature: 0026 deleted rung 0 and shipped a page
called "Free models" with no free model on it; 0034 restored rung 0 and left rung 1 with no
reason to exist. Neither note describes a ladder, so neither could degrade.

### 6.2 The answer to the question as asked

**The day every keyless source is gone, nothing breaks and nothing prompts for a card.**

Rung 0 renders as an empty state, and an empty state here is a sentence, not a spinner: *"No
keyless source is answering. Last verified 2026-09-03."* The wizard's default recommendation,
which normally sits on rung 0 for the impatient path, moves down to **rung 2** (download a model,
recommended size named from detected hardware), with **rung 1** offered beside it (paste a free
key, links to the key pages, no card required). Both are one click. Neither is a dead end. The
words "no models available" never appear, and a payment card is never the next step, because it
never has to be: rung 2 is always available on any machine that can run the app at all.

The direction of degradation matters and is deliberate: **downward, toward independence.** The
final resting state of Demido Studio, if every free tier on the internet closes, is a local model
on the user's own disk. That is the state the brief describes as the normal one anyway ("allowing
people to get the most out of even smaller models"), which means the failure mode of Nexus is the
product's main use case. This is the strongest possible failure policy and it is available for
free, so long as nothing above rung 2 is ever load-bearing.

### 6.3 Seven rules that make that true

**Rule 1: first run must not depend on rung 0.** Sequence the wizard as hardware detection, then
the model download offer, and only then keyless as *"or try one right now while that downloads"*.
This inverts the dependency: a dead rung 0 degrades a convenience, not the product. Given section
2, this is not a precaution but a description of reality: no keyless source can carry an agent
turn today, while all six were up.

**Rule 2: a terms verdict is a routing gate, not a badge.** Every entry carries a `terms` field
with the section 3 scale. Only `Permitted` sources are ever on by default or reachable by `auto`.
`Undocumented` and `Prohibited` sources may appear in the UI, off, each with its clause quoted on
the toggle, so that turning one on is an informed act by the person whose account and IP it is.
This is precisely where OmniRoute stops, and Nexus does not stop there. Note the consequence,
accepted deliberately: applying this rule today removes both of v2's default-on sources and Nexus
ships with two.

**Rule 3: classify on the body, never on the status code.** OpenCode returns `HTTP 200` with an
error object, twice in five requests. A source "answered" only when a parsed completion carried
content or a tool call. Anything else is a failure and triggers the walk, whatever the status line
said.

**Rule 4: capability is a filter before preference is.** AI Horde accepts a `tools` array,
ignores it, and returns confident prose. A source whose `tools` flag is false is removed from the
candidate set for any turn carrying tools, before ranking. A wrong answer that looks right is
worse than a refusal, and this one is only ever a routing bug away.

**Rule 5: degrade to stale, annotated, never to empty.** 9router's pattern. Discovery caches each
source's model list; when discovery fails, serve the last good list with the failure attached as a
note. Never an empty list, never a silent substitution. This is `AGENTS.md` rule 6 ("nothing is
silently dropped") applied to the catalogue.

**Rule 6: retirement is a runtime decision; rot is a release gate.** Two different clocks.

- *Runtime*: v2's `health.rs` cooldown ladder is the right mechanism and is worth porting as is
  (in memory, never persisted, demote rather than ban). Extend it so a source that answers `404`
  or `410`, or refuses every model it advertises, is retired for the session rather than merely
  cooled.
- *Release*: the catalogue carries a `verified_on` date. `a_real_keyless` prints it, and CI fails
  a **release** build (not every build, which would make the network a dependency of `cargo test`)
  when it is older than 90 days. v2's test asserts only that at least one source answered, which
  is the assertion that fires on the last day it could possibly matter. A staleness gate fires
  while there is still time to act.

**Rule 7: an exhausted walk is a message, not an error.** When every candidate on rung 0 has been
tried and none answered, the turn does not fail with a red toast. It returns a normal assistant
message naming each source tried and what it said, and offering rung 1 and rung 2 as buttons.
This is the same commitment as rule 5, at the moment the user is actually watching.

### 6.4 What to build first

In order, because the order is itself the policy:

1. **Rung 2 before rung 0.** The local download path is the floor everything else falls back to.
   Anything built above it before it exists has nothing to fall to.
2. **The ladder's empty states**, written before the catalogue is populated. If the empty state is
   written last it is written by whoever is annoyed at the end of the milestone, and it becomes a
   spinner.
3. **The `terms` field and its gate**, before any source is added, so that no source can be added
   without its clause being read. Section 3 exists because v2 had nowhere to put this and so never
   asked.
4. **The two `Permitted` sources**, OVHcloud and AI Horde, with the `Client-Agent` header and the
   rate limit surfaced in the UI.
5. **`health.rs` and the walk**, ported from v2, with rules 3 and 4 applied.

### 6.5 One open question for Stefan

AI Horde runs on volunteer GPUs and a kudos economy. An anonymous caller contributes nothing and
sits at the back of every queue, which is both the honest cost of free and, at scale, a burden on
a commons. Two options worth a decision rather than a default: offer users a one-click link to
register an AI Horde account (which moves them up the queue and is theirs, not ours), and/or
consider whether Demido should ever be able to contribute worker capacity back. Neither is
required to ship. Both are the difference between using a commons and taking from it. Recorded
here rather than decided, because the brief says the brief wins.

---

## Sources

All fetched or probed 2026-09-03.

**Primary, measured by request:**
`https://opencode.ai/zen/v1/models`, `.../chat/completions`;
`https://api.kilo.ai/api/openrouter/models`, `.../chat/completions`;
`https://api.kilo.ai/api/gateway/models`;
`https://oai.endpoints.kepler.ai.cloud.ovh.net/v1/models`, `.../chat/completions`;
`https://gen.pollinations.ai/v1/models`, `.../chat/completions`;
`https://g4f.space/api/pollinations/v1/models`, `.../chat/completions`;
`https://oai.aihorde.net/v1/models`, `.../chat/completions`;
`https://aihorde.net/api/swagger.json`;
`https://api.llm7.io/v1/models`, `.../chat/completions`;
`https://ai.hackclub.com/model`; `https://models.inference.ai.azure.com/models`.

**Primary, documents:**
- [OpenCode Zen docs](https://opencode.ai/docs/zen/)
- [OpenCode Terms of Service, Anomaly Innovations, Inc.](https://opencode.ai/legal/terms-of-service)
- [anomalyco/opencode#16844, unanswered request for Zen free-tier terms](https://github.com/anomalyco/opencode/issues/16844)
- [Kilo Code Terms of Service](https://kilo.ai/terms)
- [OVHcloud AI Endpoints, getting started and rate limits](https://docs.ovhcloud.com/en/guides/public-cloud/ai-machine-learning/ai-endpoints-getting-started)
- [OVHcloud AI Endpoints catalogue](https://www.ovhcloud.com/en/public-cloud/ai-endpoints/catalog/)
- [AI Horde README, Haidra-Org](https://github.com/Haidra-Org/AI-Horde/blob/main/README.md)
- [NVIDIA API Trial Terms of Service](https://assets.ngc.nvidia.com/products/api-catalog/legal/NVIDIA%20API%20Trial%20Terms%20of%20Service.pdf)

**Primary, source trees on this machine:**
- `S:\Development\Demido Studio Project\demido-studio-second-version\src-tauri\crates\demido-nexus\` (`keyless.rs`, `health.rs`, `AGENTS.md`, `tests/a_real_keyless.rs`)
- `S:\Development\Demido Studio Project\demido-studio-second-version\docs\decisions\0026-nexus-is-a-key-the-user-fetches.md`, `0034-keyless-is-not-dead.md`
- `S:\Development\routers\OmniRoute\docs\reference\FREE_TIERS.md`, `open-sse\config\freeTierCatalog.ts`, `package.json`
- `S:\Development\routers\9router\src\app\api\providers\kilo\free-models\route.js`
- `S:\Development\Demido Studio Project\demido-studio\docs\brief.md`
