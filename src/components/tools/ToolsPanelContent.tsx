import { useState, useEffect } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { McpSettings } from '../settings/McpSettings'
import { useSkills } from '../../stores/skills'
import { useArtifacts } from '../../stores/artifacts'
import { useWindowManager } from '../../stores/windowManager'
import { useIntegrations, type IntegrationId } from '../../stores/integrations'
import { Trash2, Mail, Calendar, Users, GripVertical, Pencil } from 'lucide-react'
import { db } from '../../lib/tauri'
import { Input } from '@/components/ui/input'
import { Button } from '@/components/ui/button'

type Tab = 'mcp' | 'skills' | 'integrations' | 'web'

const TABS: { id: Tab; label: string }[] = [
  { id: 'mcp',          label: 'MCP Servers' },
  { id: 'skills',       label: 'Skills' },
  { id: 'integrations', label: 'Integrations' },
  { id: 'web',          label: 'Web Browsing' },
]

/// `id` must match `SearchProvider::from_id` in `src-tauri/src/web.rs`: it is what gets
/// persisted in the `websearch_order` setting the backend parses.
type ProviderId = 'exa' | 'parallel' | 'searxng' | 'ddg'

const PROVIDERS: { id: ProviderId; key: string; label: string; help: string; default: boolean; apiKey?: { key: string; label: string; help: string } }[] = [
  {
    id: 'exa',
    key: 'websearch_exa_enabled', label: 'Exa', help: 'Neural search built for AI. Matches on meaning rather than keywords, and returns page content with each hit, so fewer follow-up fetches.', default: true,
    apiKey: { key: 'exa_api_key', label: 'Exa API key', help: 'Optional. Raises rate limits; the tool works without one.' },
  },
  {
    id: 'parallel',
    key: 'websearch_parallel_enabled', label: 'Parallel', help: 'Search API built for agents. Ranks by how well a page answers the question and returns long excerpts, favouring fresh and deep-web pages.', default: true,
    apiKey: { key: 'parallel_api_key', label: 'Parallel API key', help: 'Optional. Raises rate limits; the tool works without one.' },
  },
  { id: 'searxng', key: 'websearch_searxng_enabled', label: 'SearXNG', help: 'Private metasearch: aggregates ~200 engines (Google, Bing, Brave…) with no tracking or profiling. Runs inside Demido on no network port and starts on its own; first use downloads it, which takes a few minutes.', default: false },
  { id: 'ddg', key: 'websearch_ddg_enabled', label: 'DuckDuckGo', help: 'Reads DuckDuckGo’s plain HTML results. Always available with no key or setup, but gives titles and snippets only.', default: true },
]

const DEFAULT_ORDER: ProviderId[] = PROVIDERS.map(p => p.id)
const ORDER_KEY = 'websearch_order'

/// Mirrors `web::parse_order`: keep stored ids, drop junk/dupes, append anything missing.
function parseOrder(stored: string | null): ProviderId[] {
  const known = new Set(DEFAULT_ORDER)
  const order = (stored ?? '')
    .split(',')
    .map(s => s.trim() as ProviderId)
    .filter(id => known.has(id))
  return [...new Set([...order, ...DEFAULT_ORDER])]
}

function Toggle({ on, onClick }: { on: boolean; onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      className={`relative w-9 h-5 rounded-full transition-colors shrink-0 ${on ? 'bg-primary' : 'bg-muted'}`}
    >
      <span className={`absolute top-0.5 left-0.5 w-4 h-4 rounded-full bg-white transition-transform ${on ? 'translate-x-4' : ''}`} />
    </button>
  )
}

interface InstallStage { stage: string }

const SEARXNG_KEY = 'websearch_searxng_enabled'

