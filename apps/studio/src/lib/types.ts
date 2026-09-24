// TypeScript mirrors of the backend's serialized types (see src-tauri/src). Field names follow
// the backend's `camelCase` serialization.

export interface Chat {
  id: string;
  title: string;
  modelId: string | null;
  createdAt: number;
  updatedAt: number;
  pinned: boolean;
}

export type Role = 'user' | 'assistant' | 'tool';
export type MessageStatus = 'streaming' | 'awaitingApproval' | 'running' | 'done' | 'error' | 'cancelled';

export interface ToolCall {
  id: string;
  name: string;
  arguments: string;
}

export interface ToolResult {
  label: string;
  args: Record<string, unknown>;
  ok?: boolean;
  display?: ToolDisplay;
  durationMs?: number;
}

/** What a tool returned for the UI; `kind` says how to draw it. */
export type ToolDisplay = { kind?: string; error?: string; denied?: boolean } & Record<string, unknown>;

export interface MessageStats {
  model?: string;
  providerModel?: string | null;
  promptTokens?: number;
  completionTokens?: number;
  cachedTokens?: number;
  reasoningTokens?: number;
  tokensPerSecond?: number;
  promptPerSecond?: number | null;
  durationMs?: number;
  ttftMs?: number | null;
  finishReason?: string | null;
}

export interface Message {
  id: string;
  chatId: string;
  seq: number;
  role: Role;
  content: string;
  reasoning: string | null;
  toolCalls: ToolCall[];
  toolCallId: string | null;
  toolName: string | null;
  toolResult: ToolResult | null;
  modelId: string | null;
  status: MessageStatus;
  error: string | null;
  stats: MessageStats | null;
  providerMeta: unknown;
  createdAt: number;
}

export type ChatEvent =
  | { type: 'message'; chatId: string; message: Message }
  | { type: 'delta'; chatId: string; messageId: string; content: string; reasoning: string }
  | { type: 'toolCall'; chatId: string; messageId: string; name: string }
  | { type: 'truncated'; chatId: string; fromSeq: number }
  | { type: 'turnStarted'; chatId: string }
  | { type: 'turnFinished'; chatId: string; error: string | null }
  | { type: 'chat'; chat: Chat }
  | { type: 'notice'; chatId: string; kind: string; text: string };

export interface Trace {
  id: string;
  chatId: string;
  messageId: string | null;
  createdAt: number;
  modelId: string | null;
  request: Record<string, unknown>;
  response: Record<string, unknown> | null;
  durationMs: number | null;
  error: string | null;
}

export type ModelSource = 'local' | 'gemini';

export interface ModelSettings {
  name?: string | null;
  description?: string | null;
  avatar?: string | null;
  enabled?: boolean | null;
  systemPrompt?: string | null;
  temperature?: number | null;
  topP?: number | null;
  topK?: number | null;
  minP?: number | null;
  repeatPenalty?: number | null;
  maxTokens?: number | null;
  contextLength?: number | null;
  gpuLayers?: number | null;
  thinking?: boolean | null;
}

export interface EffectiveSettings {
  systemPrompt: string;
  temperature: number | null;
  topP: number | null;
  topK: number | null;
  minP: number | null;
  repeatPenalty: number | null;
  maxTokens: number | null;
  contextLength: number | null;
  gpuLayers: number | null;
  thinking: boolean | null;
}

export interface ModelEntry {
  id: string;
  source: ModelSource;
  providerId: string | null;
  providerName: string | null;
  name: string;
  defaultName: string;
  description: string | null;
  avatarPath: string | null;
  enabled: boolean;
  isDefault: boolean;
  path: string | null;
  size: number | null;
  quant: string | null;
  architecture: string | null;
  parameters: string | null;
  maxContext: number | null;
  repo: string | null;
  removable: boolean;
  hasVision: boolean;
  supportsThinking: boolean;
  settings: ModelSettings;
  effective: EffectiveSettings;
}

export type RuntimeState = 'idle' | 'loading' | 'ready' | 'error' | 'unavailable';

