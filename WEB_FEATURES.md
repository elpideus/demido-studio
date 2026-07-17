# opencode web search / web fetch — notes

Source: `S:\Development\opencode\packages\opencode\src\tool\{webfetch,websearch,mcp-websearch}.ts`

## webfetch tool (`webfetch.ts`)

Params: `url`, `format` (text/markdown/html, default markdown), `timeout` (sec, max 120, default 30).

Flow:
- Reject non-http(s) URLs.
- Perms: `ctx.ask({ permission: "webfetch", patterns: [url], always: ["*"] })` — asks once, `always: ["*"]` lets user allow-all future fetches.
- Builds `Accept` header per format w/ q-value fallback chain (e.g. markdown → `text/markdown;q=1.0, text/x-markdown;q=0.9, text/plain;q=0.8, text/html;q=0.7, */*;q=0.1`).
- Fixed browser-spoof `User-Agent` (Chrome/Windows UA string) + `Accept-Language`.
- **Cloudflare bypass**: if first request 403s with `cf-mitigated: challenge` header, retries once with `User-Agent: opencode` (honest UA) instead of the spoofed one — some CF configs allow known bots but block generic browser UA mismatches (TLS fingerprint doesn't match claimed browser).
- Size guard: checks `content-length` header AND actual byte length against 5MB cap; dies with error if either exceeds.
- Content-type dispatch:
  - image mime → returns as base64 `data:` URI attachment (not text).
  - html + format=markdown → HTML→Markdown via `TurndownService` (headingStyle atx, bulletMarker `-`, fenced code blocks; strips `script/style/meta/link` before conversion).
  - html + format=text → strips tags via streaming `htmlparser2` parser, skipping `script/style/noscript/iframe/object/embed` subtrees, returns concatenated text nodes trimmed.
  - format=html → raw passthrough.
  - non-html content in markdown/text mode → raw passthrough (no conversion).
- Whole request wrapped in `Effect.timeoutOrElse` (user timeout, capped 120s) → dies with "Request timed out".

No external search API involved — pure HTTP GET + local HTML parsing/conversion. No JS rendering (no headless browser), so SPA-only pages return the initial HTML shell.

## websearch tool (`websearch.ts` + `mcp-websearch.ts`)

Not a local scraper — proxies to **two hosted MCP web-search services** and picks one per session:

- **Exa** (`https://mcp.exa.ai/mcp`, tool `web_search_exa`) — optional `EXA_API_KEY` embedded as query param in URL if set, else uses opencode's own hosted key/quota.
- **Parallel** (`https://search.parallel.ai/mcp`, tool `web_search`) — auth via `Authorization: Bearer $PARALLEL_API_KEY` if set.

Provider selection (`selectWebSearchProvider`):
1. `OPENCODE_WEBSEARCH_PROVIDER` env override (`exa`|`parallel`) wins outright.
2. Else if only one of the two runtime flags (`enableExa`/`enableParallel`) is on, use that.
3. Else **deterministic A/B split**: `checksum(sessionID) % 2` picks exa vs parallel — same session always gets the same provider, different sessions split ~50/50.

Feature gating (`registry.ts:webSearchEnabled`): tool is exposed only if `providerID === "opencode"` (opencode's own hosted provider) OR the `enableExa`/`enableParallel` runtime flags are set — i.e. websearch is off by default for BYO-key providers unless explicitly enabled via env vars (`OPENCODE_ENABLE_EXA`, `OPENCODE_ENABLE_PARALLEL`, or legacy `OPENCODE_EXPERIMENTAL_EXA/PARALLEL`) or `OPENCODE_EXPERIMENTAL=true`.

Request mechanics (`mcp-websearch.ts`):
- Builds a raw JSON-RPC 2.0 `tools/call` envelope (`{jsonrpc:"2.0", id:1, method:"tools/call", params:{name, arguments}}`) and POSTs it directly — no real MCP client/session handshake, just one-shot HTTP call mimicking the wire format.
- `Accept: application/json, text/event-stream` — server may reply as plain JSON or SSE.
- Response parsing handles both: tries whole body as JSON first; if not JSON, scans for `data: ` prefixed SSE lines and JSON-decodes each until one has usable content. Extracts `result.content[].text` (first non-empty).
- 25s timeout, dies on timeout.

Params surfaced to model: `query`, `numResults` (default 8), `livecrawl` (`fallback`|`preferred`, default fallback — live crawl vs cached), `type` (`auto`|`fast`|`deep`, Exa-specific), `contextMaxCharacters` (default 10000, trims result blob for LLM context budget). Parallel provider ignores most of these — it just gets `objective`/`search_queries` (both = the query), plus `session_id` and `model_name` (current model id, truncated 100 chars) for its own telemetry/tuning.

Tool description injects current year dynamically (`DESCRIPTION.replace("{{year}}", ...)`) so the model doesn't search a stale year for "latest" queries.

Both tools call `ctx.ask(...)` for permission before executing, and title/metadata surfaced via `ctx.metadata()` so UI can show e.g. `Exa Web Search: "query"`.

## Relevance to DemidoStudio

DemidoStudio's own web tools live in `src-tauri/src/web.rs` (agent tool: web search/fetch, gated by `disabled_tools`). Notable opencode ideas worth stealing if not already present:
- Cloudflare 403 retry-with-honest-UA fallback for webfetch.
- Turndown-equivalent HTML→Markdown (check what DemidoStudio currently returns for fetched pages — raw HTML vs converted).
- Size cap + content-length pre-check before buffering full body.
- Image-mime short-circuit returning as attachment instead of garbled text.
