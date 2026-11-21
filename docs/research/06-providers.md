# 06 - Remote and CLI providers

Research date: 2026-11-20. Every load-bearing fact has a source URL. Items I could not confirm from a primary source are listed in "Not verified" at the end. I am not a lawyer; the legal sections are a reading of the published terms, not legal advice.

Legal texts are paraphrased here, not reproduced. Read the linked originals before acting on them.

## Question

How should Demido Studio integrate Anthropic (API key and the user's own Claude Code CLI), Google Gemini, OpenRouter and the OpenCode CLI? What is permitted, what does each provider expose for thinking, tools, vision and usage, which Rust crates (if any) should carry the load, and where do the usage indicators get their numbers?

## Short answer

1. **The brief's billing note is out of date.** The June 15, 2026 change (programmatic use moving to a separate credit) was paused on the day it was due. `claude -p`, the Agent SDK and third-party app usage under a subscription login still count against the normal plan limits. What shipped instead, on October 7, 2026, is a monthly **API credit** for Max ($100), Ultra ($200) and Team plans. It lives in a linked Claude Console organization and is spent with an **API key**, not with the subscription login. Pro gets none.
2. **Spawning the user's own Claude Code binary is permitted, with conditions.** Anthropic's legal page now has a section on running Claude Code inside other products. The conditions: unmodified binary, no auth method removed or restricted, each user signs in with their own credentials through Anthropic's own flow, no reselling or intermediating, and naming limits. Demido must never read, store or forward the OAuth token.
3. **Two kinds of provider, not one.** Anthropic API, Gemini and OpenRouter are raw model endpoints where Demido owns the loop. Claude Code and OpenCode are external agent loops that Demido drives as a client. Forcing both into one trait is a mistake. Demido's "make small models behave" harness does not apply to the second kind.
4. **OpenCode changed under the brief.** OpenCode 2 is out (v2.3.2 on 2026-11-18) with a new server API, new client packages and "Console" in place of "Zen". OpenCode 1 is still shipping (v1.19.6 on 2026-11-17). The free models are confirmed to be locked to the real OpenCode client: other clients get HTTP 403.
5. **Write thin clients of our own.** Three wire formats (Anthropic Messages, Gemini, OpenAI Chat Completions) cover every raw provider including local servers. No existing crate gives the wire-level transparency the owner's principles demand, and the provider APIs are changing faster than the crates.
6. **Gemini is API-key only.** Using Gemini CLI's OAuth from third-party software is explicitly called a terms violation in the official Gemini CLI docs.

## Findings

### 1. Anthropic API (API key route)

**Endpoint and versioning**

- `POST https://api.anthropic.com/v1/messages`, header `x-api-key`, header `anthropic-version`.
- There is a new API version, `2026-10-21`. `2023-06-01` is still listed. In `2026-10-21` the `temperature`, `top_p` and `top_k` parameters are removed and return a 400 on every model. Source: https://platform.claude.com/docs/en/api/versioning
- Top-level request parameters today: `max_tokens`, `messages`, `model`, `cache_control`, `container`, `diagnostics`, `inference_geo`, `metadata`, `output_config`, `service_tier`, `speed`, `stop_sequences`, `stream`, `system`, `thinking`, `tool_choice`, `tools`. Source: https://platform.claude.com/docs/en/api/messages/create.md

**Features that matter to Demido** (source: Anthropic's bundled API reference, cached 2026-11-03, plus the linked live pages)

| Area | Current state | Source |
| --- | --- | --- |
| Thinking | `thinking: {type: "adaptive"}` plus `output_config.effort` (`low`, `medium`, `high`, `xhigh`, `max`). Fixed `budget_tokens` is rejected on current models and survives only on Haiku 4.5 and older. Some models cannot turn thinking off at all (Opus 5.5, the Fable family); Sonnet 5.5 uses `{type: "between_tools"}` instead of `disabled`. | https://platform.claude.com/docs/en/build-with-claude/adaptive-thinking.md , https://platform.claude.com/docs/en/build-with-claude/effort.md |
| Thinking visibility | Raw reasoning is never returned. `display: "summarized"` returns a summary; the default on current models is `"omitted"` (empty text). Thinking blocks must be passed back unchanged. | https://platform.claude.com/docs/en/build-with-claude/thinking.md |
| Tools | `tools[]` with `name`, `description`, `input_schema`, optional `strict: true`. Parallel calls are default. Forced `tool_choice` (`any` / `tool`) returns 400 on the newest models; only `auto` and `none` are safe everywhere. | https://platform.claude.com/docs/en/agents-and-tools/tool-use/overview.md |
| Vision | Image blocks with `base64`, `url` or `file` sources; JPEG, PNG, GIF, WebP. PDFs as `document` blocks. | https://platform.claude.com/docs/en/api/messages/create.md , https://platform.claude.com/docs/en/build-with-claude/vision.md |
| Caching | `cache_control` breakpoints (max 4) or top-level auto-caching. Cache reads do not count toward the input-token rate limit. | https://platform.claude.com/docs/en/build-with-claude/prompt-caching.md |
| Structured output | `output_config.format`; the old `output_format` is deprecated. | https://platform.claude.com/docs/en/build-with-claude/overview.md |
| Capability discovery | `GET /v1/models` and `GET /v1/models/{id}` return `max_input_tokens`, `max_tokens` and a `capabilities` tree (`image_input`, `structured_outputs`, `thinking.types.*`, `effort.*`), each leaf with `supported: true/false`. No pricing in this endpoint. | https://platform.claude.com/docs/en/about-claude/models/overview.md |
| Token counting | `POST /v1/messages/count_tokens`. | https://platform.claude.com/docs/en/build-with-claude/token-counting.md |
| Refusals | `stop_reason: "refusal"` with `stop_details` on current models; must be handled before reading content. | https://platform.claude.com/docs/en/api/messages/create.md |

Current model IDs and list prices (bundled reference, cached 2026-11-03; live source https://platform.claude.com/docs/en/about-claude/pricing.md): `claude-opus-5-5` $4/$20 per MTok, `claude-sonnet-5-5` $2/$10, `claude-haiku-5-5` $0.10/$0.50, `claude-fable-5-2` $8/$40. Older models remain served.

**Rate-limit headers** (returned on responses). Source: https://platform.claude.com/docs/en/api/rate-limits

- `retry-after` (seconds; not on every 429, never on the spend-cap 429).
- `anthropic-ratelimit-requests-limit | -remaining | -reset`
- `anthropic-ratelimit-tokens-limit | -remaining | -reset` (most restrictive limit in effect)
- `anthropic-ratelimit-input-tokens-limit | -remaining | -reset`
- `anthropic-ratelimit-output-tokens-limit | -remaining | -reset`
- `anthropic-priority-input-tokens-*` and `anthropic-priority-output-tokens-*` (Priority Tier only)
- `anthropic-fast-*` when fast mode is used; `anthropic-workspace-id` names the workspace charged.
- Reset values are RFC 3339 timestamps. Remaining token values are rounded to the nearest thousand.

Limits are per organization, measured in RPM, ITPM and OTPM per model class, with a token-bucket algorithm. Tiers are now named Evaluation, Start (monthly spend cap $500), Build ($1,000), Scale ($200,000) and Custom. Distinguish three 429/400 cases by body, not status alone: ordinary rate limit (has `retry-after`), ramp limit (`error.details.error_code: "ramp_limit_exceeded"`), tier spend cap (`"enforced_spend_limit_reached"`, no `retry-after`, retries are pointless), and a user-set spend limit (HTTP 400 `invalid_request_error`). Same source.

**Usage and cost reporting**

- Every response carries a `usage` object (input, output, cache-creation and cache-read token counts). This is the only per-request number and it is exact.
- Organization history: Usage and Cost Analytics API (beta), `GET /v1/organizations/analytics/usage` and `GET /v1/organizations/analytics/cost`, beta header `analytics-usage-cost-2026-08-24`. Legacy endpoints `GET /v1/organizations/usage_report/messages` and `GET /v1/organizations/cost_report` remain supported. Legacy data is typically fresh within 5 minutes and may be polled once a minute. Sources: https://platform.claude.com/docs/en/manage-claude/usage-cost-analytics-api.md , https://platform.claude.com/docs/en/build-with-claude/usage-cost-api
- These need an **Admin API key** (`sk-ant-admin...`) or equivalent admin credential. Workspace API keys do not work. The Admin API is **not available to individual Console accounts** until they create an organization, and not to claude.ai Team plans. Same source.
- Configured limits can be read with the Rate Limits API, `GET /v1/organizations/rate_limits` (admin credential). Source: https://platform.claude.com/docs/en/manage-claude/rate-limits-api.md
- **I found no documented endpoint that returns the remaining prepaid credit balance.** The docs point to the Console Billing page for balances and credit expiry. Source: https://platform.claude.com/docs/en/about-claude/api-credits-for-subscribers

### 2. Claude Code through the user's own installed CLI

#### 2.1 What Anthropic's current docs permit

Source for this whole subsection: https://code.claude.com/docs/en/legal-and-compliance

- **Running Claude Code inside another product is addressed directly.** Unless otherwise agreed, preinstalling or running Claude Code in your product or service requires agreeing to the Commercial Terms of Service and meeting two conditions:
  - The binary is used as published. In Anthropic's words: "The Claude Code binary must not be modified." The product may not remove, disable or restrict any built-in authentication method.
  - The product may not pay for, resell or intermediate Claude usage for its users. Each end user authenticates with their own API key, subscription credentials or cloud-provider credential, and is billed directly.
- **Subscription OAuth is for Claude Code and Anthropic's own apps.** The page says OAuth authentication is intended exclusively for subscription purchasers and for ordinary use of Claude Code and other native Anthropic applications. Developers building products are told to use API keys. Third-party developers may not offer Claude.ai login in their own apps, may not route requests through Free/Pro/Max/Ultra credentials on behalf of users, and may not collect, store or intermediate Claude.ai credentials or session tokens. Sign-in must complete through Anthropic's own flow.
- **The carve-out that makes Demido's plan viable.** The same section says none of this prevents an end user from signing in to the unmodified Claude Code binary with their own subscription, including where a platform hosts Claude Code under the "offer Claude Code in their products" section.
- **Enforcement.** Anthropic reserves the right to enforce without prior notice, and points to sales for questions about permitted authentication for a given use case.
- **Usage policy.** Advertised Pro/Max/Ultra limits assume ordinary, individual usage of Claude Code and the Agent SDK.
- The Agent SDK overview repeats the rule: no claude.ai login or subscription rate limits for third-party products, including agents built on the SDK, unless previously approved. Source: https://code.claude.com/docs/en/agent-sdk/overview
- Consumer Terms (effective Oct 8, 2025) prohibit automated or non-human access except via an API key or where Anthropic explicitly permits it, and prohibit sharing account credentials. The documented `claude -p` mode with a subscription login is that explicit permission; anything beyond the documented surface is not. Source: https://www.anthropic.com/legal/consumer-terms

**Answer to "may a third-party open-source app spawn the user's Claude Code binary?"** Yes, on the published terms, provided Demido: runs the binary the user installed, unmodified; passes only documented flags; lets Claude Code do its own sign-in; never touches `~/.claude/.credentials.json`, the keychain entry or any token; does not hide or block any Claude Code auth method; and does not sit between the user and Anthropic's billing. The one formal step the page asks for is agreement to the Commercial Terms by whoever offers the product. That is an owner decision (see Open questions).

#### 2.2 Billing: correcting the brief

| Brief said | Verified state on 2026-11-20 |
| --- | --- |
| Since June 15, 2026 programmatic use no longer counts against the subscription's normal limits. | **Wrong.** The Help Center article carries a June 15, 2026 update saying the announced Agent SDK changes are paused. Agent SDK, `claude -p` and third-party app usage still count against subscription limits. Source: https://support.claude.com/en/articles/15036540-use-the-claude-agent-sdk-with-your-claude-plan |
| It draws from a separate monthly credit equal to the subscription fee. | **Partly right, different mechanism.** Since October 7, 2026, Max, Ultra and Team plans include monthly API credits: $100 (Max), $200 (Ultra), $20 per Standard seat and $100 per Premium seat on Team (pooled, capped at $500). Pro, Free and Enterprise get none. The credit is claimed by linking a Claude Console organization and is spent with an API key from that organization. It covers the Claude API, Managed Agents, the Agent SDK and `claude -p` **when run with that API key**. It does not cover Claude Code under a plan login. Source: https://platform.claude.com/docs/en/about-claude/api-credits-for-subscribers , https://support.claude.com/en/articles/17154008-monthly-api-credits-for-max-ultra-and-team-plans |
| Unused credit does not roll over. | **Right** for the API credits: they expire at the end of each billing cycle. Same source. |
| Beyond the credit, usage is billed at API rates through "extra usage" if enabled. | **Two separate things.** (a) When API credits run out, API requests stop unless the organization has purchased credits or auto-reload; nothing is charged to the Claude plan. (b) Under a plan login, going past the 5-hour or weekly limit can continue on "usage credits" (the current name for extra usage) at standard API rates, if the user enabled them. Sources: same, plus https://support.claude.com/en/articles/11145838-use-claude-code-with-your-pro-or-max-plan |

Plan names and prices today: Free, Pro ($20/month), Max ($100/month, 5x Pro usage), Ultra ($200/month), Team Standard and Premium seats, Enterprise. Source: https://claude.com/pricing . Pro and Max have a five-hour session limit and a weekly limit; Ultra has a weekly limit. Claude and Claude Code share the same limits. Source: https://support.claude.com/en/articles/11145838-use-claude-code-with-your-pro-or-max-plan

**Consequence for Demido.** Driving Claude Code under the user's subscription login spends the same 5-hour and weekly allowance the user has for interactive work. Driving it with an `ANTHROPIC_API_KEY` from the linked Console organization spends the monthly API credit first, then purchased credit. The usage indicator must show which of the two is in effect. Anthropic told users it would give advance notice before the paused change returns, so this can move again.

#### 2.3 Headless mode: the supported programmatic surface

Sources: https://code.claude.com/docs/en/headless , https://code.claude.com/docs/en/cli-reference

- `claude -p "<prompt>"` runs non-interactively. `--output-format` is `text`, `json` or `stream-json`. `--input-format stream-json` keeps the process open for multiple turns. Streaming tokens need `--output-format stream-json --verbose --include-partial-messages`.
- A stdin user message has the shape `{"type":"user","message":{"role":"user","content":...},"parent_tool_use_id":null}`. Content may be a string or blocks, including base64 image blocks. Source: https://code.claude.com/docs/en/agent-sdk/streaming-vs-single-mode
- The stream starts with a `system/init` event: `model`, `tools`, `mcp_servers`, `apiKeySource`, `claude_code_version`, `permissionMode`, optional `capabilities` (feature-detect on this, do not compare version strings). It ends each turn with a `result` message: `total_cost_usd`, `usage`, `modelUsage`, `session_id`, `num_turns`, `permission_denials`, `is_error`. Other documented events: `stream_event`, `assistant`, `user`, `system/api_retry`, `rate_limit_event`, `tool_progress`, `system/thinking_tokens`, task and hook events. Source: https://code.claude.com/docs/en/agent-sdk/typescript
- Useful flags: `--model`, `--effort low|medium|high|xhigh|max`, `--fallback-model`, `--max-turns`, `--max-budget-usd`, `--system-prompt`, `--append-system-prompt`, `--tools`, `--allowedTools`, `--disallowedTools`, `--permission-mode`, `--permission-prompt-tool`, `--permission-prompts none`, `--mcp-config`, `--strict-mcp-config`, `--setting-sources`, `--session-id`, `--resume`, `--continue`, `--fork-session`, `--no-session-persistence`, `--json-schema`, `--replay-user-messages`.
- **Permissions without the SDK:** `--permission-prompt-tool mcp__<server>__<tool>` hands each approval to an MCP tool that Demido serves. The result shape is allow (with `updatedInput`) or deny (with `message`). Without it, a `-p` run in Manual mode denies anything that would prompt. From v2.1.302, `-p` runs start in Manual mode when nothing sets a mode.
- **Interrupt without the SDK:** SIGINT ends the current turn cleanly; SIGTERM exits with code 143 and leaves the turn unfinished.
- **`--bare` matters for auth.** Bare mode skips hooks, skills, plugins, MCP discovery, memory and CLAUDE.md, and it never reads OAuth credentials or the keychain. A subscription login therefore requires a non-bare run. The docs say bare will become the default for `-p` in a future release.
- **Trust warning.** Without `--bare`, a `-p` session runs the hooks in a project's `.claude/settings.json` and connects the servers in its `.mcp.json` with no trust dialog, even in a folder never trusted before.
- **Credential precedence:** cloud-provider env, then `ANTHROPIC_AUTH_TOKEN`, then `ANTHROPIC_API_KEY` (always used in `-p` when present), then `apiKeyHelper`, then `CLAUDE_CODE_OAUTH_TOKEN`, then Anthropic profiles, then the subscription login. Source: https://code.claude.com/docs/en/authentication
- **Auth probe:** `claude auth status` prints JSON and exits 0 if logged in, 1 if not. Fields include `authMethod` (`none`, `claude.ai`, `oauth_token`, `api_key`, `api_key_helper`, `third_party`), `configDirectory` and, from v2.1.318, `loginExpiresAt`. Source: https://code.claude.com/docs/en/cli-reference
- **Install and identity:** official installers are `curl -fsSL https://claude.ai/install.sh | bash`, `irm https://claude.ai/install.ps1 | iex`, Homebrew cask `claude-code`, WinGet `Anthropic.ClaudeCode`, signed apt/dnf/apk repositories, and npm `@anthropic-ai/claude-code`. The native launcher is `~/.local/bin/claude`. `claude --version` prints for example `2.1.211 (Claude Code)`. Releases publish a GPG-signed `manifest.json` with SHA256 checksums (key fingerprint `31DD DE24 DDFA B679 F42D 7BD2 BAA9 29FF 1A7E CACE`). Source: https://code.claude.com/docs/en/setup

#### 2.4 Claude Agent SDK

- Languages: **Python and TypeScript only.** For any other language the docs tell you to run the CLI as a subprocess with `-p`. Source: https://code.claude.com/docs/en/agent-sdk/overview
- The SDK is a library that runs the Claude Code binary. The TypeScript package **bundles a native Claude Code binary** per platform as an optional dependency (`@anthropic-ai/claude-agent-sdk-linux-x64` and siblings); `pathToClaudeCodeExecutable` points it at a separately installed one. Source: https://code.claude.com/docs/en/agent-sdk/typescript
- Versions and licenses from the registries: npm `@anthropic-ai/claude-agent-sdk` 0.3.329, license field "SEE LICENSE IN README.md" (the docs say use is governed by Anthropic's Commercial Terms). PyPI `claude-agent-sdk` 0.2.186, license MIT, with platform-specific wheels. Sources: https://registry.npmjs.org/@anthropic-ai/claude-agent-sdk/latest , https://pypi.org/pypi/claude-agent-sdk/json
- **Verdict for Demido:** do not use either SDK. A Rust core cannot link them, the TypeScript one is not under a GPL-compatible license, and both carry the proprietary CLI binary, which would break the "no redistribution" rule. Drive the user's installed `claude` directly over stream-json. That is the route Anthropic documents for other languages.

#### 2.5 Reading usage and remaining allowance programmatically

Source: https://code.claude.com/docs/en/agent-sdk/typescript unless noted.

1. **`rate_limit_event` messages** arrive in the stream when the session meets a limit. Fields: `status` (`allowed`, `allowed_warning`, `rejected`), `resetsAt`, `utilization`, `errorCode: "credits_required"` (subscription allowance exhausted), `canUserPurchaseCredits`, `hasChargeableSavedPaymentMethod`, `overageEnabled` (usage credits switched on; v2.1.295+).
2. **`/usage` sent as a prompt** returns an assistant message whose optional `usage_report` field holds the structured plan usage: `rate_limits.limits[]` with `kind` (for example `session`, `weekly_all`, `weekly_scoped`), `group`, `percent` (0-100), `resets_at`, optional model or surface `scope`, `severity`, `is_active`; and `rate_limits.extra_usage` with `is_enabled`, `monthly_limit`, `used_credits`, `utilization`, `currency` (minor units). It also has a `session` block with cost and per-model totals. Conditions: claude.ai credential, a known plan type or the `user:profile` scope, and not usage-based billing. A `claude setup-token` token does not qualify by default (it only has `user:inference`). The type is marked experimental and requires SDK v0.3.273 or later.
3. **Per-run cost:** `result.total_cost_usd` and `modelUsage` (per model: input, output, cache-read and cache-creation tokens, `costUSD`, `costBasis`). These are client-side estimates from a bundled price table, not billing data. Under a subscription login the dollar figure is notional. Per-step `output_tokens` on assistant messages is a placeholder; read output totals from the result. Source: https://code.claude.com/docs/en/agent-sdk/cost-tracking
4. **Status line JSON** exposes `rate_limits.five_hour` and `rate_limits.seven_day` (`used_percentage`, `resets_at`), but only to a status-line script inside an interactive session. Not usable from `-p`. Source: https://code.claude.com/docs/en/statusline
5. **API-credit balance** (when the user runs Claude Code with an API key): Console Billing page only. No endpoint found.

#### 2.6 Branding and naming

- For products that run Claude Code: you may state accurately, in plain text, that the product runs Claude Code or has it preinstalled. You may not use the Claude Code or Anthropic names or logos as part of your own product, feature or company name, in your own logo, or in a way that suggests Anthropic built, endorses or partners with the product. Any other use needs written permission under the Trademark Guidelines. Source: https://code.claude.com/docs/en/legal-and-compliance
- Trademark Guidelines (effective Aug 1, 2024): marks may be used only as specifically permitted and in pre-approved materials; no alterations; no implied sponsorship or endorsement. Source: https://www.anthropic.com/legal/trademark-guidelines
- Agent SDK branding guidance (applies to SDK-built agents, listed for completeness): allowed labels are "Claude Agent", "Claude" inside an Agents menu, or "<YourAgent> Powered by Claude"; not permitted are "Claude Code", "Claude Code Agent" and Claude Code-style ASCII art or visuals. Source: https://code.claude.com/docs/en/agent-sdk/overview

**Reading for Demido's UI:** describe, do not brand. Good: a provider row reading "Claude Code CLI - runs the Claude Code you installed". Bad: a Demido feature named "Claude Code Mode", the Claude or Anthropic logo as a provider icon, a look that imitates Claude Code's terminal. Add a line in About and in the provider panel that Demido Studio is not affiliated with or endorsed by Anthropic.

### 3. Google Gemini API

The docs moved: `ai.google.dev/gemini-api/docs/*` now redirects to `aistudio.google.com/docs/*`.

- **Auth:** API key in `x-goog-api-key`, base `https://generativelanguage.googleapis.com/v1beta`. Source: https://aistudio.google.com/docs/interactions
- **Two APIs.** The **Interactions API** (`POST /v1beta/interactions`) has been generally available since June 2026 and is recommended for new projects. `generateContent` / `streamGenerateContent` are called legacy but remain fully supported, with no sunset date. Sources: https://aistudio.google.com/docs/interactions , https://aistudio.google.com/api-reference/generate-content
- **Interactions stores conversations by default** (`store: true`; retained 55 days on the paid tier, 1 day on the free tier). `store: false` opts out but disables `previous_interaction_id` and background execution. Same source. For a local-first app this default is wrong; Demido must send `store: false` or use `generateContent`, which is stateless.
- **Models today:** Gemini 4 family (`gemini-4-carbon`, `gemini-4-argon`, `gemini-4-neon`, `gemini-4-hydrogen`; 1M context; text, image, audio, video, PDF input), `gemini-3.8-flash`, `gemini-3.6-flash`, `gemini-3.5-flash-lite`, `gemini-3.1-flash-lite`, previews such as `gemini-3-flash-preview` and `gemini-3.1-pro-preview`. `gemini-2.5-flash` and `gemini-2.5-pro` are deprecated and reroute to other models on December 1, 2026. Source: https://aistudio.google.com/docs/models
- **Model list endpoint:** `GET /v1beta/models` and `GET /v1beta/models/{model}` return `name`, `baseModelId`, `displayName`, `inputTokenLimit`, `outputTokenLimit`, `supportedGenerationMethods`, `thinking` (boolean), sampling defaults. No explicit vision or tool flags, no prices. Source: https://aistudio.google.com/api-reference/models
- **Thinking configuration.**
  - Interactions: `generation_config.thinking_level` with `minimal`, `low`, `medium`, `high`, `xhigh` (which levels exist depends on the model; `xhigh` only on Gemini 4 Neon and Hydrogen; `minimal` not on Gemini 4 or 3.8 Flash). `thinking_summaries: "auto" | "none"`. Source: https://aistudio.google.com/docs/thinking
  - generateContent: `thinkingConfig.thinkingLevel` (`MINIMAL`, `LOW`, `MEDIUM`, `HIGH`, `XHIGH`), `thinkingBudget` (token budget, the 2.5-era control), `includeThoughts`. Source: https://aistudio.google.com/api-reference/generate-content
  - Thinking cannot be switched off on Gemini 2.5 Pro, Gemini 3 or Gemini 4. Source: https://aistudio.google.com/docs/openai
  - Thought signatures are opaque and must be sent back unchanged in stateless use; `MISSING_THOUGHT_SIGNATURE` is a finish reason.
- **Usage metadata.**
  - generateContent `usageMetadata`: `promptTokenCount`, `cachedContentTokenCount`, `candidatesTokenCount`, `toolUsePromptTokenCount`, `thoughtsTokenCount`, `totalTokenCount`, per-modality detail arrays, `serviceTier`.
  - Interactions `usage`: `total_input_tokens`, `total_output_tokens`, `total_thought_tokens`. Thinking tokens are billed as output.
- **Tools and vision:** `tools[].functionDeclarations`, `functionCall {id, name, args}`, `functionResponse`; images as `inlineData {mimeType, data}` or `fileData {fileUri}`. Source: https://aistudio.google.com/api-reference/generate-content
- **OpenAI-compatible endpoint** exists at `https://generativelanguage.googleapis.com/v1beta/openai/` (chat completions, embeddings, models; `reasoning_effort` mapped to thinking levels). It is still labelled beta. Source: https://aistudio.google.com/docs/openai
- **Rate limits:** measured in RPM, TPM and RPD (plus per-model extras), applied **per project, not per API key**; RPD resets at midnight Pacific. Tiers: Free, Tier 1 (billing linked), Tier 2 ($100 spent and 3 days), Tier 3 ($1,000 and 30 days). Exceeding returns `429 RESOURCE_EXHAUSTED`. The docs page **does not list per-model free-tier numbers**; it names the AI Studio rate-limit page (login required) as the source of truth. **No response headers and no API for remaining quota are documented.** Source: https://aistudio.google.com/docs/rate-limits
- **Free tier:** available on Gemini 4 Hydrogen, 3.8 Flash, 3.6 Flash, 3.5 Flash-Lite, 3.1 Flash-Lite, 3 Flash Preview and 2.5 Flash-Lite. Not on Gemini 4 Carbon, Argon or Neon, nor 3.1 Pro Preview. Free-tier content is used to improve Google's products; paid-tier content is not. Source: https://aistudio.google.com/docs/pricing
- **Terms (effective March 23, 2026):** on unpaid services, human reviewers may read and annotate inputs and outputs. Only paid services may be used when making API clients available to users in the EEA, Switzerland or the UK. Users must be 18 or older. Source: https://aistudio.google.com/docs/terms
- **Gemini CLI OAuth: prohibited, confirmed.** The official Gemini CLI terms page says that directly accessing the services behind Gemini CLI (for example Gemini Code Assist) with third-party software, tools or services is a violation of the applicable terms, names using a third-party agent with Gemini CLI OAuth as the example, and warns of suspension or termination. Sources: https://www.geminicli.com/docs/resources/tos-privacy and the same file in the canonical repo https://github.com/google-gemini/gemini-cli (docs/resources/tos-privacy.md)

### 4. OpenRouter

- **API:** base `https://openrouter.ai/api/v1`. `POST /chat/completions` (OpenAI-compatible) and `POST /responses`. `Authorization: Bearer <key>`. Optional attribution headers `HTTP-Referer`, `X-OpenRouter-Title` (`X-Title` also accepted), `X-OpenRouter-Categories`. Source: https://openrouter.ai/docs/api/reference/overview
- **Usage in every response:** `usage.prompt_tokens`, `completion_tokens`, `total_tokens`, `cost` (credits), `is_byok`, `prompt_tokens_details.cached_tokens` and `cache_write_tokens`, `completion_tokens_details.reasoning_tokens`, `cost_details.upstream_inference_cost`. In streams it arrives once in the final chunk. `GET /api/v1/generation?id=` returns stats for one request afterwards. `finish_reason` is normalized, with the provider's own value in `native_finish_reason`. Same source.
- **Model list:** `GET /api/v1/models`, auth optional. Filters include `supported_parameters`, `input_modalities`, `output_modalities`, `context`, price bounds, `zdr`, `q`, `sort`, `offset`, `limit`. Per model: `id`, `canonical_slug`, `name`, `created`, `description`, `context_length`, `architecture {input_modalities, output_modalities, tokenizer, instruct_type}`, `pricing {prompt, completion, request, image, web_search, internal_reasoning, input_cache_read, input_cache_write, overrides}`, `top_provider {context_length, max_completion_tokens, is_moderated}`, `per_request_limits`, `supported_parameters` (for example `tools`, `tool_choice`, `reasoning`, `include_reasoning`, `structured_outputs`, `response_format`), `default_parameters`, `reasoning {supported_efforts, default_effort, default_enabled, mandatory}`, `expiration_date`. `GET /api/v1/models/count` and `GET /api/v1/model/{author}/{slug}` also exist. Sources: https://openrouter.ai/docs/guides/overview/models.md , https://openrouter.ai/docs/api-reference/list-available-models
- **Reasoning control:** unified `reasoning` object with `effort` (`none`, `minimal`, `low`, `medium`, `high`, `xhigh`, `max`) or `max_tokens`, plus `exclude` and `enabled`. Reasoning comes back in `message.reasoning` and a `reasoning_details` array (`reasoning.summary`, `reasoning.encrypted`, `reasoning.text`). `reasoning_details` must be returned unchanged on tool-calling turns. Source: https://openrouter.ai/docs/use-cases/reasoning-tokens
- **Key, credits and activity endpoints.**
  - `GET /api/v1/key` works with a normal key: `label`, `limit`, `limit_remaining`, `limit_reset`, `usage`, `usage_daily`, `usage_weekly`, `usage_monthly`, BYOK equivalents, `is_free_tier`, `free_model_daily_requests {used, limit, remaining}`, budgets. Source: https://openrouter.ai/docs/api-reference/limits
  - `GET /api/v1/credits` returns `total_credits` and `total_usage` but **requires a management key**; a normal key gets 403. Source: https://openrouter.ai/docs/api/api-reference/credits/get-credits
  - `GET /api/v1/activity` and the analytics endpoints also need a management key. Source: https://openrouter.ai/docs/guides/features/activity.md
- **Free models:** append `:free` to a model that has a free entry, or use the `openrouter/free` router. Limits: 20 requests per minute; 50 per day with fewer than 10 credits ever purchased, 1,000 per day with 10 or more. A negative balance returns 402, including for free models. `X-RateLimit-*` headers appear only on OpenRouter's own 429 responses, not on successful ones. Sources: https://openrouter.ai/docs/guides/routing/model-variants/free.md , https://openrouter.ai/docs/api-reference/limits
- **Privacy:** OpenRouter says it does not store prompts or responses unless the user opts in. Each upstream provider has its own retention and training policy; account settings control, separately for paid and free models, whether requests may go to providers that train on inputs. Sources: https://openrouter.ai/docs/guides/privacy/data-collection.md , https://openrouter.ai/docs/guides/privacy/provider-logging.md

### 5. OpenCode CLI

**Canonical source and license.** `github.com/sst/opencode` now resolves to **`github.com/anomalyco/opencode`** (not a fork, about 228k stars, pushed today, homepage opencode.ai). License MIT, copyright line "Copyright (c) 2025 opencode". Sources: https://api.github.com/repos/sst/opencode , https://github.com/anomalyco/opencode/blob/dev/LICENSE

**Two major versions are live.** Latest V1 release v1.19.6 (2026-11-17) offers an in-app upgrade to OpenCode 2. V2 releases exist (v2.3.2, 2026-11-18). The docs at opencode.ai/docs are V2 docs. V2 install: `curl -fsSL https://opencode.ai/v2/install | bash`, npm `@opencode/cli`, Homebrew `anomalyco/tap/opencode-v2`, Arch `opencode` (stable) or AUR `opencode-beta`. Sources: https://api.github.com/repos/anomalyco/opencode/releases , https://github.com/anomalyco/opencode (README)

**What the brief's notes look like in V2**

| Brief (V1 era) | V2 state |
| --- | --- |
| `opencode serve` runs a headless HTTP server with an OpenAPI endpoint. | `opencode serve` still exists (`--hostname`, `--port`, `--cors`). Auth is HTTP Basic, username fixed to `opencode`, password from `OPENCODE_PASSWORD` (`OPENCODE_SERVER_PASSWORD` still accepted). By default the CLI now discovers or starts one shared background service per user (`opencode service start|stop|status`, `--standalone` for a private server, `--server <url>` to target one). Sources: https://opencode.ai/docs/migrate-v1/ , https://opencode.ai/docs/cli/ , https://opencode.ai/docs/cli/commands/ |
| OpenAPI document. | Published at https://opencode.ai/docs/openapi.json (OpenAPI 3.1.0, 173 operations, 518 schemas). Routes live under `/api/...`. The reference labels the surface experimental; some routes say they may change without compatibility guarantees. Source: https://opencode.ai/docs/api/ |
| JS SDK. | Two packages. `@opencode/client` is the TypeScript HTTP client (`OpenCode.make({baseUrl})`, `client.session.create`, `client.session.prompt`, `client.event.subscribe`, plus a Node-only service helper that can discover or start the service). `@opencode/sdk` embeds the server in-process. V1 integrations must be ported. Sources: https://opencode.ai/docs/build/client/ , https://opencode.ai/docs/sdk |
| Zen. | The hosted gateway is now called OpenCode Console. Pay-as-you-go base `https://opencode.ai/inference`; a $10/month "Go" subscription uses `https://opencode.ai/inference/go`. Source: https://opencode.ai/docs/console/harnesses/ |

**Key V2 API routes** (all from https://opencode.ai/docs/api/): `GET /api/info`; `GET|POST /api/session`; `POST /api/session/{id}/prompt`; `POST /api/session/{id}/interrupt`; `GET /api/event` (live events; volatile, a slow consumer fails the stream and missed events are not replayed); `GET /api/experimental/session/{id}/log` (durable log with `after` and `follow=true`); permission routes `GET /api/session/{id}/permission` and `POST /api/session/{id}/permission/{requestID}/reply`; `GET /api/model`, `GET /api/provider`; `GET /api/experimental/usage/limits` (Go allowance and Console budgets); `POST /api/pair` and `GET /auth/connect/{code}` for pairing. Event names include `session.status`, `session.idle`, `session.text.delta`, `session.reasoning.delta`, `session.tool.input.delta`, `permission.asked`, `permission.replied`.

**A second, better-fitting route: ACP.** `opencode acp` runs OpenCode as an Agent Client Protocol agent: newline-delimited JSON-RPC over stdin/stdout, protocol version 1, a private server, no network port, exits when the client closes stdin. Supported: sessions (create, load, resume, fork, list, close), cancellation, model and effort selection as config options, text and image prompts, client-supplied MCP servers (stdio and HTTP), permission requests (allow once, always, reject), questions, usage and compaction updates. Limits: one prompt at a time per session, usage reporting follows a draft spec and excludes child sessions. Source: https://opencode.ai/docs/cli/acp/

**Other CLI entry points:** `opencode run "<prompt>"` with `--model provider/model`, `--format json` (NDJSON), `--session`, `--continue`, `--agent`, `--file`, `--auto`, `--standalone`; `opencode stats` with `--days`, `--models`, `--cost`, `--json`; `opencode auth list --format json`; `opencode api <METHOD path>`. Source: https://opencode.ai/docs/cli/commands/

**Free models**

- Exposed through the `opencode` provider as `opencode/<model-id>`. Current free list: `big-pickle`, `iceman-free`, `inkling-2-small-free`, `deepseek-v4.1-flash-free`, `beam-free`, `flint-40b-free`, `longcat-2.5-free`, `hy4-flash-free`, `mimo-v2.6-flash-free`, `nemotron-3.5-lightning-free`, `nemotron-3.5-super-vl-free`, `jev-1.13-free`. Most are described as free for a limited time. The live list is at `https://opencode.ai/inference/v1/models`. Source: https://opencode.ai/docs/console/models/
- **Account:** the homepage says OpenCode comes with free models and no account is needed. Source: https://opencode.ai/ . I did not test this.
- **Client restriction, confirmed.** The harness docs say free-tier chat models are available only from within OpenCode; other harnesses receive HTTP 403 for those models even with a Console API key. Source: https://opencode.ai/docs/console/harnesses/
- **Training and retention differ per model.** Stated on the models page: Big Pickle and Flint 40B data may be used to improve the model; Iceman data is retained and used; Inkling 2 Small data may be used for training and is region-restricted; the two Nemotron models are trial-only, logged, and carry a warning not to submit personal or confidential data; DeepSeek V4.1 Flash Free, Hy4 flash Free and LongCat 2.5 Free are zero-retention and not used for training; Beam Free is retained 30 days for safety. Two models carry no statement.
- **Limits:** the only published number is 100 requests per day on LongCat 2.5 Free. No IP-level limits are documented.
- `OPENCODE_DISABLE_FREE_MODELS=1` on the server process hides them. Source: https://opencode.ai/docs/models/

**Terms of service for the hosted services** (effective Nov 8, 2026): the open-source software is governed only by its repository license; the hosted services (website, inference, hosted software) have a prohibited-conduct list that includes automatically or programmatically extracting data or output, running processes while not logged in, and using multiple accounts to get around usage limits or access restrictions. Content from unpaid accounts may be used to improve the services. Use is for internal purposes, not on behalf of third parties. Source: https://opencode.ai/legal/terms-of-service

### 6. Rust crates

All data from crates.io on 2026-11-20.

| Crate | Version (date) | License | What it is | Fit |
| --- | --- | --- | --- | --- |
| `genai` | 0.7.5 (2026-11-14) | MIT OR Apache-2.0 | Multi-provider chat client: OpenAI, Anthropic, Gemini (including an Interactions adapter), OpenRouter, Ollama and more; streaming, tools, images, reasoning effort, usage normalization. Single maintainer, 0.x with breaking releases; its README says it favours ergonomics and common behaviour over depth. https://crates.io/crates/genai | Closest match. Still an abstraction that normalizes away what Demido must show. |
| `rig-core` | 0.46.0 (2026-10-31), MSRV 1.97.1 | MIT | Opinionated agent framework (agents, tools, vector stores). https://crates.io/crates/rig-core | Overlaps with Demido's own harness. Wrong layer. |
| `async-openai` | 0.43.0 (2026-11-01) | MIT | Complete OpenAI client and types. https://crates.io/crates/async-openai | OpenAI wire only. Possible type source, not needed. |
| `llm` (graniet) | 1.3.8 (2026-04-19) | MIT | Multi-backend client. https://crates.io/crates/llm | Seven months without a release. Skip. |
| `agent-client-protocol` | 5.4.0 (2026-11-18), MSRV 1.88 | Apache-2.0 | Official ACP types and traits. https://crates.io/crates/agent-client-protocol | Use for the OpenCode integration. |
| `rmcp` | 3.11.0 (2026-11-17), MSRV 1.88 | Apache-2.0 | Official Rust MCP SDK. https://crates.io/crates/rmcp | Needed anyway; serves the Claude Code permission tool. |
| `reqwest` | 0.13.5 (2026-09-08) | MIT OR Apache-2.0 | HTTP client. https://crates.io/crates/reqwest | Base of the thin clients. |
| `eventsource-stream` | 0.2.3 (2022-02-17) | MIT OR Apache-2.0 | SSE parser over a byte stream. https://crates.io/crates/eventsource-stream | Works, but untouched since 2022. An SSE parser is about 100 lines; write our own. |
| `claude-codes` | 2.1.330 (2026-11-20) | Apache-2.0 | Community typed models for the Claude Code stream-json protocol, released almost daily. https://crates.io/crates/claude-codes | Reference for message shapes. Not official; do not depend on it. |

About ten other community crates wrap the Claude Code CLI (`claude-agent-sdk-rs`, `cc-sdk`, `anthropic-agent-sdk` and others). None is from Anthropic, most are stale by months. Source: https://crates.io/search?q=claude%20code%20sdk

## Options compared

### A. How to talk to raw model providers

| Option | For | Against |
| --- | --- | --- |
| A1. Adopt `genai` as the provider layer | Fast start; many providers; licence fine | The user cannot see or edit the real request. Provider-specific features (Anthropic cache breakpoints, effort, refusal handling, Gemini thought signatures, OpenRouter `reasoning_details`, rate-limit headers) arrive late or flattened. One maintainer, 0.x. A breaking provider change (Anthropic's `2026-10-21` version, Gemini's Interactions) leaves us waiting on someone else. |
| A2. Adopt `rig` | Agent features included | Duplicates and fights Demido's own harness, which is the product. |
| **A3. Own thin clients on `reqwest`, three wire adapters** | Exact wire control; a "show me the request" view is trivial; headers and usage read first-hand; small dependency set; one config file adds any OpenAI-compatible provider | We maintain three adapters. Each is a few hundred lines of serde types plus an SSE loop. |
| A4. Route everything through OpenAI-compatible endpoints (Gemini has one, OpenRouter proxies Anthropic) | One adapter | Loses native features and exact usage; Gemini's compatibility layer is still beta; sends Anthropic traffic through a third party. |

### B. How to drive Claude Code

| Option | For | Against |
| --- | --- | --- |
| **B1. Spawn `claude -p` with stream-json in and out, permissions through `--permission-prompt-tool`** | Documented for non-SDK languages; works from Rust; uses the user's installed binary; no redistribution | We track the event schema ourselves; interrupt is a signal. |
| B2. Agent SDK (TypeScript or Python) in a sidecar | Richer callbacks | Needs Node or Python at runtime; TS package is proprietary-licensed and bundles the proprietary binary; contradicts lightweight and no-redistribution. |
| B3. Use the subscription OAuth token directly against the API | None | Prohibited. Blocked server-side per the brief. |

### C. How to drive OpenCode

| Option | For | Against |
| --- | --- | --- |
| **C1. `opencode acp` over stdio** | Open protocol with an official Rust crate; no port, no password; process lives exactly as long as the session, which matches "load only when needed"; requests come from the genuine client so free models work | Usage reporting is draft-level; one prompt at a time per session. |
| C2. `opencode serve` + HTTP `/api` | Full surface: catalog with prices, budgets, durable logs | Labelled experimental; a port and a password to manage; contract already broke once between V1 and V2. |
| C3. `opencode run --format json` per prompt | Trivial | No interactive permissions, weak session control. |
| C4. Call `opencode.ai/inference` directly | None for free models | 403 by design. Faking the client is spoofing. Not an option. |

### D. Gemini API surface

| Option | For | Against |
| --- | --- | --- |
| **D1. `generateContent` (stateless)** | Nothing stored server-side; full `usageMetadata`; current thinking levels present in the reference | Called legacy; new agent features may land in Interactions first. |
| D2. Interactions with `store: false` | Recommended by Google for new work | Default is server-side storage; stateless mode needs every thought block echoed; less field-level documentation. |

## Recommendation

### 1. Provider abstraction: two traits and a usage source

```
ModelProvider   raw inference. Demido owns the loop, the tools and the prompt.
  descriptor()  -> id, label, auth fields, endpoints, legal notes, billing kind
  list_models() -> ModelInfo { normalized capabilities + the raw provider JSON }
  stream(req)   -> ChatEvent stream (text, reasoning, tool call, usage, raw frames)
  count_tokens()-> optional

AgentProvider   external agent loop. Demido is the client and the UI.
  probe()       -> installed? path, version, auth state, billing kind
  start(opts)   -> AgentSession
  session: send(input), events(), reply_permission(), interrupt(), close()

UsageSource     where the indicator numbers come from
  snapshot()    -> meters[], spend, balance, each tagged Reported | Estimated | LocalCount
```

- `ModelProvider` implementations: Anthropic API, Gemini API, OpenRouter, and the local runtime. `AgentProvider` implementations: Claude Code CLI, OpenCode CLI.
- Three wire adapters back all `ModelProvider`s: `anthropic_messages`, `gemini_generate_content`, `openai_chat`. A provider is a data file (endpoint, auth header, model-list call, quirks) plus one adapter. Adding another OpenAI-compatible service is a config file, no code. That is the "swap parts without touching many places" principle applied.
- One internal message model with typed parts: text, image, file, tool call, tool result, reasoning. Each reasoning or tool part carries an opaque `provider_raw` blob that is sent back byte-for-byte. Anthropic thinking blocks, Gemini thought signatures and OpenRouter `reasoning_details` all break if rewritten.
- Every request and response is recorded as sent, with secrets redacted, and shown in a wire inspector. This is the transparency principle, and it is the main reason not to put a third-party abstraction crate in the path.
- Say plainly in the UI that agent-type providers bring their own loop and tools. Demido's small-model techniques, prompt compression and tool indexing do not run inside them.

### 2. Crates

Own thin clients (option A3). Depend on `reqwest`, `serde`, `tokio`, `rmcp`, `agent-client-protocol`. Write the SSE parser in-tree. Use `genai` and `claude-codes` as reading material only. All are MIT, Apache-2.0 or dual, which is compatible with GPL-3.0-or-later.

### 3. Claude Code integration

- Spawn the user's installed `claude` (found on PATH or a user-set path). Never download, bundle or patch it. Offer a link to the official install page; whether Demido may run the official installer for the user is an owner decision.
- Invocation: `claude -p --input-format stream-json --output-format stream-json --verbose --include-partial-messages --permission-prompt-tool mcp__demido__approve --mcp-config <generated> [--model] [--effort] [--resume <id>] [--max-budget-usd]`. No `--bare` when the user chose their subscription login.
- Map Demido's permission levels onto `--permission-mode` plus the MCP approval tool. Demido's own tools reach Claude Code as MCP servers through `--mcp-config`.
- **Scrub the child environment.** If `ANTHROPIC_API_KEY` or `ANTHROPIC_AUTH_TOKEN` is set in Demido's process, a `-p` run uses it silently and bills the API instead of the subscription. Remove both unless the user picked "use my API key" for this provider. Show the active billing source from `system/init.apiKeySource` and `claude auth status`.
- Pass `--setting-sources` and `--strict-mcp-config` deliberately, and show the user what project hooks and MCP servers a non-bare run will load. A `-p` run does not ask for folder trust.
- Feature-detect on `system/init.capabilities` and `claude_code_version`. Refuse politely below a minimum version.
- Default to "agent backend" use: Claude Code runs its own loop with its own tools. Do not ship a mode that strips its tools and system prompt to turn the subscription into a generic model endpoint for Demido's own loop until Anthropic confirms that is acceptable. The flags exist, but that use is the closest thing in this design to routing requests through plan credentials.
- Never offer a field to paste a `claude setup-token` token. Collecting or storing Claude.ai tokens is exactly what the legal page forbids. On a server, the user signs in with `claude auth login` in their own shell.
- Tell Max and Ultra users about the monthly API credit and the direct Anthropic API provider. That route has no policy grey area and gives Demido full control of the loop.

### 4. OpenCode integration

- Primary: `opencode acp` over stdio with the `agent-client-protocol` crate. One process per active session, killed when idle.
- Secondary, for the catalog and budgets only: a short-lived `opencode serve` on 127.0.0.1 with a random port and random `OPENCODE_PASSWORD`, or `opencode api`, with Rust types generated from the published OpenAPI document and a version gate.
- Target V2. Detect V1 and tell the user it is not supported rather than guessing at its API.
- Never send a request to `opencode.ai/inference` from Demido for free models, and never forge OpenCode's headers.
- Show the per-model data-use note next to every free model, taken from the models page, and mark models that carry no statement as unknown.
- Keep scheduled, unattended and bulk jobs away from OpenCode's hosted free models by default. The hosted-service terms prohibit programmatic output extraction and processes running while not logged in.

### 5. Gemini and OpenRouter

- Gemini: API key only. Start with `generateContent` (stateless). Keep the adapter boundary clean so an Interactions adapter with `store: false` can be added. No Gemini CLI OAuth, ever.
- OpenRouter: OpenAI chat adapter plus its extras (`reasoning`, `reasoning_details`, `usage.cost`, model metadata). Attribution headers are a visible setting, not a silent default.

### 6. Capability mapping

Unified thinking setting in Demido: `off | minimal | low | medium | high | xhigh | max`, plus an optional token budget. Each model's descriptor lists which values it accepts, its default, and whether `off` exists. The selector shows only what the model supports.

| Capability | Anthropic API | Claude Code CLI | Gemini API | OpenRouter | OpenCode CLI |
| --- | --- | --- | --- | --- | --- |
| Thinking level | `thinking: adaptive` + `output_config.effort` low..max | `--effort` low..max, `/effort` mid-session | `thinkingLevel` MINIMAL..XHIGH (or `thinking_level`), per model | `reasoning.effort` none..max, or `reasoning.max_tokens` | model variant (`provider/model#high`); ACP effort config option |
| Thinking off | `disabled` only on some models; `between_tools` on Sonnet 5.5; impossible on Opus 5.5 and Fable | not a separate switch | impossible on Gemini 3 and 4; budget 0 on 2.5 Flash-Lite era | `effort: "none"` where the model allows | `none` variant where the catalog offers it |
| Token budget | `budget_tokens` on Haiku 4.5 and older only | no | `thinkingBudget` (2.5 era) | `reasoning.max_tokens` | catalog `budget` min/max |
| Reasoning shown | `display: "summarized"` summaries | thinking blocks in stream | `includeThoughts` / `thinking_summaries` | `reasoning`, `reasoning_details` | `session.reasoning.delta` events |
| Must echo back | thinking blocks unchanged | handled by the CLI | thought signatures | `reasoning_details` | handled by the CLI |
| Discover support | Models API `capabilities.thinking`, `.effort` | `system/init` | models `thinking` boolean + docs table | models `reasoning` object, `supported_parameters` | `/api/model` capabilities |
| Tool calling | `tools` / `tool_use` / `tool_result`, `strict`; no forced choice on newest models | agent runs its own tools; ours via MCP | `functionDeclarations` / `functionCall` / `functionResponse` | OpenAI `tools` / `tool_calls`; check `supported_parameters` | agent runs its own tools; ours via MCP |
| Permission prompts | Demido's own | `--permission-prompt-tool` MCP tool | Demido's own | Demido's own | ACP permission request, or `/api/session/{id}/permission/.../reply` |
| Vision input | image blocks base64 / url / file | image block in stream-json user message | `inlineData` / `fileData` | `image_url` parts; `architecture.input_modalities` | ACP image content; `--file` |
| Discover vision | `capabilities.image_input` | assume model default | not exposed; docs table | `input_modalities` contains `image` | capabilities `input` list |

### 7. Where each usage indicator gets its numbers

| Provider | Live per-request | Limits and remaining | Spend and balance | Honest label |
| --- | --- | --- | --- | --- |
| Anthropic API | `usage` in each response (exact tokens) | `anthropic-ratelimit-*` headers on every response; `retry-after`; error codes for ramp and spend cap | Local sum of tokens x editable price table. With an Admin key: cost analytics endpoint. Balance: none by API, link to Console Billing | Tokens: reported. Cost: estimated unless Admin key. Balance: not available |
| Claude Code, subscription login | `result.usage`, `modelUsage` | `rate_limit_event` (status, utilization, resetsAt); `/usage` -> `usage_report.rate_limits.limits[]` (session and weekly percent, reset time) | `extra_usage` block for usage credits; `total_cost_usd` shown as "API-equivalent estimate", not money owed | Limits: reported (experimental schema). Dollars: notional |
| Claude Code, API key | same stream fields | none from the CLI | `total_cost_usd` estimate; real balance only in Console | Estimated |
| Gemini | `usageMetadata` / `usage` (exact tokens, thinking separate) | No headers, no API. Local counters per project for RPM, TPM, RPD against limits the user confirms from AI Studio; RPD resets midnight Pacific; learn from 429s | Local tokens x editable price table; free tier shown as $0 with a data-use badge | Tokens: reported. Limits and cost: local estimate |
| OpenRouter | `usage` incl. `cost` in every response | `GET /key`: `limit_remaining`, `free_model_daily_requests`; 429 headers | `GET /key` usage (day, week, month, all-time). With a management key: `GET /credits` gives purchased minus used | Reported. Account balance needs a management key |
| OpenCode | ACP usage updates; session events | `GET /api/experimental/usage/limits` (Go allowance, Console budgets); free-model limits unpublished | `opencode stats --json`; free models $0 | Mixed; draft-level |

Rules for the indicator UI: always show where a number came from (reported, estimated, counted locally); never draw a "remaining" bar when the provider gives no limit; show the billing source (subscription limits, API credit, free tier, prepaid credits) on every remote session.

## Legal blockers and constraints

### Claude Code

1. No use of subscription OAuth tokens outside Claude Code itself. No reading of `~/.claude/.credentials.json` or the keychain entry. No "Sign in with Claude" in Demido. No token paste field.
2. Unmodified binary only, as published by Anthropic. No bundling or redistribution (also our own rule).
3. Demido may not remove, disable or restrict any Claude Code auth method. Do not force API-key-only or subscription-only.
4. Each user uses their own credentials and is billed directly. A multi-user Demido server sharing one person's subscription would break this and the Consumer Terms' no-sharing clause. Restrict the Claude Code provider to the account owner in server mode.
5. Running Claude Code inside a product "requires agreeing to" the Commercial Terms. Someone has to decide whether the project accepts that. Recommended: accept the conditions and email Anthropic for written confirmation of this exact design.
6. Subscription limits assume ordinary, individual usage. Round-the-clock schedules, monitors and parallel agents on a consumer plan invite enforcement, which can come without notice.
7. Naming: plain-text factual reference only. No Claude or Anthropic logo, no feature named after Claude Code, no imitation of its look, no implied endorsement.
8. Agent SDK: not usable (license, bundled binary, wrong language).

Nothing above blocks the integration as designed. Items 4, 5 and 6 constrain server mode, governance and scheduling.

### OpenCode

1. MIT software; spawning it is unrestricted.
2. Free models work only through the real client. Direct calls get 403; imitating the client is off the table.
3. Hosted-service terms prohibit programmatic extraction of output, unattended processes and multi-account limit dodging. Keep automation off the hosted free tier by default.
4. Several free models use prompts for training or log them. Disclosure per model is a must, not a nicety.
5. The V2 HTTP API is labelled experimental and already broke compatibility once.

### Gemini

1. Gemini CLI OAuth in third-party software is a stated terms violation. API key only.
2. Free tier: prompts may be used for product improvement and read by human reviewers. Show it.
3. EEA, Switzerland and UK: the terms restrict unpaid services for API clients offered to users there. The owner should read this clause against Demido's bring-your-own-key model.

## Risks

1. **Anthropic policy moves.** The June change was announced, then paused, with a promise of notice before it returns. Enforcement can arrive without notice. Keep the Claude Code provider isolated so it can be disabled or relabelled in one place.
2. **`--bare` becoming the default for `-p`.** Subscription auth needs a non-bare run. A future release may need a new flag to keep it working.
3. **Silent billing switch** from subscription to API if an API key leaks into the child environment.
4. **Untrusted project content** executes in non-bare `-p` runs (hooks, MCP servers) with no trust prompt.
5. **Experimental schemas:** `usage_report` (Claude Code), the OpenCode `/api` surface, ACP usage updates. Parse defensively, tolerate unknown fields and enum values.
6. **OpenCode V1/V2 split** in the installed base. Free model list rotates; free periods end without dates.
7. **Gemini limits are invisible to software.** Indicators for Gemini are local estimates and will be wrong when the same project is used elsewhere. Gemini 2.5 Flash and Pro silently reroute on December 1, 2026.
8. **Price tables drift.** Anthropic and Gemini do not return prices by API. Anything in dollars for those two is an estimate unless reconciled with an Admin key.
9. **Admin API unavailable to individual Console accounts.** Many users will have no authoritative Anthropic spend number.
10. **Windows interrupt.** Ending a Claude Code turn relies on SIGINT; Windows child-process signalling is different and needs its own implementation and test.
11. **OpenRouter free tier:** low daily caps, a negative balance blocks free models too, and upstream providers may train on prompts depending on account settings.
12. **Trademark:** official provider logos are not usable without permission. The brand kit needs neutral provider marks.
13. **Maintenance cost of own adapters.** Mitigated by three small adapters and data-driven provider files, but it is real. Anthropic alone shipped a breaking API version last month.

## Open questions for the owner

1. **Claude Code as agent backend only?** I recommend shipping it only as an external agent (its loop, its tools, Demido as the client). Do you also want a raw-model mode that strips its tools so Demido's own harness drives it? That mode is the legally riskiest part and I would hold it until Anthropic answers.
2. **Commercial Terms and confirmation.** Are you willing to accept Anthropic's conditions for products that run Claude Code, and shall we write to Anthropic describing the design and asking for written confirmation?
3. **Installer behaviour.** For Claude Code and OpenCode: detect and link to official instructions only, or may Demido run the official install command when you click a button? I recommend link-only for Claude Code and an optional button for OpenCode (MIT).
4. **Server mode.** Agree that CLI-subscription providers are available only to the server owner's own account, never to additional users?
5. **Unattended work.** May schedules and monitors use the Claude Code subscription or OpenCode's free models? I recommend off by default with a clear warning, and steering them to local models or API keys.
6. **Gemini surface.** Start with `generateContent`, or go straight to Interactions with `store: false`?
7. **OpenCode versions.** V2 only?
8. **Provider icons.** Neutral monograms in the brand kit, or do you want to ask each provider for logo permission?
9. **OpenRouter attribution.** Send "Demido Studio" in the app-title header by default (visible and switchable), or off until the user enables it?
10. **Your own accounts.** Do you have an Anthropic Console organization (needed for the Admin usage API) and which Claude plan? This decides what we can test.
11. **Test spend.** Integration tests against Anthropic, Gemini paid tier and OpenRouter cost money. What budget, if any, do you approve?

## Ideas

1. **Billing-source badge on every remote session:** "Subscription limits", "API credit", "Prepaid credits", "Free tier - prompts may be used for training", "Local". One glance answers "what is this costing me and who sees it".
2. **Wire inspector.** A panel showing the exact request and response for any turn, secrets redacted, with copy-as-curl. Falls out of owning the clients.
3. **Nudge Max and Ultra users to their monthly API credit.** It is the cleanest way to use Claude from Demido: direct API, full harness control, no grey area.
4. **ACP as the generic "external agent" slot.** One ACP client gives OpenCode today and any other ACP-speaking agent later, each subject to its own terms.
5. **Live capability detection instead of hardcoded tables.** Anthropic Models API, OpenRouter model metadata and Gemini `models.list`, cached with a visible refresh time and the raw JSON one click away.
6. **Rate-limit-aware scheduling.** Feed Anthropic's headers, OpenRouter's daily free counter and Gemini's local counters into the agent juggler (4.17) so background agents slow down before a limit is hit rather than after.
7. **A "may train on my data" switch.** One global setting that hides or blocks every model flagged as training on prompts across Gemini free tier, OpenCode free models and OpenRouter free endpoints.
8. **Budget guards.** `--max-budget-usd` on Claude Code runs, a per-provider monthly cap enforced from local accounting, and a link to Anthropic workspace spend limits for a hard server-side cap.
9. **Editable price table with provenance.** Ship list prices with the date they were read; let the user edit them; mark every derived dollar figure as an estimate.
10. **OpenRouter account connect.** OpenRouter documents an OAuth PKCE flow that lets the user create a key for an app from their own account. It could replace key copy-paste. Details need checking.
11. **Subscription-friendly preset for Claude Code:** one session at a time, no unattended loops, a visible meter from `rate_limit_event`. It keeps use inside what the usage policy calls ordinary.
12. **Pushback on the brief:** "Anthropic (subscription)" should not be presented as a peer of the API-key providers. It is a different kind of thing with different abilities and risks. Group it under "External agents" with OpenCode.

## Not verified

- The Help Center article on the Agent SDK and plans was read through a summarizing fetch; I did not see its full text. The June 15 pause and October 7 credit notes are consistent with the platform docs page, which I read in full.
- That Max 5x and Max 20x were renamed to Max and Ultra comes from a secondary source; the pricing page confirms the current names and prices only.
- A reported January 2026 block-then-reversal of subscription tokens in third-party tools comes from a secondary source.
- Whether sending `/usage` in a `-p` session consumes model tokens. The docs do not say.
- The exact Claude Code CLI version today. Agent SDK 0.3.329 on npm implies a CLI near 2.1.329 (the docs give one example of matching numbers).
- The full response `usage` field list of the Messages API as of today; the fetch reached only the request half of the page. Field names here come from Anthropic's bundled reference dated 2026-11-03.
- Gemini free-tier RPM, TPM and RPD numbers. They are published only inside AI Studio behind a login.
- Whether every Gemini 4 capability is reachable through `generateContent`.
- The exact wording of the Gemini EEA/UK/Switzerland clause.
- That OpenCode free models work with no account in V2. The homepage says so; one docs sentence about the provider being absent when signed out appears to describe the case where free models are disabled. Not tested.
- IP-level limits on OpenCode free models. Nothing published.
- Token and cost field names on OpenCode V2 session events, and the license of `@opencode/client`.
- Whether OpenCode 1 supports `opencode acp`, and the V1 docs location.
- `genai`'s support for Anthropic adaptive thinking and effort, and for prompt-cache breakpoints beyond what its README states.
- OpenRouter's per-request parameter names for excluding providers that train, and the details of its OAuth PKCE flow.
- The date the first OpenCode 2.0.0 was released.
