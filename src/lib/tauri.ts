import { invoke as _invoke } from '@tauri-apps/api/core'

const isTauri = () => !!(window as any).__TAURI_INTERNALS__

function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) return Promise.reject(new Error(`[browser] invoke('${cmd}') skipped: no Tauri runtime`))
  return _invoke<T>(cmd, args)
}
import type { Conversation, Message, Provider, AppSettings, McpServer, ModelOverride, FileAttachment, LinkPreview } from '../types'

export const db = {
  listConversations: () => invoke<Conversation[]>('list_conversations'),
  createConversation: (providerId: string, modelId: string) =>
    invoke<Conversation>('create_conversation', { providerId, modelId }),
  deleteConversation: (id: string) => invoke<void>('delete_conversation', { id }),
  updateConversationTitle: (id: string, title: string) =>
    invoke<void>('update_conversation_title', { id, title }),

  listMessages: (conversationId: string) =>
    invoke<Message[]>('list_messages', { conversationId }),
  deleteMessagesAfter: (messageId: string) =>
    invoke<void>('delete_messages_after', { messageId }),
  deleteMessagesFrom: (messageId: string) =>
    invoke<void>('delete_messages_from', { messageId }),
  deleteMessage: (messageId: string) =>
    invoke<void>('delete_message', { messageId }),
  updateMessageContent: (messageId: string, content: string) =>
    invoke<void>('update_message_content', { messageId, content }),

  listProviders: () => invoke<Provider[]>('list_providers'),
  upsertProvider: (provider: Provider) => invoke<void>('upsert_provider', { provider }),
  deleteProvider: (id: string) => invoke<void>('delete_provider', { id }),
  listModelOverrides: (providerId: string) =>
    invoke<ModelOverride[]>('list_model_overrides', { providerId }),
  upsertModelOverride: (overrideEntry: ModelOverride) =>
    invoke<void>('upsert_model_override', { overrideEntry }),
  batchUpsertModelOverrides: (overrides: ModelOverride[]) =>
    invoke<void>('batch_upsert_model_overrides', { overrides }),

  getSettings: () => invoke<AppSettings>('get_settings'),
  setSetting: (key: string, value: unknown) =>
    invoke<void>('set_setting', { key, value: JSON.stringify(value) }),
  getSetting: (key: string, defaultValue?: string) =>
    invoke<string>('get_setting', { key, default: defaultValue }),

  getSecret: (key: string) => invoke<string | null>('get_secret', { key }),
  setSecret: (key: string, value: string) => invoke<void>('set_secret', { key, value }),

  searchConversations: (query: string) =>
    invoke<{ conversation_id: string; snippet: string }[]>('search_conversations', { query }),

  setAgentMode: (conversationId: string, mode: string) =>
    invoke<void>('set_agent_mode', { conversationId, mode }),
  setCavemanLevel: (conversationId: string, level: string) =>
    invoke<void>('set_caveman_level', { conversationId, level }),
  setWorkingDirectory: (conversationId: string, path: string | null) =>
    invoke<void>('set_working_directory', { conversationId, path }),
}

export const chat = {
  sendMessage: (
    conversationId: string,
    content: string,
    disabledTools: string[] = [],
    reasoningEffort?: string,
    providerId?: string,
    modelId?: string,
    attachments?: FileAttachment[],
    skillsContext?: string,
    historicalAttachments?: FileAttachment[],
    enabledSkills?: string[],
  ) =>
    invoke<void>('send_message', {
      req: { conversationId, content, disabledTools, reasoningEffort, providerId, modelId, attachments, skillsContext, historicalAttachments, enabledSkills },
    }),
  cancelStream: () => invoke<void>('cancel_stream'),
  continueGeneration: (
    conversationId: string,
    disabledTools?: string[],
    reasoningEffort?: string,
    providerId?: string,
    modelId?: string,
    skillsContext?: string,
    enabledSkills?: string[],
  ) => invoke<void>('continue_generation', {
    conversationId,
    disabledTools,
    reasoningEffort,
    providerId,
    modelId,
    skillsContext,
    enabledSkills,
  }),
}

/** Global system prompt — a file in app-data, not a settings row. See src-tauri/src/prompt.rs. */
export const systemPrompt = {
  get: () => invoke<string>('get_system_prompt'),
  set: (content: string) => invoke<void>('set_system_prompt', { content }),
  path: () => invoke<string>('get_system_prompt_path'),
  listVars: () => invoke<string[]>('list_prompt_vars'),
}

export const reasoning = {
  getModelReasoning: (providerId: string, modelId: string) =>
    invoke<{ allowedOptions: string[]; default: string } | null>(
      'get_model_reasoning', { providerId, modelId }
    ),
}

export const agent = {
  respondToPermission: (approved: boolean) =>
    invoke<void>('respond_to_permission', { approved }),
}

