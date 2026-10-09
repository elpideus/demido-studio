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

/** `summary` stands in for the messages before it once the chat is compacted. */
export type Role = 'user' | 'assistant' | 'tool' | 'summary';
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
 * A tool asking mid-run: a market download estimated to take longer than
 * `Settings.downloadApprovalSeconds` (1-minute candles, which serve every timeframe), or saving a
 * Pine script to the user's TradingView account (`pine_publish`).
 */
export type ToolApproval =
  | { kind: 'download'; plan: MarketPlan }
  | {
      kind: 'pinePublish';
      name: string;
      lines: number;
      /** It updates the script it was saved to before, rather than adding one. */
      update: boolean;
      source: string;
    };

export type ApprovalDecision = 'once' | 'always' | 'deny';

/** What a tool returned for the UI; `kind` says how to draw it. */
export type ToolDisplay = { kind?: string; error?: string; denied?: boolean } & Record<string, unknown>;

export interface MessageStats {
  model?: string;
  /** The model the provider reported: for a router, the one it picked. */
  providerModel?: string | null;
  /** Who ran the model, when the provider says (OpenRouter names the one it sent the request to). */
  provider?: string | null;
  /** The model is a router (OpenRouter's Free Models Router, say), so `providerModel` is its pick. */
  routed?: boolean | null;
  promptTokens?: number;
  completionTokens?: number;
  cachedTokens?: number;
  reasoningTokens?: number;
  tokensPerSecond?: number;
  promptPerSecond?: number | null;
  durationMs?: number;
  ttftMs?: number | null;
  finishReason?: string | null;
  /** Times the model was busy and the request went again. */
  retries?: number | null;
}

/** The stats of a `summary` message. */
export interface SummaryStats {
  /** Written when the chat neared the end of the context window, not by `/compact`. */
  auto?: boolean;
  /** Messages it summarizes. */
  messages?: number;
  tokensBefore?: number;
  tokensAfter?: number;
  /** The model's name. */
  model?: string;
  durationMs?: number;
  /** The part being written, while a long conversation is summarized part by part. */
  part?: number;
  parts?: number;
}

/** What decides how large a model's window is: what the model is made for, the memory free, or
 * the context length set in its settings. */
export type WindowBy = 'model' | 'memory' | 'setting';

/** An auto-compact threshold the person set, for one model or for every model. */
export interface CustomThreshold {
  tokens: number;
  scope: 'model' | 'all';
}

/** How full a chat's context window is, measured as a turn measures it before compacting. */
export interface ContextUsage {
  /** Tokens the model reads at once. */
  window: number;
  /** A local model not loaded yet: its server may give it a smaller window when it loads. */
  windowEstimated: boolean;
  /** What decides `window`. */
  windowBy: WindowBy;
  /** Tokens the model is made for (trained on, or its provider takes); `window` is never more. */
  modelLimit: number | null;
  /** Tokens of the window every request keeps for the answer. */
  reserve: number;
  /** Tokens the next request takes as the chat stands, before the message being written. */
  used: number;
  /** `used` is the model's own count of its latest answer rather than an estimate. */
  counted: boolean;
  /** Tokens at which the chat is compacted; null with auto-compact off. */
  threshold: number | null;
  /** The highest threshold the window allows: where the chat compacts unless a lower one is set. */
  ceiling: number;
  /** The threshold the person set. Above `ceiling`, it is never reached. */
  custom: CustomThreshold | null;
  /** There is an older part worth summarizing; without one, a full chat drops its oldest messages. */
  canCompact: boolean;
  /** Summaries written in this chat so far. */
  compactions: number;
  /** `used`, part by part. */
  parts: { system: number; tools: number; summary: number; files: number; conversation: number };
}

/** The slash command that wrote a user message: `/name args`. */
export interface CommandUse {
  name: string;
  args: string;
  /** The skill that provides it, by id. */
  skill: string | null;
}

export type AttachmentKind = 'image' | 'audio' | 'document' | 'text' | 'data' | 'other';