function WebBrowsingSettings() {
  const [values, setValues] = useState<Record<string, string>>({})
  const [saved, setSaved] = useState<Record<string, boolean>>({})
  const [toggles, setToggles] = useState<Record<string, boolean>>({})
  const [loading, setLoading] = useState(true)
  const [starting, setStarting] = useState(false)
  const [stage, setStage] = useState('')
  const [searxngError, setSearxngError] = useState('')
  const [order, setOrder] = useState<ProviderId[]>(DEFAULT_ORDER)
  const [dragging, setDragging] = useState<ProviderId | null>(null)

  useEffect(() => {
    (async () => {
      const apiKeys = PROVIDERS.map(t => t.apiKey).filter((k): k is NonNullable<typeof k> => !!k)
      const keyEntries = await Promise.all(apiKeys.map(async k => [k.key, (await db.getSecret(k.key)) ?? ''] as const))
      const toggleEntries = await Promise.all(
        PROVIDERS.map(async t => [t.key, (await db.getSetting(t.key, String(t.default))) === 'true'] as const)
      )
      setValues(Object.fromEntries(keyEntries))
      setToggles(Object.fromEntries(toggleEntries))
      setOrder(parseOrder(await db.getSetting(ORDER_KEY, '')))
      setLoading(false)
    })()
  }, [])

  // Reordering is local while a drag is in flight (it fires on every card crossed) and is
  // persisted once the drag ends; keyboard moves persist immediately.
  const move = (id: ProviderId, to: number) => {
    setOrder(prev => {
      const from = prev.indexOf(id)
      if (to < 0 || to >= prev.length || from === to) return prev
      const next = prev.filter(p => p !== id)
      next.splice(to, 0, id)
      return next
    })
  }

  const persistOrder = (next: ProviderId[]) => db.setSetting(ORDER_KEY, next.join(','))

  const handleSave = async (key: string) => {
    await db.setSecret(key, values[key]?.trim() ?? '')
    setSaved(s => ({ ...s, [key]: true }))
    setTimeout(() => setSaved(s => ({ ...s, [key]: false })), 1500)
  }

  // SearXNG has no separate controls: the toggle *is* the lifecycle. Turning it on installs
  // (first time only) and starts the in-process worker; turning it off shuts it down.
  useEffect(() => {
    const uns = [
      listen<InstallStage>('python_install_progress', e => setStage(`python: ${e.payload.stage}`)),
      listen<InstallStage>('searxng_install_progress', e => setStage(e.payload.stage)),
    ]
    return () => { uns.forEach(u => u.then(f => f())) }
  }, [])

  const handleToggle = async (key: string) => {
    const next = !toggles[key]
    setToggles(t => ({ ...t, [key]: next }))
    await db.setSetting(key, String(next))
    if (key !== SEARXNG_KEY) return

    setSearxngError('')
    if (!next) {
      setStage('')
      await invoke('stop_searxng').catch(() => {})
      return
    }
    setStarting(true)
    try {
      await invoke('start_searxng')
    } catch (e) {
      setSearxngError(String(e))
      setToggles(t => ({ ...t, [key]: false }))
      await db.setSetting(key, 'false')
    } finally {
      setStarting(false)
      setStage('')
    }
  }

  if (loading) return null

  return (
    <div className="space-y-5">
      <div className="space-y-1">
        <h3 className="text-sm font-semibold text-foreground">Web Browsing</h3>
        <p className="text-xs text-muted-foreground">
          web_search tries enabled providers top to bottom, moving down the list whenever one fails or returns
          nothing. Drag to reorder, and turn off any you don't want used.
        </p>
      </div>

      <div className="space-y-2">
        {order.map((id, index) => {
          const t = PROVIDERS.find(p => p.id === id)!
          return (
          <div
            key={t.id}
            onDragOver={e => { e.preventDefault(); if (dragging && dragging !== t.id) move(dragging, index) }}
            onDrop={e => e.preventDefault()}
            className={`p-3 rounded-lg border border-border space-y-3 transition-opacity ${dragging === t.id ? 'opacity-50' : ''}`}
          >
            <div className="flex items-center gap-3">
              <button
                draggable
                onDragStart={() => setDragging(t.id)}
                onDragEnd={() => { setDragging(null); persistOrder(order) }}
                onKeyDown={e => {
                  const to = e.key === 'ArrowUp' ? index - 1 : e.key === 'ArrowDown' ? index + 1 : -1
                  if (to < 0 || to >= order.length) return
                  e.preventDefault()
                  move(t.id, to)
                  const next = order.filter(p => p !== t.id)
                  next.splice(to, 0, t.id)
                  persistOrder(next)
                }}
                aria-label={`Reorder ${t.label}, position ${index + 1} of ${order.length}. Drag, or use arrow keys.`}
                title="Drag to reorder (or focus and use ↑ / ↓)"
                className="shrink-0 cursor-grab active:cursor-grabbing p-1 -m-1 rounded text-muted-foreground hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
              >
                <GripVertical size={14} />
              </button>
              <span className="shrink-0 w-4 text-xs tabular-nums text-muted-foreground/60">{index + 1}</span>
              <div className="flex-1 min-w-0">
                <div className="text-sm font-medium text-foreground">{t.label}</div>
                <div className="text-xs text-muted-foreground mt-0.5">{t.help}</div>
              </div>
              <Toggle on={toggles[t.key] ?? t.default} onClick={() => handleToggle(t.key)} />
            </div>
            {t.key === SEARXNG_KEY && starting && (
              <p className="text-[11px] text-muted-foreground">
                {stage || 'starting'}… this can take a few minutes the first time.
              </p>
            )}
            {t.key === SEARXNG_KEY && searxngError && (
              <p className="text-[11px] text-red-400 break-words">{searxngError}</p>
            )}
            {t.apiKey && (
              <div className="space-y-1.5 pt-3 border-t border-border/60">
                <label className="text-xs font-medium text-foreground">{t.apiKey.label}</label>
                <div className="flex gap-2">
                  <Input
                    type="password"
                    value={values[t.apiKey.key] ?? ''}
                    onChange={e => setValues(v => ({ ...v, [t.apiKey!.key]: e.target.value }))}
                    placeholder="Leave empty to use the free tier"
                  />
                  <Button size="sm" onClick={() => handleSave(t.apiKey!.key)}>
                    {saved[t.apiKey.key] ? 'Saved' : 'Save'}
                  </Button>
                </div>
                <p className="text-xs text-muted-foreground">{t.apiKey.help}</p>
              </div>
            )}
          </div>
          )
        })}
      </div>
    </div>
  )
}

