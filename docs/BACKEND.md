# Demido Studio — Backend & Rust Deep Dive

> Complete reference for backend developers, LLMs working with Tauri commands, and anyone needing to understand the Rust side.

---

## Entry Points

### `main.rs`
```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
fn main() {
    demido_studio_lib::run()
}
```
- **Purpose**: Tauri app entry point.
- **Windows-specific**: Prevents additional console window in release builds.

### `lib.rs` — Command Registry & Global State
```rust
pub struct AppState {
    pub conn: Mutex<rusqlite::Connection>,
    pub secrets: Secrets,
    pub mcp: Mutex<McpManager>,
    pub active_cancel: Mutex<Option<Arc<AtomicBool>>>,
    pub http_client: reqwest::Client,
    pub pending_permission: Mutex<Option<tokio::sync::oneshot::Sender<bool>>>,
}
```
**Key Design Decisions:**
- **Mutex for DB**: Single-threaded Rust backend; no connection pooling needed.
- **Arc<AtomicBool> for cancel token**: Shared across async tasks without data races.
- **oneshot channel for permissions**: Delivers user decision exactly once to `respond_to_permission()`.

---

## Tauri Commands — Complete Reference

All commands are defined in `commands.rs` with `#[tauri::command]`. Each receives `State<AppState>` for access to global state.

### Conversation Management
| Command | Signature | Purpose |
|---------|-----------|---------|
| `list_conversations` | `fn(state: State<AppState>) -> Result<Vec<Conversation>, String>` | Fetch all conversations from DB |
| `create_conversation` | `fn(state, provider_id: String, model_id: String) -> Result<Conversation, String>` | Insert new conversation; returns created record |
| `delete_conversation` | `fn(state, id: String) -> Result<(), String>` | Delete + cascade delete messages via FK |
| `update_conversation_title` | `fn(state, id, title) -> Result<(), String>` | Update title; triggers `updated_at` change |

### Message Management
| Command | Signature | Purpose |
|---------|-----------|---------|
| `list_messages` | `fn(state, conversation_id) -> Result<Vec<Message>, String>` | Fetch messages for conversation |
| `delete_messages_after` | `fn(state, message_id) -> Result<(), String>` | Delete all messages after this one (keeps context) |
| `delete_messages_from` | `fn(state, message_id) -> Result<(), String>` | Delete this and all following messages (for long convos) |
| `update_message_content` | `fn(state, message_id, content) -> Result<(), String>` | Update a specific message's content |

### Provider Management
| Command | Signature | Purpose |
|---------|-----------|---------|
| `list_providers` | `fn(state) -> Result<Vec<Provider>, String>` | Fetch all providers |
| `upsert_provider` | `fn(state, provider: Provider) -> Result<(), String>` | Insert or update provider record |
| `delete_provider` | `fn(state, id) -> Result<(), String>` | Delete + cascade delete associated secret via `state.secrets.delete(key_ref)` |

### Settings & Secrets
| Command | Signature | Purpose |
|---------|-----------|---------|
| `get_settings` | `fn(state) -> Result<AppSettings, String>` | Fetch all settings from DB |
| `set_setting` | `fn(state, key: String, value: String) -> Result<(), String>` | Save single setting; **value is JSON-encoded** (e.g., `"\"hello\""` for string `hello`) |
| `get_secret` | `fn(state, key) -> Result<Option<String>, String>` | Retrieve encrypted secret by reference |
| `set_secret` | `fn(state, key, value) -> Result<(), String>` | Store encrypted secret |

### Search & Query
| Command | Signature | Purpose |
|---------|-----------|---------|
| `search_conversations` | `fn(state, query: String) -> Result<Vec<SearchResult>, String>` | FTS5 full-text search across message content; returns `{ conversation_id, snippet }[]` |

### Model & Reasoning
| Command | Signature | Purpose |
|---------|-----------|---------|
| `list_models` | `fn(state, provider_id) -> Result<Vec<String>, String>` | Fetch available models from provider API (async) |
| `get_model_reasoning` | `fn(state, provider_id, model_id) -> Result<Option<ReasoningInfo>, String>` | Query model-specific reasoning capabilities; supports LM Studio native API fallback |

### Generation Control
| Command | Signature | Purpose |
|---------|-----------|---------|
| `cancel_stream` | `fn(state)` | Sets cancel flag on active generation; also sends `false` to pending permission channel |
| `respond_to_permission` | `fn(state, approved: bool)` | Delivers user's Allow/Deny decision via oneshot channel |