/** A file sent with a message: staged in the composer until sent, then in the chat's workspace. */
export interface Attachment {
  id: string;
  /** File name as the person sees it. */
  name: string;
  /** Absolute path on disk: for opening the file and image thumbnails (fileUrl). */
  path: string;
  /** Path inside the chat's workspace once sent ("uploads/report.pdf"); null while staged. */
  file: string | null;
  mime: string;
  kind: AttachmentKind;
  /** Bytes. */
  size: number;
  /** PDF pages, slides, or spreadsheet sheets. */
  pages: number | null;
  /** Estimated tokens of the text the model reads; null for images, audio and other files. */
  tokens: number | null;
  width: number | null;
  height: number | null;
  /** A warning for the person, e.g. "No text found. It may be a scanned PDF." */
  note: string | null;
}

export interface Message {
  id: string;
  chatId: string;
  seq: number;
  role: Role;
  content: string;
  /** Files sent with a user message; empty otherwise. */
  attachments: Attachment[];
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
  /** Set on a user message a slash command wrote; `content` is what it sent. */
  command?: CommandUse | null;
}

export type ChatEvent =
  | { type: 'message'; chatId: string; message: Message }
  | { type: 'delta'; chatId: string; messageId: string; content: string; reasoning: string }
  | { type: 'toolCall'; chatId: string; messageId: string; name: string }
  | {
      type: 'retrying';
      chatId: string;
      messageId: string;
      attempt: number;
      attempts: number;
      waitMs: number;
      reason: string;
    }
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

export type ModelSource = 'local' | 'gemini' | 'openrouter';

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
  /** Tokens at which chats with this model are compacted, instead of the threshold for every model. */
  autoCompactTokens?: number | null;
}

export interface EffectiveSettings {
  systemPrompt: string;
  temperature: number | null;
  topP: number | null;
  topK: number | null;
  minP: number | null;
  repeatPenalty: number | null;
  maxTokens: number | null;
  /** Null for a local model llama.cpp sizes as it loads: the runtime status says what it took. */
  contextLength: number | null;
  gpuLayers: number | null;
  thinking: boolean | null;
}

