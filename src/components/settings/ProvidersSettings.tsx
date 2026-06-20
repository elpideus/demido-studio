import { useEffect, useMemo, useRef, useState } from 'react'
import Fuse from 'fuse.js'
import { ChevronDown } from 'lucide-react'
import { useProviders } from '../../stores/providers'
import { ProviderCard } from './ProviderCard'
import { Button } from '@/components/ui/button'

const PROVIDER_TEMPLATES = [
  { key: 'custom',      label: 'Custom',         name: '',               type: 'openai_compat' as const, base_url: '',                                                   api_key_ref: null },
  { key: 'anthropic',   label: 'Anthropic',     name: 'Anthropic',      type: 'anthropic' as const,     base_url: 'https://api.anthropic.com',                          api_key_ref: 'anthropic_key' },
  { key: 'openai',      label: 'OpenAI',         name: 'OpenAI',         type: 'openai' as const,        base_url: 'https://api.openai.com/v1',                          api_key_ref: 'openai_key' },
  { key: 'gemini',      label: 'Google Gemini',  name: 'Google Gemini',  type: 'gemini' as const,        base_url: 'https://generativelanguage.googleapis.com/v1beta',   api_key_ref: 'gemini_key' },
  { key: 'groq',        label: 'Groq',           name: 'Groq',           type: 'openai_compat' as const, base_url: 'https://api.groq.com/openai/v1',                     api_key_ref: 'groq_key' },
  { key: 'openrouter',  label: 'OpenRouter',     name: 'OpenRouter',     type: 'openai_compat' as const, base_url: 'https://openrouter.ai/api/v1',                       api_key_ref: 'openrouter_key' },
  { key: 'ollama',      label: 'Ollama',         name: 'Ollama',         type: 'openai_compat' as const, base_url: 'http://localhost:11434/v1',                          api_key_ref: null },
  { key: 'lmstudio',   label: 'LM Studio',      name: 'LM Studio',      type: 'openai_compat' as const, base_url: 'http://localhost:1234/v1',                           api_key_ref: null },
]

const fuse = new Fuse(PROVIDER_TEMPLATES, { keys: ['label'], threshold: 0.4 })

export function ProvidersSettings() {
  const { providers, addProvider } = useProviders()
  const [selected, setSelected] = useState(PROVIDER_TEMPLATES[0])
  const [open, setOpen] = useState(false)
  const [query, setQuery] = useState('')
  const containerRef = useRef<HTMLDivElement>(null)
  const searchRef = useRef<HTMLInputElement>(null)

  const filtered = useMemo(() =>
    query ? fuse.search(query).map(r => r.item) : PROVIDER_TEMPLATES,
    [query]
  )

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

  useEffect(() => { if (open) searchRef.current?.focus() }, [open])

  const handleAdd = () => addProvider(selected)

  return (
    <div className="space-y-4">
      <h3 className="text-sm font-semibold text-foreground">Providers & Models</h3>
      <div className="flex gap-2">
        <div ref={containerRef} className="relative flex-1">
          <button
            onClick={() => setOpen(o => !o)}
            className="w-full flex items-center justify-between bg-background border border-border rounded-lg px-3 py-1.5 text-sm text-foreground outline-none focus:border-ring/50 cursor-pointer"
          >
            <span>{selected.label}</span>
            <ChevronDown className="w-3.5 h-3.5 text-muted-foreground shrink-0" />
          </button>
          {open && (
            <div className="absolute z-50 top-full mt-1 left-0 right-0 bg-popover border border-border rounded-lg shadow-lg overflow-hidden">
              <div className="p-1.5 border-b border-border">
                <input
                  ref={searchRef}
                  value={query}
                  onChange={e => setQuery(e.target.value)}
                  placeholder="Search presets…"
                  className="w-full bg-transparent text-sm text-foreground placeholder:text-muted-foreground outline-none px-1"
                />
              </div>
              <div className="max-h-48 overflow-y-auto py-1">
                {filtered.map(t => (
                  <button
                    key={t.key}
                    onClick={() => { setSelected(t); setOpen(false); setQuery('') }}
                    className={`w-full text-left px-3 py-1.5 text-sm cursor-pointer hover:bg-accent transition-colors ${selected.key === t.key ? 'text-foreground font-medium' : 'text-muted-foreground'}`}
                  >
                    {t.label}
                  </button>
                ))}
                {filtered.length === 0 && (
                  <p className="text-xs text-muted-foreground text-center py-3">No presets match</p>
                )}
              </div>
            </div>
          )}
        </div>
        <Button onClick={handleAdd} size="sm" className="shrink-0">+ Add</Button>
      </div>
      <div className="space-y-3">
        {providers.map(p => (
          <ProviderCard key={p.id} provider={p} />
        ))}
        {providers.length === 0 && (
          <p className="text-xs text-muted-foreground text-center py-6">
            No providers added yet. Use the dropdown above to add one.
          </p>
        )}
      </div>
    </div>
  )
}
