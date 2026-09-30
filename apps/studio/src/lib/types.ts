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
  /** What a running tool is waiting for the person to decide (set only while `awaitingApproval`). */
  approval?: ToolApproval;
}

/**
 * A tool asking mid-run. The only kind so far is a market download estimated to take longer than
 * `Settings.downloadApprovalSeconds`: 1-minute candles, which serve every timeframe.
 */
export interface ToolApproval {
  kind: 'download';
  plan: MarketPlan;
}

export type ApprovalDecision = 'once' | 'always' | 'deny';

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
  /** The assistant asks before a market download estimated to take longer than this. */
  downloadApprovalSeconds: number;
  /** Change through `api.setUpdatePreferences`, which also acts on the change. */
  updateChannel: UpdateChannel;
  autoUpdate: boolean;
}

export type UpdateChannel = 'release' | 'prerelease';

export type UpdatePhase =
  'idle' | 'checking' | 'upToDate' | 'available' | 'downloading' | 'ready' | 'installing' | 'error';

/** A newer version of Demido Studio, as its GitHub release describes it. */
export interface UpdateRelease {
  version: string;
  /** Markdown. */
  notes: string;
  publishedAt: string | null;
  /** The release's page on GitHub. */
  url: string;
  /** Installer size in bytes. */
  size: number;
  prerelease: boolean;
}

export interface UpdateProgress {
  downloaded: number;
  total: number | null;
  bytesPerSecond: number;
}

/** The updater's state, sent on every change as `updater://status`. */
export interface UpdateStatus {
  currentVersion: string;
  channel: UpdateChannel;
  auto: boolean;
  phase: UpdatePhase;
  /** The newer version found, downloading or ready to install. */
  release: UpdateRelease | null;
  progress: UpdateProgress | null;
  /** RFC 3339. */
  lastChecked: string | null;
  /** Set with phase `error`. */
  error: string | null;
  /** Why this copy cannot install updates itself (checking still works); null when it can. */
  unsupported: string | null;
  /** A machine-wide installation: Windows asks for administrator permission to install. */
  needsAdmin: boolean;
  /** The installer already ran once for the ready update and did not finish. */
  failedAttempt: boolean;
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

// The market data store (sidecars/market/src/store). Every time is in seconds. Dukascopy data is
// kept at three tiers (`m1`, `h1`, `d1`); TradingView data per chart timeframe.

export type MarketSource = 'dukascopy' | 'tradingview';

/** The store keys a symbol reads from; charts match `store.updated` events against them. */
export interface MarketKeys {
  dukascopy?: string;
  tradingview?: string;
}

/** A contiguous run of returned bars from one source; `from`/`to` are the first and last bar times. */
export interface MarketSpan {
  source: MarketSource;
  from: number;
  to: number;
  count: number;
}

/** Uncovered but available time right before the oldest returned bar. */
export interface MarketGap {
  from: number;
  to: number;
  source: MarketSource;
}

export type MarketJobStatus = 'queued' | 'running' | 'waiting' | 'paused' | 'done' | 'error';
export type MarketOrigin = 'chart' | 'chat' | 'data';

/** One tier's (or TradingView timeframe's) share of a job or plan. */
export interface MarketTierProgress {
  tier: string;
  total: number;
  done: number;
}

/** A background download of one requested range. Counts are requests (files); TradingView: pages. */
export interface MarketJob {
  id: string;
  source: MarketSource;
  key: string;
  symbol: string;
  name: string;
  from: number;
  to: number;
  tiers: string[];
  status: MarketJobStatus;
  total: number;
  done: number;
  bytes: number;
  /** Files left out because the source has no data that far back (a learned start); not in `total`. */
  skipped?: number;
  etaSeconds: number | null;
  rate: number;
  inFlight: number;
  perTier: MarketTierProgress[];
  origin: MarketOrigin;
  createdAt: number;
  updatedAt: number;
  message?: string;
}

/** What a download would fetch, without fetching anything. */
export interface MarketPlan {
  source: MarketSource;
  key: string;
  name: string;
  from: number;
  to: number;
  tiers: string[];
  requests: number;
  bytes: number;
  seconds: number;
  queuedAhead: number;
  complete: boolean;
  approximate: boolean;
  perTier: Array<{ tier: string; from: number; to: number; requests: number }>;
  /** An unfinished job already covering this range. */
  job: { id: string; status: MarketJobStatus } | null;
  /** Dukascopy: where the newest run of stored 1-minute history starts (null: nothing stored). */
  storedFrom?: number | null;
}

export interface MarketTierCoverage {
  tier: string;
  /** Covered `[from, to)` ranges. */
  intervals: Array<[number, number]>;
  /** Fetched, but the source has no data there. */
  empty?: Array<[number, number]>;
  available: [number, number] | null;
  learnedStart?: number | null;
  bytes: number;
}

/** What is stored for one market (a Dukascopy key, else a TradingView symbol). */
export interface MarketCoverageItem {
  market: string;
  name: string;
  symbols: string[];
  sources: Array<{ source: MarketSource; key: string; bytes: number; tiers: MarketTierCoverage[] }>;
  /** Unfinished jobs for this market. */
  jobs: MarketJob[];
}

export interface MarketCacheSummary {
  bytes: number;
  items: MarketCoverageItem[];
}

/** `cached`: more stored data right before the oldest bar; `gap`: nothing stored but more exists. */
export type MarketMore = 'cached' | 'gap' | 'none';

export interface MarketBarsPage {
  bars: Bar[];
  spans: MarketSpan[];
  more: MarketMore;
  gap?: MarketGap;
  keys: MarketKeys;
}

export interface MarketLatestBars extends MarketBarsPage {
  symbol: string;
  info: ChartInfo;
  timeframe: string;
  /** Served from the store without a live source (a TradingView-routed symbol while signed out). */
  stale?: boolean;
  /** Why fetching the newest bars failed (offline, throttled), when it did; `bars` is what was stored. */
  fetchError?: string;
}

/** Optional bounds of a download: seconds, and tiers to restrict it to. */
/** A download's range, up to `to` or now. History is 1-minute candles, so there is nothing to pick
 *  per timeframe; `back` counts from the stored 1-minute history instead of a date. */
export interface MarketRange {
  from?: number;
  /** One more month or year than the stored 1-minute history (instead of `from`). */
  back?: 'month' | 'year';
  to?: number;
  /** TradingView only: which of its timeframes to page. */
  tiers?: string[];
}

export interface MarketStoreUpdate {
  source: MarketSource;
  key: string;
  from: number;
  to: number;
}
