# Demido Studio — Architecture & Design

> Deep dive into architectural decisions, design patterns, and system-level understanding.

---

## Core Architectural Decisions

### 1. **Tauri 2 + React 19**
- **Why Tauri?** Desktop app with minimal bundle size (Rust backend), native file system access for MCP servers, secure secret storage via plugin-store.
- **Why React 19?** Latest stable features; ErrorBoundary in `main.tsx` catches runtime errors and displays them gracefully instead of crashing the app.

### 2. **Zustand over Redux/Context**
- Zustand stores are module-scoped, survive component remounts, and persist to localStorage where appropriate (message blocks).
- Each store encapsulates its own logic; no global reducer needed.

### 3. **SQLite with FTS5**
- WAL mode prevents corruption during concurrent writes.
- Foreign keys enforced (`PRAGMA foreign_keys=ON`).
- FTS5 virtual table indexes message content for fast search via `search_conversations` command.

### 4. **Unified Provider Interface**
- All non-Anthropic/non-Gemini providers share the same OpenAI-compatible code path in `providers/openai_compat.rs`.
- Reduces duplication; new provider just needs to implement the unified interface.

### 5. **MCP stdio-first design**
- MCP servers run as child processes via stdio transport (no network overhead).
- Tools are cached after initialization — no per-call API overhead.
- Server-level override feature: toggling a server disables ALL its tools at once, but individual tool states are preserved in a snapshot.

### 6. **Agent Mode Permission Gating**
- Three modes (`cautious`, `balanced`, `autonomous`) provide graduated trust levels.
- Balanced mode uses path-sensitive heuristics to avoid false positives while protecting sensitive files.

---

## Design Patterns Used

### Event Sourcing (Partial)
- User messages are persisted immediately upon receipt, then streamed back via events (`user_message`).
- Assistant responses and tool results are also emitted as events before final DB insertion.
- This allows the UI to render optimistically while maintaining a source of truth in SQLite.

### Command Query Responsibility Segregation (CQRS)
- **Commands**: `send_message`, `continue_generation` — mutate state, trigger side effects (streaming, tool execution).
- **Queries**: `list_conversations`, `list_messages`, `search_conversations` — read-only operations against SQLite.

### Repository Pattern
- Each DB module (`conversations.rs`, `messages.rs`, etc.) encapsulates its own SQL queries and triggers.
- Centralized in `db/mod.rs` with migration tracking.

### Builder/Factory Pattern
- `build_api_messages()` reconstructs API-compatible message arrays from raw DB rows, handling tool call/result pairing.
- Provider-specific implementations (`stream_chat`, `complete_chat`) use this builder to prepare requests.

---

## Data Flow Diagrams

### Message Lifecycle
```
User types → InputBar (frontend) → send_message command → 
  ├─ Persist user message (DB)
  ├─ Emit user_message event → UI renders
  └─ run_generation_loop:
       ├─ Build API messages from DB
       ├─ stream_chat() → emits thinking/token events
       ├─ Tool calls detected → execute in spawn_blocking
       │    ├─ Permission check (balanced mode)
       │    ├─ Execute tool → emit tool_call_result
       │    └─ Insert tool message to DB
  └─ stream_done event → UI completes rendering
```

### MCP Server Lifecycle
```
Startup → save_mcp_servers command → 
  McpManager::load_servers() → 
    For each enabled stdio server:
      ├─ Spawn process (Command)
      ├─ Call /initialize → cache capabilities
      └─ Call /tools/list → cache tools with server_id attached
```

### Permission Flow (Balanced Mode)
```
Model emits tool call → is_permitted() checks: 
  ├─ write_file/edit_file/run_command → ALWAYS ASK
  ├─ read_file + sensitive path pattern → ASK
  └─ list_dir/search_files → ALLOW
→ If ASK: emit tool_permission_request → 
    Frontend shows bubble → user clicks Allow/Deny → 
    respond_to_permission channel → execute or deny
```

---

## Security Considerations

### Secret Storage
- API keys are stored encrypted via Tauri's `plugin-store` (AES-256).
- Keys are referenced by `api_key_ref` in providers table, not stored plaintext.
- `get_secret()` and `set_secret()` commands provide controlled access.

### Path Resolution for Built-in Tools
- All file operations resolve paths relative to the conversation's `working_directory` setting (or current directory if null).
- Prevents accidental absolute path escapes via user input.

### Tool Execution Sandboxing
- **Current limitation**: Built-in tools run with full process privileges. No sandboxing implemented yet.
- MCP servers also run as child processes — users must trust the server's code.
- Recommendation: Future work should add capability-based restrictions (e.g., only allow read operations in certain modes).

---

## Performance Characteristics

### Streaming Latency
- SSE parser (`streaming.rs`) buffers incoming bytes until complete lines are received.
- Thinking blocks are sealed when token stream begins — prevents interleaving issues.
- Tool execution runs in `spawn_blocking` to avoid blocking the async runtime.

### Database I/O
- WAL mode allows concurrent reads/writes without locking.
- FTS5 triggers run on INSERT/UPDATE/DELETE — minimal overhead for typical chat volumes.
- No connection pooling needed (single-threaded Rust backend).

