import { create } from 'zustand'
import { load as loadStore } from '@tauri-apps/plugin-store'
import { mcp } from '../lib/tauri'

export interface McpToolEntry {
  server_id: string
  server_name: string
  name: string
  description: string
  enabled: boolean
}

interface ServerOverride {
  snapshot: Record<string, boolean>
}

interface McpToolsStore {
  tools: McpToolEntry[]
  collapsed: Record<string, boolean>
  serverOverrides: Record<string, ServerOverride>
  load: () => Promise<void>
  toggleTool: (toolKey: string) => Promise<void>
  toggleServer: (serverId: string) => Promise<void>
  toggleCollapse: (serverId: string) => void
  enabledTools: () => McpToolEntry[]
}

let _storePromise: ReturnType<typeof loadStore> | null = null

function getStore() {
  if (!_storePromise) {
    _storePromise = loadStore('prefs.json', { defaults: {}, autoSave: true })
  }
  return _storePromise
}

export const useMcpTools = create<McpToolsStore>((set, get) => ({
  tools: [],
  collapsed: {},
  serverOverrides: {},

  load: async () => {
    const rawTools = await mcp.listTools()
    const store = await getStore()

    const savedEnabled = (await store.get<Record<string, boolean>>('mcp_tool_enabled')) ?? {}
    const savedOverrides = (await store.get<Record<string, ServerOverride>>('mcp_server_overrides')) ?? {}

    const tools: McpToolEntry[] = rawTools.map(t => ({
      ...t,
      enabled: savedEnabled[`${t.server_id}:${t.name}`] ?? true,
    }))

    set({ tools, serverOverrides: savedOverrides })
  },

  toggleTool: async (toolKey) => {
    const { tools, serverOverrides } = get()
    const updated = tools.map(t => {
      const key = `${t.server_id}:${t.name}`
      return key === toolKey ? { ...t, enabled: !t.enabled } : t
    })
    const savedEnabled: Record<string, boolean> = {}
    updated.forEach(t => { savedEnabled[`${t.server_id}:${t.name}`] = t.enabled })

    const store = await getStore()
    await store.set('mcp_tool_enabled', savedEnabled)

    // If this tool's server was overridden OFF, update the snapshot too
    const tool = updated.find(t => `${t.server_id}:${t.name}` === toolKey)
    if (tool && serverOverrides[tool.server_id]) {
      const newOverrides = {
        ...serverOverrides,
        [tool.server_id]: {
          snapshot: {
            ...serverOverrides[tool.server_id].snapshot,
            [toolKey]: tool.enabled,
          },
        },
      }
      await store.set('mcp_server_overrides', newOverrides)
      set({ tools: updated, serverOverrides: newOverrides })
    } else {
      set({ tools: updated })
    }
  },

  toggleServer: async (serverId) => {
    const { tools, serverOverrides } = get()
    const store = await getStore()
    const isOverridden = !!serverOverrides[serverId]

    let updatedTools: McpToolEntry[]
    let updatedOverrides: Record<string, ServerOverride>

    if (isOverridden) {
      // Restore individual states from snapshot
      const snapshot = serverOverrides[serverId].snapshot
      updatedTools = tools.map(t =>
        t.server_id === serverId ? { ...t, enabled: snapshot[`${t.server_id}:${t.name}`] ?? true } : t
      )
      updatedOverrides = { ...serverOverrides }
      delete updatedOverrides[serverId]
    } else {
      // Snapshot current states, disable all tools in group
      const snapshot: Record<string, boolean> = {}
      tools.filter(t => t.server_id === serverId).forEach(t => {
        snapshot[`${t.server_id}:${t.name}`] = t.enabled
      })
      updatedTools = tools.map(t =>
        t.server_id === serverId ? { ...t, enabled: false } : t
      )
      updatedOverrides = { ...serverOverrides, [serverId]: { snapshot } }
    }

    const savedEnabled: Record<string, boolean> = {}
    updatedTools.forEach(t => { savedEnabled[`${t.server_id}:${t.name}`] = t.enabled })

    await store.set('mcp_tool_enabled', savedEnabled)
    await store.set('mcp_server_overrides', updatedOverrides)
    set({ tools: updatedTools, serverOverrides: updatedOverrides })
  },

  toggleCollapse: (serverId) => {
    set(s => ({
      collapsed: { ...s.collapsed, [serverId]: !s.collapsed[serverId] },
    }))
  },

  enabledTools: () => get().tools.filter(t => t.enabled),
}))
