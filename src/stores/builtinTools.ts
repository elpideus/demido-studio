import { create } from 'zustand'

export interface BuiltinTool {
  id: string
  name: string
  description: string
  group: string
  enabled: boolean
}

const STORAGE_KEY = 'demido:builtinTools'

const DEFAULT_TOOLS: BuiltinTool[] = [
  { id: 'web_search',          name: 'web_search',          description: 'Search the web via DuckDuckGo (up to 15 results)',         group: 'Web Browse', enabled: true },
  { id: 'web_fetch',           name: 'web_fetch',           description: 'Fetch and extract text from a web page URL',               group: 'Web Browse', enabled: true },
  { id: 'list_emails',         name: 'list_emails',         description: 'Search or list emails from connected Gmail account',        group: 'Email',      enabled: true },
  { id: 'get_email',           name: 'get_email',           description: 'Read the full body of an email by ID',                     group: 'Email',      enabled: true },
  { id: 'list_calendar_events',name: 'list_calendar_events',description: 'List upcoming events from connected Google Calendar',      group: 'Calendar',   enabled: true },
  { id: 'list_contacts',       name: 'list_contacts',       description: 'Search or list contacts from connected Google account',     group: 'Contacts',   enabled: true },
]

function loadState(): Record<string, boolean> {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    return raw ? JSON.parse(raw) : {}
  } catch {
    return {}
  }
}

function saveState(state: Record<string, boolean>) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(state))
  } catch {}
}

function applyState(tools: BuiltinTool[], state: Record<string, boolean>): BuiltinTool[] {
  return tools.map(t => ({ ...t, enabled: state[t.id] ?? t.enabled }))
}

interface BuiltinToolsStore {
  tools: BuiltinTool[]
  toggle: (id: string) => void
  disabledKeys: () => string[]
}

export const useBuiltinTools = create<BuiltinToolsStore>((set, get) => ({
  tools: applyState(DEFAULT_TOOLS, loadState()),

  toggle: (id) => {
    const tools = get().tools.map(t => t.id === id ? { ...t, enabled: !t.enabled } : t)
    const state: Record<string, boolean> = {}
    tools.forEach(t => { state[t.id] = t.enabled })
    saveState(state)
    set({ tools })
  },

  disabledKeys: () => get().tools.filter(t => !t.enabled).map(t => `builtin:${t.id}`),
}))
