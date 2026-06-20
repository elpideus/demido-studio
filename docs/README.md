# Demido Studio — Codebase Semantic Map

> A semantic map for humans and LLMs to understand the codebase quickly without reading every file.

## TL;DR

**Demido Studio** is a desktop AI chat application built with:
- **Frontend**: React + TypeScript + Vite (UI, stores, components)
- **Backend**: Tauri 2 + Rust (database, LLM API calls, MCP server management, file system tools)
- **Data**: SQLite database for conversations, messages, providers, settings
- **AI Providers**: OpenAI, Anthropic, Gemini, LM Studio, Ollama, Groq (via OpenAI-compatible or native APIs)
- **MCP Support**: Model Context Protocol integration with stdio-based servers
- **Agent Modes**: `off` / `cautious` / `balanced` / `autonomous` — controls permission gating for tool use
- **Built-in Tools**: read_file, write_file, edit_file, list_dir, run_command, search_files

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│                         Demido Studio                        │
├───────────────────┬───────────────────┬─────────────────────┤
│    Frontend       │     Backend        │      Data           │
│  (React/TS/Vite)  │   (Tauri/Rust)    │   (SQLite + Store)  │
├───────────────────┼───────────────────┼─────────────────────┤
│ • App.tsx         │ • main.rs          │ • conversations     │
│ • stores/*.ts     │ • commands.rs     │ • messages           │
│ • components/*    │ • agent/          │ • providers          │
│ • lib/tauri.ts    │   - executor.rs   │ • settings           │
│                   │   - permissions.rs│ • mcp_servers        │
│                   │   - streaming.rs  │ • model_overrides    │
├───────────────────┴───────────────────┼─────────────────────┤
│              Event Bus (Tauri)         │                      │
│              • user_message            │                      │
│              • stream_thinking        │                      │
│              • stream_token           │                      │
│              • tool_call              │                      │
│              • tool_call_result       │                      │
│              • stream_done            │                      │
└───────────────────────────────────────┴─────────────────────┘
```

---

## Frontend — `src/` (TypeScript)

### Entry Points
| File | Purpose |
|------|---------|
| `main.tsx` | React 19 app entry with ErrorBoundary wrapper |
| `App.tsx` | Main layout: Sidebar + ChatView + SettingsPanel; handles auth gate, keyboard shortcuts (Ctrl+Shift+I / F12 for devtools) |

### State Management — Zustand Stores
All stores use **Zustand** and persist where appropriate. They are the single source of truth for UI state.

| Store | File | Key Responsibilities |
|-------|------|---------------------|
| `useConversations` | `stores/conversations.ts` | CRUD conversations; active conversation ID; auto-title generation via event listener (`conversation_title_updated`) |
| `useProviders` | `stores/providers.ts` | List of providers + models per provider; model overrides (custom names); add/delete providers; fetch models from APIs |
| `useMcpTools` | `stores/mcpTools.ts` | MCP tool list with enable/disable state; server-level override (toggle all tools in a server); collapsed UI state; persisted to `prefs.json` via Tauri store plugin |
| `useMessages` | `stores/messages.ts` | Messages for active conversation; streaming state (`streaming`, `streamBuffer`, `streamBlocks`); permission requests queue; persists message blocks (thinking/tool) per message in localStorage |
| `useSettings` | `stores/settings.ts` | App-wide settings: default provider/model, system prompt, auth PIN, context window limit, title generation frequency |

### Components — `components/`

#### Auth & Layout
| Component | File | Purpose |
|-----------|------|---------|
| `AuthGate` | `auth/AuthGate.tsx` | PIN entry screen; reads secret from Tauri (`auth_pin`) |
| `Sidebar` | `sidebar/Sidebar.tsx` | Conversation list with search bar and new chat button |

#### Chat UI — `chat/`
| Component | File | Purpose |
|-----------|------|---------|
| `ChatView` | `chat/ChatView.tsx` | Main chat container; renders header, message list, input bar |
| `ChatHeader` | `chat/ChatHeader.tsx` | Conversation title, model selector, agent mode dropdown, working directory picker, permission bubble (if pending) |
| `InputBar` | `chat/InputBar.tsx` | Textarea for user messages; file attachment support; disabled tools toggle; reasoning effort selector |
| `MessageBubble` | `chat/MessageBubble.tsx` | Renders a single message: text/thinking blocks, tool calls with results, streaming indicators |
| `MessageList` | `chat/MessageList.tsx` | Scrollable list of messages; handles truncation (delete after/from) |
| `ModelSelector` | `chat/ModelSelector.tsx` | Dropdown to select model from current provider's available models |
| `ReasoningSelector` | `chat/ReasoningSelector.tsx` | Select reasoning effort (`off`, `low`, `medium`, `high`) — only shown for supported models |
| `TimelineStrip` | `chat/TimelineStrip.tsx` | Visual timeline of tool calls and results interleaved with assistant messages |
| `ToolSelector` / `ToolSelectorPopup` | `chat/ToolSelector*.tsx` | UI to toggle built-in or MCP tools on/off for the current turn |

#### Settings — `settings/`
| Component | File | Purpose |
|-----------|------|---------|
| `SettingsPanel` | `settings/SettingsPanel.tsx` | Modal overlay; opens via sidebar button; contains all settings tabs |
| `ProvidersSettings` | `settings/ProvidersSettings.tsx` | Add/edit/delete providers; shows model overrides per provider |
| `InterfaceSettings` | `settings/InterfaceSettings.tsx` | UI preferences (theme, font size, etc.) — *not yet implemented* |
| `McpSettings` | `settings/McpSettings.tsx` | Configure MCP servers: add/edit/delete; test connection; toggle enable/disable |
| `SystemPromptSettings` | `settings/SystemPromptSettings.tsx` | Edit the global system prompt injected into every assistant message |

### Utilities — `lib/`
| File | Purpose |
|------|---------|
| `tauri.ts` | TypeScript bindings to Tauri commands (`db`, `chat`, `reasoning`, `agent`, `exportChat`, `mcp`) |
| `utils.ts` | Small helpers (currently empty) |

### Types — `types.ts`
Core data structures shared between frontend and backend:
```typescript
Conversation  // id, title, provider_id, model_id, agent_mode, working_directory
Message       // id, conversation_id, role, content, tool_call_id, thinking
Provider      // id, name, type (openai_compat|openai|anthropic|gemini), base_url, api_key_ref
ModelOverride // custom name for a specific provider+model pair
McpServer     // id, name, transport (stdio|sse), command/args/env/url, enabled
AppSettings   // default_provider_id, system_prompt, auth_enabled, context_window_limit, etc.
FileAttachment // name + content as base64 or raw text
```

---

## Backend — `src-tauri/src/` (Rust)

### Entry Points
| File | Purpose |
|------|---------|
| `main.rs` | Tauri app entry; calls `demido_studio_lib::run()` |
| `lib.rs` | Exposes all Tauri commands via `#[tauri::command]`; holds global state (`AppState`) |

### Global State — `AppState`
```rust
pub struct AppState {
    pub conn: Mutex<rusqlite::Connection>,      // SQLite database
    pub secrets: Secrets,                        // Encrypted key-value store for API keys
    pub mcp: Mutex<McpManager>,                  // MCP server connections and tool cache
    pub active_cancel: Mutex<Option<Arc<AtomicBool>>>,  // Cancel flag for current stream
    pub http_client: reqwest::Client,            // HTTP client for LLM APIs
    pub pending_permission: Mutex<Option<tokio::sync::oneshot::Sender<bool>>>,  // Channel to deliver permission decision
}
```

### Tauri Commands — `commands.rs`
All user-facing commands. Each is annotated with `#[tauri::command]` and receives `State<AppState>` for access to global state.

| Command | Purpose |
|---------|---------|
| `list_conversations` / `create_conversation` / `delete_conversation` / `update_conversation_title` | Conversation CRUD |
| `list_messages` / `insert` (via stream) / `delete_after` / `delete_from` / `update_content` | Message management |
| `list_providers` / `upsert_provider` / `delete_provider` | Provider management; deletes associated secret on removal |
| `get_settings` / `set_setting` | App settings read/write |
| `get_secret` / `set_secret` | Encrypted key-value store (used for API keys) |
| `search_conversations` | FTS5 full-text search across message content |
| `list_models` | Fetch available models from a provider's API |
| `get_model_reasoning` | Query model-specific reasoning capabilities; supports LM Studio native API fallback |
| `cancel_stream` / `respond_to_permission` | Control active generation and permission flow |
| `send_message` | **Main entry point** for user messages; orchestrates streaming, tool execution, title generation |
| `continue_generation` | Resume an interrupted assistant response without new user input |
| `list_mcp_servers` / `save_mcp_servers` / `test_mcp_server` | MCP server management |
| `list_mcp_tools` | Returns cached tools from all enabled MCP servers |
| `export_conversation` | Exports a conversation to JSON with reconstructed tool calls and results |
| `open_devtools` | Opens browser devtools (F12 shortcut) |

### Agent & Tool Execution — `agent/`

#### `mod.rs` — Built-in Tools Definition
Defines 6 built-in tools available when agent mode is not `off`:
```rust
ToolDef {
    name: "read_file",
    description: "Read the full contents of a file at the given path.",
    input_schema: {"path": string}
}
// ... write_file, edit_file, list_dir, run_command, search_files ...
```

#### `executor.rs` — Tool Execution Engine
Executes built-in tools synchronously in `spawn_blocking` to avoid blocking the async runtime.

| Tool | Behavior |
|------|----------|
| `read_file` | Resolves path relative to working directory; returns file contents or error |
| `write_file` | Creates parent directories if needed; writes content; returns byte count written |
| `edit_file` | Replaces **first** occurrence of `old_str` with `new_str`; returns error if not found |
| `list_dir` | Returns sorted list of entries with type and size (empty dir → "(empty directory)") |
| `run_command` | Runs PowerShell command via `powershell.exe -NonInteractive -Command ...`; output capped at 10KB; appends STDERR if present |
| `search_files` | Walks directory tree, filters by glob pattern, skips files >1MB to avoid OOM; returns up to 200 matches with file:line:content format |

#### `permissions.rs` — Permission Gating
Determines whether a tool call requires user approval based on **agent mode** and **path sensitivity**.

| Mode | Behavior |
|------|----------|
| `cautious` | Always ask for permission before any tool execution |
| `autonomous` | Never asks; executes all tools immediately |
| `balanced` | Asks only for:
- Any write_file, edit_file, run_command
- read_file if path matches sensitive patterns (`.env`, `secret*`, `.key`, etc.)
- Allows list_dir and search_files without asking |

Sensitive pattern matching checks both filename and full path (case-insensitive).

### Database — `db/`
SQLite schema with FTS5 for message search. Uses WAL mode and foreign keys.

#### Tables
| Table | Columns | Purpose |
|-------|---------|---------|
| `conversations` | id, title, provider_id, model_id, created_at, updated_at, agent_mode, working_directory | Conversation metadata; auto-updated via triggers |
| `messages` | id, conversation_id (FK), role, content, tool_call_id, created_at, token_count, thinking | Chat history; FTS5 virtual table for full-text search |
| `providers` | id, name, type, base_url, api_key_ref, enabled, sort_order, visible | LLM provider configuration |
| `settings` | key (PK), value (JSON-encoded) | App-wide settings |
| `mcp_servers` | id, name, transport, command, args, url, env, enabled | MCP server configurations |
| `model_overrides` | provider_id, model_id, custom_name, enabled | Custom names for specific models |

#### Migrations — `db/mod.rs`
5 migrations tracked via `schema_version` table:
1. Core tables + FTS5 triggers (AI/AD)
2. Additional FTS5 triggers (DE/AU) — *note: DE trigger has a bug, see below*
3. Rename OpenAI type to `openai_compat`
4. Add `agent_mode` and `working_directory` columns
5. Add `env` column to mcp_servers

**Known Bug**: Migration 2's `messages_ad` (DELETE) trigger incorrectly references `old.rowid` instead of `new.rowid`. This causes FTS corruption on message deletion. **Fix**: Replace `old.rowid` with `new.rowid` in the DELETE trigger.

### MCP — `mcp/`
Manages stdio-based Model Context Protocol servers.

#### `mod.rs` — `McpManager`
- Holds list of configured servers and cached tools
- On `load_servers()`: spawns each enabled stdio server, calls `/initialize`, caches returned tools with server_id attached
- `list_tools()` returns all cached tools (used by frontend to build tool selector)
- `get_stdio_client(server_id)` returns Arc for calling MCP tool methods

#### `stdio.rs` — Stdio Client Implementation
Implements MCP stdio transport:
- Spawns process via `std::process::Command`
- Reads/writes to stdin/stdout with proper buffering and line termination handling
- Implements `/initialize`, `/tools/list`, `/call_tool` per MCP spec
- Handles errors gracefully (initialization failures, tool call panics)

#### `types.rs`
```rust
pub struct McpServer {
    pub id: String,
    pub name: String,
    pub transport: String,  // "stdio" or "sse"
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    pub env: Option<Record<string, string>>,
    pub url: Option<String>,
    pub enabled: bool
}
```

### Streaming — `streaming.rs`
SSE (Server-Sent Events) parser for Gemini API responses.
- Reads byte stream from `reqwest::Response.bytes_stream()`
- Strips `data: ` prefix, skips blank lines and comments (`:`)
- Yields complete event payloads as strings
- Used by Gemini provider to stream chat completions

### Providers — `providers/`
Abstracts LLM API differences behind a unified interface.

#### Unified Interface (in `mod.rs`)
```rust
pub async fn list_models(client: &Client, type_, base_url, api_key) -> Vec<String>
pub async fn stream_chat(client, app, type_, base_url, api_key, model, messages, system_prompt, tools, reasoning_effort, cancel) -> StreamOutput
pub async fn complete_chat(client, type_, base_url, api_key, model, messages, system_prompt) -> String
```

#### Provider Implementations
| File | API Style | Notes |
|------|-----------|-------|
| `openai_compat.rs` | OpenAI Chat Completions v1 | Used by LM Studio, Ollama, Groq; handles streaming with cancellation token |
| `anthropic.rs` | Anthropic Messages API | Native implementation; supports tool use and reasoning (thinking) blocks |
| `gemini.rs` | Google Generative AI API | Uses SSE parser from `streaming.rs`; handles thinking blocks natively |

**Key Design Decision**: All non-Anthropic/non-Gemini providers share the same OpenAI-compatible code path, reducing duplication.

### Secrets — `secrets.rs`
Encrypted key-value store for storing API keys locally.
- Uses Tauri's `@tauri-apps/plugin-store` with encryption (AES-256)
- Keys are stored under `prefs.json` but encrypted at rest
- Provides `get(key)` and `set(key, value)` methods used by `commands.rs`

---

## Data Flow — Core Operations

### Sending a User Message (`send_message` command)
1. **Preparation** (sync):
   - Resolve provider/model from request or conversation defaults
   - Fetch system prompt and agent mode from DB
   - Build tool list: MCP tools + built-in tools if agent_mode ≠ `off`
   - Emit `stream_status("Processing prompt")` to open frontend stream gate
2. **Persist user message** (sync):
   - Insert into `messages` table with role=`user`
   - Update conversation's `updated_at` via trigger
3. **Emit event** (async):
   - Frontend receives `user_message` event and adds to UI
4. **Async generation loop** (`run_generation_loop`):
   - Build API message array from DB messages
   - If first iteration and attachments provided, replace last user message content with blocks
   - Check for pending tool results → emit `stream_status("Processing tool results")`
   - Call provider's `stream_chat()` with cancel token
5. **Stream handling**:
   - On assistant text: insert into DB, emit `stream_done` (if no tools) or continue loop
   - On tool calls: emit `tool_call` event → frontend asks permission if needed → execute in `spawn_blocking` → emit `tool_call_result` and `tool` message to DB
6. **Post-generation**:
   - If no tools were called, attempt auto-title generation using task model
   - Emit `conversation_title_updated` for UI sync

### Permission Flow (Balanced Mode)
1. Model emits tool call → backend receives via `stream_chat`
2. Check `is_permitted(agent_mode="balanced", tool_name, args)`
3. If sensitive or write operation: emit `tool_permission_request` to frontend
4. Frontend shows permission bubble → user clicks Allow/Deny
5. Backend receives via `respond_to_permission` channel
6. Execute tool if approved; otherwise return "Permission denied by user"

### Auto-Title Generation
Triggered after every assistant response with no tools:
1. Count assistant messages in conversation
2. If count == 1 or divisible by `title_every_n_messages`, generate title
3. Use task provider/model to call LLM with conversation history + prompt: "Summarise this conversation as a short title (5 words or fewer)..."
4. Strip markdown and punctuation from response
5. Update DB and emit event for UI

---

## Event Bus — Tauri Emitter Events
Events flow from backend → frontend via `app.emit()`:

| Event | Payload | Trigger |
|-------|---------|---------|
| `user_message` | Message object | User sends message |
| `stream_thinking` | String chunk | Model emitting thinking block (Anthropic/Gemini) |
| `stream_token` | String chunk | Model streaming text tokens |
| `tool_call` | `{name, id, args}` | Model made tool call |
| `tool_call_result` | `{id, name, result}` | Tool execution completed |
| `stream_done` | Message object (with `__tool_calls__` if applicable) | Generation finished |
| `stream_status` | `{label: string|null}` | Status label for UI indicator |
| `stream_cancelled` | — | User cancelled generation |
| `conversation_title_updated` | `{id, title}` | Auto-title generated |

---

## Configuration & Defaults

### Default Providers (seeded on first run)
| ID | Name | Type | Base URL | Enabled |
|----|------|------|----------|---------|
| `lmstudio` | LM Studio | openai_compat | http://localhost:1234/v1 | ✅ |
| `ollama` | Ollama | openai_compat | http://localhost:11434/v1 | ❌ |
| `openai` | OpenAI | openai_compat | https://api.openai.com/v1 | ❌ |
| `anthropic` | Anthropic | anthropic | https://api.anthropic.com | ❌ |
| `groq` | Groq | openai_compat | https://api.groq.com/openai/v1 | ❌ |

### Default App Settings
```typescript
default_provider_id: ""
default_model_id: ""
system_prompt: ""
auth_enabled: false
context_window_limit: 8192
task_provider_id: ""
task_model_id: ""
title_every_n_messages: 5
```

---

## Known Issues & TODOs

| Issue | Location | Description |
|-------|----------|-------------|
| FTS DELETE trigger bug | `db/mod.rs` migration 2 | Uses `old.rowid` instead of `new.rowid`; corrupts FTS index on message deletion |
| Context window limit not enforced | `commands.rs` `build_api_messages()` | Setting exists but messages are never trimmed; will cause API errors on long conversations |
| Interface settings unimplemented | `settings/InterfaceSettings.tsx` | Component exists but no backend support yet |

---

## Quick Reference — File Map

### Frontend (`src/`)
```
App.tsx                    → Main layout, auth gate, keyboard shortcuts
main.tsx                   → React entry with ErrorBoundary
stores/
  conversations.ts         → Conversation CRUD + active ID
  providers.ts             → Providers, models, overrides
  mcpTools.ts              → MCP tool states + server overrides
  messages.ts              → Messages, streaming state, permission queue
  settings.ts              → App-wide settings
components/
  auth/AuthGate.tsx        → PIN entry screen
  sidebar/*                → Conversation list UI
  chat/*                   → Chat UI (header, input, bubbles, etc.)
  settings/*               → Settings modal and tabs
lib/tauri.ts               → Tauri command bindings
types.ts                   → Shared TypeScript interfaces
```

### Backend (`src-tauri/src/`)
```
main.rs                    → Tauri entry point
lib.rs                     → Command definitions + AppState global state
commands.rs                → All #[tauri::command] handlers
agent/
  mod.rs                   → Built-in tool definitions
  executor.rs              → Tool execution engine (6 built-in tools)
  permissions.rs           → Permission gating logic
db/
  mod.rs                   → Schema, migrations, FTS setup
  conversations.rs        → Conversation CRUD + triggers
  messages.rs             → Message CRUD + search
  mcp_servers.rs          → MCP server persistence
  model_overrides.rs      → Custom model names
  providers.rs            → Provider CRUD
  settings.rs             → Settings read/write
mcp/
  mod.rs                   → McpManager lifecycle
  stdio.rs                 → Stdio transport implementation
  types.rs                 → McpServer / McpTool structs
providers/
  mod.rs                   → Unified API interface + routing
  openai_compat.rs        → OpenAI-compatible providers (LM Studio, Ollama, etc.)
  anthropic.rs            → Anthropic Messages API
  gemini.rs               → Google Generative AI API
streaming.rs               → SSE parser for Gemini
secrets.rs                 → Encrypted key-value store
```

---

## How to Work with This Codebase (LLM Tips)

1. **Start at the top**: Read `README.md` and this semantic map before diving into files.
2. **Understand data flow first**: Trace how a user message becomes an assistant response (`commands.rs::send_message` → `run_generation_loop`).
3. **Use types as contracts**: Frontend and backend share `types.ts`; changes here affect both sides.
4. **State is centralized**: All UI state lives in Zustand stores; they are the single source of truth for React components.
5. **Backend commands are pure functions**: Each Tauri command receives `State<AppState>` but doesn't mutate it directly (except via DB operations).
6. **MCP tools are cached**: Tools from MCP servers are fetched once on startup and reused; no per-call API overhead.
7. **Agent mode controls permissions**: Check `permissions.rs` to understand when the UI will show a permission bubble.
8. **Streaming is event-driven**: The frontend listens to Tauri events, not polling; ensure you're handling all relevant events in `useMessages.ts::startListening()`.
9. **Database uses FTS5**: Full-text search works across message content; use `search_conversations` command for queries.
10. **Known bug on DELETE triggers**: Be careful modifying migration 2 — the `old.rowid` → `new.rowid` fix is required before deleting messages in production.

---

*Last updated: 2026-06-12*
