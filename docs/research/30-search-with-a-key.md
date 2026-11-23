# 30. Search services that need a key

Written 2026-11-23, directly, for plan item 6 (search with a key). Research note 15 left this for when the vault
existed: "then your own keys, once there is a safe place to keep them". The vault exists (DECISIONS U). This note
says which services are worth a key, what each costs and gives free, how each is asked, and what each one's
terms say about keeping its results.

How sure each line is: **Read** (the service's own documentation, pricing page or terms, on 23 November 2026;
the address is given). Nothing here was **tried** against a real service: that needs a key, which is the
owner's, and may cost money. The build was tried against a stand-in that answers as the documentation shows.

## The three worth building

| Service | How it is asked | What it costs, what is free | What it gives | Read |
|---|---|---|---|---|
| **Brave Search** | `GET https://api.search.brave.com/res/v1/llm/context?q=…`, key in `X-Subscription-Token`. Also `/res/v1/web/search` (titles and snippets, "for human consumption"). The query: 1 to 600 characters, 75 words at most | "Search" plan: $5 per 1,000 requests, web search and LLM context alike; "free $5 in credits every month" (so 1,000 searches); prepaid credits, no subscription. 50 requests a second | LLM Context: "extracted page chunks and source metadata in a shape and length that satisfies the needs of an LLM": `grounding.generic[]` with `url`, `title`, `snippets[]`; `sources[url].age` with the page's date (second place `YYYY-MM-DD`). Its size is set by `maximum_number_of_tokens` (1,024 to 32,768), `maximum_number_of_urls`, `maximum_number_of_tokens_per_url` (512 to 8,192). Its own index, no Google or Bing behind it | <https://api-dashboard.search.brave.com/documentation/pricing>, `/documentation/services/llm-context`, `/documentation/services/web-search`, `/documentation/guides/rate-limiting` |
| **Exa** (with a key) | `POST https://api.exa.ai/search`, key in `x-api-key` (or `Authorization: Bearer`). `{"query", "type": "lite", "numResults", "contents": {"highlights": true}}` | `lite`: $4 per 1,000 requests up to 10 results; `auto`: $7. "The Free Tier gives you $10 in credits (up to 2,500 Lite searches) the day you sign up, and your free balance resets to $10 on the first of every month. No payment method required." 10 requests a second | `results[]` with `title`, `url`, `publishedDate`, `highlights[]` (the passages that match); `costDollars.total`, the request's own cost | <https://exa.ai/docs/reference/pricing.md>, `/docs/reference/search.md`, `/docs/reference/rate-limits.md` |
| **Tavily** | `POST https://api.tavily.com/search`, key as `Authorization: Bearer tvly-…`. `{"query", "search_depth": "basic", "max_results", "include_usage": true}` | 1,000 free credits a month, no card. Basic search: 1 credit; advanced: 2. Pay as you go $0.008 a credit; plans $0.0075 to $0.005 | `results[]` with `title`, `url`, `content` (a passage), `score`, `published_date` (when asked); `usage.credits`. Errors: 401 key, 432 the plan's limit, 433 the pay-as-you-go limit | <https://docs.tavily.com/documentation/api-credits.md>, `/documentation/api-reference/endpoint/search.md` |

All three answer with passages of the pages, not a line under a title, which is what a small model needs (note
15, N8): the first page need not be read separately.

## What their terms say about keeping results (Read)

Demido keeps a search for an hour on disk (N10, the caching rule P). Two of the three forbid that.

- **Brave** (API terms of service, section on use restrictions): the customer shall not "store, cache, or create
  a database of Search Results, in whole or in part, other than transient storage required for operation of
  Customer Applications". Also: no use of results "to create, evaluate, train, re-train, fine-tune, benchmark
  or otherwise improve artificial intelligence models"; no attempt "to obfuscate or conceal Customer's identity
  or the identity of the Customer Applications". <https://api-dashboard.search.brave.com/documentation/resources/terms-of-service>
- **Exa** (terms of service, 4.2(a), a PDF at <https://exa.ai/terms>): you may not "download, modify, copy …
  duplicate … any information contained on, or obtained from or through, the Services, except for temporary
  files that are automatically cached by your web browser for display purposes". These are Exa's terms for the
  website, search engine and API together, so they cover **the keyless service Demido already uses** (N6), whose
  results Demido has kept for an hour since 2026-11-21. That was wrong by these terms, and is put right with
  this item.
- **Tavily** (terms of service, <https://www.tavily.com/terms>): nothing found about keeping results. It says
  the service is "solely for Customer's internal business purposes", forbids reselling it, and that Tavily and
  its providers "may use, process, analyze, and retain Customer Input … for purposes of training" AI models:
  the words of a search may be used to train Tavily's models.

My reading, not a lawyer's: the conversation in which a model was given a few passages is the person's own
record of what happened, the "operation" of the application, and keeping it is not "a database of Search
Results". Keeping whole answers on disk to answer the same search later is caching, which Brave forbids by name
and Exa's wording covers. So Brave's and Exa's answers are not kept; Tavily's are, for the hour every search is.

Whose terms these are: each person makes their own account and key, so each person is the "Customer" and
agrees to that service's terms themselves. Demido is the software they use with it, says its own name (Brave
asks that the application not be hidden), and never shares one key between people.

## The others, and why not now

| Service | What was found | Why not built |
|---|---|---|
| Kagi | An API with a key, `Authorization: Bearer` (<https://help.kagi.com/kagi/api/search.html>). Its price was not on the page read | Needs a paid Kagi account; little gained over the three |
| Mwmbl with an account | 2,000 searches a month instead of 1,000 (note 15) | Its key and how it is sent were not read; Mwmbl is already the last rung without one |
| Marginalia | A free key for non-commercial use, results under CC BY-NC-SA 4.0 (note 15) | A small index of its own; a key for a few more searches |
| Serper, SerpAPI, SearchAPI and the like | Sell Google's or Bing's result pages, read by them | Those pages' own sites say programs are not to read them; Demido follows that (N7, N10). Buying the reading from someone else does not change what the site said |
| Google Programmable Search, Bing Web Search API | Google's is closed to new customers; Microsoft retired Bing's (Brave's page says both; note 15 found the same) | Not available |

## What this means for the ladder

A key is the person saying "use this one, I pay for it". So a service with a key is asked first, before the
free rungs; each can be turned off without removing its key. When it answers, the search ends there, as with
any rung; when it refuses (key wrong, credits used up, limit reached), the ladder goes on to the free rungs and
says why. What each keyed search cost is counted with the providers' spending (U7) and held by the same month's
limit (U8), at the list price, never below: the free monthly credits are not subtracted, so the count is above
the bill. A month's limit reached stops the keyed rungs and leaves the free ones.

What was not measured: whether a small model answers better from Brave's, Exa's or Tavily's passages than from
the free ladder. That needs real searches with a key.
