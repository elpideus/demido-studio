# 18. Keys and providers: where keys are kept, how Anthropic, Gemini and OpenRouter are asked, and Claude Code and OpenCode

Written 2026-11-21, while building plan item 6 (the vault, the providers, usage shown; brief 4.10, 4.20).

How sure each line is: **Tried** (run on the owner's computer today), **Read** (read today in the source
named), **Stand-in** (built and tested against a stand-in that answers in the documented shape: no real
provider was asked, because that needs the owner's own key and costs money).

## Where keys are kept

| Fact | How sure |
|---|---|
| `keyring-core` 1.1.0, `zbus-secret-service-keyring-store` 1.0.1, `apple-native-keyring-store` 1.0.2, `windows-native-keyring-store` 1.1.0: all MIT OR Apache-2.0. The Secret Service store talks D-Bus in Rust (zbus) and can use Rust's own crypto (`rt-tokio-crypto-rust`): no C library, no OpenSSL | Read: crates.io and each crate's `Cargo.toml` |
| On this computer the Secret Service is there (KWallet's `ksecretd`). A key set, read back and deleted through it: 63 ms for all of it, from a plain thread and from inside the async runtime's blocking pool | Tried, with a throwaway entry that was deleted again |
| The store adds about 60 crates to Linux builds (zbus and its async parts) | Tried: `cargo tree` |
| Claude Code keeps its login in the macOS Keychain and, on Linux, in a file with mode 0600 | Read in note 05 §2.5 (not checked again today) |
| A server without a desktop has no Secret Service: the store cannot be opened there | Read in note 05 §2.5; what Demido does then is a file only the user can read, and it says so |

## Anthropic (the Messages API)

From Anthropic's own documentation for the API (the claude-api reference on this computer), read today.

| Fact | How sure |
|---|---|
| `POST https://api.anthropic.com/v1/messages`, headers `x-api-key`, `anthropic-version: 2026-10-21`, `content-type: application/json`. `max_tokens` is required. What the model is told first is `system`, not a message | Read; Stand-in |
| A stream is server-sent events: `message_start` (with `usage.input_tokens`, `cache_creation_input_tokens`, `cache_read_input_tokens`), `content_block_delta` with `text_delta` or `thinking_delta`, `message_delta` (with `stop_reason` and `usage.output_tokens`), `message_stop`; an `error` event can come mid-stream (`overloaded_error`) | Read; Stand-in |
| On API version 2026-10-21 a sampling setting (`temperature`, `top_p`, `top_k`) is refused by the newer models | Read |
| A reply cannot be begun for the model (no "prefill" on the newer models): the last message is the person's | Read |
| Thinking: `{"type": "adaptive", "display": "summarized"}` and `output_config.effort` (`low` … `max`). Opus 5.5 and the Fable models cannot be told not to think (`disabled` is a 400): effort is the only control. Sonnet 5.5 turns it off with `{"type": "between_tools"}` at effort `high` or below. Opus 5 and Haiku 5.5 take `{"type": "disabled"}` at effort `high` or below. The 4.x models do not think unless asked; Haiku 4.5 thinks with `{"type": "enabled", "budget_tokens": N}` and has no effort | Read |
| Structured replies: `output_config.format = {"type": "json_schema", "schema": …}`. Every object needs `additionalProperties: false`; numbers, string lengths and complex array rules are not checked (they are left out of the schema) | Read |
| `GET /v1/models` lists what the key may use, with `max_input_tokens`, `max_tokens` and a tree of `capabilities` (`thinking.types.adaptive.supported`, `effort.low.supported`, `image_input`, `structured_outputs`) | Read; Stand-in |
| A top-level `cache_control: {"type": "ephemeral"}` caches the prompt up to its last block. Reading from the cache costs 0.1 of the input price (0.05 on Opus 5.5 and Sonnet 5.5, 0.025 on Fable 5.1 and 5.2); writing to it 1.25 for five minutes | Read |
| Errors: 400 `invalid_request_error`, 401 `authentication_error`, 403, 404 `not_found_error`, 413, 429 `rate_limit_error` with `retry-after`, 529 `overloaded_error`. A safety refusal is a 200 with `stop_reason: "refusal"` | Read |
| Prices in dollars per million tokens, in and out: Fable 5.2 8 / 40, Fable 5.1 10 / 50, Opus 5.5 4 / 20, Opus 5 and 4.6–4.8 5 / 25, Sonnet 5.5 and 5 2 / 10, Sonnet 4.6 3 / 15, Haiku 5.5 0.10 / 0.50 (0.50 / 2.50 above 100,000 prompt tokens), Haiku 4.5 1 / 5 | Read |
| An ordinary key cannot ask what it has spent | Read: spending is in the console, or the Admin API with an admin key |

## Gemini (Google's OpenAI-compatible API, in beta)

From ai.google.dev, read today (the compatibility page and the price page).

| Fact | How sure |
|---|---|
| `https://generativelanguage.googleapis.com/v1beta/openai/` with `Authorization: Bearer <key>`: `chat/completions` (streaming, `stream_options.include_usage`), `models` | Read; Stand-in |
| `reasoning_effort` is `minimal`, `low`, `medium`, `high`, and `xhigh` for Gemini 4 Neon and Hydrogen only. `none` turns thinking off for the 2.5 models only; "reasoning cannot be turned off for Gemini 2.5 Pro, Gemini 3, or Gemini 4 models" | Read |
| Structured replies through `response_format` with a JSON schema | Read |
| Prices, paid tier, dollars per million tokens, in and out (out includes thinking): Gemini 4 Carbon and Argon 2 / 10 through 30 November 2026, 4 / 20 from 1 December; Gemini 4 Neon 3 / 15; Gemini 4 Hydrogen 1.50 / 7.50; Gemini 3.8 and 3.6 Flash 0.75 / 3.75 through 31 December 2026, 1.50 / 7.50 from 1 January 2027; 3.5 Flash-Lite 0.30 / 2.50; 3.1 Flash-Lite 0.25 / 1.50; 3 Flash Preview 0.50 / 3.00; 3.1 Pro Preview 2 / 12 up to 200,000 prompt tokens, 4 / 18 above; 2.5 Pro 1.25 / 10, 2.50 / 15 above 200,000; 2.5 Flash 0.30 / 2.50; 2.5 Flash-Lite 0.10 / 0.40 | Read |
| The free tier bills nothing, and what is sent there is used to improve Google's products | Read |
| Signing in with Gemini CLI's login in another program is reported as prohibited; keys are the way | Read in the brief and note 06 |

## OpenRouter

From openrouter.ai/docs, read today (overview, structured outputs, reasoning, usage, limits).

| Fact | How sure |
|---|---|
| `https://openrouter.ai/api/v1/chat/completions` with `Authorization: Bearer <key>`; `HTTP-Referer` and `X-OpenRouter-Title` say which program asks | Read; Stand-in |
| Every answer's `usage` holds `cost`, in credits, which are dollars; `prompt_tokens_details.cached_tokens` is what was read from a cache | Read; Stand-in |
| `reasoning: {"effort": …}` with `none`, `minimal` … `max`. A model in `GET /api/v1/models` may say `reasoning.supported_efforts` and `reasoning.mandatory` (it must think) | Read |
| `GET /api/v1/models` needs no key; prices are strings in dollars per token; `-1` means it depends (OpenRouter's own router) | Read; Stand-in |
| `GET /api/v1/key` says what the key spent this month (`usage_monthly`), its own limit (`limit`, `limit_remaining`, `limit_reset`), and its free-model requests today (`free_model_daily_requests`) | Read; Stand-in |
| 402 when credit is used up, or the key's limit or the in-flight budget is reached; errors mid-stream come as a chunk with `error` | Read |

## Claude Code (the person's own, through `claude -p`)

Read today on code.claude.com/docs (legal-and-compliance, headless, cli-reference, errors, env-vars,
agent-sdk/typescript, model-config). The installed one here: 2.1.330.

| Fact | How sure |
|---|---|
| Running Claude Code in a product requires the Commercial Terms and two conditions: the binary as published, with no way of signing in removed, disabled or restricted; and no paying for, reselling or intermediating Claude usage: each end user signs in with their own key or plan and is billed directly | Read (legal-and-compliance) |
| Developers may not offer Claude.ai login in their own apps, route requests through plan credentials on their users' behalf, or collect, store or intermediate Claude.ai credentials. This "does not prevent an end user from signing in to the unmodified Claude Code binary with their own Claude subscription" | Read (the same page) |
| The name: a product may say in plain text that it runs Claude Code; it may not use the names or logos as part of its own name, feature or logo, or suggest Anthropic built or endorses it | Read (the same page) |
| `claude -p` with `--input-format stream-json --output-format stream-json --verbose --include-partial-messages`: one JSON line in for the message, `{"type":"user","message":{"role":"user","content":…},"parent_tool_use_id":null}`; JSON lines out: `system/init` (with `apiKeySource`: `ANTHROPIC_API_KEY`, `apiKeyHelper`, `/login managed key` or `none`), `stream_event` (the Messages API's own stream events, text and thinking deltas), `assistant` (with `tool_use` blocks), `user` (with `tool_result` blocks), `system/permission_denied`, `system/api_retry`, `rate_limit_event` (`status`, `resetsAt`, `utilization`), and at the end `result` (`result`, `is_error`, `total_cost_usd`, `usage`, `session_id`) | Read; Stand-in |
| `--session-id <uuid>` names a new session; `--resume <id>` goes on in one | Read |
| Without `--bare`, a `-p` run uses the person's own login, and also runs a folder's `.claude/settings.json` hooks and `.mcp.json` servers with no trust question. `--setting-sources user` loads only the person's own settings; `--strict-mcp-config` loads only the servers given with `--mcp-config` | Read |
| In a `-p` run, what needs approval is denied unless something approves it: `--permission-prompt-tool mcp__<server>__<tool>` names an MCP tool that does. The tool answers with one text block: `{"behavior":"allow"}` or `{"behavior":"deny","message":…}`, and no `structuredContent` | Read (headless, errors) |
| An MCP tool on a server over HTTP times out after 60 seconds per request, unless `MCP_TOOL_TIMEOUT` is set above 60000; an idle check of 5 minutes applies too (`CLAUDE_CODE_MCP_TOOL_IDLE_TIMEOUT`) | Read (env-vars) |
| Permission modes: `manual`, `acceptEdits`, `auto` (a classifier decides what needs a person), `dontAsk`, `bypassPermissions`, `plan` | Tried: `claude --help` |
| SIGINT ends the turn cleanly; SIGTERM leaves it unfinished | Read (headless) |
| `ANTHROPIC_API_KEY` in the environment is always used by `-p` in place of the plan | Read (env-vars) |
| Model aliases: `default`, `opus`, `sonnet`, `haiku`, `fable` (and `best`, `opusplan`, `[1m]` forms); `--effort low … max` | Read (model-config); Tried: `--help` |
| `claude auth status` prints JSON with `authMethod` (`none`, `claude.ai`, `oauth_token`, `api_key`, `api_key_helper`, `third_party`), exit 0 when signed in | Read; Tried: Demido reads only that field, and showed "signed in with a Claude account" |
| `total_cost_usd` is the program's own estimate at API prices; under a plan nothing is billed per token | Read (headless: "client-side estimates") |

## OpenCode (the person's own, through `opencode acp`)

Read today on opencode.ai/docs (cli/acp, console/models) and its terms (effective 8 November 2026). The
installed one here: v2.2.7.

| Fact | How sure |
|---|---|
| `opencode acp` speaks the Agent Client Protocol, version 1: newline-delimited JSON-RPC over stdin and stdout, a private server of its own, no network port; it ends when its input closes | Read; Tried |
| `initialize` answered: protocol 1, `loadSession`, sessions that can be resumed, closed, deleted, forked, listed; one auth method, `opencode-login` (run `opencode auth login` in a terminal) | Tried |
| `session/new` with a folder answered, with no account: a session and its options: `model` (12 free models today, `opencode/big-pickle`, `opencode/deepseek-v4.1-flash-free` and others) and `mode` (`build`, `plan`) | Tried, in throwaway data folders, nothing sent to a model |
| The model is set with `session/set_config_option`; text with `annotations.audience: ["assistant"]` is context for the model only | Read |
| Permission requests offer `allow_once`, `allow_always`, `reject_once`; `session/cancel` ends a prompt with `stopReason: "cancelled"`; `usage_update` carries the session's whole cost so far | Read; Stand-in |
| What each free model does with what is sent: Big Pickle, Iceman, Tidepool, Flint 40B may use it to improve the model; the Nemotron models are trials that log it; DeepSeek V4.1 Flash, LongCat 2.5 and Hy4 Flash keep nothing; Beam keeps it 30 days for safety; several newer ones say nothing | Read (console/models). Demido shows this per model, dated |
| Free models are for OpenCode only: other clients get 403 | Read (console/models) |
| The hosted service's terms forbid "automatically or programmatically" extracting output, and processes that run "while you are not logged into the Services"; on unpaid accounts what is sent may be used to improve the services | Read (terms of service) |

## What was tried with the programs

- **Tried, nothing sent to a model:** the scratch Demido found both programs, read Claude Code's version and
  that it is signed in with a Claude account, and listed OpenCode's 12 free models with each one's data-use
  note; one was chosen, and a session switched to it said "Runs in OpenCode".
- **Tested against stand-ins:** a Claude Code turn in a folder at Balanced, where the stand-in asks for a
  command through Demido's approval tool, the person says yes in Demido's card, the folder changes, the answer
  shows its cost by Claude Code's count, and the whole change is undone in one go; the next turn resumes the
  same Claude Code session and is given only the new message. An OpenCode turn at Cautious, where OpenCode's
  permission request becomes Demido's card.
- **Not tried:** a real message to either program. A message to Claude Code spends the owner's plan; one to
  OpenCode's free models goes to its hosted models, some of which keep it. That is the owner's call.

## What was tried with the providers, and what was not

- **Tried:** the keyring on this computer, with a throwaway entry. The whole flow in the app against
  `core/dev/providers-stand-in.py`: a made-up key added on the Providers screen and kept in a file (the
  trial keeps keys out of the keyring), Claude Opus 5.5 chosen from the stand-in's list, a session switched
  to it, a question asked: Demido first asked it whether a tool was needed, then for the answer; both went to
  the stand-in; the answer showed its cost, $0.0147, which is the stand-in's tokens at the table's prices.
- **Not tried:** a real provider. That needs the owner's own key (Demido's builder may not enter one) and it
  costs money. The first real request to each provider is where a wrong reading of these pages would show;
  every provider's path has a fallback for a request the provider does not take as it is (a
  way of thinking, or a reply's shape, is dropped once and remembered).