### Memory Footprint
- MCP tools are cached in memory after initialization (~10-50 KB per server depending on tool count).
- Message blocks persist only to localStorage, not DB — ephemeral UI state.

---

## Window System

Demido Studio is designed as a **power-user AI workbench**, not just a chat interface. The window system is the foundation that enables multiple panels to coexist on screen — settings, tool inspectors, MCP consoles, and more — each independently movable, resizable, and dockable alongside the chat.

### Design Philosophy

The chat is always the primary surface. Unlike a traditional windowing system where any window can cover any other, snapping in Demido Studio is a cooperative layout operation: snapping a panel to an edge splits the screen, and the chat reflows to fill the remaining space. The chat can never be hidden by a snap action (only by the user manually dragging a free window over it). This keeps the AI conversation always accessible.

### Two-Layer Architecture

The app has two layers that coexist at all times:

**Base layer** — `Sidebar + ChatView` in a flex row. This always fills the full viewport, minus the width of any snapped panels. The layout is driven by transparent spacer columns whose widths come from the window store's `snapLayout`. When nothing is snapped, the base layer fills 100%. When a panel snaps left at 50%, the left spacer takes 50% and the chat takes the remaining 50%.

**Window layer** — A `position: fixed; inset: 0` overlay rendered by `WindowManager`. It uses `pointer-events: none` so clicks pass through to the chat by default. Individual `WindowFrame` components override this with `pointer-events: auto`. All open windows (snapped and floating) render in this layer; snapped windows are just positioned to coincide with their spacer column in the base layer.

### Snap System

Snap detection runs continuously during drag. `WindowFrame` checks the dragged window's edges against:
1. The left and right viewport edges
2. The inner edges of any already-snapped windows (enabling side-by-side panel stacks)

When the drag position is within 40px of a snap boundary, `WindowManager` shows a `SnapPreview` ghost at the target position. On release, the snap is committed if the resulting chat width would be ≥ 400px, otherwise the snap is rejected and the window drops at the cursor position.

Snapped windows can be freed by dragging their title bar — `unsnapWindow` is called on the first drag tick, restoring the window's last known free position and size at the current cursor location (no teleport).

### Store-Driven, Not DOM-Driven

All window state lives in the Zustand `windowManager` store. Components are pure renderers of store state — they never hold position or size in local component state (except for the transient `snapCandidate` during drag). This makes window state inspectable, testable, and easy to extend with persistence in a future phase.

### Future Phases

The current implementation covers Phases 1 and 2. Planned future work:
- **Layout persistence** — save/restore window positions and snap state across sessions
- **Top/bottom snapping** — currently deferred due to layout complexity
- **Snap fraction resizing** — drag the boundary between a snapped panel and the chat to resize
- **More panel types** — MCP Console, Prompt Editor, Tool Inspector, Agent Timeline
- **Taskbar** — list of open/minimized windows for quick access

See `docs/superpowers/specs/2026-06-13-window-system-design.md` for the full design spec.

---

## Extensibility Points

### Adding a New LLM Provider
1. Implement `list_models()`, `stream_chat()`, and optionally `complete_chat()` in `providers/` following the unified interface.
2. Handle provider-specific message formats (e.g., Anthropic's thinking blocks, Gemini's SSE).
3. Register in `commands.rs::get_model_reasoning()` if model has special capabilities.

### Adding a New Built-in Tool
1. Define `ToolDef` in `agent/mod.rs::builtin_tool_defs()`
2. Implement execution logic in `agent/executor.rs::execute_tool()`
3. Add to `is_builtin()` check in `commands.rs`
4. Update `format_tool_description()` for permission requests

### Adding MCP SSE Transport
1. Extend `mcp/types.rs` with SSE-specific fields (endpoint URL, headers)
2. Implement `sse.rs` similar to `stdio.rs` but using HTTP client instead of process spawn
3. Add validation in `commands.rs::test_mcp_server()` for SSE transport

---

## Testing Strategy Recommendations

### Unit Tests (Rust)
- Test each built-in tool with edge cases (empty files, missing paths, large outputs)
- Test permission gating logic with various path patterns
- Test FTS5 triggers via integration tests against a fresh DB

### Integration Tests
- End-to-end message flow: send → stream → tool call → result → completion
- MCP server lifecycle: spawn → initialize → list tools → call tool → cleanup
- Permission flow: balanced mode with sensitive vs non-sensitive paths

### Frontend Tests (Vitest/Jest)
- Zustand stores: verify persistence, event listeners, state updates
- Components: render tests for message bubbles with/without tool calls
- E2E: Playwright tests for full chat interaction including file attachments

---

## Known Bugs & Technical Debt

| Bug | Severity | Fix Effort |
|-----|----------|------------|
| FTS DELETE trigger uses `old.rowid` instead of `new.rowid` | High | 5 min |
| Context window limit not enforced in `build_api_messages()` | Medium | 30 min (requires trimming oldest messages while preserving last user+assistant pair) |
| Interface settings component has no backend support | Low | TBA |

---

*Last updated: 2026-06-12*
