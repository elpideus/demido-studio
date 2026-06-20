# Demido Studio — Frontend Deep Dive

> Complete reference for frontend developers and LLMs working with the React/TypeScript side.

---

## Application Entry & Layout

### `main.tsx` — Error Boundary Wrapper
```tsx
<ErrorBoundary>
  <React.StrictMode>
    <App />
  </React.StrictMode>
</ErrorBoundary>
```
- **Purpose**: Catches any React runtime errors and displays them instead of crashing the app.
- **Devtools shortcut**: Ctrl+Shift+I or F12 opens browser devtools (via Tauri command).

### `App.tsx` — Main Layout

After the window system is implemented, layout becomes a three-column split driven by `useWindowManager`:

```tsx
<div className="flex h-screen bg-[#0d0d0f] text-[#f0f0f5] overflow-hidden">
  {/* Left-snapped panel column (0 width when nothing snapped) */}
  <div style={{ flex: `0 0 ${leftSnappedWidth}px` }}>...</div>

  {/* Base layer: always visible, fills remaining space */}
  <div style={{ flex: 1, minWidth: MIN_CHAT_WIDTH }}>
    <Sidebar onOpenSettings={() => openWindow('settings', 'settings', 'Settings')} />
    <ChatView />
  </div>

  {/* Right-snapped panel column */}
  <div style={{ flex: `0 0 ${rightSnappedWidth}px` }}>...</div>

  {/* Floating window layer (absolutely positioned, on top) */}
  <WindowManager />
</div>
```

- **Auth Gate**: If `settings.auth_enabled` and not unlocked, renders `<AuthGate />` instead.
- **Settings** no longer uses `showSettings` boolean — it's opened via `windowManager.openWindow('settings', ...)`.
- **Keyboard shortcuts**:
  - Ctrl+Shift+I or F12 → open devtools
  - No other global shortcuts implemented yet

---

## Zustand Stores — Complete Reference

### `useConversations` (`stores/conversations.ts`)
```typescript
interface ConversationsStore {
  conversations: Conversation[]      // All conversations, sorted by updated_at desc
  activeId: string | null            // Currently selected conversation ID
  
  load()                             // Fetch from DB via db.listConversations()
  create(providerId, modelId)        // Create new conversation, set as active
  remove(id)                         // Delete and update UI state
  setActive(id)                      // Change active conversation (no DB write)
  updateTitle(id, title)             // Update title in DB + local state
  setAgentMode(id, mode)             // Set agent_mode enum: 'off' | 'cautious' | 'balanced' | 'autonomous'
  setWorkingDirectory(id, path)     // Set working_directory string or null
  listenForTitleUpdates()            // Returns cleanup function for event listener
}
```
**Key Implementation Details:**
- `listenForTitleUpdates()` sets up a Tauri event listener for `conversation_title_updated` events.
- On cleanup, the unlisten function is called to prevent memory leaks.
- Title updates are triggered by auto-generation in `commands.rs::maybe_generate_title()`.  

### `useProviders` (`stores/providers.ts`)
```typescript
interface ProvidersStore {
  providers: Provider[]              // All providers, sorted by sort_order
  models: Record<string, string[]>   // provider_id → [model_ids]
  modelOverrides: Record<string, ModelOverride[]>  // provider_id → overrides
  selectedProviderId: string         // Currently selected in UI
  selectedModelId: string            // Currently selected model
  
  load()                             // Fetch providers + models + overrides for all enabled providers
  setSelected(providerId, modelId)  // Update selection (no DB write)
  fetchModels(providerId)            // Call provider API to list available models
  upsert(provider)                   // Save new/updated provider
  addProvider(template)              // Create from template with auto-generated ID and key_ref
  deleteProvider(id)                 // Delete + cascade delete associated secret
  loadModelOverrides(providerId)     // Fetch overrides for specific provider
  upsertModelOverride(override)      // Save single override
  batchUpsertModelOverrides(overrides)  // Save multiple at once
}
```
**Key Implementation Details:**
- `load()` fetches models and overrides in parallel via `Promise.all()`.
- Model fetching uses Tauri's `invoke<string[]>('list_models', { providerId })`.
- On provider deletion, the associated secret is deleted via `state.secrets.delete(key_ref)`.

