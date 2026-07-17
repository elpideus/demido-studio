import { create } from 'zustand'
import { db } from '../lib/tauri'
import type { AppSettings } from '../types'

interface SettingsStore {
  settings: AppSettings
  loaded: boolean
  load: () => Promise<void>
  update: <K extends keyof AppSettings>(key: K, value: AppSettings[K]) => Promise<void>
}

const defaults: AppSettings = {
  default_provider_id: '',
  default_model_id: '',
  auth_enabled: false,
  context_window_limit: 8192,
  task_provider_id: '',
  task_model_id: '',
  title_every_n_messages: 5,
}

export const useSettings = create<SettingsStore>((set) => ({
  settings: defaults,
  loaded: false,

  load: async () => {
    try {
      const settings = await db.getSettings()
      set({ settings, loaded: true })
    } catch (err) {
      console.error('[settings] load failed, using defaults:', err)
      set({ loaded: true })
    }
  },

  update: async (key, value) => {
    await db.setSetting(key, value)
    set(s => ({ settings: { ...s.settings, [key]: value } }))
  },
}))
