import { useEffect, useMemo, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import Markdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import rehypeRaw from 'rehype-raw'
import rehypeSanitize from 'rehype-sanitize'
import { Search, Download, Heart, Loader2, Check, Lock, ExternalLink, HardDrive, Eye, Wrench, Brain, Boxes, HelpCircle } from 'lucide-react'
import { useProviders } from '../../../stores/providers'
import { LOCAL_PROVIDER_ID } from '../LocalProviderCard'
import { CAPS_SOURCE_LABEL } from '../ModelRow'
import type { ModelCaps } from '../../../types'
import { Input } from '@/components/ui/input'
import { Button } from '@/components/ui/button'

interface HfModel { id: string; downloads: number; likes: number; updated: string; pipelineTag: string | null; gated: boolean; tags: string[] }
interface QuantOption { quant: string; files: string[]; size: number }
interface LocalModel { id: string; repo: string; quant: string; filePath: string; size: number }

function fmtCount(n: number): string {
  if (n >= 1e6) return `${(n / 1e6).toFixed(1)}M`
  if (n >= 1e3) return `${(n / 1e3).toFixed(1)}k`
  return String(n)
}
function fmtSize(bytes: number): string {
  if (!bytes) return '—'
  const gb = bytes / 1e9
  return gb >= 1 ? `${gb.toFixed(2)} GB` : `${(bytes / 1e6).toFixed(0)} MB`
}
function relDate(iso: string): string {
  if (!iso) return ''
  const d = new Date(iso)
  const days = Math.floor((Date.now() - d.getTime()) / 86400000)
  if (days < 1) return 'today'
  if (days < 30) return `${days}d ago`
  if (days < 365) return `${Math.floor(days / 30)}mo ago`
  return `${Math.floor(days / 365)}y ago`
}

export function ModelsTab() {
  const { fetchModels, upsertModelOverride } = useProviders()
  const [query, setQuery] = useState('')
  const [models, setModels] = useState<HfModel[]>([])
  const [listLoading, setListLoading] = useState(true)
  const [listError, setListError] = useState('')

  const [selected, setSelected] = useState<HfModel | null>(null)
  const [quants, setQuants] = useState<QuantOption[]>([])
  const [quantsLoading, setQuantsLoading] = useState(false)
  const [quantsError, setQuantsError] = useState('')
  const [card, setCard] = useState('')
  const [cardLoading, setCardLoading] = useState(false)
  const [selectedCaps, setSelectedCaps] = useState<ModelCaps | null>(null)

  const [installed, setInstalled] = useState<Set<string>>(new Set())
  const [downloading, setDownloading] = useState<string | null>(null)
  const [progress, setProgress] = useState<{ downloaded: number; total: number } | null>(null)

  const refreshInstalled = async () => {
    const list = await invoke<LocalModel[]>('list_local_models')
    setInstalled(new Set(list.map(m => m.id)))
  }

  // Initial: trending + installed set + progress listener.
  useEffect(() => { refreshInstalled() }, [])
  useEffect(() => {
    const un = listen<{ downloaded: number; total: number }>('local_download_progress', e => setProgress(e.payload))
    return () => { un.then(f => f()) }
  }, [])

  // Debounced list: trending when query < 3 chars, else search.
  useEffect(() => {
    const q = query.trim()
    const t = setTimeout(async () => {
      setListLoading(true); setListError('')
      try {
        const res = q.length >= 3
          ? await invoke<HfModel[]>('hf_search_models', { query: q })
          : await invoke<HfModel[]>('hf_trending_models')
        setModels(res)
      } catch (e) {
        setListError(String(e))
      } finally {
        setListLoading(false)
      }
    }, q.length >= 3 ? 350 : 0)
    return () => clearTimeout(t)
  }, [query])

  // Load detail (quants + card) when a model is selected.
  useEffect(() => {
    if (!selected) return
    const repo = selected.id
    setQuants([]); setQuantsError(''); setQuantsLoading(true)
    setCard(''); setCardLoading(true)
    setSelectedCaps(null)
    // The repo isn't downloaded yet, so llama.cpp can't tell us anything: models.dev is
    // the only source that knows. It answers 'unknown' rather than guessing from tags.
    invoke<Record<string, ModelCaps>>('lookup_model_caps', { modelIds: [repo] })
      .then(r => setSelectedCaps(r[repo] ?? null))
      .catch(() => setSelectedCaps(null))
    invoke<QuantOption[]>('hf_list_quants', { url: repo })
      .then(setQuants)
      .catch(e => setQuantsError(String(e)))
      .finally(() => setQuantsLoading(false))
    invoke<string>('hf_model_card', { repo })
      .then(setCard)
      .catch(() => setCard(''))
      .finally(() => setCardLoading(false))
  }, [selected])

  const download = async (quant: string) => {
    if (!selected) return
    setDownloading(quant); setProgress(null)
    try {
      const model = await invoke<LocalModel>('download_local_model', { url: selected.id, quant })
      await upsertModelOverride({ provider_id: LOCAL_PROVIDER_ID, model_id: model.id, custom_name: undefined, enabled: true })
      await fetchModels(LOCAL_PROVIDER_ID)
      await refreshInstalled()
    } catch (e) {
      setQuantsError(String(e))
    } finally {
      setDownloading(null); setProgress(null)
    }
  }

  const pct = progress && progress.total > 0 ? Math.round((progress.downloaded / progress.total) * 100) : null
  const heading = useMemo(() => query.trim().length >= 3 ? 'Search results' : 'Trending', [query])

  return (
    <div className="flex h-full">
      {/* LEFT: search + list */}
      <div className="w-72 border-r border-border flex flex-col shrink-0">
        <div className="p-2 border-b border-border">
          <div className="relative">
            <Search size={13} className="absolute left-2.5 top-1/2 -translate-y-1/2 text-muted-foreground/60" />
            <Input
              value={query}
              onChange={e => setQuery(e.target.value)}
              placeholder="Search GGUF models…"
              className="pl-8 h-8 text-sm"
            />
          </div>
          <p className="text-[11px] uppercase tracking-wide text-muted-foreground/70 px-1 pt-2">{heading}</p>
        </div>
        <div className="flex-1 overflow-y-auto">
          {listLoading ? (
            <div className="flex items-center justify-center py-8 text-muted-foreground">
              <Loader2 size={16} className="animate-spin" />
            </div>
          ) : listError ? (
            <p className="text-xs text-red-400 p-3">{listError}</p>
          ) : models.length === 0 ? (
            <p className="text-xs text-muted-foreground p-3">No models found.</p>
          ) : (
            models.map(m => {
              const [owner, ...rest] = m.id.split('/')
              const name = rest.join('/')
              const active = selected?.id === m.id
              return (
                <button
                  key={m.id}
                  onClick={() => setSelected(m)}
                  className={`w-full text-left px-3 py-2.5 border-b border-border/50 transition-colors ${
                    active ? 'bg-primary/10' : 'hover:bg-accent/50'
                  }`}
                >
                  <div className="flex items-center gap-1.5">
                    <p className="text-sm text-foreground truncate flex-1">{name || owner}</p>
                    {m.gated && <Lock size={11} className="text-amber-400 shrink-0" />}
                  </div>
                  <p className="text-[11px] text-muted-foreground truncate">{owner}</p>
                  <div className="flex items-center gap-3 mt-1 text-[11px] text-muted-foreground">
                    <span className="flex items-center gap-1"><Download size={10} />{fmtCount(m.downloads)}</span>
                    <span className="flex items-center gap-1"><Heart size={10} />{fmtCount(m.likes)}</span>
                    {m.updated && <span>{relDate(m.updated)}</span>}
                  </div>
                </button>
              )
            })
          )}
        </div>
      </div>

      {/* RIGHT: detail */}
      <div className="flex-1 overflow-y-auto">
        {!selected ? (
          <div className="h-full flex flex-col items-center justify-center text-muted-foreground gap-2">
            <HardDrive size={28} className="opacity-40" />
            <p className="text-sm">Select a model to view quantizations & details.</p>
          </div>
        ) : (
          <div className="p-6 space-y-5">
            {/* Header */}
            <div>
              <div className="flex items-center gap-2">
                <h3 className="text-base font-semibold text-foreground">{selected.id.split('/').slice(1).join('/')}</h3>
                {selected.gated && <span className="flex items-center gap-1 text-[11px] text-amber-400"><Lock size={11} /> gated</span>}
                <a
                  href={`https://huggingface.co/${selected.id}`}
                  target="_blank"
                  rel="noreferrer"
                  className="text-muted-foreground hover:text-foreground"
                  title="Open on Hugging Face"
                >
                  <ExternalLink size={13} />
                </a>
              </div>
              <div className="flex items-center gap-4 mt-1 text-xs text-muted-foreground">
                <span>{selected.id.split('/')[0]}</span>
                <span className="flex items-center gap-1"><Download size={11} />{fmtCount(selected.downloads)}</span>
                <span className="flex items-center gap-1"><Heart size={11} />{fmtCount(selected.likes)}</span>
                {selected.updated && <span>updated {relDate(selected.updated)}</span>}
              </div>

              {/* Format + capabilities */}
              <div className="flex items-center flex-wrap gap-1.5 mt-2.5">
                <span className="flex items-center gap-1 text-[11px] font-medium rounded-md bg-primary/10 text-primary px-2 py-0.5">
                  <Boxes size={11} /> GGUF
                </span>
                {selectedCaps && selectedCaps.source !== 'unknown' && <>
                  {selectedCaps.vision && <span title={CAPS_SOURCE_LABEL[selectedCaps.source]} className="flex items-center gap-1 text-[11px] rounded-md bg-sky-500/10 text-sky-300 px-2 py-0.5"><Eye size={11} /> Vision</span>}
                  {selectedCaps.tools && <span title={CAPS_SOURCE_LABEL[selectedCaps.source]} className="flex items-center gap-1 text-[11px] rounded-md bg-violet-500/10 text-violet-300 px-2 py-0.5"><Wrench size={11} /> Tool use</span>}
                  {selectedCaps.reasoning && <span title={CAPS_SOURCE_LABEL[selectedCaps.source]} className="flex items-center gap-1 text-[11px] rounded-md bg-amber-500/10 text-amber-300 px-2 py-0.5"><Brain size={11} /> Reasoning</span>}
                </>}
                {selectedCaps?.source === 'unknown' && (
                  <span title={CAPS_SOURCE_LABEL.unknown} className="flex items-center gap-1 text-[11px] rounded-md bg-muted text-muted-foreground px-2 py-0.5">
                    <HelpCircle size={11} /> Capabilities unknown until downloaded
                  </span>
                )}
                {selected.pipelineTag && <span className="text-[11px] rounded-md bg-muted text-muted-foreground px-2 py-0.5">{selected.pipelineTag}</span>}
              </div>
            </div>

            {selected.gated && (
              <p className="text-xs text-amber-400/90 rounded-lg border border-amber-500/30 bg-amber-500/10 px-3 py-2">
                This is a gated repo, downloads may require a Hugging Face token (not yet supported).
              </p>
            )}

            {/* Quants */}
            <div>
              <p className="text-xs font-medium text-foreground mb-2">Quantizations</p>
              {quantsLoading ? (
                <div className="flex items-center gap-2 text-xs text-muted-foreground"><Loader2 size={13} className="animate-spin" /> Loading files…</div>
              ) : quantsError ? (
                <p className="text-xs text-red-400">{quantsError}</p>
              ) : quants.length === 0 ? (
                <p className="text-xs text-muted-foreground">No GGUF files in this repo.</p>
              ) : (
                <div className="rounded-lg border border-border divide-y divide-border/60">
                  {quants.map(q => {
                    const id = `${selected.id}::${q.quant}`
                    const isInstalled = installed.has(id)
                    const busy = downloading === q.quant
                    return (
                      <div key={q.quant} className="flex items-center gap-3 px-3 py-2">
                        <span className="text-sm font-mono text-foreground flex-1">{q.quant}</span>
                        <span className="text-xs text-muted-foreground w-20 text-right">{fmtSize(q.size)}</span>
                        {isInstalled ? (
                          <span className="flex items-center gap-1 text-xs text-green-400 w-24 justify-end"><Check size={13} /> Installed</span>
                        ) : busy ? (
                          <div className="w-24">
                            <div className="h-1.5 rounded-full bg-muted overflow-hidden">
                              <div className="h-full bg-primary transition-all" style={{ width: pct != null ? `${pct}%` : '100%' }} />
                            </div>
                            <p className="text-[10px] text-muted-foreground text-right mt-0.5">{pct != null ? `${pct}%` : '…'}</p>
                          </div>
                        ) : (
                          <Button onClick={() => download(q.quant)} disabled={!!downloading} size="xs" variant="ghost" className="gap-1 w-24 justify-end">
                            <Download size={13} /> Get
                          </Button>
                        )}
                      </div>
                    )
                  })}
                </div>
              )}
            </div>

            {/* Model card */}
            <div>
              <p className="text-xs font-medium text-foreground mb-2">Model card</p>
              {cardLoading ? (
                <div className="flex items-center gap-2 text-xs text-muted-foreground"><Loader2 size={13} className="animate-spin" /> Loading…</div>
              ) : card ? (
                <div className="prose prose-invert prose-sm max-w-none prose-headings:text-foreground prose-a:text-primary prose-code:text-foreground prose-img:rounded-md rounded-lg border border-border p-4 bg-muted/20 overflow-x-auto">
                  <Markdown remarkPlugins={[remarkGfm]} rehypePlugins={[rehypeRaw, rehypeSanitize]}>{card}</Markdown>
                </div>
              ) : (
                <p className="text-xs text-muted-foreground">No model card available.</p>
              )}
            </div>
          </div>
        )}
      </div>
    </div>
  )
}
