# Salvage sweep: v1 and the qwen variant

Date: 2026-09-03
Scope: primary-source read of `demido-studio-first-version` (v1, 47 commits, tag v0.6.5),
`demido-studio-second-version` (referred to as "v2" in the originating ticket; the actual
in-repo name for this attempt is v2 per its own `docs/roadmap.md`/`AGENTS.md`, 214 commits),
the qwen variant at `S:\Development\DemiPlanComander\qwen`, and this repo
(`demido-studio`, the current working tree).

**Naming correction, load-bearing for everything below.** The ticket describes "v2 itself (the
current repo)" as having a `demido-mcp` crate, an Accounts rail entry, and a Caveman
implementation "measured against a real 9B". None of that exists in the current repo
(`S:\Development\Demido Studio Project\demido-studio`): its working tree is `docs/`,
`licenses/`, `LICENSE`, `README.md`, `THIRD_PARTY_NOTICES.md` and a `.research/` planning
folder, with no `src-tauri`, no `web/`, no code at all. The features the ticket describes live
in a *third* directory not named in the ticket: `S:\Development\Demido Studio Project\demido-studio-second-version`.
This repo's own `.research/map.md` confirms the lineage explicitly: this is "the fourth attempt",
listing v1, the qwen variant, and `demido-studio-second-version` (**called "v2" in that table**)
as the three prior attempts, all predating this repo. Below, "v2" means
`demido-studio-second-version`, matching both the ticket's intent (it is the only tree with
`demido-mcp` and a benchmarked Caveman) and this repo's own map.md.

## TL;DR

- **Google/accounts: port v1's OAuth code, do not start from scratch.** v1's Google
  integration (`src-tauri/src/google_apis.rs`, `src-tauri/src/commands.rs` lines ~5405-5985,
  `src-tauri/src/db/accounts.rs`) is a complete, real, working PKCE OAuth loopback-redirect
  implementation with real Gmail/Calendar/People API calls, token refresh, and a Bring-Your-Own
  Google-Cloud-app-credentials model. It was **not** touched by any of the three final "strip"
  commits (verified below) and stands intact in the final `ed6d887` tree. It is reasonably
  self-contained (5 files: `google_apis.rs`, `db/accounts.rs`, the OAuth command block in
  `commands.rs`, `secrets.rs` for credential storage, plus frontend windows), with sane, small
  dependencies (`reqwest`, `sha2`, `base64`, `getrandom`, all already idiomatic Rust crates).
  Recommendation: port it near-verbatim into a new `demido-google` (or similar) crate in this
  repo, adapting to whatever secret-storage/DB layer this repo settles on.
- **v1's MCP code is a client, not a bridge/server, and is smaller/less rigorous than v2's
  `demido-mcp`.** v1's `src-tauri/src/mcp/` (3 files, ~200 total lines read) spawns stdio MCP
  servers and caches their tool lists with minimal error handling. v2's `demido-mcp` crate
  (9 files, 2,881 lines across `manifest`/`disclosure`/`approvals`/`protocol`/`session`/`host`/
  `supervisor`/`servers`, plus a real-process integration test and a "ran a stranger's real MCP
  server end-to-end" test) is a materially more complete and more carefully specified
  implementation, with install-time capability disclosure, approval fingerprinting, Windows
  job-object process-tree teardown, dead-server retry policy, etc. Build on v2's `demido-mcp`,
  not v1's.
- **v1 additionally had a separate "MCP bridge" (exposing Demido *as* an MCP server, for
  dev/testing) that was deliberately deleted in the second-to-last commit** (`26dc12d`,
  "chore: strip repo tooling, docs, and QA bridge") along with `vendor/tauri-plugin-mcp` and the
  `mcp-bridge` Cargo feature. This is a different thing from the stdio MCP *client* code above
  and from v2's `demido-mcp`, which is confirmed to be dev-only by design (v2's `AGENTS.md`:
  "The MCP bridge is dev-only. It must never compile into a release binary.") — the removal in
  v1 looks like the same class of thing, not something to mourn.
- **Caveman: use v2's, not v1's.** Both define the same 7 levels (off, lite, full, ultra,
  wenyan-lite, wenyan-full, wenyan-ultra) as prompt-injection text blocks (word/register
  substitution via system-prompt instructions, not a dictionary or fine-tune). v1's
  `src-tauri/src/caveman.rs` (229 lines) has the prompts and a thinking-channel "rider" but no
  visible benchmark artifact in the tree at HEAD (its test module was stripped in the final
  commit, see below). v2 has both the same prompt set (`demido-prompts/defaults/caveman.*.md`)
  **and** a dedicated measurement harness,
  `src-tauri/crates/demido-chat/tests/a_caveman_measurement.rs`, that runs real prompts through
  a real GGUF over llama.cpp's `/tokenize` endpoint and records reply/reasoning token savings
  separately, and the resulting numbers are wired into the settings schema
  (`demido-settings/src/schema.rs`): lite 85%, full 90%, ultra 85%, wenyan-lite 95%,
  wenyan-full 95%, wenyan-ultra 95% (thinking-channel savings tracked separately as
  `chat.caveman_thinking`). v1's is prompt-engineering only; v2's is the same idea validated
  against a real small model with a reproducible measurement.
- **qwen's `agent-guide/graphify-and-web.md` is a goldmine of non-obvious, hard-won facts**
  about graphify (bundled-Python, two-stage build, a "build succeeded but no graph" bug class,
  `vis-network` CDN inlining with a regex trap) and the web search/sources-footer system
  (Cloudflare-challenged hosts, DNS-failure-vs-SSRF-guard conflation, favicon-only fallback
  images). See Task 4 below for the extracted facts verbatim; do not re-derive these from
  scratch.
