import { useState, useMemo } from 'react'
import Fuse from 'fuse.js'
import { ChevronRight, ChevronDown, Pencil, X, Check, Trash2, Search } from 'lucide-react'
import { useProviders } from '../../stores/providers'
import { db } from '../../lib/tauri'
import { ModelRow } from './ModelRow'
import type { Provider } from '../../types'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Switch } from '@/components/ui/switch'

const TYPE_LABELS: Record<string, string> = {
  openai_compat: 'OpenAI Compatible',
  openai: 'OpenAI',
  anthropic: 'Anthropic',
  gemini: 'Google Gemini',
}

interface Props { provider: Provider }

export function ProviderCard({ provider }: Props) {
  const { upsert, fetchModels, models, modelOverrides, loadModelOverrides, upsertModelOverride, batchUpsertModelOverrides, deleteProvider } = useProviders()
  const [editOpen, setEditOpen] = useState(false)
  const [modelsOpen, setModelsOpen] = useState(false)
  const [modelsLoading, setModelsLoading] = useState(false)
  const [editName, setEditName] = useState(provider.name)
  const [editType, setEditType] = useState(provider.type)
  const [editUrl, setEditUrl] = useState(provider.base_url)
  const [apiKey, setApiKey] = useState('')
  const [testStatus, setTestStatus] = useState<'idle' | 'loading' | 'ok' | 'error'>('idle')
  const [testMsg, setTestMsg] = useState('')
  const [modelSearch, setModelSearch] = useState('')

  const handleDelete = () => deleteProvider(provider.id)

  const providerModels = models[provider.id] ?? []
  const providerOverrides = modelOverrides[provider.id] ?? []

  const modelFuse = useMemo(
    () => new Fuse(providerModels.map(id => ({ id })), { keys: ['id'], threshold: 0.4 }),
    [providerModels]
  )

  const filteredModels = useMemo(() => {
    if (!modelSearch.trim()) return providerModels
    return modelFuse.search(modelSearch).map(r => r.item.id)
  }, [providerModels, modelSearch, modelFuse])

  const handleToggleEnabled = () => upsert({ ...provider, enabled: !provider.enabled })

  const handleSaveEdit = async () => {
    if (provider.api_key_ref && apiKey.trim()) {
      await db.setSecret(provider.api_key_ref, apiKey.trim())
      setApiKey('')
    }
    await upsert({ ...provider, name: editName, type: editType as Provider['type'], base_url: editUrl })
    setEditOpen(false)
  }

  const handleCancelEdit = () => {
    setEditName(provider.name)
    setEditType(provider.type)
    setEditUrl(provider.base_url)
    setEditOpen(false)
    setTestStatus('idle')
    setTestMsg('')
  }

  const handleActivateAll = () => {
    const overrides = filteredModels.map(modelId => ({
      provider_id: provider.id,
      model_id: modelId,
      custom_name: undefined as string | undefined,
      enabled: true,
    }))
    batchUpsertModelOverrides(overrides)
  }

  const handleDeactivateAll = () => {
    const overrides = filteredModels.map(modelId => ({
      provider_id: provider.id,
      model_id: modelId,
      custom_name: undefined as string | undefined,
      enabled: false,
    }))
    batchUpsertModelOverrides(overrides)
  }

  const handleTest = async () => {
    setTestStatus('loading')
    setTestMsg('')
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      const count = await invoke<number>('test_provider', {
        providerType: editType,
        baseUrl: editUrl,
        apiKeyRef: provider.api_key_ref ?? null,
        apiKeyOverride: apiKey.trim() || null,
      })
      setTestStatus('ok')
      setTestMsg(`${count} model${count !== 1 ? 's' : ''}`)
    } catch (e) {
      setTestStatus('error')
      setTestMsg(String(e))
    }
  }

  const handleModelsToggle = async () => {
    if (!modelsOpen) {
      setModelsLoading(true)
      try {
        await Promise.all([fetchModels(provider.id), loadModelOverrides(provider.id)])
      } finally {
        setModelsLoading(false)
      }
    }
    setModelsOpen(o => !o)
  }

  return (
    <div className="border border-border rounded-xl overflow-hidden">
      {/* Header row */}
      <div className="p-4 space-y-3">
        <div className="flex items-center justify-between">
          {editOpen ? (
            <Input
              value={editName}
              onChange={e => setEditName(e.target.value)}
              className="flex-1 mr-3 h-8 text-sm"
            />
          ) : (
            <div>
              <p className="text-sm font-medium text-foreground">
                {provider.name || <span className="text-muted-foreground">Unnamed provider</span>}
              </p>
              <p className="text-xs text-muted-foreground">{TYPE_LABELS[provider.type]} · {provider.base_url}</p>
            </div>
          )}
          <div className="flex items-center gap-2 shrink-0">
            <Button onClick={handleDelete} title="Delete provider" variant="ghost" size="icon-xs" className="text-destructive hover:text-destructive">
              <Trash2 size={14} />
            </Button>
            <Switch checked={provider.enabled} onCheckedChange={handleToggleEnabled} />
          </div>
        </div>

        {/* Edit fields */}
        {editOpen && (
          <div className="space-y-2">
            <select
              value={editType}
              onChange={e => setEditType(e.target.value as Provider['type'])}
              className="w-full bg-background border border-border rounded-lg px-3 py-1.5 text-sm text-foreground outline-none focus:border-ring/50"
            >
              <option value="openai_compat">OpenAI Compatible</option>
              <option value="openai">OpenAI</option>
              <option value="anthropic">Anthropic</option>
              <option value="gemini">Google Gemini</option>
            </select>
            <Input value={editUrl} onChange={e => setEditUrl(e.target.value)} placeholder="Base URL" className="h-8 text-sm" />
            {provider.api_key_ref && (
              <Input type="password" placeholder="New API key" value={apiKey} onChange={e => setApiKey(e.target.value)} className="h-8 text-sm" />
            )}
          </div>
        )}

        {/* Action buttons */}
        <div className="flex items-center gap-2">
          {editOpen ? (
            <>
              <Button onClick={handleSaveEdit} variant="ghost" size="xs" className="text-primary gap-1">
                <Check size={13} /> Save
              </Button>
              <Button onClick={handleCancelEdit} variant="ghost" size="xs" className="text-muted-foreground gap-1">
                <X size={13} /> Cancel
              </Button>
              <Button onClick={handleTest} disabled={testStatus === 'loading'} variant="ghost" size="xs" className="text-muted-foreground">
                Test
              </Button>
              {testStatus === 'loading' && (
                <span className="text-xs text-muted-foreground">Testing...</span>
              )}
              {testStatus === 'ok' && (
                <span className="text-xs text-green-400">✓ {testMsg}</span>
              )}
              {testStatus === 'error' && (
                <span className="text-xs text-red-400 truncate max-w-[200px]" title={testMsg}>✗ {testMsg}</span>
              )}
            </>
          ) : (
            <Button onClick={() => setEditOpen(true)} variant="ghost" size="xs" className="text-muted-foreground gap-1">
              <Pencil size={12} /> Edit
            </Button>
          )}
          <Button onClick={handleModelsToggle} variant="ghost" size="xs" className="ml-auto text-muted-foreground gap-1">
            {modelsOpen ? <ChevronDown size={13} /> : <ChevronRight size={13} />}
            Models
          </Button>
        </div>
      </div>

      {/* Models panel */}
      {modelsOpen && (
        <div className="border-t border-border px-4 py-3 space-y-3">
          {modelsLoading ? (
            <p className="text-xs text-muted-foreground">Loading models...</p>
          ) : providerModels.length === 0 ? (
            <p className="text-xs text-muted-foreground">No models found. Check that the provider is reachable.</p>
          ) : (
            <>
              {/* Search + batch actions */}
              <div className="flex items-center gap-2">
                <div className="relative flex-1">
                  <Search size={12} className="absolute left-2 top-1/2 -translate-y-1/2 text-muted-foreground/60" />
                  <Input
                    type="text"
                    value={modelSearch}
                    onChange={e => setModelSearch(e.target.value)}
                    placeholder="Search models..."
                    className="pl-7 h-7 text-xs"
                  />
                </div>
                <Button onClick={handleActivateAll} variant="ghost" size="xs" className="text-primary shrink-0">Activate All</Button>
                <Button onClick={handleDeactivateAll} variant="ghost" size="xs" className="text-muted-foreground shrink-0">Deactivate All</Button>
              </div>
              <div className="space-y-0.5">
                {filteredModels.length > 0 ? (
                  filteredModels.map(modelId => (
                    <ModelRow
                      key={modelId}
                      providerId={provider.id}
                      modelId={modelId}
                      override={providerOverrides.find(o => o.model_id === modelId)}
                      onUpdate={upsertModelOverride}
                    />
                  ))
                ) : (
                  <p className="text-xs text-muted-foreground py-1">No models match your search.</p>
                )}
              </div>
            </>
          )}
        </div>
      )}
    </div>
  )
}
