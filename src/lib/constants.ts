import type { McpToolEntry } from '../stores/mcpTools'

/** Sentinel prefix for assistant tool-call messages stored in DB.
 *  Messages starting with this string are hidden from the chat UI. */
export const TOOL_CALLS_CONTENT_PREFIX = '{"__tool_calls__"'

/** Returns the qualified key for an MCP tool, scoped to its server. */
export const toolKey = (t: Pick<McpToolEntry, 'server_id' | 'name'>) =>
  `${t.server_id}:${t.name}`