### `useMcpTools` (`stores/mcpTools.ts`)
```typescript
interface McpToolEntry {
  server_id: string
  server_name: string
  name: string                       // Tool name (e.g., "read_file")
  description: string
  enabled: boolean                   // Individual tool enable/disable state
}

interface ServerOverride {
  snapshot: Record<string, boolean>  // Current states of all tools in this server
}

interface McpToolsStore {
  tools: McpToolEntry[]              // All tools from all servers
  collapsed: Record<string, boolean> // server_id → UI collapse state
  serverOverrides: Record<string, ServerOverride>
  
  load()                             // Fetch from Tauri + restore persisted states
  toggleTool(toolKey)                // Toggle individual tool (updates snapshot if overridden)
  toggleServer(serverId)             // Override all tools in server ON/OFF
  toggleCollapse(serverId)           // UI-only: collapse/expand server group
  enabledTools()                     // Filter to only enabled tools
}
```
**Key Implementation Details:**
- Tool key format: `${server_id}:${name}` (e.g., `mcp_server_1:read_file`).
- Individual tool states are persisted in Tauri store under `mcp_tool_enabled`.
- Server overrides are stored separately under `mcp_server_overrides` with a snapshot of current states.
- When toggling an overridden tool, the snapshot is updated to reflect the new state.

### `useMessages` (`stores/messages.ts`)
```typescript
interface StreamBlock {
  type: 'thinking' | 'tool'
  content?: string                  // For thinking blocks
  id?: string                       // For tool blocks (tool_call_id)
  name?: string                     // Tool name
  args?: unknown                    // Tool arguments
  result?: string                   // Tool execution result
  done: boolean                     // Whether this block is complete
}

interface MessagesStore {
  messages: Message[]                // All messages for active conversation
  streaming: boolean                 // Currently in streaming state
  streamBuffer: string               // Accumulated token chunks
  streamBlocks: StreamBlock[]        // Thinking and tool blocks during streaming
  messageBlocks: Record<string, StreamBlock[]>  // Per-message block history (persisted)
  statusLabel: string | null         // Current streaming label (e.g., "Processing prompt")
  pendingPermission: PermissionRequest | null  // Currently waiting for user approval
  resolvedPermissions: ResolvedPermission[]   // History of permission decisions
  
  load(conversationId)               // Fetch messages from DB
  addMessage(msg)                    // Append new message (no DB write)
  truncateAfter(messageId)           // Delete all messages after this one
  truncateFrom(messageId)            // Delete this and all following messages
  updateMessage(messageId, content)  // Update a specific message's content
  respondToPermission(approved)      // Handle user permission decision
  startListening()                   // Set up Tauri event listeners for streaming
}
```
**Key Implementation Details:**
- `startListening()` sets up **8 separate Tauri event listeners**:
  - `user_message` → adds to messages array
  - `stream_thinking` → appends to last thinking block or creates new one
  - `stream_thinking_end` → marks last thinking block as done
  - `stream_token` → appends to streamBuffer; seals any open thinking block when content arrives
  - `stream_done` → saves blocks to localStorage, adds final message, resets streaming state
  - `tool_call` → creates tool block (seals any open thinking first)
  - `tool_call_result` → updates tool block with result and marks done
  - `stream_status` → sets statusLabel for UI indicator
  - `stream_cancelled` → cleanup and reset all streaming state
- Message blocks are persisted per-message in localStorage under key `demido:messageBlocks`.
- On stream start, any open thinking block is sealed (marked as done) before content arrives.

### `useSettings` (`stores/settings.ts`)
```typescript
interface SettingsStore {
  settings: AppSettings             // Current settings object
  loaded: boolean                   // Whether settings have been fetched
  
  load()                             // Fetch from DB via db.getSettings()
  update<K extends keyof AppSettings>(key, value)  // Save single setting + update local state
}
```
**Key Implementation Details:**
- Defaults are defined inline and used if DB fetch fails.
- `update()` JSON-decodes the string value before saving (Tauri sends values as JSON strings).
- Settings are stored in SQLite under table `settings` with key/value pairs.

---

## Components — Complete Reference

### Auth & Layout

#### `components/auth/AuthGate.tsx`
```tsx
interface Props { onUnlock: () => void }
```
- Reads PIN from Tauri secret store (`auth_pin`).
- On correct PIN → calls `onUnlock()` to set unlocked state in parent.
- Error message shown if PIN is incorrect.