/** What a model can do; null while nobody has said. */
export interface ModelCapabilities {
  /** Reads images. */
  vision: boolean | null;
  /** Reads sound. */
  audio: boolean | null;
  /** Calls tools. */
  tools: boolean | null;
  /** Thinks before answering. */
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
  capabilities: ModelCapabilities;
  /** llama.cpp has yet to say what this local model can do. */
  checkingCapabilities: boolean;
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

export interface CloudModel {
  id: string;
  displayName: string;
  description: string;
  inputTokenLimit: number;
  outputTokenLimit: number;
  thinking: boolean;
  /** Thinks whatever it is asked. */
  alwaysThinks?: boolean;
  /** What it can do, when the provider's list says (OpenRouter's does). */
  capabilities?: ModelCapabilities;
  /** Costs nothing to use. */
  free?: boolean;
}

export type ProviderKind = 'gemini' | 'openrouter';

/** Which of a provider's models are offered: OpenRouter lists paid models next to free ones. */
export type ModelGroup = 'all' | 'free';

export interface ProviderView {
  id: string;
  kind: ProviderKind;
  name: string;
  enabled: boolean;
  baseUrl: string | null;
  modelGroup: ModelGroup;
  /** The models of `modelGroup`. */
  models: CloudModel[];
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
  /** Slash commands from its `commands.json`. */
  commands: SkillCommand[];
  /** What is wrong in its `commands.json`; the commands that are fine still work. */
  commandsProblem: string | null;
}

export interface SkillCommand {
  name: string;
  description: string;
  args: string | null;
  prompt: string;
}

/** A command the composer offers after a `/`. */
export interface SlashCommand {
  name: string;
  description: string;
  /** What to type after the name, such as `<symbol> [timeframe]`. */
  args: string | null;
  /** The skill that provides it; null for the app's own. */
  skill: string | null;
  skillName: string | null;
}

/** What running a slash command did. */
export type SlashOutcome =
  /** It ran: `text` says what it changed or found; `settings` are the settings it changed. */
  | { kind: 'done'; title: string; text: string; settings: Settings | null }
  /** It sent a message, as `sendMessage` does. */
  | { kind: 'sent'; chat: Chat; message: Message }
  /** It started work in the chat, which streams in like a turn. */
  | { kind: 'started' };

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
  /** Summarize the chat when it nears the end of the model's context window. */
  autoCompact: boolean;
  /** Tokens at which that happens; null is near the model's limit. Never above that limit. */
  autoCompactTokens: number | null;
  /** The Mail window loads the images emails link to from the internet without asking. */
  mailShowImages: boolean;
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

// Chart indicators (sidecars/market/src/indicators). TradingView computes them on the live chart;
// signed out, a few classics are computed from the stored bars instead (market/indicators.ts).

export type IndicatorPlotKind = 'line' | 'step' | 'area' | 'histogram' | 'columns' | 'circles' | 'cross' | 'shapes';

export interface IndicatorPlot {
  id: string;
  title: string;
  kind: IndicatorPlotKind;
  color: string;
  width: number;
  /** 0 solid, 1 dotted, 2 dashed (TradingView's numbering). */
  dash: number;
  /** The column whose value picks this plot's color on each bar. */
  colorer?: string;
  /** Colorer value → color. */
  colors?: Record<string, string>;
  /** Shapes: TradingView's shape name (`triangle_up`, `label_down`, `circle`, `char`, `arrow`…). */
  shape?: string;
  location?: 'abovebar' | 'belowbar' | 'top' | 'bottom' | 'absolute';
  text?: string;
  textColor?: string;
  /** Arrows: the color of negative values. */
  downColor?: string;
  /** Histograms and columns: the value bars grow from. */
  base?: number;
  /** Not drawn (the script's `display.none`, a layout's or the settings' unticked box); its values
   *  still come in every row. */
  hidden?: boolean;
  /** Its last value on the price scale (default: yes). */
  axisLabel?: boolean;
}

export interface IndicatorBand {
  id: string;
  title: string;
  value: number;
  color: string;
  width: number;
  dash: number;
  hidden?: boolean;
}

export interface IndicatorInput {
  id: string;
  name: string;
  /** TradingView's input type: integer, float, bool, text, source, resolution, color, symbol… */
  type: string;
  value: unknown;
  defval: unknown;
  /** Not shown in the settings (internal inputs). */
  hidden: boolean;
  fake: boolean;
  min?: number;
  max?: number;
  step?: number;
  options?: string[];
  group?: string;
  tooltip?: string;
  /** Shown after the name in the chart's legend, as in TradingView's status line (default: yes). */
  legend?: boolean;
}

export interface IndicatorMeta {
  id: string;
  version: string;
  name: string;
  short: string;
  kind: 'study' | 'strategy';
  /** Drawn over the candles (true) or in a pane of its own. */
  overlay: boolean;
  /** Decimals of its values; null uses the chart's. */
  precision: number | null;
  plots: IndicatorPlot[];
  bands: IndicatorBand[];
  inputs: IndicatorInput[];
  /** The plot ids each row carries after its time, in order. */
  columns: string[];
  /** Colors of the script's drawings, by index. */
  palette: string[];
}

/** Bar time (seconds), then one value per column of the meta (null: no value on that bar). */
export type IndicatorRow = [number, ...(number | null)[]];

export interface IndicatorLabel {
  id: number;
  t: number;
  y: number | null;
  yloc: 'price' | 'abovebar' | 'belowbar';
  text: string;
  style: string;
  color: string | null;
  textColor: string | null;
  size: string;
  tooltip?: string;
}

export interface IndicatorLine {
  id: number;
  t1: number;
  y1: number;
  t2: number;
  y2: number;
  extend: 'none' | 'left' | 'right' | 'both';
  style: string;
  color: string | null;
  width: number;
}

export interface IndicatorBox {
  id: number;
  t1: number;
  /** Top. */
  y1: number;
  t2: number;
  /** Bottom. */
  y2: number;
  color: string | null;
  bg: string | null;
  extend: 'none' | 'left' | 'right' | 'both';
  style: string;
  width: number;
  text: string;
  textColor: string | null;
  textSize: string;
  halign: string;
  valign: string;
}

export interface IndicatorTableCell {
  row: number;
  col: number;
  text: string;
  textColor: string | null;
  bg: string | null;
  size: string;
  halign: string;
  valign: string;
  colspan: number;
  rowspan: number;
  tooltip?: string;
}

export interface IndicatorTable {
  id: number;
  position: string;
  rows: number;
  columns: number;
  bg: string | null;
  frame: string | null;
  frameWidth: number;
  border: string | null;
  borderWidth: number;
  cells: IndicatorTableCell[];
}

/** What a script draws besides its plots (Pine's labels, lines, boxes and tables). */
export interface IndicatorGraphics {
  labels: IndicatorLabel[];
  lines: IndicatorLine[];
  boxes: IndicatorBox[];
  tables: IndicatorTable[];
}

/** How an indicator is set up: input values, and the look a saved TradingView layout gave it. */
export interface IndicatorSetup {
  inputs?: Record<string, unknown>;
  styles?: Record<string, Record<string, unknown>>;
  palettes?: Record<string, Record<string, unknown>>;
  bands?: unknown[];
  overlay?: boolean;
}

/** One indicator offered by the Indicators menu. */
export interface IndicatorEntry {
  id: string;
  /** The script version to run; null runs the latest. */
  version: string | null;
  name: string;
  short?: string;
  overlay?: boolean;
  kind: 'study' | 'strategy';
  author?: string;
  /** Community scripts: open source, protected (closed source) or invite-only. */
  access?: 'open' | 'protected' | 'invite';
}

export interface IndicatorCatalog {
  favorites: IndicatorEntry[];
  mine: IndicatorEntry[];
  builtins: IndicatorEntry[];
}

/** A chart layout saved on the user's TradingView account. */
export interface ChartLayoutEntry {
  id: string;
  name: string;
  symbol: string;
  interval: string;
  /** Last saved, seconds. */
  modified: number | null;
}

export interface ChartLayoutStudy {
  id: string;
  version: string;
  name: string;
  /** Hidden in the layout. */
  hidden: boolean;
  state: IndicatorSetup;
}

export interface ChartLayout {
  name: string;
  symbol: string;
  interval: string;
  studies: ChartLayoutStudy[];
  /** Names of what could not be brought over (TradingView's older built-in studies). */
  skipped: string[];
}

/**
 * How an indicator looks on this chart, as its settings' Style and Visibility tabs set it. Applied
 * here, over what TradingView describes, so changing it does not compute the indicator again.
 */
export interface IndicatorLook {
  plots?: Record<string, PlotLook>;
  bands?: Record<string, BandLook>;
  /** Decimals of its values; absent: the script's. */
  precision?: number;
  /** The timeframes it shows on; absent: all. */
  timeframes?: string[];
  /** The inputs after its name in the legend (default: yes). */
  legendInputs?: boolean;
  /** Its values in the legend (default: yes). */
  legendValues?: boolean;
  /** Its last values on the price scale (default: yes). */
  scaleLabels?: boolean;
}

export interface PlotLook {
  color?: string;
  /** Per-bar colors, by the colorer's value. */
  colors?: Record<string, string>;
  width?: number;
  dash?: number;
  kind?: IndicatorPlotKind;
  hidden?: boolean;
}

export interface BandLook {
  color?: string;
  width?: number;
  dash?: number;
  value?: number;
  hidden?: boolean;
}

// Pine scripts written in Demido (sidecars/market/src/indicators/pine.ts), compiled and run by
// TradingView. On the chart a library script is `DEMIDO;<id>`.

export interface PineSummary {
  id: string;
  name: string;
  kind: 'indicator' | 'strategy' | 'library' | null;
  /** Milliseconds. */
  created: number;
  modified: number;
  /** Goes up with every change of its source. */
  revision: number;
  lines: number;
  /** The script on the user's TradingView account it was saved to or imported from. */
  tradingview?: { id: string; version: string; synced: number; changed: boolean };
}

export interface PineScript extends PineSummary {
  source: string;
}

/** A compiler message; lines and columns count from 1. */
export interface PineMessage {
  line: number;
  column: number;
  endLine: number;
  endColumn: number;
  message: string;
  code?: string;
}

export interface PineCheck {
  ok: boolean;
  errors: PineMessage[];
  warnings: PineMessage[];
  kind: PineSummary['kind'];
  title: string;
  overlay?: boolean;
  meta?: Pick<IndicatorMeta, 'name' | 'overlay' | 'inputs' | 'plots' | 'bands'>;
}

export interface PinePublished {
  script: PineSummary;
  tradingview: { id: string; version: string; created: boolean };
  warnings: PineMessage[];
}

/** One drawing of a set the assistant drew (`chart_draw`); times are seconds. */
export interface ChartDrawingItem {
  type: 'hline' | 'line' | 'box' | 'label' | 'marker' | 'series';
  time?: number;
  price?: number;
  time2?: number;
  price2?: number;
  points?: Array<[number, number | null]>;
  text?: string;
  color?: string;
  width?: number;
  style?: 'solid' | 'dashed' | 'dotted';
  extend?: 'none' | 'left' | 'right' | 'both';
  position?: 'above' | 'below';
  shape?: string;
}

/** A named set of drawings on a chart, kept in the Market window's props. */
export interface ChartDrawing {
  name: string;
  pane: 'overlay' | 'separate';
  items: ChartDrawingItem[];
  /** The market it was drawn on; it shows on that market's chart only. */
  symbol?: string;
  hidden?: boolean;
}

// ---------------------------------------------------------------- Mail (src-tauri mail/)

export type MailKind = 'gmail' | 'imap';

export interface MailAccount {
  id: string;
  kind: MailKind;
  email: string;
  /** The name the person gave the account, like Work; the assistant takes it in place of the address. */
  nickname: string | null;
  host: string;
  /** Why the account cannot be read right now (a refused password). */
  error: string | null;
  /** The password is missing and must be entered again. */
  needsPassword: boolean;
  /** The credential store refused the password: it is kept only until the app closes. */
  sessionOnly: boolean;
}

export interface NewMailAccount {
  kind: MailKind;
  email: string;
  password: string;
  host?: string | null;
  port?: number | null;
  username?: string | null;
  nickname?: string | null;
}

export interface MailFolder {
  path: string;
  name: string;
  /** inbox, flagged, important, sent, drafts, archive, all, junk, trash */
  role: string | null;
  selectable: boolean;
  depth: number;
  total: number | null;
  unseen: number | null;
}

export interface MailAddress {
  name: string;
  email: string;
}

export interface MailSummary {
  id: number;
  account: string;
  folder: string;
  uid: number;
  date: number;
  from: MailAddress;
  to: MailAddress[];
  cc: MailAddress[];
  subject: string;
  snippet: string;
  unread: boolean;
  flagged: boolean;
  answered: boolean;
  draft: boolean;
  attachments: number;
  labels: string[];
  size: number;
  messageId: string | null;
}

/** Mail that arrived between two moments (ms, both included); a missing end is open. */
export interface MailDateRange {
  since: number | null;
  until: number | null;
}

export interface MailPage {
  account: string;
  folder: string;
  messages: MailSummary[];
  total: number | null;
  unseen: number | null;
  /** The folder has been loaded at least once. */
  loaded: boolean;
  /** No older messages remain on the server. */
  complete: boolean;
  checkedAt: number | null;
}

export interface MailAttachment {
  /** IMAP part number, which names the attachment to the backend. */
  section: string;
  name: string;
  mime: string;
  size: number;
}

export interface MailMessage {
  summary: MailSummary;
  html: string | null;
  text: string;
  attachments: MailAttachment[];
}