- **Other v1 features with real, working code that neither v2 nor this repo currently has**:
  a full non-destructive image editor (layers, adjustments, canvas, rulers), a first-run
  `SetupWizard.tsx`, JSON graph/tree artifact viewers, and MT5/CCXT market-data broker
  integration wired to a `MarketWindow.tsx`. See Task 5.

---

## Task 1: v1's Google integration

### Files

- `src-tauri/src/google_apis.rs` (991 lines) — Gmail, Calendar, People (Contacts) REST calls
  and token refresh.
- `src-tauri/src/db/accounts.rs` (86 lines) — SQLite storage for linked accounts and their
  per-service enable flags.
- `src-tauri/src/secrets.rs` (80 lines) — flat-file (`secrets.json` in the Tauri app-data dir)
  key/value store for the OAuth client id/secret (and other secrets: MT5/CCXT credentials).
  Explicitly documented as **not encrypted**: "Not encrypted. This is known debt (see AGENTS.md
  'Known debt')" (`secrets.rs` lines 3-6).
- `src-tauri/src/commands.rs` lines 5405-5985 — the Tauri commands: `has_google_credentials`,
  `set_google_credentials`, `initiate_google_oauth` (the OAuth flow itself), plus one command
  per Gmail/Calendar/Contacts operation (list/read/trash/mark-read emails; list/create/update
  calendar events; list/get/update contacts), each resolving `(client_id, client_secret)` via
  `get_google_creds(&state)` and calling into `google_apis.rs`.
- Frontend: `src/components/windows/AccountsWindow.tsx`, `EmailWindow.tsx`,
  `CalendarWindow.tsx`, `ContactsWindow.tsx`, `src/components/settings/ConnectionsSettings.tsx`
  (client id/secret entry UI), `src/components/auth/AuthGate.tsx`.
- `src/components/windows/accounts/` exists as a directory but is **empty** at HEAD — dead
  leftover, not a live component (superseded by `AccountsWindow.tsx` directly under `windows/`).

### Auth flow: PKCE + loopback redirect, confirmed by reading the code directly

`commands.rs::initiate_google_oauth` (lines 5452-5605), doc comment at line 5445:

> "Initiates Google OAuth PKCE flow: 1. Opens the auth URL in the system browser 2. Starts a
> local TCP server to capture the redirect 3. Exchanges the code for tokens 4. Fetches user
> info 5. Saves the account to DB"

Concretely:
- Binds `TcpListener::bind("127.0.0.1:0")` for a random port, uses it as the redirect URI
  (`http://127.0.0.1:{port}`) — a real loopback redirect, not a fixed port and not a custom
  `demido://` deep-link scheme.
- Generates a PKCE code verifier via `getrandom` (CSPRNG, 64 bytes), SHA-256 challenge, sent as
  `code_challenge`/`code_challenge_method=S256` (lines 5474-5479, 5492-5493).
- Generates a random `state` param (16 bytes) and verifies it on callback with
  `constant_time_eq` (CSRF protection, lines 5482-5484, 5529-5536).
- Opens the system browser via `rundll32 url.dll,FileProtocolHandler` on Windows (explicitly to
  avoid `cmd` metacharacter injection, line 5504), `open` on macOS, `xdg-open` on Linux.
- Scopes requested (line 5481): `openid email profile
  https://www.googleapis.com/auth/gmail.modify
  https://www.googleapis.com/auth/calendar.readonly
  https://www.googleapis.com/auth/contacts` — note Calendar is requested read-only but
  `create_event`/`update_event` (google_apis.rs lines 517-619) do POST/PUT to the calendar API,
  which would 403 against a read-only scope grant; this is either an unnoticed bug or the scope
  string is stale relative to the write-capable code. **Flag this explicitly for whoever ports
  it** — it needs `calendar` (full) or `calendar.events`, not `calendar.readonly`, if writes are
  to keep working.
- 5-minute timeout on the callback wait (`tokio::time::timeout`, line 5523).
- Token exchange: POSTs to `https://oauth2.googleapis.com/token` with
  `grant_type=authorization_code`, `code_verifier` (completing PKCE), `client_id`,
  `client_secret`, `redirect_uri` (lines 5558-5568).
- Refresh: `google_apis.rs::ensure_token` (lines 20-72) — checks `token_expiry` against
  `now + 60`, POSTs `grant_type=refresh_token` to the same endpoint, persists the refreshed
  token via `db::accounts::upsert`, careful to drop the DB mutex guard before the `.await`
  (documented in the module doc comment, lines 6-9, to avoid blocking other commands during the
  HTTP round-trip).

### Where credentials and tokens live

- **Client id/secret**: user-supplied, not shipped by Demido. `set_google_credentials`
  (`commands.rs` 5427-5443) writes them into `secrets.json` via the `Secrets` store
  (`secrets.rs`) under keys `"google_client_id"` / `"google_client_secret"`. This is a
  **Bring-Your-Own-Google-Cloud-app model**: the user creates their own OAuth client in Google
  Cloud Console and pastes the id/secret into `ConnectionsSettings.tsx`. `has_google_credentials`
  gates whether the "Connect Google account" button is enabled.
- **Access/refresh tokens**: stored per-account in the SQLite `accounts` table
  (`db/accounts.rs`), columns `access_token`, `refresh_token`, `token_expiry`, alongside
  `provider`, `email`, `name`, `picture`, and a JSON `services` array (which of Gmail/
  Calendar/Contacts are switched on for that account — `db/accounts.rs` lines 11-25, 70-78).
