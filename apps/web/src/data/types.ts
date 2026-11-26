/* Shapes shared by the UI and the core. In the preview they are filled from fixtures; in the app they
   arrive from the core over the API. Keep them plain data: no functions, no classes. */

export type TaskState = 'pending' | 'running' | 'done' | 'failed' | 'needs-input' | 'skipped' | 'delegated'
export type ModelState = 'unloaded' | 'queued' | 'loading' | 'loaded' | 'generating' | 'lent' | 'error'
export type PermissionLevel = 'cautious' | 'balanced' | 'autonomous' | 'smart'
export type ThinkingLevel = 'off' | 'low' | 'medium' | 'high'
export type SignalLevel = 'note' | 'caution' | 'warning' | 'error'
export type Capability = 'vision' | 'audio' | 'tools' | 'thinking' | 'embedding'

export interface Task {
  id: string
  label: string
  state: TaskState
  meta?: string
  children?: Task[]
}

export interface Step {
  id: string
  state: 'running' | 'done' | 'failed' | 'needs-input'
  what: string
  code?: string
  meta: string
  request?: string
  output?: string
  fullLines?: number
  untrusted?: 'web' | 'email' | 'file' | 'session'
}

export interface PermissionRequest {
  id: string
  risk: 'low' | 'medium' | 'high'
  riskLabel: string
  title: string
  what: string
  why: string
  raw: string
  state: 'waiting' | 'allowed' | 'allowed-session' | 'denied'
}

export type Block =
  | { kind: 'thinking'; seconds: number; text: string }
  | { kind: 'tasks'; tasks: Task[] }
  | { kind: 'step'; step: Step }
  | { kind: 'permission'; request: PermissionRequest }
  | { kind: 'text'; plain: string; html: string; shown?: number }
  | { kind: 'signal'; level: SignalLevel; text: string }
  | { kind: 'artifact'; artifactId: string }

export interface Message {
  id: string
  from: 'user' | 'assistant'
  text?: string
  blocks?: Block[]
  streaming?: boolean
}

export interface Session {
  id: string
  title: string
  tags: string[]
  when: 'Today' | 'This week' | 'Earlier'
  updated: string
  state: TaskState
  modelId: string
  characterId?: string
  permission: PermissionLevel
  thinking: ThinkingLevel
  contextUsed: number
  taint: string[]
  /** Where the model works. Demido's own private folder, or one you assigned. */
  folder: string
  /** The group it is filed under, if any. */
  groupId?: string
  messages: Message[]
}
/** A group of sessions in the list. You make them; sessions are moved in and out. */
export interface SessionGroup {
  id: string
  name: string
  open: boolean
}

export interface Quant {
  id: string
  fileGb: number
  memoryGb: number
  fits: 'yes' | 'tight' | 'no'
  note: string
  recommended?: boolean
}

export interface Model {
  id: string
  name: string
  publisher: string
  license: string
  params: string
  context: number
  caps: Capability[]
  summary: string
  downloads: string
  /** People who marked it as liked on the hub. */
  likes?: string
  /** How much its downloads grew this week, in percent. Only the fast risers have one. */
  trend?: number
  updated: string
  recommended?: boolean
  gated?: boolean
  quants: Quant[]
  installed?: string
  download?: { quant: string; progress: number }
  state: ModelState
  loadProgress?: number
  speed?: number
  lentTo?: string
}

export type ToolKind = 'builtin' | 'tool' | 'external' | 'mcp' | 'skill'

export interface Tool {
  id: string
  name: string
  publisher: string
  version: string
  kind: ToolKind
  icon: string
  summary: string
  guidance: string
  guidanceEdited?: boolean
  indexTokens: number
  rawTokens?: number
  can: string[]
  cannot: string[]
  functions: { name: string; summary: string; effect: 'read' | 'write' | 'destructive' | 'external' }[]
  audit: 'passed' | 'none' | 'failed' | 'running'
  auditNote?: string
  update?: string
  enabled: boolean
  installed: boolean
}

export interface Schedule {
  id: string
  title: string
  kind: 'schedule' | 'monitor'
  when: string
  next: string
  modelId: string
  enabled: boolean
  prompt: string
  state: 'idle' | 'running'
  runs: { at: string; result: 'ok' | 'changed' | 'failed' | 'no change'; note: string }[]
}

export interface Character {
  id: string
  name: string
  initials: string
  role: string
  description: string
  modelId: string
  temperature: number
  voice: string
  prompt: string
}

export interface Artifact {
  id: string
  name: string
  type: string
  sessionId: string
  versions: { v: number; at: string; size: string; note: string; content: string }[]
}

export interface Agent {
  id: string
  sessionId: string
  label: string
  state: 'running' | 'waiting' | 'done'
  memoryGb: number
  borrows?: string
}

export interface Provider {
  id: string
  name: string
  kind: 'api' | 'cli'
  status: 'connected' | 'not set up' | 'not installed'
  note: string
  billing: string
  usage?: { label: string; used: number; limit: number; unit: string; reset?: string; source: 'reported' | 'estimated' }[]
  models: number
}

export interface Toast {
  id: string
  text: string
  /** On its way out: shown leaving for a moment, then gone. */
  leaving?: boolean
  action?: { label: string; run: () => void }
  glyph?: string
}