#### `components/sidebar/Sidebar.tsx`
```tsx
interface Props { onOpenSettings: () => void }
```
**Components:**
- **SearchBar**: Filters conversations by title/content (uses FTS5 via Tauri).
- **ConversationList**: Renders list of `<ConversationItem />`.
- **New Chat button**: Creates new conversation with default provider/model.

#### `components/sidebar/ConversationItem.tsx`
```tsx
interface Props {
  conversation: Conversation
  isActive: boolean
  onSelect: (id: string) => void
}
```
- Shows truncated title, agent mode indicator, working directory if set.
- Click handler calls `onSelect(id)` which updates `useConversations::setActive()` and loads messages via `startListening()`.  

### Chat UI — Core Components

#### `components/chat/ChatView.tsx`
```tsx
interface Props {
  activeConversationId: string | null
}
```
**Components:**
- **ChatHeader**: Conversation metadata and controls.
- **MessageList**: Scrollable message container with custom scroll behavior (Radix UI ScrollArea).
- **InputBar**: User input area with attachments, tool toggles, reasoning selector.

#### `components/chat/ChatHeader.tsx`
```tsx
interface Props {
  conversation: Conversation | null
  provider: Provider | null
  modelId: string
  agentMode: string
  workingDirectory: string | null
  onModelChange: (modelId: string) => void
  onAgentModeChange: (mode: string) => void
  onWorkingDirChange: (path: string | null) => void
}
```
**Components:**
- **Conversation title**: Auto-updates via `useConversations::updateTitle()`.
- **Model Selector**: Dropdown populated from `useProviders::models[providerId]`.
- **Agent Mode dropdown**: Options are `off`, `cautious`, `balanced`, `autonomous`.
- **Working Directory picker**: File dialog to select directory; clears on cancel.
- **Permission Bubble**: Shows when `pendingPermission` is set in `useMessages`.  

#### `components/chat/InputBar.tsx`
```tsx
interface Props {
  disabledTools: string[]            // Tools disabled for this turn
  reasoningEffort: string | null     // Current reasoning effort selection
}
```
**Components:**
- **Textarea**: Auto-expanding, multiline input.
- **File Attachment button**: Opens file picker; reads content as base64 or raw text.
- **Disabled Tools toggle**: Checkbox list for built-in tools (only shown when agent mode ≠ `off`).
- **Reasoning Effort selector**: Dropdown with options from `useMessages::reasoning.getModelReasoning()`.

#### `components/chat/MessageBubble.tsx`
```tsx
interface Props {
  message: Message
  blocks?: StreamBlock[]             // Thinking/tool blocks for this message
}
```
**Rendering Logic:**
- **User messages**: Simple text content.
- **Assistant messages with tools**: Renders tool calls as interactive cards showing name, args (truncated), and result if available.
- **Thinking blocks**: Rendered as collapsible sections with streaming indicators.
- **Streaming indicator**: Shows when `streaming` is true in parent context.

#### `components/chat/MessageList.tsx`
```tsx
interface Props {
  messages: Message[]
  onTruncateAfter(messageId: string) => void
  onTruncateFrom(messageId: string) => void
}
```
- Uses Radix UI ScrollArea with custom scrollbars.
- **Truncate After**: Deletes all messages after the selected one (keeps context).
- **Truncate From**: Deletes the selected message and everything after it (for long conversations).

#### `components/chat/ModelSelector.tsx`
```tsx
interface Props {
  providerId: string
  modelId: string
  onChange: (modelId: string) => void
}
```
- Populated from `useProviders::models[providerId]`.
- Updates via `useProviders::setSelected(providerId, newModelId)`.

#### `components/chat/ReasoningSelector.tsx`
```tsx
interface Props {
  providerId: string
  modelId: string
  reasoningInfo?: ReasoningInfo      // From get_model_reasoning()
  onChange: (effort: string) => void
}
```
- Only shown if `reasoningInfo` is provided and not empty.
- Options come from `reasoningInfo.allowedOptions`, default selected.

#### `components/chat/TimelineStrip.tsx`
```tsx
interface Props {
  messages: Message[]
  blocks: Record<string, StreamBlock[]>  // Per-message block history
}
```
- Renders a horizontal timeline below assistant messages showing tool calls and results.
- Each tool call shows name, args (truncated), and result if available.
- Clicking a tool entry opens `ToolSelectorPopup` to toggle it for future turns.

