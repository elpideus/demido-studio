export interface Conversation {
  id: string
  title: string
  provider_id: string
  model_id: string
  created_at: number
  updated_at: number
  agent_mode: 'off' | 'cautious' | 'balanced' | 'autonomous'
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

export interface ModelOverride {
  provider_id: string
  model_id: string
  custom_name?: string
  enabled: boolean
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

export interface AppSettings {
  default_provider_id: string
  default_model_id: string
  system_prompt: string
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
export type WindowComponent = 'settings' | 'tools' | 'image-editor'

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
