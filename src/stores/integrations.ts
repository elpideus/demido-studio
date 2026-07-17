import { create } from 'zustand'
import { load as loadStore } from '@tauri-apps/plugin-store'

export type IntegrationId = 'email' | 'calendar' | 'contacts'

interface IntegrationsStore {
  enabled: Record<IntegrationId, boolean>
  loaded: boolean
  load: () => Promise<void>
  toggle: (id: IntegrationId) => Promise<void>
}

const defaults: Record<IntegrationId, boolean> = {
  email: true,
  calendar: true,
  contacts: true,
}

let _storePromise: ReturnType<typeof loadStore> | null = null
function getStore() {
  if (!_storePromise) _storePromise = loadStore('prefs.json', { defaults: {}, autoSave: true })
  return _storePromise
}

export const useIntegrations = create<IntegrationsStore>((set, get) => ({
  enabled: defaults,
  loaded: false,

  load: async () => {
    const store = await getStore()
    const saved = (await store.get<Record<IntegrationId, boolean>>('integrations_enabled')) ?? {}
    set({ enabled: { ...defaults, ...saved }, loaded: true })
  },

  toggle: async (id) => {
    const updated = { ...get().enabled, [id]: !get().enabled[id] }
    const store = await getStore()
    await store.set('integrations_enabled', updated)
    set({ enabled: updated })
  },
}))
