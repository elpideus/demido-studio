import { useMemo, useState } from 'react'
import Fuse from 'fuse.js'
import { ChevronRight, ChevronDown, Search, Download } from 'lucide-react'
import { useProviders } from '../../stores/providers'
import { ModelRow } from './ModelRow'
import type { Provider } from '../../types'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Switch } from '@/components/ui/switch'

// Keep in sync with db::LOCAL_PROVIDER_ID on the backend.
export const LOCAL_PROVIDER_ID = 'demido-local'

interface Props { provider: Provider; onDownloadModels?: () => void }

/** The pinned "Demido Studio" provider: models are installed via Settings → Engine → Models. */
export function LocalProviderCard({ provider, onDownloadModels }: Props) {
  const { upsert, models, modelOverrides, fetchModels, loadModelOverrides, upsertModelOverride, batchUpsertModelOverrides } = useProviders()
  const [modelsOpen, setModelsOpen] = useState(false)
  const [modelsLoading, setModelsLoading] = useState(false)
  const [modelSearch, setModelSearch] = useState('')

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

  const handleActivateAll = () => {
    batchUpsertModelOverrides(filteredModels.map(modelId => ({
      provider_id: provider.id, model_id: modelId, custom_name: undefined, enabled: true,
    })))
  }

  const handleDeactivateAll = () => {
    batchUpsertModelOverrides(filteredModels.map(modelId => ({
      provider_id: provider.id, model_id: modelId, custom_name: undefined, enabled: false,
    })))
  }

  return (
    <div className="border border-primary/30 rounded-xl overflow-hidden">
      <div className="p-4 space-y-3">
        <div className="flex items-center justify-between">
          <div>
            <p className="text-sm font-medium text-foreground">{provider.name}</p>
            <p className="text-xs text-muted-foreground">Run GGUF models locally, fully enclosed, no external server.</p>
          </div>
          <Switch checked={provider.enabled} onCheckedChange={handleToggleEnabled} />
        </div>

        <div className="flex items-center gap-2">
          {onDownloadModels && (
            <Button onClick={onDownloadModels} variant="ghost" size="xs" className="text-primary gap-1">
              <Download size={13} />
              Download Models
            </Button>
          )}
          <Button onClick={handleModelsToggle} variant="ghost" size="xs" className="ml-auto text-muted-foreground gap-1">
            {modelsOpen ? <ChevronDown size={13} /> : <ChevronRight size={13} />}
            Models
          </Button>
        </div>
      </div>

      {modelsOpen && (
        <div className="border-t border-border px-4 py-3 space-y-3">
          {modelsLoading ? (
            <p className="text-xs text-muted-foreground">Loading models...</p>
          ) : providerModels.length === 0 ? (
            <p className="text-xs text-muted-foreground">
              No models installed.{' '}
              {onDownloadModels
                ? <button onClick={onDownloadModels} className="text-primary hover:underline">Download some</button>
                : 'Add some in Settings → Engine → Models.'}
            </p>
          ) : (
            <>
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