#### `components/chat/ToolSelector.tsx`
```tsx
interface Props {
  disabledTools: string[]
  onToggle: (toolKey: string) => void
}
```
- Renders checkboxes for all enabled tools (built-in + MCP).
- Tool key format: `${server_id}:${name}`.
- Updates via `useMcpTools::toggleTool(toolKey)`.

#### `components/chat/ToolSelectorPopup.tsx`
```tsx
interface Props {
  disabledTools: string[]
  onToggle: (toolKey: string) => void
}
```
- Radix UI DropdownMenu for quick tool toggling from timeline strip.
- Same functionality as ToolSelector but in popup form.

### Window System

Demido Studio uses a floating window system that lets panels like Settings coexist on screen simultaneously with the chat. Every panel opens as a managed window: a draggable, resizable frame that floats above the chat or snaps to the left or right edge to create a split-screen layout.

**How it works day-to-day:** A contributor adding a new panel (e.g., MCP Console, Prompt Editor) creates a content component, registers it in `WindowManager.tsx::renderContent()`, and calls `openWindow(id, component, title)` from wherever the panel is triggered. The window system handles all the positioning, z-order, drag, resize, and snapping — the panel content just fills the frame.

**The two-layer model:** The base layer (`Sidebar + ChatView`) is always visible and fills whatever horizontal space remains after snapped panels are accounted for. The window layer (`WindowManager`) is a fixed overlay rendered on top via `position: fixed; pointer-events: none` — individual window frames use `pointer-events: auto` so only actual windows intercept mouse events, not the invisible overlay.

**Snap layout:** When a panel snaps to the left edge, the app layout inserts a transparent spacer column on the left that is exactly as wide as the snapped panel. The chat column has `flex: 1` and shrinks accordingly. The snapped panel in the window layer is positioned to cover that same spacer area. The chat is never visually covered — the spacer pushes it out of the way. The minimum chat width is 400px; snaps that would violate this are rejected by the store.

**Z-order:** Each `openWindow` or `focusWindow` call assigns the current `nextZIndex` (starting at 100, incrementing by 1) to the window. Clicking a window calls `focusWindow`, bringing it to the front. There is no explicit stacking list — CSS `z-index` handles it entirely.

#### `stores/windowManager.ts`

Central Zustand store. Owns all runtime window state: the window registry (`windows: Record<string, ManagedWindow>`), the snap layout (`snapLayout: { left, right }`), and the next z-index counter.

Key behaviors:
- `openWindow` is a singleton: calling it with an id that already exists focuses the existing window instead of opening a duplicate.
- `snapWindow(id, edge, appWidth)` validates that the snap leaves the chat at least 400px wide before committing. Returns `false` if rejected.
- `unsnapWindow` restores `lastFreePosition` and `lastFreeSize`, which are saved on every free `moveWindow` / `resizeWindow` call.
- Closing a window automatically clears its snap slot if it was snapped.

```typescript
interface ManagedWindow {
  id: string
  title: string
  component: WindowComponent              // Key string, e.g. 'settings'
  position: { x: number; y: number }
  size: { width: number; height: number }
  zIndex: number
  snapState: { edge: 'left' | 'right'; fraction: number } | null
  lastFreePosition: { x: number; y: number }
  lastFreeSize: { width: number; height: number }
}
```

#### `components/windows/WindowFrame.tsx`

The window chrome. Wraps `react-rnd` for drag and resize, with `dragHandleClassName="wm-drag-handle"` so only the title bar initiates a drag. During drag, it calls `detectSnapEdge()` on every move event to check proximity to viewport edges and the inner boundaries of already-snapped windows (snap-to-snapped). When a candidate edge is found, it signals `WindowManager` via `onSnapCandidateChange` to show the ghost preview. On drag release, the snap is committed or the window is placed at the dropped position.

When a window is snapped, its `react-rnd` `position` and `size` are overridden to fill the snap slot (e.g., `x=0, y=0, width=50%, height=100vh` for a left snap). Resize handles are disabled while snapped. Dragging a snapped window's title bar lazily calls `unsnapWindow` on the first drag event, passing the current cursor-relative position so the window follows the cursor without teleporting to `lastFreePosition`.

#### `components/windows/WindowManager.tsx`