### Main Generation Entry Point — `send_message`
```rust
#[tauri::command]
pub async fn send_message(
    app: AppHandle,
    state: State<'_, AppState>,
    req: SendMessageRequest,
) -> Result<(), String>
```
**Parameters:**
- `app`: Used to emit Tauri events (`stream_status`, `user_message`, etc.)
- `state`: Access to DB, secrets, MCP manager, cancel token
- `req`: `{ conversation_id, content, disabled_tools?, reasoning_effort?, provider_id?, model_id?, attachments? }`

**Execution Flow:**
1. **Sync prep**: Resolve provider/model from request or conversation defaults; fetch system prompt and agent mode
2. **Build tool list**: MCP tools + built-in tools if `agent_mode ≠ "off"`
3. **Emit event**: `stream_status("Processing prompt")` opens frontend stream gate
4. **Persist user message**: Insert into DB with role=`user`; update conversation's `updated_at` via trigger
5. **Emit event**: `user_message` for UI rendering
6. **Async loop** (`run_generation_loop`):
   - Build API messages from DB (handles tool call/result pairing)
   - If first iteration + attachments, replace last user message content with blocks
   - Check for pending tool results → emit `stream_status("Processing tool results")`
   - Call provider's `stream_chat()` with cancel token
7. **Stream handling**:
   - No tools: insert assistant message, emit `stream_done`, trigger auto-title generation
   - Has tools: insert assistant message with `__tool_calls__` sentinel, execute each tool in `spawn_blocking`, emit `tool_call`/`tool_call_result` events
8. **Cleanup**: Reset cancel token

### Continue Generation — `continue_generation`
```rust
#[tauri::command]
pub async fn continue_generation(
    app: AppHandle,
    state: State<'_, AppState>,
    conversation_id: String,
    provider_id: Option<String>,
    model_id: Option<String>,
    disabled_tools: Option<Vec<String>>,
    reasoning_effort: Option<String>,
) -> Result<(), String>
```
- Same execution flow as `send_message` but without new user message
- Used when user clicks "Continue" on an interrupted assistant response
- Calls `conversations::touch()` on success to update `updated_at`

### MCP Server Management
| Command | Signature | Purpose |
|---------|-----------|---------|
| `list_mcp_servers` | `fn(state) -> Result<Vec<McpServer>, String>` | Fetch from DB |
| `save_mcp_servers` | `fn(state, servers: Vec<McpServer>) -> Result<(), String>` | Save to DB + reload into McpManager (spawns stdio clients) |
| `test_mcp_server` | `fn(state, server: McpServer) -> Result<usize, String>` | Test connection; returns tool count after initialize call |

### MCP Tools Query — `list_mcp_tools`
```rust
#[tauri::command]
pub fn list_mcp_tools(state: State<AppState>) -> Result<Vec<McpTool>, String>
```
- Returns cached tools from all enabled stdio servers
- Called by frontend to populate tool selector dropdowns
- No per-call API overhead — fetched once on startup via `McpManager::load_servers()`

### Export Conversation — `export_conversation`
```rust
#[tauri::command]
pub fn export_conversation(
    state: State<AppState>,
    conversation_id: String,
    file_path: String,
) -> Result<(), String>
```
**Export format (JSON):**
```json
{
  "version": "1.0",
  "exported_at": "2026-06-12T...Z",
  "conversation": { ... },
  "provider_name": "LM Studio",
  "messages": [
    {
      "role": "assistant",
      "content": "",
      "thinking": null,
      "tool_calls": [
        {
          "id": "call_abc123",
          "name": "read_file",
          "arguments": {"path": "/src/main.rs"},
          "result": "..."
        }
      ],
      "created_at": 1749800000000
    },
    ...
  ]
}
```
- Reconstructs tool calls into their parent assistant message (removes `__tool_calls__` sentinel)
- Tool results are looked up by `tool_call_id` from the `tool` role messages

### Utility Commands
| Command | Signature | Purpose |
|---------|-----------|---------|
| `open_devtools` | `fn(app: AppHandle)` | Opens browser devtools via Tauri's webview API (F12 shortcut) |

---

## Agent & Tool Execution — `agent/`

### Built-in Tools Definition (`mod.rs::builtin_tool_defs()`)
```rust
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,  // JSON Schema for validation
}
```
**6 built-in tools:**
1. `read_file` — Read file contents at given path
2. `write_file` — Create/overwrite file with content (creates parent dirs)
3. `edit_file` — Replace first occurrence of `old_str` with `new_str`
4. `list_dir` — List directory entries with type and size
5. `run_command` — Execute PowerShell command via `powershell.exe -NonInteractive -Command ...`
6. `search_files` — Regex search across files in directory tree (skips files >1MB)