export const exportChat = {
  exportConversation: (conversationId: string, filePath: string) =>
    invoke<void>('export_conversation', { conversationId, filePath }),
}

export const skillMcp = {
  /** Report which skills are on so the backend spawns/kills their bundled MCP servers to match. */
  sync: (enabledSkills: string[]) =>
    invoke<void>('sync_skill_mcp_servers', { enabledSkills }),
}

export const skills = {
  list: () => invoke<{ id: string; name: string; description: string; version: string; commands: { name: string; description: string; file?: string; prompt?: string }[]; tools?: import('../stores/skills').SkillToolDef[]; metaJson: string; files: string[]; path: string }[]>('list_skills'),
  delete: (id: string) => invoke<void>('delete_skill', { id }),
  readFiles: (id: string) => invoke<import('../types').SkillFile[]>('read_skill_files', { id }),
  readCommand: (id: string, file: string) => invoke<string>('read_skill_command', { id, file }),
  writeFile: (id: string, file: string, content: string) =>
    invoke<void>('write_skill_file', { id, file, content }),
}

export const fs = {
  listDir: (conversationId: string, path: string) =>
    invoke<import('../types').FsEntry[]>('fs_list_dir', { conversationId, path }),
  readFile: (conversationId: string, path: string) =>
    invoke<string>('fs_read_file', { conversationId, path }),
  readFileBase64: (conversationId: string, path: string) =>
    invoke<string>('fs_read_file_base64', { conversationId, path }),
  saveFileBase64: (filename: string, data: string) =>
    invoke<void>('save_file_base64', { filename, data }),
  copyFileToClipboard: (data: string, filename: string) =>
    invoke<void>('copy_file_to_clipboard', { data, filename }),
  walk: (conversationId: string) =>
    invoke<import('../types').FsEntry[]>('fs_walk', { conversationId }),
  rename: (conversationId: string, path: string, newName: string) =>
    invoke<void>('fs_rename', { conversationId, path, newName }),
  delete: (conversationId: string, path: string) =>
    invoke<void>('fs_delete', { conversationId, path }),
  copyDir: (conversationId: string, srcPath: string, destDir: string) =>
    invoke<void>('fs_copy_dir', { conversationId, srcPath, destDir }),
}

export const google = {
  fetchEmails: (query?: string, maxResults?: number) =>
    invoke<{ emails: { id: string; subject: string; from: string; date: string; snippet: string }[]; next_page_token: string | null }>('fetch_emails', { query, maxResults }),
  fetchCalendarEvents: (daysAhead?: number, daysBehind?: number, maxResults?: number) =>
    invoke<{ id: string; summary: string; start: string; end: string; location: string | null; description: string | null }[]>('fetch_calendar_events', { daysAhead, daysBehind, maxResults }),
  fetchContacts: (query?: string, maxResults?: number) =>
    invoke<{ contacts: { id: string; display_name: string; emails: { value: string; label: string }[]; phones: { value: string; label: string }[] }[]; next_page_token: string | null }>('fetch_contacts', { query, maxResults }),
  getEmailBody: (id: string) =>
    invoke<string>('get_email_body', { id }),
}

export const links = {
  fetchPreviews: (urls: string[]) => invoke<LinkPreview[]>('fetch_link_previews', { urls }),
}

export interface GraphifyStatus {
  pythonReady: boolean
  installed: boolean
  graphBuilt: boolean
  /** Whether automatic graph building is enabled for this folder (default true). */
  autoBuild: boolean
}

export const graphify = {
  status: (folder: string) => invoke<GraphifyStatus>('graphify_status', { folder }),
  setAutoBuild: (folder: string, enabled: boolean) =>
    invoke<void>('graphify_set_auto_build', { folder, enabled }),
  install: () => invoke<void>('install_graphify'),
  uninstall: () => invoke<void>('uninstall_graphify'),
  build: (folder: string, update: boolean) => invoke<void>('build_graphify', { folder, update }),
  query: (folder: string, kind: 'query' | 'path' | 'explain', args: string[]) =>
    invoke<string>('query_graphify', { folder, kind, args }),
  graphHtml: (folder: string) => invoke<string>('graphify_graph_html', { folder }),
  getPositions: (folder: string) =>
    invoke<Record<string, { x: number; y: number }> | null>('graphify_get_positions', { folder }),
  setPositions: (folder: string, positions: Record<string, { x: number; y: number }>) =>
    invoke<void>('graphify_set_positions', { folder, positions }),
}

export const mcp = {
  listServers: () => invoke<McpServer[]>('list_mcp_servers'),
  saveServers: (servers: McpServer[]) =>
    invoke<void>('save_mcp_servers', { servers }),
  listTools: () => invoke<{ server_id: string; server_name: string; name: string; description: string }[]>('list_mcp_tools'),
  testServer: (server: McpServer) => invoke<number>('test_mcp_server', { server }),
}