Renders all open windows. Maintains `snapCandidate` state (transient — not in the store) to pass the current snap edge to `SnapPreview`. Maps `WindowComponent` keys to their content components in `renderContent()`. This is the only place new panel types are registered.

#### `components/windows/SnapPreview.tsx`

A semi-transparent blue ghost that renders at the snap target position while the user drags near an edge. It gives immediate visual feedback before the user releases the mouse.

**Adding a new panel type:**
1. Create `src/components/<name>/<Name>Content.tsx` — pure content, no positioning wrapper.
2. Add `'<name>'` to the `WindowComponent` union in `src/types.ts`.
3. Add a `case` for it in `WindowManager.tsx::renderContent()`.
4. Call `openWindow('<name>', '<name>', 'Panel Title')` from the trigger location.

---

### Settings — Complete Reference

#### `components/settings/SettingsPanel.tsx`
```tsx
interface Props { onClose: () => void }
```
**Tabs:**
1. **Providers**: Add/edit/delete providers, view model overrides.
2. **Interface**: UI preferences (currently unimplemented).
3. **MCP Servers**: Configure MCP servers with test connection button.
4. **System Prompt**: Edit global system prompt injected into every assistant message.

#### `components/settings/ProvidersSettings.tsx`
```tsx
interface Props {
  providers: Provider[]
  modelOverrides: Record<string, ModelOverride[]>
  onAddProvider: (template: ProviderTemplate) => void
  onUpdateProvider: (provider: Provider) => void
  onDeleteProvider: (id: string) => void
  onUpdateModelOverride: (override: ModelOverride) => void
}
```
**Components:**
- **Provider Card**: Shows name, type, base URL, enabled state.
- **Add Provider form**: Template options include LM Studio, Ollama, OpenAI, Anthropic, Groq, and Custom.
- **Model Overrides table**: Lists custom names for specific models; add/edit/delete rows.

#### `components/settings/McpSettings.tsx`
```tsx
interface Props {
  servers: McpServer[]
  onAddServer: (server: McpServer) => void
  onUpdateServer: (server: McpServer) => void
  onDeleteServer: (id: string) => void
  onTestConnection: (server: McpServer) => Promise<void>
}
```
**Components:**
- **Server list**: Shows name, transport type, enabled state.
- **Add Server form**: Fields for name, transport (stdio/SSE), command/args/env for stdio, URL for SSE.
- **Test Connection button**: Calls `mcp.test_server(server)` via Tauri; shows result in toast or inline message.

#### `components/settings/SystemPromptSettings.tsx`
```tsx
interface Props {
  systemPrompt: string
  onChange: (prompt: string) => void
}
```
- Textarea with auto-expanding behavior.
- Updates via `useSettings::update('system_prompt', prompt)`.

#### `components/settings/InterfaceSettings.tsx`
```tsx
interface Props {
  settings: AppSettings
  onChange: <K extends keyof AppSettings>(key: K, value: AppSettings[K]) => void
}
```
- Currently empty placeholder component — no backend support implemented yet.

---

## Utilities & Types

### `lib/tauri.ts` — Tauri Command Bindings
```typescript
// Database operations
db.listConversations() → Conversation[]
db.createConversation(providerId, modelId) → Conversation
db.deleteConversation(id)
db.updateConversationTitle(id, title)
db.listMessages(conversationId) → Message[]
db.deleteMessagesAfter(messageId)
db.deleteMessagesFrom(messageId)
db.updateMessageContent(messageId, content)
ddb.listProviders() → Provider[]
db.upsertProvider(provider)
db.deleteProvider(id)
db.listModelOverrides(providerId) → ModelOverride[]
db.upsertModelOverride(overrideEntry)
db.batchUpsertModelOverrides(overrides)
db.getSettings() → AppSettings
db.setSetting(key, value)  // value is JSON-encoded string
db.getSecret(key) → string | null
db.setSecret(key, value)
db.searchConversations(query) → { conversation_id, snippet }[]
db.setAgentMode(conversationId, mode)
db.setWorkingDirectory(conversationId, path)

// Chat operations
chat.sendMessage(conversationId, content, disabledTools?, reasoningEffort?, providerId?, modelId?, attachments?)
chat.cancelStream()
chat.continueGeneration(conversationId, disabledTools?, reasoningEffort?, providerId?, modelId?)

// Reasoning capabilities
reasoning.getModelReasoning(providerId, modelId) → { allowedOptions: string[], default: string } | null

// Agent operations
agent.respondToPermission(approved)

// Export
exportChat.exportConversation(conversationId, filePath)

// MCP
mcp.listServers() → McpServer[]
mcp.saveServers(servers)
mcp.listTools() → { server_id, server_name, name, description }[]
mcp.testServer(server) → number  // tool count
```

