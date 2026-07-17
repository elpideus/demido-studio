import { create } from 'zustand'
import { db } from '../lib/tauri'
import type { Provider, ModelOverride, ModelCaps, CapName } from '../types'
import { useSettings } from './settings'
import { LOCAL_PROVIDER_ID } from '../components/settings/LocalProviderCard'

interface ProviderTemplate {
  key: string
  name: string
  type: 'openai_compat' | 'openai' | 'anthropic' | 'gemini'
  base_url: string
  api_key_ref: string | null
}

interface ProvidersStore {
  providers: Provider[]
  models: Record<string, string[]>
  modelCapabilities: Record<string, Record<string, ModelCaps>>
  modelOverrides: Record<string, ModelOverride[]>
  selectedProviderId: string
  selectedModelId: string
  load: () => Promise<void>
  /** Spawn llama-server for the selected local model, if one is selected. */
  preloadSelectedLocalModel: () => Promise<void>
  setSelected: (providerId: string, modelId: string) => void
  saveDefaultModel: (providerId: string, modelId: string) => void
  fetchModels: (providerId: string) => Promise<void>
  upsert: (provider: Provider) => Promise<void>
  /** Resolves to the new provider's id, so the caller can open it for editing. */
  addProvider: (template: ProviderTemplate) => Promise<string>
  deleteProvider: (id: string) => Promise<void>
  loadModelOverrides: (providerId: string) => Promise<void>
  upsertModelOverride: (override: ModelOverride) => Promise<void>
  batchUpsertModelOverrides: (overrides: ModelOverride[]) => Promise<void>
  /** Tell the system what a model supports. null for a field = back to auto-detect. */
  setModelCapsOverride: (
    providerId: string,
    modelId: string,
    caps: Partial<Record<CapName, boolean | null>>,
  ) => Promise<void>
}

export const useProviders = create<ProvidersStore>((set, get) => ({
  providers: [],
  models: {},
  modelCapabilities: {},
  modelOverrides: {},
  selectedProviderId: '',
  selectedModelId: '',

  load: async () => {
    const [providers, settings] = await Promise.all([db.listProviders(), db.getSettings()])
    const enabled = providers.filter(p => p.enabled)
    const savedProvider = settings.default_provider_id && enabled.find(p => p.id === settings.default_provider_id)
    set({
      providers,
      selectedProviderId: savedProvider ? settings.default_provider_id : (enabled[0]?.id ?? ''),
      selectedModelId: savedProvider ? (settings.default_model_id ?? '') : '',
    })
    await Promise.all(enabled.map(p => Promise.all([
      get().fetchModels(p.id),
      get().loadModelOverrides(p.id),
    ])))
    await get().preloadSelectedLocalModel()
  },

  // The restored default model is only a string until llama-server holds it. Spawn it now so
  // the first send doesn't pay the multi-GB read, and so caps are probed before an attachment.
  preloadSelectedLocalModel: async () => {
    const { selectedProviderId, selectedModelId } = get()
    if (selectedProviderId !== LOCAL_PROVIDER_ID || !selectedModelId) return
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      await invoke('preload_local_model', { modelId: selectedModelId })
      await get().fetchModels(LOCAL_PROVIDER_ID)
    } catch (e) {
      console.warn('[demido] preload of selected local model failed:', e)
    }
  },

  setSelected: (providerId, modelId) =>
    set({ selectedProviderId: providerId, selectedModelId: modelId }),

  saveDefaultModel: (providerId, modelId) => {
    set({ selectedProviderId: providerId, selectedModelId: modelId })
    useSettings.getState().update('default_provider_id', providerId)
    useSettings.getState().update('default_model_id', modelId)
  },

  fetchModels: async (providerId) => {
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      const [models, caps] = await Promise.all([
        invoke<string[]>('list_models', { providerId }),
        invoke<Record<string, ModelCaps>>('list_model_capabilities', { providerId }).catch((e) => {
          console.warn('[demido] list_model_capabilities failed:', e)
          return {} as Record<string, ModelCaps>
        }),
      ])
      console.debug('[demido] model caps for', providerId, caps)
      set(s => ({
        models: { ...s.models, [providerId]: models },
        modelCapabilities: { ...s.modelCapabilities, [providerId]: caps },
      }))
    } catch {
      set(s => ({ models: { ...s.models, [providerId]: [] } }))
    }
  },

  upsert: async (provider) => {
    await db.upsertProvider(provider)
    await get().load()
  },

  addProvider: async (template) => {
    const id = crypto.randomUUID()
    const api_key_ref = template.api_key_ref ?? (template.key === 'custom' ? `provider_${id}` : undefined)
    const provider: Provider = {
      id,
      name: template.name,
      type: template.type,
      base_url: template.base_url,
      api_key_ref,
      enabled: true,
      sort_order: get().providers.length,
      visible: true,
    }
    await db.upsertProvider(provider)
    await get().load()
    return id
  },

  deleteProvider: async (id) => {
    await db.deleteProvider(id)
    await get().load()
  },

  loadModelOverrides: async (providerId) => {
    const overrides = await db.listModelOverrides(providerId)
    set(s => ({ modelOverrides: { ...s.modelOverrides, [providerId]: overrides } }))
  },

  upsertModelOverride: async (override) => {
    await db.upsertModelOverride(override)
    await get().loadModelOverrides(override.provider_id)
  },

  batchUpsertModelOverrides: async (overrides) => {
    if (overrides.length === 0) return
    const providerId = overrides[0].provider_id
    await db.batchUpsertModelOverrides(overrides)
    await get().loadModelOverrides(providerId)
  },

  setModelCapsOverride: async (providerId, modelId, caps) => {
    const { invoke } = await import('@tauri-apps/api/core')
    // The command writes all three columns every time, and an absent field arrives as null,
    // same as "clear to auto". So send the other two back as they already are, or setting
    // one flag wipes the rest.
    const prev = get().modelOverrides[providerId]?.find(o => o.model_id === modelId)
    const pick = (name: CapName, stored: boolean | null | undefined) =>
      name in caps ? caps[name] ?? null : stored ?? null
    await invoke('set_model_caps_override', {
      providerId,
      modelId,
      vision: pick('vision', prev?.caps_vision),
      tools: pick('tools', prev?.caps_tools),
      reasoning: pick('reasoning', prev?.caps_reasoning),
    })
    // Re-resolve: the backend merges the override into the chain, so the effective caps
    // (and which fields now read as user-set) come back from it, not from local guesswork.
    await Promise.all([get().loadModelOverrides(providerId), get().fetchModels(providerId)])
  },
}))