const INTEGRATIONS: { id: IntegrationId; label: string; icon: React.ReactNode }[] = [
  { id: 'email',    label: 'Email',    icon: <Mail size={16} /> },
  { id: 'calendar', label: 'Calendar', icon: <Calendar size={16} /> },
  { id: 'contacts', label: 'Contacts', icon: <Users size={16} /> },
]

function IntegrationsList() {
  const { enabled, toggle } = useIntegrations()

  return (
    <div className="space-y-2">
      {INTEGRATIONS.map(i => (
        <div key={i.id} className="p-3 rounded-lg border border-border flex items-center gap-3">
          <div className="text-muted-foreground">{i.icon}</div>
          <div className="flex-1 text-sm font-medium text-foreground">{i.label}</div>
          <button
            onClick={() => toggle(i.id)}
            className={`relative w-9 h-5 rounded-full transition-colors shrink-0 ${
              enabled[i.id] ? 'bg-primary' : 'bg-muted'
            }`}
            title={enabled[i.id] ? 'Disable' : 'Enable'}
          >
            <span
              className={`absolute top-0.5 left-0.5 w-4 h-4 rounded-full bg-white transition-transform ${
                enabled[i.id] ? 'translate-x-4' : ''
              }`}
            />
          </button>
        </div>
      ))}
    </div>
  )
}

