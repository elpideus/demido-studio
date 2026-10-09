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
  ChartLayout,
  ChartLayoutEntry,
  ContextUsage,
  DownloadJob,
  HfRepo,
  HfRepoFiles,
  HfModelFile,
  HfProjector,
  IndicatorCatalog,
  IndicatorEntry,
  IndicatorMeta,
  IndicatorSetup,
  MailAccount,
  MailDateRange,
  MailFolder,
  MailMessage,
  MailPage,
  MailSummary,
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
  ModelGroup,
  ModelSettings,
  NewMailAccount,
  PineCheck,
  PinePublished,
  PineScript,
  PineSummary,
  ProviderKind,
  ProviderUsage,
  ProviderView,
  Quote,
  Recommendation,
  RuntimeStatus,
  Settings,
  Skill,
  SlashCommand,
  SlashOutcome,
  SpeechChoice,
  SymbolMatch,
  ToolGroup,
  Trace,
  Transcript,
  TranscriptionRecord,
  UpdateChannel,
  UpdateStatus,
  VoiceStatus,
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
  listSlashCommands: () => invoke<SlashCommand[]>('list_slash_commands'),
  /** Runs `/name args`; built-in commands leave the staged files staged. */
  runSlashCommand: (chatId: string | null, text: string, modelId: string, attachmentIds: string[]) =>
    invoke<SlashOutcome>('run_slash_command', { chatId, text, modelId, attachmentIds }),
  runningTurns: () => invoke<string[]>('running_turns'),
  /** How full the context window of `chatId` (a new chat when null) is with `modelId`. */
  contextUsage: (chatId: string | null, modelId: string) => invoke<ContextUsage>('context_usage', { chatId, modelId }),
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

  // Voice. A recording is a 16 kHz mono WAV, sent as the raw body.
  /** How a recording for `modelId` goes; `warm` starts loading the speech model when it will be needed. */
  voiceStatus: (modelId: string | null, warm = false) => invoke<VoiceStatus>('voice_status', { modelId, warm }),
  /** Writes down a dictation; the words arrive on `voice://text` as they are written. */
  transcribeRecording: (wav: Uint8Array, job: string, chatId: string | null) =>
    invoke<Transcript>('transcribe_recording', wav, { headers: { 'x-job': job, 'x-chat': chatId ?? '' } }),
  cancelTranscription: (job: string) => invoke<boolean>('cancel_transcription', { job }),
  /** Stages a recording as a voice note, for a model that hears. */
  attachVoiceNote: (wav: Uint8Array) => invoke<Attachment>('attach_voice_note', wav),
  /** Writes down a voice note sent earlier and returns it with its transcript. */
  transcribeVoiceNote: (id: string) => invoke<Attachment>('transcribe_voice_note', { id }),
  downloadSpeechModel: () => invoke<DownloadJob>('download_speech_model'),
  openMicrophoneSettings: () => invoke<void>('open_microphone_settings'),
  chatTranscriptions: (chatId: string) => invoke<TranscriptionRecord[]>('chat_transcriptions', { chatId }),
  speechModels: () => invoke<SpeechChoice[]>('speech_models'),

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
  downloadModel: (repo: string, file: HfModelFile, projector: HfProjector | null) =>
    invoke<DownloadJob>('download_model', {
      spec: {
        repo,
        name: file.name,
        quant: file.quant,
        paths: file.paths,
        sizes: file.sizes,
        sha256: file.sha256,
        projector,
      },
    }),
  findProjector: (id: string) => invoke<HfProjector | null>('find_projector', { id }),
  downloadProjector: (id: string) => invoke<DownloadJob>('download_projector', { id }),
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
  providerUsage: () => invoke<ProviderUsage[]>('provider_usage'),
  addProvider: (kind: ProviderKind, apiKey: string, name?: string, modelGroup?: ModelGroup) =>
    invoke<ProviderView>('add_provider', { kind, name: name ?? null, apiKey, modelGroup: modelGroup ?? null }),
  updateProvider: (
    id: string,
    patch: { name?: string; enabled?: boolean; baseUrl?: string; apiKey?: string; modelGroup?: ModelGroup },
  ) => invoke<ProviderView>('update_provider', { id, patch }),
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
  // Mail: reading only. `mailMessages` answers from the cache; `mailSync` asks the server for
  // what changed, announced with `mail://changed`.
  mailAccounts: () => invoke<MailAccount[]>('mail_accounts'),
  mailAddAccount: (account: NewMailAccount) => invoke<MailAccount>('mail_add_account', { account }),
  mailRemoveAccount: (id: string) => invoke<void>('mail_remove_account', { id }),
  /** Names the account; an empty name takes its name away. */
  mailRenameAccount: (id: string, nickname: string) => invoke<MailAccount>('mail_rename_account', { id, nickname }),
  mailFolders: (account: string, refresh = false) => invoke<MailFolder[]>('mail_folders', { account, refresh }),
  mailMessages: (
    account: string,
    folder: string,
    limit: number,
    filter: string | null,
    unreadOnly: boolean,
    dates: MailDateRange | null = null,
  ) =>
    invoke<MailPage>('mail_messages', {
      account,
      folder,
      limit,
      filter,
      unreadOnly,
      since: dates?.since ?? null,
      until: dates?.until ?? null,
    }),
  mailSync: (account: string, folder: string, force = false) =>
    invoke<boolean>('mail_sync', { account, folder, force }),
  mailLoadOlder: (account: string, folder: string) => invoke<number>('mail_load_older', { account, folder }),
  /** Searches the folder on the server for `query`, mail that arrived within `dates`, or both. */
  mailSearch: (account: string, folder: string, query: string, dates: MailDateRange | null = null, limit?: number) =>
    invoke<MailSummary[]>('mail_search', {
      account,
      folder,
      query,
      since: dates?.since ?? null,
      until: dates?.until ?? null,
      limit: limit ?? null,
    }),
  mailOpen: (id: number) => invoke<MailMessage>('mail_open', { id }),
  mailSaveAttachment: (id: number, section: string, path: string) =>
    invoke<void>('mail_save_attachment', { id, section, path }),
  mailSetWatching: (open: boolean) => invoke<void>('mail_set_watching', { open }),

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
  /** More history on a live chart's TradingView session, so its indicators reach further back.
   *  Returns how many bars were asked for (0 once the session holds all it can). */
  marketExtendStream: (streamId: string, bars: number) =>
    invoke<{ asked: number }>('market_extend_stream', { streamId, bars }),

  // Chart indicators, computed by TradingView on a live stream. Their values arrive as
  // `indicator.data` / `indicator.graphics` events; `indicator.error` says one stopped.
  marketIndicatorCatalog: (refresh = false) => invoke<IndicatorCatalog>('market_indicator_catalog', { refresh }),
  /** Community scripts, most popular first. */
  marketIndicatorSearch: (query: string) => invoke<IndicatorEntry[]>('market_indicator_search', { query }),
  marketIndicatorLayouts: () => invoke<ChartLayoutEntry[]>('market_indicator_layouts'),
  marketIndicatorLayout: (layoutId: string) => invoke<ChartLayout>('market_indicator_layout', { layoutId }),
  /** Rejects with the plan's limit (STUDY_LIMIT) when the chart has no room for another one. */
  marketIndicatorAdd: (streamId: string, script: string, version: string | null, setup: IndicatorSetup) =>
    invoke<{ id: string; meta: IndicatorMeta }>('market_indicator_add', { streamId, script, version, setup }),
  marketIndicatorRemove: (indicatorId: string) => invoke<void>('market_indicator_remove', { indicatorId }),

  // Pine scripts written in Demido, kept on this computer; `pine.changed` events follow every
  // change. TradingView compiles and runs them; only `marketPinePublish` changes the account.
  marketPineList: () => invoke<PineSummary[]>('market_pine_list'),
  marketPineGet: (id: string) => invoke<PineScript>('market_pine_get', { id }),
  /** Creates a script (no `id`) or replaces one's source. */
  marketPineSave: (id: string | null, source: string, name?: string) =>
    invoke<PineScript>('market_pine_save', { id, source, name: name ?? null }),
  marketPineDelete: (id: string) => invoke<void>('market_pine_delete', { id }),
  /** Compiles with TradingView's compiler without saving anything. */
  marketPineCheck: (source: string) => invoke<PineCheck>('market_pine_check', { source }),
  /** Saves a library script to the user's TradingView account: a new script the first time, the
   *  next version of the same one afterwards. Ask first. */
  marketPinePublish: (id: string, name?: string) =>
    invoke<PinePublished>('market_pine_publish', { id, name: name ?? null }),
  /** Copies a TradingView script's source into the library (the user's own stay linked to it). */
  marketPineImport: (script: string) =>
    invoke<{ script: PineSummary; updated: boolean }>('market_pine_import', { script }),

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
