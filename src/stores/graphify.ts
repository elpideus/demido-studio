import { create } from 'zustand'
import { listen } from '@tauri-apps/api/event'
import { graphify as api, type GraphifyStatus } from '../lib/tauri'

type QueryKind = 'query' | 'path' | 'explain'

interface GraphifyState {
  /** Latest status per working folder. Keyed by absolute folder path. */
  statusByFolder: Record<string, GraphifyStatus>
  /** Cached inlined graph HTML per folder — avoids re-fetch + regex re-processing on every window open. */
  htmlByFolder: Record<string, string>
  /** Cached vis-network node positions per folder — re-applied so a reopened graph paints instantly. */
  positionsByFolder: Record<string, Record<string, { x: number; y: number }>>
  installing: boolean
  installStage: string
  building: boolean
  buildLog: string[]

  /** Wire up install/build progress event listeners exactly once. */
  ensureListeners: () => void
  refreshStatus: (folder: string) => Promise<GraphifyStatus | undefined>
  install: () => Promise<void>
  /** Build (or refresh) the graph, then refresh status for the folder. */
  build: (folder: string, update: boolean) => Promise<void>
  query: (folder: string, kind: QueryKind, args: string[]) => Promise<string>
  graphHtml: (folder: string) => Promise<string>
  /** Cache the settled node positions reported by the graph iframe (memory + disk). */
  setPositions: (folder: string, positions: Record<string, { x: number; y: number }>) => void
  /** Hydrate positions for a folder from disk into memory if not already present. Returns them. */
  loadPositions: (folder: string) => Promise<Record<string, { x: number; y: number }> | undefined>
  /** Toggle the per-folder "auto-build graph on new projects" preference, then refresh status. */
  setAutoBuild: (folder: string, enabled: boolean) => Promise<void>
}

let listenersReady = false

export const useGraphify = create<GraphifyState>((set, get) => ({
  statusByFolder: {},
  htmlByFolder: {},
  positionsByFolder: {},
  installing: false,
  installStage: '',
  building: false,
  buildLog: [],

  ensureListeners: () => {
    if (listenersReady) return
    listenersReady = true
    listen<{ stage: string }>('graphify_install_progress', e => {
      set({ installStage: e.payload.stage })
    })
    listen<{ line: string }>('graphify_build_progress', e => {
      set(s => ({ buildLog: [...s.buildLog, e.payload.line].slice(-500) }))
    })
  },

  refreshStatus: async (folder) => {
    if (!folder) return undefined
    try {
      const status = await api.status(folder)
      set(s => ({ statusByFolder: { ...s.statusByFolder, [folder]: status } }))
      return status
    } catch {
      return undefined
    }
  },

  install: async () => {
    get().ensureListeners()
    set({ installing: true, installStage: 'starting' })
    try {
      await api.install()
    } finally {
      set({ installing: false })
    }
  },

  build: async (folder, update) => {
    get().ensureListeners()
    set({ building: true, buildLog: [] })
    try {
      await api.build(folder, update)
      // Graph changed — drop cached HTML and stale node positions so the next view re-fetches
      // the fresh render and re-derives the layout.
      set(s => {
        const { [folder]: _h, ...html } = s.htmlByFolder
        const { [folder]: _p, ...positions } = s.positionsByFolder
        return { htmlByFolder: html, positionsByFolder: positions }
      })
      await get().refreshStatus(folder)
    } finally {
      set({ building: false })
    }
  },

  query: (folder, kind, args) => api.query(folder, kind, args),
  graphHtml: async (folder) => {
    const cached = get().htmlByFolder[folder]
    if (cached !== undefined) return cached
    const html = await api.graphHtml(folder)
    set(s => ({ htmlByFolder: { ...s.htmlByFolder, [folder]: html } }))
    return html
  },

  setPositions: (folder, positions) => {
    set(s => ({ positionsByFolder: { ...s.positionsByFolder, [folder]: positions } }))
    // Persist so the layout survives an app restart (and an early window close). Fire-and-forget.
    api.setPositions(folder, positions).catch(() => {})
  },

  loadPositions: async (folder) => {
    const existing = get().positionsByFolder[folder]
    if (existing) return existing
    try {
      const p = await api.getPositions(folder)
      if (p) {
        set(s => ({ positionsByFolder: { ...s.positionsByFolder, [folder]: p } }))
        return p
      }
    } catch { /* no cache on disk yet — fall back to stabilization */ }
    return undefined
  },

  setAutoBuild: async (folder, enabled) => {
    // Optimistic: flip the cached status immediately so the toggle feels instant.
    set(s => {
      const cur = s.statusByFolder[folder]
      return cur ? { statusByFolder: { ...s.statusByFolder, [folder]: { ...cur, autoBuild: enabled } } } : {}
    })
    await api.setAutoBuild(folder, enabled)
    await get().refreshStatus(folder)
  },
}))
