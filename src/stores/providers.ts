import { create } from 'zustand'
import { db } from '../lib/tauri'
import type { Provider, ModelOverride } from '../types'
import { useSettings } from './settings'

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
  modelCapabilities: Record<string, Record<string, { vision: boolean; tools: boolean; reasoning: boolean }>>
  modelOverrides: Record<string, ModelOverride[]>
  selectedProviderId: string
  selectedModelId: string
  load: () => Promise<void>
  setSelected: (providerId: string, modelId: string) => void
  saveDefaultModel: (providerId: string, modelId: string) => void
  fetchModels: (providerId: string) => Promise<void>
  upsert: (provider: Provider) => Promise<void>
  addProvider: (template: ProviderTemplate) => Promise<void>
  deleteProvider: (id: string) => Promise<void>
  loadModelOverrides: (providerId: string) => Promise<void>
  upsertModelOverride: (override: ModelOverride) => Promise<void>
  batchUpsertModelOverrides: (overrides: ModelOverride[]) => Promise<void>
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
        invoke<Record<string, { vision: boolean; tools: boolean; reasoning: boolean }>>('list_model_capabilities', { providerId }).catch((e) => {
          console.warn('[demido] list_model_capabilities failed:', e)
          return {} as Record<string, { vision: boolean; tools: boolean; reasoning: boolean }>
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
}))