- **Not encrypted at rest** in either case — `secrets.json` is plain JSON on disk, and the
  SQLite DB is unencrypted. This is called out as known, accepted debt in v1's own code
  comments (`secrets.rs` lines 3-6), not something the port should silently inherit without a
  decision: a real accounts ticket for this repo should decide whether to move to OS
  keychain/DPAPI before porting, since v1 shipped without it.

### Is it real code or a stub?

Real, working code, not a shell. Evidence:
- Actual HTTP calls to `gmail.googleapis.com`, `www.googleapis.com/calendar/v3`, and
  `people.googleapis.com` — not mocked, not commented out.
- Gmail: `list_emails` (concurrent per-message metadata fetch via
  `futures_util::future::try_join_all`, explicitly optimized — "ponytail: fetch metadata for all
  messages concurrently instead of one await per email", line 145), `get_email_body` (MIME-part
  walking with base64url decoding, padding-indifferent, `google_apis.rs` lines 270-298),
  `trash_message`, `set_message_read`.
- Calendar: `list_events`, `list_events_all_calendars` (fetches all of a user's calendars
  concurrently, colors resolved through a hardcoded Google `colorId` → hex palette,
  lines 390-406), `create_event`, `update_event`.
- Contacts: `list_contacts` (both plain listing and `searchContacts`), `get_contact`,
  `update_contact` — full field mapping (names, emails, phones, addresses, org, birthday,
  anniversary, website, notes, photo) both parse (`parse_person`) and serialize
  (`update_contact`'s body construction) directions.
- Real `Cargo.toml` dependencies backing this: `reqwest = { version = "0.12", features =
  ["json", "stream"] }`, `sha2 = "0.10"`, `base64 = "0.22"`, `getrandom = "0.2"`,
  `rusqlite = { version = "0.31", features = ["bundled"] }`, `chrono` — all mainstream,
  unremarkable crates, nothing exotic to untangle.
- Frontend windows (`EmailWindow.tsx` 17.9K, `CalendarWindow.tsx` 28.6K, `ContactsWindow.tsx`
  26.8K) are large, feature-sized files, consistent with actual built UI rather than a
  placeholder screen.

### Did the final "strip/remove" commits touch it?

No. Verified directly with `git show --stat` on all three trailing commits on `main`:

- `ed6d887` "chore: remove Rust test modules" (HEAD) — removes 64 `#[cfg(test)]` blocks across
  59 files, **including** `src-tauri/src/commands.rs` (-706 lines, test code) and
  `src-tauri/src/caveman.rs` (-80 lines, its own test module), but **does not touch**
  `google_apis.rs`, `db/accounts.rs`, or `secrets.rs` at all (absent from the commit's file
  list). The commit message states plainly: "Pure deletion... cargo check and cargo clippy
  --all-targets are both clean," i.e. it removed only test modules, not functional code.
- `26dc12d` "chore: strip repo tooling, docs, and QA bridge" — removes CI workflows, `AGENTS.md`,
  `agent-guide/`, `README.md`, `CONTRIBUTING.md`, `vendor/tauri-plugin-mcp` and the `mcp-bridge`
  feature (separate from Google, see Task 2), and 16 frontend vitest test files. Again, no
  Google-related file appears in this diff.
- `3a6a398` "chore: remove Astro docs site" — removes the in-repo `docs/` Astro site only.

So: **the "strip/remove" commits were repo-hygiene commits (dead tests, unused dev tooling, a
duplicated docs site), not a rollback of features.** As of the final commit `ed6d887`
(`v0.6.5`), the Google integration exists in the working tree fully intact, functionally
identical to how it looked before the strip commits.

### Portability verdict

Self-contained and portable. The whole feature is: 1 backend module
(`google_apis.rs`, no internal dependencies beyond `db::accounts` and `sync::LockExt`), 1 DB
module (`db/accounts.rs`, trivial CRUD), 1 secrets module (generic, already needed for other
credentials), a block of Tauri commands in `commands.rs` that could lift out into their own
file, and standalone frontend windows. Recommend: port `google_apis.rs` and `db/accounts.rs`
close to verbatim (fixing the calendar scope issue noted above), re-home `secrets.rs`'s
functionality onto whatever secret storage this repo picks (v1's own comments call the current
approach known debt), and rebuild the frontend windows against this repo's actual window/panel
system rather than porting v1's `WindowManager.tsx` machinery.

### MCP bridge relation

The Google integration has **no code relationship** to v1's MCP bridge or MCP client (Task 2)
— it is called directly from `commands.rs`/`agent/executor.rs` as regular Tauri commands and
agent tools (`agent/executor.rs` line ~1678 reads `google_client_id`/`google_client_secret` from
secrets to call `google_apis::ensure_token` when the agent invokes a Google-backed tool). It
does not go through the MCP protocol in either direction.

---

## Task 2: v1's MCP bridge/client vs v2's `demido-mcp`

### v1

Two distinct things existed in v1, and they are easy to conflate:

1. **An MCP client**, still present in the final tree at
   `src-tauri/src/mcp/{mod.rs, stdio.rs, types.rs}`. Its own doc comment (`mod.rs` lines 1-9):
   "MCP client over stdio transport: spawning configured servers, caching their tool lists, and
   handing out client handles for tool calls." `McpManager::load_servers` (lines 43-76) spawns
   every enabled stdio server (`stdio::StdioClient::spawn`), calls `initialize()` then
   `list_tools()`, and caches the merged tool list; a server that fails to spawn or initialize
   is logged (`eprintln!`) and skipped, not fatal. This is stdio transport only — no HTTP
   transport. `mod.rs` is 94 lines; `stdio.rs`/`types.rs` were not read line-by-line in this
   pass but the manager surface (`list_tools`, `get_server`, `get_stdio_client`) is minimal:
   no tool-discovery caching invalidation beyond a full reload, no resources, no prompts, no
   approval/disclosure step before a server is spawned.
2. **A separate "MCP bridge"** (exposing *Demido itself* as an MCP server, apparently for
   development/testing use from an external MCP client like Claude Code), added in commit
   `a3be1be` ("chore: bump version to 0.6.0, graphify builtin tools + auto-build toggle, MCP
   bridge for testing") and feature-gated in `2704f14` ("fix(ci): unbreak rust workflow —
   feature-gate MCP bridge, fmt + clippy"). This lived in `vendor/tauri-plugin-mcp` plus a
   `mcp-bridge` Cargo feature and `src-tauri/mcp.capability.json`, and was **deliberately
   deleted** in the penultimate commit `26dc12d`: "vendor/tauri-plugin-mcp and the mcp-bridge
   feature: dependency, tauri:mcp script, Cargo feature, lib.rs cfg blocks, main.tsx dynamic
   import, and src-tauri/mcp.capability.json" (commit message). This is consistent with v2's own
   `demido-mcp` crate calling its equivalent "dev-only" and stating "It must never compile into
   a release binary" (`demido-studio-second-version/AGENTS.md`, Hard rule 7) — the same class of
   tooling, correctly kept out of shipped code in both lineages.

### v2 (`demido-studio-second-version/src-tauri/crates/demido-mcp`)

A materially larger and more rigorously specified MCP **client** implementation: 9 source files,
2,881 lines (`wc -l`): `manifest.rs` (242), `disclosure.rs` (169), `approvals.rs` (343),
`protocol.rs` (489), `session.rs` (627), `host.rs` (203), `supervisor.rs` (339), `servers.rs`
(410), `lib.rs` (59). Its own `AGENTS.md` describes the pipeline precisely: "One file starts a
process, and everything else is pure. `host` spawns; the other four parse, render, frame and
talk." Concretely, compared to v1's single-file client:

- **Install-time capability disclosure + approval**: `manifest` parses `mcp.json`
  (`{"mcpServers": {...}}`, upstream's own shape, not a Demido-invented one — "a skill author
  pastes the block their server's README gives them"); `disclosure` renders what a server would
  do for a human and fingerprints exactly what was shown; `approvals` persists that fingerprint
  and reports one of three states on next boot (approved / never asked / asked-and-changed).
  v1 has no approval/disclosure step at all — any enabled stdio server in the DB is spawned
  silently.
- **Process-tree correctness on Windows**: documented invariant, "Dropping a `Running` kills
  the tree, not the child... On Windows the child is very often not the server: `npx` is
  `npx.cmd`, a `.cmd` runs under `cmd.exe`, and `node` is a grandchild that `Child::kill` never
  touches... `tree` puts the child in a job object with `KILL_ON_JOB_CLOSE`." v1's client has no
  equivalent tree/job-object handling visible in `mod.rs`.
- **Liveness/retry policy**: "A server that died is started again by the call that found it
  dead, and by nothing else... Three restarts, then it says so," with a dedicated decision note
  `docs/decisions/0014-a-dead-server-is-started-again-once.md`. v1 has no restart policy — a
  failed spawn/initialize is logged once and the server is simply absent from the tool list
  until the next full `load_servers` reload.
- **Testing depth**: `tests/a_real_process.rs` runs `harness = false` so the test binary
  re-executes itself as the scripted MCP server, proving wire correctness with no Node/Python/
  network dependency; a sibling crate's `tests/a_stranger.rs` (referenced in this crate's
  `AGENTS.md` "Not built yet" section) runs a real third-party MCP server
  (`npx -y warframe-market-mcp`) end-to-end and found two real bugs by doing so (a too-short
  handshake timeout that broke on first-run `npx` package installs, and a `stop()` that killed
  the wrong process). v1's `mcp/` module had a test module, but it was deleted whole in the
  final "remove Rust test modules" commit, and even before deletion it would have been testing
  a single, much smaller surface.
- **Protocol coverage**: still stdio-only, and both explicitly log unsolicited server-initiated
  messages (roots/sampling/elicitation) rather than answering them ("Not built yet" in v2's
  `AGENTS.md`) — neither implementation has resources or prompts, and neither has an HTTP/SSE
  transport. On raw protocol surface the two are closer than the above might suggest; the gap is
  entirely in operational rigor (disclosure, approvals, process-tree safety, retry policy,
  integration testing), where v2 is unambiguously further along.

**Verdict**: build on v2's `demido-mcp`, not v1's `mcp/` module. v1's client is not wrong, just
thin; v2's is the same idea taken through several rounds of "what actually breaks on Windows /
with a real third-party server" that would otherwise have to be rediscovered.

---

## Task 3: v1's `caveman.rs` vs v2's Caveman implementation

### Compression mechanism (both)

Identical mechanism in both lineages: **prompt-injection text blocks**, not a word-substitution
dictionary, not an algorithmic transform, not a model fine-tune. Each level is a hand-written
system-prompt fragment instructing the model how to write (drop articles/filler/pleasantries,
use fragments, keep code/facts/technical terms verbatim, etc.), appended to the composed system
prompt. v1's `caveman.rs` doc comment states this explicitly: "Each level carries a standalone
prompt: no shared preamble is concatenated at runtime... `Off` has no prompt at all: the model
must see nothing, not an instruction saying 'be normal'." v2's prompts live as separate Markdown
files (`demido-prompts/defaults/caveman.{full,lite,ultra,wenyan-full,wenyan-lite,wenyan-ultra}.md`)
rather than Rust string constants, otherwise the same idea.

### Level coverage

Both implement all 7 levels named in the brief: off, lite, full, ultra, wenyan-lite,
wenyan-full, wenyan-ultra. v1: `src-tauri/src/caveman.rs` lines 12-20 (`LEVELS` const) and the
`LITE`/`FULL`/`ULTRA`/`WENYAN_LITE`/`WENYAN_FULL`/`WENYAN_ULTRA` string constants (lines 42-160).
v2: same 7 values as `CAVEMAN_LEVELS` choices in `demido-settings/src/schema.rs` (~line 469
onward) plus the six markdown prompt files (off has none, matching v1's "off has no prompt"
design).

### Thinking-time caveman (brief: "toggle + selector... enables or disables caveman during
model's thinking process... Ultra by default")

v1 has this: `caveman.rs::thinking_rider` (lines 180-207) returns a level-specific "rider" text
appended only when reasoning is local (llama.cpp/GGUF) — hosted providers' reasoning is either
raw-and-signed (Anthropic) or a model-side summary (Gemini) and "a style rule reaches neither"
(comment, lines 165-167). Notably, v1's own code comment records a **measured result** for this
rider even though this file's own test module was stripped: "Measured on Qwen3.5-9B-Q4_K_M under
a realistic prompt (artifact block + `ULTRA` + tools), 'What can you do?', n=6 per arm: no rider
→ 60 words, 6/6 opening 'The user is asking…'; a rider that only asked for compression → 73
words, 5/6 still prose, i.e. inert; the same rider naming the banned openers → 36 words, 0/6."
(lines 175-179). So v1's thinking-rider design was itself validated against a real 9B at some
point, even though the harness that produced that number is not present in the final tree (only
the result, as a comment).

v2 has the same distinction (`chat.caveman` vs `chat.caveman_thinking` as two separate settings
keys, `demido-settings/src/schema.rs` lines 279-280) and a dedicated, currently-runnable
measurement harness for it (see below), rather than a one-off number recorded as a comment.

### Benchmark/validation evidence — the "measured against a real 9B" claim, confirmed

v2 has a dedicated integration test,
`demido-studio-second-version/src-tauri/crates/demido-chat/tests/a_caveman_measurement.rs`,
whose own header states the purpose: "What each caveman level actually saves, measured... this
asks a real model the same questions at every level and counts what the server itself reports."
Concretely:
- Run via `DEMIDO_GGUF=<a .gguf> cargo test -p demido-chat --test a_caveman_measurement --
  --ignored --nocapture` (a real llama.cpp-served GGUF is required; not mocked).
- Tokens counted via the loaded model's own tokenizer over llama.cpp's `/tokenize` endpoint
  ("a count from any other tokeniser is a count of a different model"), specifically because
  Wenyan/Classical Chinese has a very different characters-per-token ratio than English and a
  character-based measurement would misrank the levels.
- Reply tokens and reasoning tokens are counted **separately** (`chat.caveman` vs
  `chat.caveman_thinking`), because the harness's own first run "found: a thinking model asked
  to compress its reply writes *more* in total, because it spends the reasoning working out how
  to comply, and one of those runs looped until it ran out of room" — a real, documented failure
  mode discovered by actually running the measurement, not hypothesized.
- Three fixed questions (mutex-vs-channel, laptop-fan-noise triage, GGUF-quantization
  explanation — chosen to elicit prose, not code, "a level that only compresses sentences would
  look like it saved nothing if every answer were a snippet"), three fixed seeds (1, 2, 3) per
  question per level, for reproducibility.
- The resulting percentages are wired live into the product, not just left in test output:
  `demido-settings/src/schema.rs`'s `CAVEMAN_LEVELS` choices carry `saving: Some(N)` per level —
  lite 85, full 90, ultra 85, wenyan-lite 95, wenyan-full 95, wenyan-ultra 95 (percentages as
  read directly from schema.rs; note lite and ultra show the same 85% figure and full shows a
  higher 90% than both its neighbors in this data — recorded here as found, not smoothed over;
  whoever revisits this should treat it as a fact to explain, not a typo to silently "fix").

v1 has no equivalent artifact in the tree: its own caveman test module (`#[cfg(test)] mod
tests` inside `caveman.rs`, part of the 80 lines removed from that file in the "remove Rust test
modules" commit) is gone, and no separate measurement harness or benchmark file exists anywhere
in v1's tree for caveman specifically (the market or graphify modules have their own tests, but
nothing caveman-shaped was found searching the tree). The one hard number that survives is the
thinking-rider comment quoted above.

**Verdict, confirmed**: v2's Caveman is validated end-to-end against a real small model with a
reproducible, currently-runnable harness and the numbers it produced are live in the settings
UI; v1's is well-written prompt engineering with one surviving anecdotal measurement in a code
comment and no harness left to reproduce or extend it. Port v2's prompts and its measurement
harness; v1's prompt text is worth a diff-read for phrasing ideas but is not the implementation
to build on.

---

## Task 4: qwen's `agent-guide/graphify-and-web.md` — extracted engineering facts

Read in full: `S:\Development\DemiPlanComander\qwen\agent-guide\graphify-and-web.md` (25 lines,
dense). It is written in the repo's own "Caveman Ultra" house style (see its own header note:
"Fragment of `AGENTS.md`. Same rules: Caveman Ultra, no em-dashes, facts never dropped for
brevity."), so what follows unpacks it rather than re-quoting it verbatim.

### Graphify

- **Implementation choice**: not a native port. It shells out to the `graphifyy` PyPI package
  (`pip install graphifyy`, note the double-y) inside a bundled/portable Python runtime
  (`local/python.rs`), mirroring how `local/searxng.rs` manages its own bundled process. Every
  operation is a short-lived `python -m graphify …` child process (module entry point
  `graphify/__main__:main`) — **never a long-running server**. This is the concrete answer to
  "how would we implement graphify" for this repo: wrap the same PyPI package, don't write a
  Rust/TS graph engine from scratch.
- **Two-stage build, and a real bug class from shipping only stage one**: `extract` (with
  `--code-only`) writes `graph.json` + `.graphify_analysis.json` and **exits 0 without writing
  `graph.html`**, printing "next: run `graphify cluster-only <folder>`" — extract alone is *not*
  a complete build. `cluster-only` is the stage that actually writes `graph.html` +
  `GRAPH_REPORT.md`. The qwen variant apparently shipped only the first stage at some point:
  "every build 'succeeded' and `graph_built` stayed false → window returned to its build prompt
  forever, no error anywhere." The fix recorded: `run_build` must re-check that `graph.html`
  actually exists after both stages and error if it's still absent — a stage exiting 0 is not
  proof of the output existing. **Lesson for a fresh implementation**: always run both stages,
  and never trust exit code 0 alone as "build done" for this tool.
- **`--code-only` is load-bearing, not optional**: without it, `extract` hard-errors with
  "no LLM API key found (N doc/paper/image file(s) need semantic extraction)" on any repo
  containing a README or images — measured on one project with 1,380 images. Demido wires no
  LLM key into graphify by design, so `--code-only` must always be passed for a structural,
  keyless build.
- **`graph_html` needs its CDN dependency inlined to work offline/sandboxed**: the generated
  `graph.html` pulls the `vis-network` library from unpkg by default. The fix is to cache that
  script into app-data once (`ensure_vis_network`) and regex-swap the `<script src=…
  vis-network…></script>` tag for an inline copy, then render the result in an `iframe
  srcdoc` with `sandbox="allow-scripts"` (zero network at view time). **A specific, easy-to-hit
  trap documented here**: the replacement regex must use `regex::NoExpand` for the replacement
  string, because the replacement is the *whole minified vis-network bundle*, and Rust's
  `regex` crate reads `$g`/`$1`/`${…}` sequences inside that JS as capture-group references,
  silently deleting ~330 characters (undefined group → empty string) and producing
  `Unexpected token '='`, i.e. a blank graph with no error surfaced (the static HTML still
  renders). This is called out as "the same `$`-expansion family as skill-command `$name`,
  Rust-side" — i.e. a recurring class of bug in this codebase family whenever a raw string is
  spliced in via `regex::Regex::replace` without `NoExpand`. **Directly reusable warning for any
  future Rust code that regex-substitutes a large blob of untrusted-shaped text.**
- **Node-position caching to avoid a ~1.5s physics re-stabilization on every open**: the
  generated HTML is patched with a `POS_MARKER` placeholder and a hook
  (`position_cache_hook`) so that when `window.__GRAPHIFY_POS__` holds cached coordinates,
  physics is disabled and nodes are placed pre-stabilized for an instant paint; otherwise
  settled coordinates are reported up via `postMessage` once `stabilizationIterationsDone`
  fires, on `dragEnd`, **and on a 4-second timeout** (so closing the window early still captures
  a usable layout). Positions persist to a `positions.json` file in app-data (same pattern as
  `prefs.json`), not just in memory, specifically because in-memory-only meant every
  first-open-per-session re-stabilized from scratch. A rebuild clears the cached positions
  because node ids/layout can change. Noted remaining cost even with this cache: ~0.9s of iframe
  reconstruction on every reopen (a 2.1MB `srcdoc` reparsed each mount, since the window is
  destroyed on close) — described as needing iframe keep-alive to fix, not something position
  caching alone solves.
- **Tool exposure to the model**: two builtin tools, `graphify_query` (kind: query/path/explain)
  and `graphify_build` (optional `update` flag), offered only when agent mode is not "off" and a
  working folder is set — same tier as `read_file`/`run_command`, deliberately **not** exposed
  in the frontend's user-toggleable tool registry (mode-gated and non-optional by design).
- **Two behavioral guards worth copying directly**:
  - `graphify_build` forces `update=true` whenever a graph already exists for that folder,
    overriding whatever the model passed, because models routinely call build with
    `update=false` on a folder that already has a graph, and a full rebuild is slow and throws
    away the cached layout for no benefit.
  - `graphify_query` is "freshness-aware": before answering, it checks whether any source file's
    mtime is newer than the graph's last-build marker (`graph.html`'s mtime), scoped correctly
    via a `.gitignore`-aware directory walk (the `ignore` crate, `require_git(false)` so a bare
    `.gitignore` still applies, plus a hardcoded `JUNK_DIRS` prune list since `ignore` has no
    built-in `node_modules` default — "that is ripgrep's, not the library's") and a source-file
    extension allowlist (so a changed README/lockfile/image never counts as staleness), capped
    at 50k files and fails open (`false`) on any filesystem error since this is a best-effort
    nudge, not a gate. If stale and the folder's auto-build toggle is on, it refreshes
    incrementally before answering; if stale and auto-build is off, it prefixes a
    `[Note: … may be stale …]` warning rather than silently answering from an outdated graph.
  - The auto-build preference is a small per-folder JSON file (`graphify/prefs.json` in
    app-data, default **true**), read specifically because this has to be visible
    backend-side (it's checked from the message-send path itself), unlike the frontend-only
    skills-enabled toggle.

### Web search / fetch / sources footer

- **Sources footer is a two-sided contract**: a Rust module (`sources.rs`) appends a system
  prompt rider instructing the model to end its reply with a literal `Sources:` line followed by
  `- [Label](url)` bullets, and a frontend parser (`src/lib/parseSources.ts`) peels that block
  off the *tail* of the message only (not mid-message occurrences, which are left as answer
  content — "mid-message `Sources:` list is answer content"). The rider is appended **last**,
  after the caveman block, specifically because caveman compresses prose but a URL is not prose
  and needs to be the nearest instruction to survive intact. It's only appended when
  `web_search`/`web_fetch` are actually offered to the model in that turn — offering the
  citation format to a model with no browsing tool either gets ignored or produces fabricated
  citations.
- **A system-prompt rider alone was measured inert**: "Qwen3.5-9B-Q4_K_M searched, answered from
  results, emitted no footer" with only the top-of-prompt rider present. The fix was a *second*
  reminder injected directly next to the tool result itself (`sources::append_to_web_result`,
  called at the point web_search/web_fetch results are inserted into context) — the same lesson
  the file draws explicitly from the caveman thinking-rider work: "a rule stated once at prompt
  top loses to intervening context." This reminder is skipped when the tool result contains no
  `http` at all (an error or empty result), because telling the model to cite sources when there
  are none produces invented citations, which is worse than a missing footer.
- **Link-preview fetching, several dead ends documented so they aren't re-tried**:
  - Preview fetches only read up to `</head>` or 128KB, not the whole page.
  - Some hosts (`store.epicgames.com`, `gamespot.com`, `bloomberg.com` — measured 2026-08-18)
    return HTTP 403 to every header combination tried, including a full "real browser" header
    set (`Accept`, `Upgrade-Insecure-Requests`, `Sec-Fetch-*`, `sec-ch-ua*`, a proper 4-part
    Chrome UA string) — this is attributed to Cloudflare scoring the TLS/HTTP2 fingerprint, not
    headers, since even the system `curl` binary passed once and then failed fifteen minutes
    later on the same hosts. Impersonating a named crawler UA (`facebookexternalhit/1.1`) does
    get past some of these (GameSpot does serve it) but was rejected on principle: "a named
    company's crawler id is that company's, not ours."
  - **The accepted mitigation, not a full fix**: since `web_fetch` already holds the full HTML
    body for any page it successfully fetched (as opposed to previewed), it opportunistically
    harvests OG-tag metadata from that body into a small session-lived cache
    (`PREVIEW_CACHE`, 512 entries, keyed on URL minus fragment/trailing slash, stored under both
    the cited and post-redirect URL spellings) and the preview panel reads that cache *before*
    attempting its own network request. Deliberately not persisted to disk, since an `og:image`
    is often a CDN path that expires. Rows for genuinely unpreviewable hosts fall back to a
    favicon-only card rather than showing nothing (a blank image slot "reads as half-loaded
    rather than as complete").
  - **DNS-failure red herring**: three simultaneous `theverge.com` preview requests in one panel
    all reported "No such host is known" while a manual `Resolve-DnsName` succeeded three times
    in a row for the same host at the same time. Root cause: the app's own SSRF guard
    (`check_ssrf`) does its own blocking hostname resolution *before* any request is attempted,
    and doing several of these concurrently was starving the process's stub resolver — **not** a
    reqwest-level DNS problem. Fix: distinguish `SsrfRefusal::Dns` (transient, retried once after
    300ms) from `SsrfRefusal::Policy` (final, not retried), and fetch previews via
    `stream::iter(...).buffered(4)` rather than `join_all`, specifically `buffered` (ordered) and
    not `buffer_unordered`, because the caller zips results back against the source list by
    position.
  - Friendlier failure text matters: raw reqwest error chains (three lines of Winsock DNS prose)
    or a bare "HTTP 403" read to a user as "the cited page is dead," when it opens fine in a
    real browser. Fixed with `friendly_status`/`friendly_fetch_error` strings like "Site blocked
    the preview request (HTTP 403)" / "Could not reach this site," plus one retry for a network
    error or a 401/403/429 using an honest, self-identifying UA (`Demido/<ver> link-preview`)
    rather than a spoofed browser UA.
- **A concrete, still-relevant Rust regex trap, unrelated to graphify's own**: `web.rs`'s
  `extract_text` originally used a backreference (`<(script|style)[^>]*>.*?</\1>`), which Rust's
  `regex` crate rejects at compile time (no backreference support) — it panicked on *every* HTML
  `web_fetch` call. Fixed by using one regex per tag instead of one regex with a backreference.
  A test (`extract_text_does_not_panic_on_scripts_and_styles`) guards this. **General lesson for
  this repo**: Rust's `regex` crate supports neither backreferences nor arbitrary replacement
  strings without `NoExpand` — both traps were hit independently in this one file family and are
  worth a standing lint/review note for whoever writes the graphify or web-fetch equivalent here.

### Other qwen-tree observations (kept light per the task's own scope)

- qwen is a close sibling/fork of v1, not an independent design: it shares the exact same
  `AGENTS.md`/`agent-guide/` structure, the same file (`src-tauri/src/caveman.rs` is referenced
  by qwen's own `AGENTS.md` header as defining the Caveman Ultra style used to write the guide
  itself), the same `skills/{agent-manager,demido-dev,market,skill-manager}` and
  `subagents/{code-reviewer.md,explorer.md}` bundled-content layout. Treat qwen's non-graphify
  agent-guide files (`frontend-chat-ui.md`, `generation-loop.md`, `local-engine.md`,
  `market-feeds.md`, `market-tools.md`, `nexus.md`, `plumbing.md`, `prompt-and-skills.md`,
  `subagents.md`) as very likely near-duplicates of v1's own equivalent knowledge rather than
  independent additional research; they were not read in this pass, but skimming their sizes
  (14K-48K each) suggests they'd repay a similar close read before any subagents/plumbing/
  generation-loop ticket is written, given how dense `graphify-and-web.md` turned out to be.
- `S:\Development\DemiPlanComander\qwen\DEMIDO_REBIRTH_ANALYSIS.md` (605 lines) is **not**
  qwen-specific engineering notes; it's a self-critical architecture review of v1 itself
  (explicitly titled "Demido Studio v0.6.5... Comprehensive Codebase Analysis," referencing
  `src-tauri/src/commands.rs` and `src/stores/*` — v1's own file layout). It compares v1 against
  Jan, Open WebUI, and Ollama and catalogs concrete issues by severity (e.g. "C1: Silent
  Failures in Generation Loop," "C2: Poisoned Lock Recovery Missing," "H1: Cross-Store
  Dependencies Without Ordering," "M4: API Keys in Plaintext JSON" — the last one corroborates
  the `secrets.rs` "not encrypted" finding in Task 1 independently). Worth a full read before
  designing this repo's state-management and error-handling story, but it is a critique
  document, not a source of code to port.

---

## Task 5: other v1 features that neither v2 nor this repo currently have

Skimmed against the brief's feature list and v1's own `README.md` (read at commit `26dc12d~1`,
the last commit before the README was deleted). Kept to concrete, real (non-shell) findings not
already covered above.

- **A full non-destructive image editor**, `src/components/image-editor/`: `EditorCanvas.tsx`
  (72.8K, the largest single component file found across v1's frontend), `LayerPanel.tsx`
  (14.1K), `AdjustmentsPanel.tsx` (21.2K), `Toolbar.tsx`, `ToolOptionsBar.tsx`, `Rulers.tsx`.
  This is well beyond what the brief asks for (the brief's artifact system mentions "pictures"
  only as a "down the road" item) — flag as a scope question for whoever triages accounts/
  artifacts tickets, since it represents real, sizeable built functionality that isn't a stated
  v3 requirement yet.
- **Artifact viewers**: `src/components/artifacts/JsonGraphViewer.tsx` (12.6K),
  `JsonTreeViewer.tsx` (3.6K), `ChartArtifactView.tsx` (14.3K), `ArtifactPanel.tsx` (19.4K) —
  concrete prior art for the brief's "Artifact system" section, beyond the generic Claude-Code-
  style sidebar the brief describes; the chart/JSON-graph/JSON-tree viewer types are specific
  ideas worth reviewing before the artifact-system ticket is written from scratch.
- **First-run guided setup**: `src/components/setup/SetupWizard.tsx` (11.2K) — directly answers
  the brief's "Guided set-up on first launch (GPU & GPU Ecosystem selector, Runtime & Dependencies
  installation, etc.)" bullet. Neither this repo nor (per its own `.research/map.md` "Not yet
  specified" list) v2 has this built; v2's map explicitly lists "Cross-platform... packaging,
  installer, updater" as unspecified, and setup/onboarding isn't mentioned there at all, so this
  looks like a genuine gap across both later attempts that v1 alone addressed.
- **Market data / broker integration**, `src-tauri/src/market/{broker,bulk,ccxt,mt5,router,
  store,symbols,quality,gate}.rs` plus `src/components/windows/MarketWindow.tsx` (44.5K, the
  largest window component) and a `market` skill (`skills/market/`) matching the brief's example
  `skill.json` for Market Data almost verbatim (same tool names: `market_quote`,
  `market_history`, `market_search`, `market_sources`, `market_download_estimate`,
  `market_download`). This is real, wired functionality (MT5 + CCXT/keyless crypto), not a
  stub — v1's own commit history shows it landing incrementally (`aa44d0e` "keyless market data +
  market skill, cTrader accounts, v0.6.2", `0874ef0` "MT5 market data, v0.6.4"). v2's `.research/
  map.md` lists "Market data and the charts window, TradingView cookie capture, CCXT" as
  unspecified/not-yet-built, so v1's broker/CCXT code is a real candidate for reference even
  though the brief's TradingView-cookie-capture flow specifically wasn't found in v1 (v1's
  approach is MT5 + CCXT, not TradingView).
- **A splash window**: not confirmed present in v1 in this pass (no `splash.html`/`splash/`
  found in v1's tree during this sweep — this repo's `.research/map.md` for v2 does reference
  `web/src/splash/` as an existing v2 feature, so the splash screen appears to already be a v2,
  not v1, contribution and is out of scope for a v1-salvage call).
- **Nexus (free-model router)**: `src-tauri/src/nexus/{catalog,commands,health,models,router,
  settings}.rs` — matches the brief's "Free models system via OmniRoute/9Route-like system... I
  want the system to be called Nexus" bullet exactly, including the name. Real router/health-
  check/catalog code, not a stub, landed in `0874ef0`/`aa44d0e`. Worth reviewing directly against
  the brief's Nexus bullet when that ticket comes up.

No attempt was made in this task to exhaustively diff every v1 window against the brief;
the above is what surfaced from a structural skim (`find`, file sizes, README, commit
messages) rather than a full read of each feature's code, per the task's own "keep this
efficient" instruction.