### Tool Execution Engine (`executor.rs::execute_tool()`)
```rust
pub fn execute_tool(name: &str, args: &Value, working_dir: Option<&str>) -> String
```
**Key Implementation Details:**
- **Path resolution**: Relative paths are resolved against `working_directory` (or current dir if null)
- **Spawn_blocking for tools**: Prevents blocking the async runtime during file I/O or command execution
- **Output truncation**: `run_command` caps output at 10KB to prevent OOM on large outputs
- **Glob matching**: Simple implementation supporting only `*` (match any substring); no regex support in glob patterns

### Permission Gating (`permissions.rs::is_permitted()`) | Returns `PermissionResult::{Allow, Ask}`

| Mode | Behavior |
|------|----------|
| `cautious` | Always ask for permission before ANY tool execution |
| `autonomous` | Never asks; executes all tools immediately |
| `balanced` | Asks only for:
- Any write_file, edit_file, run_command
- read_file if path matches sensitive patterns (`.env`, `secret*`, `.key`, etc.)
- Allows list_dir and search_files without asking |

**Sensitive pattern matching:** Checks both filename and full path (case-insensitive). E.g., `.env`, `secrets/database.toml`, `config/password.yml` all trigger permission requests.

---

## Database — `db/`

### Schema Overview
```sql
-- Conversations with auto-updated timestamp via triggers
CREATE TABLE conversations (
    id          TEXT PRIMARY KEY,
    title       TEXT NOT NULL DEFAULT 'New conversation',
    provider_id TEXT NOT NULL DEFAULT '',
    model_id    TEXT NOT NULL DEFAULT '',
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL,
    agent_mode  TEXT NOT NULL DEFAULT 'off',
    working_directory TEXT
);

-- Messages with FTS5 virtual table for full-text search
CREATE TABLE messages (
    id              TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    role            TEXT NOT NULL,  -- user | assistant | system | tool
    content         TEXT NOT NULL,
    tool_call_id    TEXT,
    created_at      INTEGER NOT NULL,
    token_count     INTEGER,
    thinking        TEXT
);

-- FTS5 virtual table for fast search
CREATE VIRTUAL TABLE messages_fts USING fts5(
    content, conversation_id UNINDEXED, message_id UNINDEXED
);

-- Providers with API key references (not stored plaintext)
CREATE TABLE providers (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    type        TEXT NOT NULL,  -- openai_compat | openai | anthropic | gemini
    base_url    TEXT NOT NULL,
    api_key_ref TEXT,           -- Reference to encrypted secret
    enabled     INTEGER NOT NULL DEFAULT 1,
    sort_order  INTEGER NOT NULL DEFAULT 0,
    visible     INTEGER NOT NULL DEFAULT 0
);

-- App settings as key/value pairs
CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL          -- JSON-encoded
);

-- MCP server configurations
CREATE TABLE mcp_servers (
    id        TEXT PRIMARY KEY,
    name      TEXT NOT NULL,
    transport TEXT NOT NULL DEFAULT 'stdio',
    command   TEXT,
    args      TEXT,
    url       TEXT,
    env       TEXT,              -- JSON-encoded environment variables
    enabled   INTEGER NOT NULL DEFAULT 1
);

-- Custom model names per provider/model pair
CREATE TABLE model_overrides (
    provider_id  TEXT NOT NULL,
    model_id     TEXT NOT NULL,
    custom_name  TEXT,
    enabled      INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY (provider_id, model_id)
);
```

### Migrations (`db/mod.rs::MIGRATIONS`)
| Version | Description |
|---------|-------------|
| 1 | Core tables + FTS5 triggers (AI/AD) |
| 2 | Additional FTS5 triggers (DE/AU) — **BUG: uses `old.rowid` instead of `new.rowid`** |
| 3 | Rename OpenAI type to `openai_compat` |
| 4 | Add `agent_mode` and `working_directory` columns |
| 5 | Add `env` column to mcp_servers |

### Database Modules
Each module encapsulates its own SQL queries:
- `conversations.rs`: CRUD + triggers for `updated_at`
- `messages.rs`: CRUD + FTS5 search (`search()` uses `fts5 MATCH` query)
- `mcp_servers.rs`: Save/load all servers with JSON serialization for args/env
- `model_overrides.rs`: Batch upserts for efficiency
- `providers.rs`: Find by ID, list, upsert, delete
- `settings.rs`: Get all as map, set single key/value

---

## MCP — Model Context Protocol Implementation

