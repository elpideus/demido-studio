# TODO — Demido Studio

Items are grouped by area and tagged `[bug]`, `[security]`, `[perf]`, `[refactor]`, `[feat]`, `[chore]`.

---

## Backend (Rust / Tauri)

### High priority

- **[refactor]** `commands.rs` is 1 461 lines — God file. Split into:
  - `commands/conversation.rs` — CRUD for conversations/messages
  - `commands/streaming.rs` — `send_message`, `cancel_stream`, `continue_generation`
  - `commands/fs.rs` — `fs_list_dir`, `fs_read_file`, `fs_walk`, `fs_rename`, `fs_delete`, `fs_copy_dir`
  - `commands/provider.rs` — model listing, provider management
  - `commands/settings.rs` — settings, secrets, agent mode

- **[perf]** `AppState.conn: Mutex<Connection>` serialises every DB read/write through a single lock. Replace with a connection pool (`r2d2` + `r2d2_sqlite`) or at minimum two connections (WAL allows one writer + many readers).

- **[security]** `secrets.rs` stores API keys as plaintext JSON in the app-data directory. Use the OS keychain via `keyring` crate (or the Tauri `keyring` plugin) so keys are never on disk in cleartext.

- **[security]** `tauri.conf.json` has `"csp": null` — no Content Security Policy. Set a strict CSP that allows only `'self'` and whitelisted API origins.

- **[bug]** `Mutex::lock().unwrap()` panics if a previous lock holder panicked (poison). Replace with `.lock().map_err(|_| "lock poisoned")` or `unwrap_or_else` pattern throughout `commands.rs`.

- **[bug]** FTS5 index only covers messages inserted *after* migration 1. Fresh installs are fine, but upgraded databases where messages pre-date the trigger have stale/empty FTS entries. Add a one-time backfill in migration 2 or a dedicated migration.

- **[chore]** `Cargo.toml` has `authors = []` — fill in the project author(s).

- **[chore]** `package.json` name is `"demido-temp"` — rename to `"demido-studio"`.

### Medium priority

- **[refactor]** `streaming.rs` / `agent/` — the generation loop is split across `commands.rs`, `streaming.rs`, and `agent/`. Consolidate the entry-point and clarify ownership so the cancel-flag lifecycle is obvious.

- **[perf]** `build_api_messages()` re-reads the entire conversation history on every tool-call loop iteration. Cache the slice and append incrementally.

- **[refactor]** `mcp/types.rs` still includes `transport: 'sse'` variant in the Rust enum and the TS type even though the SSE transport was removed (`mcp/sse.rs` deleted). Remove the dead arm from the match and the TS union.

- **[feat]** MCP servers currently have no timeout on `initialize` or `tools/list`. Long-hanging servers block the startup path. Add a per-server timeout (e.g. 10 s) with a graceful kill.

- **[feat]** No structured logging — errors surface as `eprintln!` or silent `.ok()` swallows. Add `tracing` + `tracing-subscriber` and emit structured events for stream errors, tool failures, and DB errors.

- **[perf]** `reqwest::Client` is shared globally (good), but no connection-pool limits are set. Large concurrent tool calls could exhaust file descriptors. Configure `pool_max_idle_per_host`.

### Low priority

- **[refactor]** `db/mod.rs` `run_migrations` contains a bare backfill (`UPDATE providers SET visible = 1 …`) outside of the migration table — it runs on every startup. Move it into a proper numbered migration.

- **[chore]** `devtools` feature flag on `tauri = { version = "2", features = ["devtools"] }` should be behind a `#[cfg(debug_assertions)]` conditional so it's excluded from release builds.

- **[feat]** No database backup / export mechanism beyond `export_conversation` (single conversation to Markdown). Consider periodic WAL checkpoint + copy to a user-chosen path.

---

## Frontend (React / TypeScript)

### High priority

- **[security]** `DOMPurify` is imported but verify it's actually applied to *all* HTML rendered from AI output — a missed usage site could enable stored XSS via tool results injected into the DOM.

- **[bug]** `messageBlocks` are persisted to `localStorage` keyed by conversation ID but never evicted. For users with many conversations this will grow unboundedly. Add an LRU cap or TTL eviction.

- **[refactor]** `InputBar.tsx` is 20+ KB — it handles file attachment, drag-and-drop, audio, clipboard paste, model overrides, and message submission. Extract into focused sub-components.

### Medium priority

- **[refactor]** `stores/messages.ts` — `_activeCleanup` is a module-level mutable variable that tracks the current Tauri event listener. This is fragile; move cleanup state into the store itself.

- **[perf]** `MessageList.tsx` (10 KB) renders all messages without virtualisation. Long conversations will cause noticeable jank. Add `react-window` or a scroll-anchor approach.

- **[feat]** No optimistic UI rollback on stream error — if `stream_error` fires, the partial message remains. Clear or clearly mark errored partial messages.

- **[refactor]** `stores/imageEditor.ts` is 13+ KB — the image editor state machine is embedded in a Zustand store. Extract the business logic into a plain module and keep the store thin.

- **[chore]** `src/lib/utils.ts` is 166 B (basically just `cn()`). If that's all it does, inline it or merge with `constants.ts`.

### Low priority

- **[feat]** No keyboard shortcut system. Common actions (new conversation, focus input, cancel generation) have no keybindings.

- **[feat]** No theme support beyond what Tailwind provides. Dark/light toggle is absent.

- **[chore]** `vision_capability_detection.md` and `AGENTS.md` are in the repo root — move to `docs/` or remove if internal-only.

---

## Testing

- **[chore]** Zero Rust unit tests. Add tests for at minimum:
  - `db/messages.rs` — insert/query/delete round-trips
  - `providers/mod.rs` — `build_api_messages` edge cases
  - `agent/permissions.rs` — path-sensitive heuristics

- **[chore]** Frontend tests (`src/test/`) — expand coverage for `windowManager` store and streaming state machine.

- **[chore]** No integration / e2e tests. The `@playwright/test` dev-dep exists but no tests are written. Write at least a smoke test (app launches, can create a conversation).

---

## DevOps / CI

- **[chore]** No CI pipeline. Add GitHub Actions with:
  - `cargo check` + `cargo clippy --deny warnings`
  - `cargo test`
  - `npm run build` (type-check)
  - `npm test`

- **[chore]** No release workflow. Add a GitHub Actions workflow that builds NSIS/MSI installers on tag push and attaches them to a GitHub Release.

- **[chore]** No Dependabot / Renovate config for automated dependency updates.

---

## Documentation

- **[chore]** `docs/ARCHITECTURE.md` references SSE transport as a supported option — update to reflect removal.

- **[chore]** No changelog (`CHANGELOG.md`). Start one before the first public release.

- **[chore]** No API / IPC surface documentation. Consider generating it from the `#[tauri::command]` annotations.
