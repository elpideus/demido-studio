/** Response-compression style, per conversation. Mirrors `caveman::LEVELS` in the Rust backend. */
export type CavemanLevel =
  | 'off'
  | 'lite'
  | 'full'
  | 'ultra'
  | 'wenyan-lite'
  | 'wenyan-full'
  | 'wenyan-ultra'

export interface Conversation {
  id: string
  title: string
  provider_id: string
  model_id: string
  created_at: number
  updated_at: number
  agent_mode: 'off' | 'cautious' | 'balanced' | 'autonomous'
  caveman_level: CavemanLevel
  working_directory: string | null
}

export interface Message {
  id: string
  conversation_id: string
  role: 'user' | 'assistant' | 'system' | 'tool'
  content: string
  tool_call_id?: string
  created_at: number
  token_count?: number
  thinking?: string
}

export interface Provider {
  id: string
  name: string
  type: 'openai_compat' | 'openai' | 'anthropic' | 'gemini'
  base_url: string
  api_key_ref?: string
  enabled: boolean
  sort_order: number
  visible: boolean
}

/// Where a model's capability flags came from. Mirrors `caps::CapsSource` in Rust.
/// 'unknown' means nothing authoritative knew this model: the flags are defaults.
export type CapsSource = 'provider' | 'llamaCpp' | 'registry' | 'huggingFace' | 'unknown'

/** The three capabilities the app gates behaviour on. */
export type CapName = 'vision' | 'tools' | 'reasoning'

export interface ModelCaps {
  vision: boolean
  tools: boolean
  reasoning: boolean
  /** Where the *detected* values came from; says nothing about overridden fields. */
  source: CapsSource
  /** Which flags the user set by hand. Those beat detection. */
  overridden: Record<CapName, boolean>
}

export interface ModelOverride {
  provider_id: string
  model_id: string
  custom_name?: string
  enabled: boolean
  /** Manual capability overrides; null/undefined = auto. Written via
   *  `setModelCapsOverride`, not `upsertModelOverride` (the latter leaves them alone). */
  caps_vision?: boolean | null
  caps_tools?: boolean | null
  caps_reasoning?: boolean | null
}

export interface McpServer {
  id: string
  name: string
  transport: 'stdio' | 'sse'
  command?: string
  args?: string[]
  env?: Record<string, string>
  url?: string
  enabled: boolean
}

/** No system_prompt — it lives in system_prompt.md, read/written via the `systemPrompt` IPC group. */
export interface AppSettings {
  default_provider_id: string
  default_model_id: string
  auth_enabled: boolean
  context_window_limit: number
  task_provider_id: string
  task_model_id: string
  title_every_n_messages: number
}

export interface FileAttachment {
  name: string
  content: string
  mimeType?: string
}

// ─── Artifacts ────────────────────────────────────────────────────────────────

/** One editable file inside a skill folder; `name` is relative to the skill dir. */
export interface SkillFile {
  name: string
  content: string
}

export interface Artifact {
  id: string
  messageId: string
  type: string
  title: string
  content: string
  identifier?: string
}

// ─── Window System ────────────────────────────────────────────────────────────

/** Keys that identify which panel content to render inside a WindowFrame. */
export type WindowComponent = 'settings' | 'tools' | 'image-editor' | 'accounts' | 'email' | 'calendar' | 'contacts' | 'artifact-viewer' | 'graphify'

export interface ManagedWindow {
  id: string
  title: string
  component: WindowComponent
  position: { x: number; y: number }
  size: { width: number; height: number }
  zIndex: number
  /** Non-null when the window is docked to a screen edge. */
  snapState: { edge: 'left' | 'right'; fraction: number } | null
  /** Position restored when the window is undocked. */
  lastFreePosition: { x: number; y: number }
  /** Size restored when the window is undocked. */
  lastFreeSize: { width: number; height: number }
}

export interface SnapLayout {
  left:  { windowId: string; fraction: number } | null
  right: { windowId: string; fraction: number } | null
}

// ─── File-system ──────────────────────────────────────────────────────────────

export interface FsEntry {
  name: string
  path: string
  isDir: boolean
}

export interface EmailSummary {
  id: string
  subject: string
  from: string
  date: string
  snippet: string
}

export interface CalendarEvent {
  id: string
  summary: string
  start: string
  end: string
  location: string | null
  description: string | null
  color: string | null
}

export interface Contact {
  name: string
  emails: string[]
  phones: string[]
  photo_url: string | null
}

/** Open Graph metadata for one cited link — mirrors `web::LinkPreview` (serde camelCase).
 *  Every field past `url` is best-effort; `error` explains a gap instead of dropping the row. */
export interface LinkPreview {
  url: string
  title: string | null
  description: string | null
  image: string | null
  siteName: string | null
  error: string | null
}

export interface GItem {
  type: 'email' | 'event' | 'contact'
  id: string
  title: string
  subtitle?: string
  content?: string
}
