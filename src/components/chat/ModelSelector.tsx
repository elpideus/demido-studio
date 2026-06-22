import { useEffect, useRef, useState, useMemo } from 'react'
import { ChevronDown, Eye, Wrench, Brain, RefreshCw } from 'lucide-react'
import Fuse from 'fuse.js'
import { useProviders } from '../../stores/providers'

export function ModelSelector() {
  const {
    providers, models, modelOverrides, modelCapabilities,
    selectedProviderId, selectedModelId,
    saveDefaultModel, fetchModels, loadModelOverrides,
  } = useProviders()
  const [open, setOpen] = useState(false)
  const [query, setQuery] = useState('')
  const [reloading, setReloading] = useState(false)
  const [maxDropHeight, setMaxDropHeight] = useState('60vh')
  const containerRef = useRef<HTMLDivElement>(null)
  const searchRef = useRef<HTMLInputElement>(null)

  const enabledProviders = providers.filter(p => p.enabled)

  useEffect(() => {
    if (!selectedProviderId) return
    if (!models[selectedProviderId]) {
      fetchModels(selectedProviderId)
      loadModelOverrides(selectedProviderId)
    }
  }, [selectedProviderId])

  // Close on outside click
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

  // Focus search input when dropdown opens
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

  const selectedDisplayName = selectedModelId
    ? getDisplayName(selectedProviderId, selectedModelId)
    : 'Select model'

  const handleSelect = (providerId: string, modelId: string) => {
    saveDefaultModel(providerId, modelId)
    setOpen(false)
    setQuery('')
  }

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Escape') {
      setOpen(false)
      setQuery('')
    }
  }

  // Build fuse indexes over all enabled models (flat list for tool search)
  const allModelEntries = useMemo(() =>
    enabledProviders.flatMap(p =>
      getEnabledModels(p.id).map(modelId => ({
        providerId: p.id,
        providerName: p.name,
        modelId,
        displayName: getDisplayName(p.id, modelId),
      }))
    ),
    // ponytail: re-derive when models/overrides change; enabledProviders ref is stable enough
    [models, modelOverrides, providers]
  )

  const modelFuse = useMemo(
    () => new Fuse(allModelEntries, { keys: ['displayName', 'modelId', 'providerName'], threshold: 0.4 }),
    [allModelEntries]
  )

  const filteredProviders = useMemo(() => {
    if (!query.trim()) {
      return enabledProviders.map(provider => ({
        provider,
        matchingModels: getEnabledModels(provider.id),
      }))
    }
    const hits = new Map<string, Set<string>>()
    for (const r of modelFuse.search(query)) {
      const { providerId, modelId } = r.item
      if (!hits.has(providerId)) hits.set(providerId, new Set())
      hits.get(providerId)!.add(modelId)
    }
    return enabledProviders
      .filter(p => hits.has(p.id))
      .map(provider => ({
        provider,
        matchingModels: getEnabledModels(provider.id).filter(m => hits.get(provider.id)!.has(m)),
      }))
  }, [query, allModelEntries, modelFuse])

  return (
    <div className="relative" ref={containerRef} onKeyDown={handleKeyDown} tabIndex={-1}>
      <button
        onClick={() => {
          if (!open && containerRef.current) {
            const rect = containerRef.current.getBoundingClientRect()
            const available = window.innerHeight - rect.bottom - 96
            setMaxDropHeight(`${Math.max(120, available)}px`)
          }
          setOpen(o => !o)
        }}
        className="flex items-center gap-1.5 px-2 py-1 rounded-md text-sm text-foreground hover:bg-accent transition-colors"
      >
        <span className="max-w-[200px] truncate">{selectedDisplayName}</span>
        <ChevronDown size={14} className="text-muted-foreground shrink-0" />
      </button>

      {open && (
        <div className="absolute top-full mt-1 left-0 w-72 bg-secondary border border-border rounded-lg shadow-xl z-20 flex flex-col" style={{ maxHeight: maxDropHeight }}>
          {/* Search input */}
          <div className="p-2 border-b border-border shrink-0 flex items-center gap-1.5">
            <input
              ref={searchRef}
              type="text"
              value={query}
              onChange={e => setQuery(e.target.value)}
              placeholder="Search models..."
              className="flex-1 bg-background border border-border rounded-md px-3 py-1.5 text-xs text-foreground placeholder:text-muted-foreground outline-none focus:border-ring/50"
            />
            <button
              onClick={async () => {
                setReloading(true)
                await Promise.all(enabledProviders.map(p => fetchModels(p.id)))
                setReloading(false)
              }}
              title="Reload models"
              className="shrink-0 p-1.5 rounded-md text-muted-foreground hover:text-foreground hover:bg-accent transition-colors"
            >
              <RefreshCw size={13} className={reloading ? 'animate-spin' : ''} />
            </button>
          </div>

          {/* Provider sections */}
          <div className="overflow-y-auto">
            {filteredProviders.map(({ provider, matchingModels }) => (
              <div key={provider.id}>
                <div className="px-3 py-1.5 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground bg-card sticky top-0 z-10">
                  {provider.name}
                </div>
                {matchingModels.map(modelId => {
                  const caps = modelCapabilities[provider.id]?.[modelId]
                  const isAnthropic = provider.type === 'anthropic'
                  const vision = caps?.vision ?? (isAnthropic ? true : undefined)
                  const tools = caps?.tools ?? (isAnthropic ? true : undefined)
                  const reasoning = caps?.reasoning ?? (isAnthropic ? true : undefined)
                  return (
                    <button
                      key={modelId}
                      onClick={() => handleSelect(provider.id, modelId)}
                      className="w-full text-left px-3 py-2 text-xs text-foreground hover:bg-accent transition-colors flex items-center gap-1.5"
                    >
                      <span className="flex-1 truncate">{getDisplayName(provider.id, modelId)}</span>
                      <span className="flex items-center gap-1 shrink-0">
                        {vision && <Eye size={11} className="text-muted-foreground/50" aria-label="Supports vision" />}
                        {tools && <Wrench size={11} className="text-muted-foreground/50" aria-label="Supports tool calling" />}
                        {reasoning && <Brain size={11} className="text-muted-foreground/50" aria-label="Supports reasoning/thinking" />}
                      </span>
                    </button>
                  )
                })}
                {!models[provider.id] && (
                  <p className="px-3 py-2 text-xs text-muted-foreground">Loading...</p>
                )}

                {models[provider.id] && matchingModels.length === 0 && (
                  <p className="px-3 py-2 text-xs text-muted-foreground">
                    {query ? 'No matches.' : 'All models disabled.'}
                  </p>
                )}
              </div>
            ))}
            {filteredProviders.length === 0 && query !== '' && (
              <p className="px-3 py-3 text-xs text-muted-foreground">No models match "{query}".</p>
            )}
            {enabledProviders.length === 0 && (
              <p className="px-3 py-3 text-xs text-muted-foreground">No providers enabled. Open Settings.</p>
            )}
          </div>
        </div>
      )}
    </div>
  )
}
