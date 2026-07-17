import { create } from 'zustand'

export interface BuiltinTool {
  id: string
  name: string
  description: string
  group: string
  enabled: boolean
}

const STORAGE_KEY = 'demido:builtinTools'
const OVERRIDES_KEY = 'demido:builtinGroupOverrides'

const DEFAULT_TOOLS: BuiltinTool[] = [
  { id: 'web_search',          name: 'web_search',          description: 'Search the web via DuckDuckGo (up to 15 results)',         group: 'Web Browse', enabled: true },
  { id: 'web_fetch',           name: 'web_fetch',           description: 'Fetch and extract text from a web page URL',               group: 'Web Browse', enabled: true },
  { id: 'list_emails',         name: 'list_emails',         description: 'Search or list emails from connected Gmail account',        group: 'Email',      enabled: true },
  { id: 'read_email',          name: 'read_email',          description: 'Read the full body of an email by ID',                     group: 'Email',      enabled: true },
  { id: 'list_calendar_events',name: 'list_calendar_events',description: 'List upcoming events from connected Google Calendar',      group: 'Calendar',   enabled: true },
  { id: 'list_contacts',       name: 'list_contacts',       description: 'Search or list contacts from connected Google account',     group: 'Contacts',   enabled: true },
  { id: 'read_contact',        name: 'read_contact',        description: 'Read the full details of a contact by ID',                  group: 'Contacts',   enabled: true },
]

/** DEFAULT_TOOLS must list every tool `web_tool_defs` + `google_tool_defs` return
 *  (`src-tauri/src/agent/mod.rs`) — i.e. the ones offered unconditionally, whatever the agent mode.
 *  Anything missing here is unreachable by `disabledKeys` below,
 *  so it stays switched on however the user sets Tools — that is how `read_contact` survived
 *  Contacts being turned off. `builtinTools.test.ts` reads the Rust source and enforces this.
 *
 *  `skills_tool_defs` (`install_skill`/`delete_skill`) is deliberately absent: those are not
 *  offered on their own any more. `skill-manager` claims them in its `tools.json`, so its toggle
 *  is their switch and they render under it — not here. */
export { DEFAULT_TOOLS }

function loadState(): Record<string, boolean> {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    return raw ? JSON.parse(raw) : {}
  } catch {
    return {}
  }
}

function loadOverrides(): Record<string, Record<string, boolean>> {
  try {
    const raw = localStorage.getItem(OVERRIDES_KEY)
    return raw ? JSON.parse(raw) : {}
  } catch {
    return {}
  }
}

function saveState(tools: BuiltinTool[]) {
  try {
    const state: Record<string, boolean> = {}
    tools.forEach(t => { state[t.id] = t.enabled })
    localStorage.setItem(STORAGE_KEY, JSON.stringify(state))
  } catch {}
}

function saveOverrides(overrides: Record<string, Record<string, boolean>>) {
  try {
    localStorage.setItem(OVERRIDES_KEY, JSON.stringify(overrides))
  } catch {}
}

function applyState(tools: BuiltinTool[], state: Record<string, boolean>): BuiltinTool[] {
  return tools.map(t => ({ ...t, enabled: state[t.id] ?? t.enabled }))
}

interface BuiltinToolsStore {
  tools: BuiltinTool[]
  groupOverrides: Record<string, Record<string, boolean>>
  toggle: (id: string) => void
  toggleGroup: (group: string) => void
  disabledKeys: () => string[]
}

export const useBuiltinTools = create<BuiltinToolsStore>((set, get) => ({
  tools: applyState(DEFAULT_TOOLS, loadState()),
  groupOverrides: loadOverrides(),

  toggle: (id) => {
    const { tools, groupOverrides } = get()
    const tool = tools.find(t => t.id === id)
    // If group is overridden, toggling a tool first lifts the override
    if (tool && groupOverrides[tool.group]) {
      const newOverrides = { ...groupOverrides }
      delete newOverrides[tool.group]
      const updated = tools.map(t => t.id === id ? { ...t, enabled: !t.enabled } : t)
      saveState(updated)
      saveOverrides(newOverrides)
      set({ tools: updated, groupOverrides: newOverrides })
    } else {
      const updated = tools.map(t => t.id === id ? { ...t, enabled: !t.enabled } : t)
      saveState(updated)
      set({ tools: updated })
    }
  },

  toggleGroup: (group) => {
    const { tools, groupOverrides } = get()
    const isOverridden = !!groupOverrides[group]
    let updated: BuiltinTool[]
    let newOverrides: Record<string, Record<string, boolean>>

    if (isOverridden) {
      const snapshot = groupOverrides[group]
      updated = tools.map(t => t.group === group ? { ...t, enabled: snapshot[t.id] ?? true } : t)
      newOverrides = { ...groupOverrides }
      delete newOverrides[group]
    } else {
      const snapshot: Record<string, boolean> = {}
      tools.filter(t => t.group === group).forEach(t => { snapshot[t.id] = t.enabled })
      updated = tools.map(t => t.group === group ? { ...t, enabled: false } : t)
      newOverrides = { ...groupOverrides, [group]: snapshot }
    }

    saveState(updated)
    saveOverrides(newOverrides)
    set({ tools: updated, groupOverrides: newOverrides })
  },

  disabledKeys: () => get().tools.filter(t => !t.enabled).map(t => `builtin:${t.id}`),
}))