function SkillsList() {
  const { skills, load, delete: deleteSkill } = useSkills()
  const { openSkill, setPoppedOut } = useArtifacts()
  const { openWindow } = useWindowManager()
  const [confirmId, setConfirmId] = useState<string | null>(null)
  const [error, setError] = useState('')
  useEffect(() => { load() }, [])

  const edit = async (id: string, name: string) => {
    setError('')
    try {
      await openSkill(id, name)
      openWindow('artifact-viewer', 'artifact-viewer', name, {
        initialSize: { width: Math.round(window.innerWidth * 0.92), height: Math.round(window.innerHeight * 0.92) },
      })
      setPoppedOut(true)
    } catch (e) {
      setError(String(e))
    }
  }

  if (!skills.length) return <p className="text-sm text-muted-foreground">No skills installed.</p>

  return (
    <>
      {error && <p className="mb-2 text-xs text-red-400">{error}</p>}
      <div className="space-y-2">
        {skills.map(s => (
          <div key={s.id} className="p-3 rounded-lg border border-border flex items-start gap-2">
            <div className="flex-1 min-w-0">
              <div className="text-sm font-medium text-foreground">{s.name}</div>
              {s.description && <div className="text-xs text-muted-foreground mt-0.5">{s.description}</div>}
              {s.version && <div className="text-xs text-muted-foreground/60 mt-0.5">v{s.version}</div>}
            </div>
            <button
              onClick={() => edit(s.id, s.name)}
              className="shrink-0 p-1.5 rounded text-muted-foreground hover:text-foreground hover:bg-accent transition-colors"
              title="Edit skill"
            >
              <Pencil size={14} />
            </button>
            <button
              onClick={() => setConfirmId(s.id)}
              className="shrink-0 p-1.5 rounded text-muted-foreground hover:text-destructive hover:bg-destructive/10 transition-colors"
              title="Delete skill"
            >
              <Trash2 size={14} />
            </button>
          </div>
        ))}
      </div>

      {confirmId && (() => {
        const skill = skills.find(s => s.id === confirmId)
        return (
          <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
            <div className="bg-background border border-border rounded-xl p-6 w-80 shadow-xl space-y-4">
              <div className="text-sm font-semibold text-foreground">Delete skill?</div>
              <div className="text-sm text-muted-foreground">
                <span className="font-medium text-foreground">{skill?.name}</span> will be permanently removed.
              </div>
              <div className="flex gap-2 justify-end">
                <button
                  onClick={() => setConfirmId(null)}
                  className="px-3 py-1.5 text-sm rounded-md border border-border hover:bg-accent transition-colors"
                >
                  Cancel
                </button>
                <button
                  onClick={async () => { await deleteSkill(confirmId); setConfirmId(null) }}
                  className="px-3 py-1.5 text-sm rounded-md bg-destructive text-destructive-foreground hover:bg-destructive/90 transition-colors"
                >
                  Delete
                </button>
              </div>
            </div>
          </div>
        )
      })()}
    </>
  )
}

export function ToolsPanelContent() {
  const [tab, setTab] = useState<Tab>('mcp')

  return (
    <div className="flex flex-1 overflow-hidden h-full">
      <div className="w-44 border-r border-border p-3 space-y-0.5 shrink-0 bg-[#1b1b1b]">
        {TABS.map(t => (
          <button
            key={t.id}
            onClick={() => setTab(t.id)}
            className={`w-full text-left px-3 py-2 rounded-md text-sm transition-colors ${
              tab === t.id
                ? 'bg-primary/20 text-foreground'
                : 'text-muted-foreground hover:bg-accent hover:text-foreground'
            }`}
          >
            {t.label}
          </button>
        ))}
      </div>
      <div className="flex-1 overflow-y-auto p-6">
        {tab === 'mcp' && <McpSettings />}
        {tab === 'skills' && (
          <div className="space-y-3">
            <h3 className="text-sm font-semibold text-foreground">Skills</h3>
            <SkillsList />
          </div>
        )}
        {tab === 'integrations' && (
          <div className="space-y-3">
            <h3 className="text-sm font-semibold text-foreground">Integrations</h3>
            <IntegrationsList />
          </div>
        )}
        {tab === 'web' && <WebBrowsingSettings />}
      </div>
    </div>
  )
}
