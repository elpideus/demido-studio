import { create } from 'zustand'
import { load as loadStore } from '@tauri-apps/plugin-store'
import { skills as skillsApi } from '../lib/tauri'

export interface SkillEntry {
  id: string
  name: string
  description: string
  version: string
  commands: { name: string; description: string; file?: string }[]
  content: string
  enabled: boolean
}

interface SkillsStore {
  skills: SkillEntry[]
  load: () => Promise<void>
  toggle: (id: string) => Promise<void>
  delete: (id: string) => Promise<void>
  enabledContext: () => string
}

let _storePromise: ReturnType<typeof loadStore> | null = null
function getStore() {
  if (!_storePromise) _storePromise = loadStore('prefs.json', { defaults: {}, autoSave: true })
  return _storePromise
}

export const useSkills = create<SkillsStore>((set, get) => ({
  skills: [],

  load: async () => {
    const raw = await skillsApi.list()
    const store = await getStore()
    const saved = (await store.get<Record<string, boolean>>('skill_enabled')) ?? {}
    const skills: SkillEntry[] = raw.map(s => ({ ...s, enabled: saved[s.id] ?? true }))
    set({ skills })
  },

  delete: async (id) => {
    await skillsApi.delete(id)
    const updated = get().skills.filter(s => s.id !== id)
    const saved: Record<string, boolean> = {}
    updated.forEach(s => { saved[s.id] = s.enabled })
    const store = await getStore()
    await store.set('skill_enabled', saved)
    set({ skills: updated })
  },

  toggle: async (id) => {
    const updated = get().skills.map(s => s.id === id ? { ...s, enabled: !s.enabled } : s)
    const saved: Record<string, boolean> = {}
    updated.forEach(s => { saved[s.id] = s.enabled })
    const store = await getStore()
    await store.set('skill_enabled', saved)
    set({ skills: updated })
  },

  enabledContext: () => {
    const enabled = get().skills.filter(s => s.enabled && s.content)
    if (!enabled.length) return ''
    return enabled.map(s => `# Skill: ${s.name}\n\n${s.content}`).join('\n\n---\n\n')
  },
}))