### McpManager Lifecycle (`mcp/mod.rs`)
```rust
pub struct McpManager {
    servers: Vec<McpServer>,           // Configured servers from DB
    stdio_clients: HashMap<String, Arc<StdioClient>>,  // Active connections
    cached_tools: Vec<McpTool>,       // Tools fetched after initialize()
}
```
**Initialization (`load_servers()`):**
1. Clone configured servers (filter enabled + stdio transport)
2. For each server:
   - Spawn process via `StdioClient::spawn(cmd, args, env)`
   - Call `/initialize` → log capabilities
   - Call `/tools/list` → attach `server_id` and `server_name` to each tool
3. Store Arc clients in HashMap for later tool invocation

### Stdio Transport (`mcp/stdio.rs::StdioClient`)
Implements MCP stdio transport per spec:
- **Process spawning**: Uses `std::process::Command` with proper argument quoting
- **I/O handling**: Reads/writes to stdin/stdout with buffering and line termination normalization
- **Protocol methods:**
  - `/initialize` → returns capabilities (tools, prompts, resources)
  - `/tools/list` → returns tool definitions
  - `/call_tool` → executes tool via `execute_tool()` for built-ins or direct MCP call for external servers
- **Error handling**: Graceful failures on initialization, tool list fetch, and individual tool calls

### McpTool Structure
```rust
pub struct McpTool {
    pub server_id: String,
    pub server_name: String,
    pub name: String,           // e.g., "read_file"
    pub description: String,
    pub input_schema: Value,
}
```
- `server_id` and `server_name` are attached during initialization for frontend display
- Frontend uses `${server_id}:${name}` as the unique tool key (e.g., `mcp_server_1:read_file`)

---

## Streaming & Provider Abstraction — `providers/`

### Unified Interface (`mod.rs`)
```rust
pub async fn list_models(client: &Client, type_, base_url, api_key) -> Vec<String>
pub async fn stream_chat(
    client, app, type_, base_url, api_key, model,
    messages: Vec<ChatMessage>, system_prompt, tools, reasoning_effort, cancel
) -> StreamOutput
pub async fn complete_chat(client, type_, base_url, api_key, model, messages, system_prompt) -> String
```
**Design Decision**: All non-Anthropic/non-Gemini providers share the same OpenAI-compatible code path in `openai_compat.rs`. This reduces duplication and makes adding new providers (e.g., Mistral, Cohere) a one-line change.

### Provider-Specific Implementations
| File | API Style | Notes |
|------|-----------|-------|
| `openai_compat.rs` | OpenAI Chat Completions v1 | Handles streaming with cancellation token; used by LM Studio, Ollama, Groq |
| `anthropic.rs` | Anthropic Messages API | Native implementation; supports tool use and reasoning (thinking) blocks natively |
| `gemini.rs` | Google Generative AI API | Uses SSE parser from `streaming.rs`; handles thinking blocks natively |

### Streaming Output Structure
```rust
pub struct StreamOutput {
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
    pub thinking: Option<String>,  // Raw thinking block (Anthropic/Gemini)
}
```
- `content`: Plain text response (no JSON wrapping)
- `tool_calls`: Array of `{ id, name, arguments, thought_signature? }` — used to trigger tool execution
- `thinking`: Raw thinking block from Anthropic or Gemini; stored in message's `thinking` field

---

## Secrets — Encrypted Key-Value Store (`secrets.rs`)
```rust
pub struct Secrets {
    // Uses Tauri's plugin-store with AES-256 encryption
}
```
**Usage:**
```rust
let api_key = api_key_ref.as_deref()
    .and_then(|r| state.secrets.get(r).ok().flatten());
```
- Keys are stored under `prefs.json` but encrypted at rest
- Provides controlled access via `get(key)` and `set(key, value)` methods
- Used for API keys in providers table (`api_key_ref` is a reference, not the key itself)

---

## Event Emission — Tauri Emitter Pattern
```rust
app.emit("event_name", serde_json::json!({ ... }))
```
**Common events:**
- `stream_status`: `{ label: string|null }` — UI indicator ("Processing prompt")
- `user_message`: Message object — renders user input immediately
- `tool_call`: `{ name, id, args }` — triggers permission request or execution
- `tool_call_result`: `{ id, name, result }` — displays tool output
- `stream_done`: Final message (with `__tool_calls__` sentinel if applicable)
- `conversation_title_updated`: `{ id, title }` — syncs auto-generated title

---

## Known Bugs & Technical Debt

| Bug | Severity | Fix Effort |
|-----|----------|------------|
| FTS DELETE trigger uses `old.rowid` instead of `new.rowid` | High | 5 min (see `db/mod.rs` migration 2) |
| Context window limit not enforced in `build_api_messages()` | Medium | 30 min (requires trimming oldest messages while preserving last user+assistant pair) |
| No sandboxing for built-in tools or MCP servers | Low/Medium | TBA (recommendation: add capability-based restrictions) |

---

*Last updated: 2026-06-12*
