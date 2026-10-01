// Typed wrappers over the backend's Tauri commands. Every call the UI makes goes through here,
// so the command surface is visible in one file.

import { invoke } from '@tauri-apps/api/core';

import type {
  AppInfo,
  ApprovalDecision,
  Attachment,
  Bar,
  Chat,
  ChartInfo,
  DownloadJob,
  HfRepo,
  HfRepoFiles,
  HfModelFile,
  MarketBarsPage,
  MarketCacheSummary,
  MarketJob,
  MarketKeys,
  MarketLatestBars,
  MarketOrigin,
  MarketPlan,
  MarketRange,
  MarketStatus,
  Message,
  ModelEntry,
  ModelFolder,
  ModelSettings,
  ProviderView,
  Quote,
  Recommendation,
  RuntimeStatus,
  SearchStatus,
  Settings,
  Skill,
  SymbolMatch,
  ToolGroup,
  Trace,
  UpdateChannel,
  UpdateStatus,
} from './types';

/** Error message of a failed command. */
export function errorText(error: unknown): string {
  if (typeof error === 'string') return error;
  if (error instanceof Error) return error.message;
  return String(error);
}

export const api = {
  // App
  appInfo: () => invoke<AppInfo>('app_info'),
  acceptDisclaimer: () => invoke<void>('accept_disclaimer'),
  getSettings: () => invoke<Settings>('get_settings'),
  updateSettings: (patch: Partial<Settings>) => invoke<Settings>('update_settings', { patch }),
  windowReady: () => invoke<void>('window_ready'),
  openPath: (path: string) => invoke<void>('open_path', { path }),
  revealPath: (path: string) => invoke<void>('reveal_path', { path }),
  readWorkspaceFile: (path: string, maxBytes?: number) => invoke<string>('read_workspace_file', { path, maxBytes }),
  listToolGroups: () => invoke<ToolGroup[]>('list_tool_groups'),
  setToolGroup: (id: string, enabled: boolean) => invoke<ToolGroup[]>('set_tool_group', { id, enabled }),
  revokeToolPermission: (name: string) => invoke<Settings>('revoke_tool_permission', { name }),

  // Chats
  listChats: () => invoke<Chat[]>('list_chats'),
  getMessages: (chatId: string) => invoke<Message[]>('get_messages', { chatId }),
  /** Reads a file into the staging area; rejects for folders, missing files and files over 100 MB. */
  attachFile: (path: string) => invoke<Attachment>('attach_file', { path }),
  /** The same for bytes without a file on disk (a pasted screenshot); the name travels URI-encoded. */
  attachData: (name: string, bytes: Uint8Array) =>
    invoke<Attachment>('attach_data', bytes, { headers: { 'x-name': encodeURIComponent(name) } }),
  /** Drops a staged file the person removed before sending. */
  discardAttachment: (id: string) => invoke<void>('discard_attachment', { id }),
  sendMessage: (chatId: string | null, text: string, modelId: string, attachmentIds: string[]) =>
    invoke<{ chat: Chat; message: Message }>('send_message', { chatId, text, modelId, attachmentIds }),
  regenerate: (chatId: string, modelId: string) => invoke<void>('regenerate', { chatId, modelId }),
  editMessage: (chatId: string, messageId: string, text: string, modelId: string) =>
    invoke<Message>('edit_message', { chatId, messageId, text, modelId }),
  stopTurn: (chatId: string) => invoke<void>('stop_turn', { chatId }),
  runningTurns: () => invoke<string[]>('running_turns'),
  resolveApproval: (messageId: string, decision: ApprovalDecision) =>
    invoke<boolean>('resolve_approval', { messageId, decision }),
  /** Ends a running tool call early (a command's Stop); the turn goes on with what it printed. */
  stopTool: (messageId: string) => invoke<boolean>('stop_tool', { messageId }),
  renameChat: (chatId: string, title: string) => invoke<Chat>('rename_chat', { chatId, title }),
  pinChat: (chatId: string, pinned: boolean) => invoke<Chat>('pin_chat', { chatId, pinned }),
  deleteChat: (chatId: string) => invoke<void>('delete_chat', { chatId }),
  getTrace: (messageId: string) => invoke<Trace | null>('get_trace', { messageId }),
  chatTraces: (chatId: string) => invoke<Trace[]>('chat_traces', { chatId }),
  workspaceDir: (chatId: string) => invoke<string>('workspace_dir', { chatId }),

  // Models
  listModels: () => invoke<ModelEntry[]>('list_models'),
  rescanModels: () => invoke<ModelEntry[]>('rescan_models'),
  updateModel: (id: string, settings: ModelSettings) => invoke<ModelEntry>('update_model', { id, settings }),
  setModelsEnabled: (ids: string[], enabled: boolean) => invoke<ModelEntry[]>('set_models_enabled', { ids, enabled }),
  importModelAvatar: (id: string, source: string) => invoke<ModelEntry>('import_model_avatar', { id, source }),
  setDefaultModel: (id: string) => invoke<ModelEntry[]>('set_default_model', { id }),
  deleteModel: (id: string) => invoke<ModelEntry[]>('delete_model', { id }),
  runtimeStatus: () => invoke<RuntimeStatus>('runtime_status'),
  runtimeLogs: () => invoke<string[]>('runtime_logs'),
  loadModel: (id: string) => invoke<RuntimeStatus>('load_model', { id }),
  unloadModel: () => invoke<RuntimeStatus>('unload_model'),
  hfSearch: (query: string) => invoke<HfRepo[]>('hf_search', { query }),
  hfRepoFiles: (repo: string) => invoke<HfRepoFiles>('hf_repo_files', { repo }),
  recommendedModels: () => invoke<Recommendation[]>('recommended_models'),
  downloadModel: (repo: string, file: HfModelFile) =>
    invoke<DownloadJob>('download_model', {
      spec: {
        repo,
        name: file.name,
        quant: file.quant,
        paths: file.paths,
        sizes: file.sizes,
        sha256: file.sha256,
      },
    }),
  searchStatus: () => invoke<SearchStatus>('search_status'),
  downloadSearchModel: () => invoke<DownloadJob>('download_search_model'),
  listDownloads: () => invoke<DownloadJob[]>('list_downloads'),
  pauseDownload: (id: string) => invoke<void>('pause_download', { id }),
  resumeDownload: (id: string) => invoke<void>('resume_download', { id }),
  cancelDownload: (id: string) => invoke<void>('cancel_download', { id }),
  clearDownloads: () => invoke<DownloadJob[]>('clear_downloads'),
  hasHfToken: () => invoke<boolean>('has_hf_token'),
  setHfToken: (token: string | null) => invoke<boolean>('set_hf_token', { token }),
  modelFolders: () => invoke<ModelFolder[]>('model_folders'),
  suggestedModelFolders: () => invoke<string[]>('suggested_model_folders'),
  addModelFolder: (path: string) => invoke<ModelEntry[]>('add_model_folder', { path }),
  removeModelFolder: (path: string) => invoke<ModelEntry[]>('remove_model_folder', { path }),

  // Providers
  listProviders: () => invoke<ProviderView[]>('list_providers'),
  addProvider: (apiKey: string, name?: string) =>
    invoke<ProviderView>('add_provider', { kind: 'gemini', name: name ?? null, apiKey }),
  updateProvider: (id: string, patch: { name?: string; enabled?: boolean; baseUrl?: string; apiKey?: string }) =>
    invoke<ProviderView>('update_provider', { id, patch }),
  refreshProvider: (id: string) => invoke<ProviderView>('refresh_provider', { id }),
  removeProvider: (id: string) => invoke<void>('remove_provider', { id }),

  // Skills
  listSkills: () => invoke<Skill[]>('list_skills'),
  setSkillEnabled: (id: string, enabled: boolean) => invoke<Skill[]>('set_skill_enabled', { id, enabled }),
  deleteSkill: (id: string) => invoke<Skill[]>('delete_skill', { id }),
  readSkillFile: (id: string, path: string) => invoke<string>('read_skill_file', { id, path }),
  writeSkillFile: (id: string, path: string, content: string) =>
    invoke<Skill>('write_skill_file', { id, path, content }),
  createSkill: (name: string, description: string, instructions: string) =>
    invoke<Skill>('create_skill', { name, description, instructions }),
  openSkillsFolder: (id?: string) => invoke<void>('open_skills_folder', { id: id ?? null }),

  // Market
  marketStatus: () => invoke<MarketStatus>('market_status'),
  marketLogin: () => invoke<void>('market_login'),
  marketLogout: () => invoke<MarketStatus>('market_logout'),
  marketSearch: (query: string, kind?: string) => invoke<SymbolMatch[]>('market_search', { query, kind: kind ?? null }),
  marketQuote: (symbols: string[]) => invoke<Quote[]>('market_quote', { symbols }),
  /** `keys` names the store keys the stream records into (what `store.updated` and jobs match on). */
  marketOpenStream: (symbol: string, timeframe: string, bars?: number) =>
    invoke<{ id: string; info: ChartInfo; bars: Bar[]; keys?: MarketKeys }>('market_open_stream', {
      symbol,
      timeframe,
      bars,
    }),
  marketCloseStream: (streamId: string) => invoke<void>('market_close_stream', { streamId }),

  // Market data store: bars read from what is stored, downloads that fill it, and what it holds.
  // Times are seconds. `bars.older` never touches the network.
  marketBarsLatest: (symbol: string, timeframe: string, count: number) =>
    invoke<MarketLatestBars>('market_bars_latest', { symbol, timeframe, count }),
  marketBarsOlder: (symbol: string, timeframe: string, before: number, count: number) =>
    invoke<MarketBarsPage>('market_bars_older', { symbol, timeframe, before, count }),
  marketBarsFreshen: (symbol: string, timeframe: string) =>
    invoke<{ requests: number }>('market_bars_freshen', { symbol, timeframe }),
  marketPlanDownload: (symbol: string, range: MarketRange = {}) =>
    invoke<MarketPlan>('market_download_plan', {
      symbol,
      from: range.from ?? null,
      back: range.back ?? null,
      to: range.to ?? null,
      tiers: range.tiers ?? null,
    }),
  /** Starts (or joins) a download; `plan` is for the work this call adds. */
  marketStartDownload: (symbol: string, range: MarketRange, origin: MarketOrigin) =>
    invoke<{ jobId: string; plan: MarketPlan }>('market_download_start', {
      symbol,
      from: range.from ?? null,
      back: range.back ?? null,
      to: range.to ?? null,
      tiers: range.tiers ?? null,
      origin,
    }),
  marketPauseDownload: (jobId: string) => invoke<void>('market_download_pause', { jobId }),
  marketResumeDownload: (jobId: string) => invoke<void>('market_download_resume', { jobId }),
  /** Removes the job record; the data it fetched stays. */
  marketCancelDownload: (jobId: string) => invoke<void>('market_download_cancel', { jobId }),
  /** Null once the job was cancelled or removed. */
  marketGetDownload: (jobId: string) => invoke<MarketJob | null>('market_download_status', { jobId }),
  marketListDownloads: (symbol?: string) => invoke<MarketJob[]>('market_download_list', { symbol: symbol ?? null }),
  marketCacheSummary: (symbol?: string) =>
    invoke<MarketCacheSummary>('market_cache_summary', { symbol: symbol ?? null }),
  /** Refused while a job for the market runs. */
  marketCacheDelete: (market: string) => invoke<{ bytes: number }>('market_cache_delete', { market }),
  /** Forgets learned starts and "unavailable" answers, so older data is looked for again. */
  marketCacheRecheck: (market: string) => invoke<void>('market_cache_recheck', { market }),

  // Updates
  updateStatus: () => invoke<UpdateStatus>('update_status'),
  /** Resolves once the check is done; a failure shows in the status, not as a rejection. */
  checkForUpdates: () => invoke<UpdateStatus>('check_for_updates'),
  /** "Update" / "Restart and update": installs what is ready, or downloads what was found (never installing it by itself). */
  applyUpdate: () => invoke<UpdateStatus>('apply_update'),
  cancelUpdate: () => invoke<UpdateStatus>('cancel_update'),
  setUpdatePreferences: (prefs: { channel?: UpdateChannel; auto?: boolean }) =>
    invoke<UpdateStatus>('set_update_preferences', { channel: prefs.channel ?? null, auto: prefs.auto ?? null }),
};