### `types.ts` — Core Data Structures
```typescript
interface Conversation {
  id: string
  title: string
  provider_id: string
  model_id: string
  created_at: number           // Unix timestamp in ms
  updated_at: number
  agent_mode: 'off' | 'cautious' | 'balanced' | 'autonomous'
  working_directory: string | null
}

interface Message {
  id: string
  conversation_id: string
  role: 'user' | 'assistant' | 'system' | 'tool'
  content: string              // For assistant messages with tools, this is JSON with __tool_calls__ key
  tool_call_id?: string        // Links to tool result message
  created_at: number
  token_count?: number
  thinking?: string            // Raw thinking block from Anthropic/Gemini
}

interface Provider {
  id: string
  name: string
  type: 'openai_compat' | 'openai' | 'anthropic' | 'gemini'
  base_url: string
  api_key_ref?: string         // Reference to encrypted secret, not the key itself
  enabled: boolean
  sort_order: number
  visible: boolean             // UI visibility filter
}

interface ModelOverride {
  provider_id: string
  model_id: string
  custom_name?: string
  enabled: boolean
}

interface McpServer {
  id: string
  name: string
  transport: 'stdio' | 'sse'
  command?: string
  args?: string[]
  env?: Record<string, string>
  url?: string
  enabled: boolean
}

interface AppSettings {
  default_provider_id: string
  default_model_id: string
  system_prompt: string
  auth_enabled: boolean
  context_window_limit: number
  task_provider_id: string     // Provider for auto-title generation
  task_model_id: string        // Model for auto-title generation
  title_every_n_messages: number
}

interface FileAttachment {
  name: string
  content: string              // Base64 or raw text depending on file type
}
```

---

## Component Interaction Patterns

### Creating a New Conversation
1. User clicks "New Chat" in Sidebar.
2. `useConversations::create(providerId, modelId)` is called.
3. Tauri command creates DB record and returns it.
4. Store updates: adds to conversations array, sets activeId.
5. `ChatView` receives new conversation ID via prop (or state lift).
6. `useMessages::load(conversationId)` fetches messages from DB.
7. `startListening()` sets up streaming event listeners.

### Sending a User Message with Tool Calls
1. User types message, optionally attaches files and toggles tools.
2. Clicks send → `chat.sendMessage()` called via Tauri.
3. Backend executes `send_message` command:
   - Persists user message to DB
   - Emits `user_message` event (UI renders immediately)
   - Calls provider's `stream_chat()`
4. Provider emits tool calls → backend executes them in `spawn_blocking`
5. Each tool execution emits events: `tool_call`, `tool_call_result`
6. Frontend receives and displays tools as they execute
7. Final assistant message emitted via `stream_done` event
8. Auto-title generation triggered if no tools were called

### Permission Request Flow (Balanced Mode)
1. Model emits tool call → backend checks `is_permitted()`
2. If sensitive or write operation, emits `tool_permission_request` event
3. Frontend shows permission bubble with description and args preview
4. User clicks Allow/Deny → `agent.respondToPermission(approved)` called
5. Backend executes tool if approved; otherwise returns "Permission denied by user"
6. Tool result emitted via `tool_call_result` event

---

## Common Patterns & Anti-Patterns

### ✅ Good Patterns
- **Single source of truth**: Zustand stores are the only place UI state lives.
- **Event-driven streaming**: Tauri events allow real-time updates without polling.
- **Sealing thinking blocks**: When token stream begins, any open thinking block is sealed to prevent interleaving issues.
- **Spawn_blocking for tools**: Tool execution doesn't block the async runtime.

### ❌ Anti-Patterns to Avoid
- **Direct DB access from components**: Always use stores → Tauri commands → DB modules.
- **Assuming message content is plain text**: Assistant messages with tools contain JSON; parse carefully.
- **Ignoring tool_call_id**: Tool results are linked via this field in the `tool` role message.
- **Not sealing thinking blocks**: Content arriving while a thinking block is open will corrupt it.

---

*Last updated: 2026-06-12*