export interface RuntimeStatus {
  state: RuntimeState;
  modelId: string | null;
  modelName: string | null;
  message: string | null;
  contextLength: number | null;
  loadSeconds: number | null;
}

export interface GeminiModel {
  id: string;
  displayName: string;
  description: string;
  inputTokenLimit: number;
  outputTokenLimit: number;
  thinking: boolean;
}

export interface ProviderView {
  id: string;
  kind: 'gemini';
  name: string;
  enabled: boolean;
  baseUrl: string | null;
  models: GeminiModel[];
  modelsFetchedAt: number | null;
  createdAt: number;
  hasKey: boolean;
  keyHint: string | null;
}

export interface SkillFile {
  path: string;
  size: number;
}

export interface Skill {
  id: string;
  name: string;
  description: string;
  enabled: boolean;
  folder: string;
  files: SkillFile[];
  body: string;
  updatedAt: number;
  author: string | null;
  problem: string | null;
}

export interface ToolGroup {
  id: string;
  label: string;
  description: string;
  enabled: boolean;
  available: boolean;
  reason: string | null;
  tools: string[];
}

export interface MarketStatus {
  available: boolean;
  reason: string | null;
  running: boolean;
  loggedIn: boolean;
  username: string | null;
  loginPending: boolean;
}

export type JobState = 'queued' | 'downloading' | 'paused' | 'done' | 'failed';

export interface DownloadJob {
  id: string;
  repo: string;
  name: string;
  quant: string | null;
  total: number;
  downloaded: number;
  bytesPerSecond: number;
  state: JobState;
  error: string | null;
  createdAt: number;
}

export interface HfRepo {
  id: string;
  author: string;
  downloads: number;
  likes: number;
  lastModified: string | null;
  preferred: boolean;
}

export type Fit = 'fits' | 'tight' | 'large' | 'unknown';

export interface HfModelFile {
  name: string;
  paths: string[];
  sizes: number[];
  sha256: Array<string | null>;
  size: number;
  quant: string | null;
  fit: Fit;
  recommended: boolean;
}

export interface HfRepoFiles {
  repo: string;
  gated: boolean;
  files: HfModelFile[];
}

export interface Recommendation {
  family: string;
  familyLabel: string;
  description: string;
  name: string;
  repo: string;
  file: string;
  quant: string;
  size: number;
  sha256: string;
  installed: boolean;
}

export interface ModelFolder {
  path: string;
  managed: boolean;
  exists: boolean;
}

export interface ComponentInfo {
  name: string;
  version: string | null;
  installed: boolean;
  detail: string | null;
}

export interface AppInfo {
  version: string;
  dataDir: string;
  installDir: string | null;
  disclaimerAccepted: boolean;
  os: string;
  cpu: string;
  memoryGb: number;
  gpu: string | null;
  vramGb: number | null;
  backend: string | null;
  components: ComponentInfo[];
  dev: boolean;
}

export interface Settings {
  disclaimerAccepted: string | null;
  defaultModel: string | null;
  preloadDefaultModel: boolean;
  toolGroups: Record<string, boolean>;
  alwaysAllowedTools: string[];
  extraModelDirs: string[];
  chatListOpen: boolean;
  windowLayout: unknown;
  lastChatId: string | null;
}

export interface Bar {
  t: number;
  o: number;
  h: number;
  l: number;
  c: number;
  v: number;
}

export interface ChartInfo {
  symbol: string;
  description: string;
  exchange: string;
  currency?: string;
  type?: string;
  pricescale?: number;
  timezone?: string;
}

export interface SymbolMatch {
  symbol: string;
  ticker: string;
  description: string;
  exchange: string;
  type: string;
}

export interface Quote {
  symbol: string;
  description?: string;
  price?: number;
  change?: number;
  changePercent?: number;
  bid?: number;
  ask?: number;
  high?: number;
  low?: number;
  open?: number;
  prevClose?: number;
  volume?: number;
  currency?: string;
  exchange?: string;
  type?: string;
  time?: string;
  error?: string;
}
