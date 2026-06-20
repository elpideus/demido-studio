import { useEffect, useRef, useState } from 'react'
import { ChevronDown } from 'lucide-react'
import { useProviders } from '../../stores/providers'
import { useSettings } from '../../stores/settings'

function TaskModelSelector() {
  const { providers, models, modelOverrides, fetchModels, loadModelOverrides } = useProviders()
  const { settings, update } = useSettings()
  const [open, setOpen] = useState(false)
  const [query, setQuery] = useState('')
  const containerRef = useRef<HTMLDivElement>(null)
  const searchRef = useRef<HTMLInputElement>(null)

  const enabledProviders = providers.filter(p => p.enabled)

  // Fetch models for all enabled providers on mount
  useEffect(() => {
    enabledProviders.forEach(p => {
      if (!models[p.id]) {
        fetchModels(p.id)
        loadModelOverrides(p.id)
      }
    })
  }, [providers])

  useEffect(() => {
    if (!open) return
    const handler = (e: MouseEvent) => {
      if (containerRef.current && !containerRef.current.contains(e.target as Node)) {
        setOpen(false)
        setQuery('')
      }
    }
    document.addEventListener('mousedown', handler)
    return () => document.removeEventListener('mousedown', handler)
  }, [open])

  useEffect(() => {
    if (open) searchRef.current?.focus()
  }, [open])

  const getDisplayName = (providerId: string, modelId: string): string => {
    const overrides = modelOverrides[providerId] ?? []
    return overrides.find(o => o.model_id === modelId)?.custom_name ?? modelId
  }

  const getEnabledModels = (providerId: string): string[] => {
    const allModels = models[providerId] ?? []
    const overrides = modelOverrides[providerId] ?? []
    return allModels.filter(modelId => {
      const override = overrides.find(o => o.model_id === modelId)
      return override?.enabled !== false
    })
  }

  const selectedLabel = settings.task_provider_id && settings.task_model_id
    ? getDisplayName(settings.task_provider_id, settings.task_model_id)
    : 'None (use conversation model)'

  const handleSelect = (providerId: string, modelId: string) => {
    update('task_provider_id', providerId)
    update('task_model_id', modelId)
    setOpen(false)
    setQuery('')
  }

  const handleClear = () => {
    update('task_provider_id', '')
    update('task_model_id', '')
    setOpen(false)
    setQuery('')
  }

  const q = query.toLowerCase()
  const filteredProviders = enabledProviders
    .map(provider => {
      const enabledModels = getEnabledModels(provider.id)
      const providerMatches = provider.name.toLowerCase().includes(q)
      const matchingModels = providerMatches
        ? enabledModels
        : enabledModels.filter(modelId =>
            modelId.toLowerCase().includes(q) ||
            getDisplayName(provider.id, modelId).toLowerCase().includes(q)
          )
      return { provider, matchingModels }
    })
    .filter(({ matchingModels, provider }) =>
      q === '' || provider.name.toLowerCase().includes(q) || matchingModels.length > 0
    )

  return (
    <div className="relative" ref={containerRef}>
      <button
        onClick={() => setOpen(o => !o)}
        className="flex items-center gap-1.5 px-3 py-1.5 rounded-md text-sm text-foreground bg-secondary border border-border hover:border-[var(--primary)]/50 transition-colors w-64"
      >
        <span className="flex-1 text-left truncate">{selectedLabel}</span>
        <ChevronDown size={14} className="text-muted-foreground shrink-0" />
      </button>

      {open && (
        <div className="absolute top-full mt-1 left-0 w-72 bg-secondary border border-border rounded-lg shadow-xl z-20 flex flex-col max-h-[50vh]">
          <div className="p-2 border-b border-border shrink-0">
            <input
              ref={searchRef}
              type="text"
              value={query}
              onChange={e => setQuery(e.target.value)}
              placeholder="Search models..."
              className="w-full bg-background border border-border rounded-md px-3 py-1.5 text-xs text-foreground placeholder:text-muted-foreground outline-none focus:border-ring/50"
            />
          </div>
          <div className="overflow-y-auto">
            {q === '' && (
              <button
                onClick={handleClear}
                className="w-full text-left px-3 py-2 text-xs text-muted-foreground hover:bg-accent hover:text-foreground transition-colors italic"
              >
                None (use conversation model)
              </button>
            )}
            {filteredProviders.map(({ provider, matchingModels }) => (
              <div key={provider.id}>
                <div className="px-3 py-1.5 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground bg-card sticky top-0 z-10">
                  {provider.name}
                </div>
                {matchingModels.map(modelId => (
                  <button
                    key={modelId}
                    onClick={() => handleSelect(provider.id, modelId)}
                    className="w-full text-left px-3 py-2 text-xs text-foreground hover:bg-accent transition-colors truncate"
                  >
                    {getDisplayName(provider.id, modelId)}
                  </button>
                ))}
                {!models[provider.id] && (
                  <p className="px-3 py-2 text-xs text-muted-foreground">Loading...</p>
                )}
                {models[provider.id] && matchingModels.length === 0 && (
                  <p className="px-3 py-2 text-xs text-muted-foreground">
                    {q ? 'No matches.' : 'All models disabled.'}
                  </p>
                )}
              </div>
            ))}
            {filteredProviders.length === 0 && q !== '' && (
              <p className="px-3 py-3 text-xs text-muted-foreground">No models match "{query}".</p>
            )}
          </div>
        </div>
      )}
    </div>
  )
}

export function InterfaceSettings() {
  const { settings, update } = useSettings()

  return (
    <div className="space-y-6">
      <div>
        <h3 className="text-sm font-semibold text-foreground mb-1">Task Model</h3>
        <p className="text-xs text-muted-foreground mb-3">
          Used for background tasks like auto-titling. Falls back to the conversation's model if unset.
        </p>
        <TaskModelSelector />
      </div>

      <div>
        <h3 className="text-sm font-semibold text-foreground mb-1">Auto-title frequency</h3>
        <p className="text-xs text-muted-foreground mb-3">
          Re-generate the conversation title every N messages (after the first reply).
        </p>
        <div className="flex items-center gap-2">
          <input
            type="number"
            min={1}
            value={settings.title_every_n_messages}
            onChange={e => {
              const n = parseInt(e.target.value, 10)
              if (n >= 1) update('title_every_n_messages', n)
            }}
            className="w-20 bg-background border border-border rounded-md px-3 py-1.5 text-sm text-foreground outline-none focus:border-ring/50"
          />
          <span className="text-xs text-muted-foreground">messages</span>
        </div>
      </div>
    </div>
  )
}
