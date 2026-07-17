import { useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { open } from '@tauri-apps/plugin-dialog'
import { Check, Download, AlertTriangle, Cpu, CircuitBoard, Apple, RefreshCw, FolderOpen, X } from 'lucide-react'
import { useProviders } from '../../../stores/providers'
import { LOCAL_PROVIDER_ID } from '../LocalProviderCard'
import { Button } from '@/components/ui/button'

interface Hardware { os: string; arch: string; gpus: string[]; recommended: string }
interface VariantInfo {
  id: string
  label: string
  available: boolean
  installed: boolean
  recommended: boolean
  note: string
}
interface RuntimeProgress { downloaded: number; total: number; stage: string }

function fmtSize(bytes: number): string {
  if (!bytes) return ''
  const gb = bytes / 1e9
  return gb >= 1 ? `${gb.toFixed(2)} GB` : `${(bytes / 1e6).toFixed(0)} MB`
}

const ICONS: Record<string, typeof Cpu> = {
  cuda: CircuitBoard,
  hip: CircuitBoard,
  metal: Apple,
  cpu: Cpu,
}
const ACCENT: Record<string, string> = {
  cuda: 'text-green-400',
  hip: 'text-red-400',
  metal: 'text-sky-300',
  cpu: 'text-amber-400',
}

export function RuntimeTab() {
  const { fetchModels } = useProviders()
  const [hw, setHw] = useState<Hardware | null>(null)
  const [variants, setVariants] = useState<VariantInfo[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [installing, setInstalling] = useState<string | null>(null)
  const [progress, setProgress] = useState<RuntimeProgress | null>(null)
  const [modelsDirs, setModelsDirs] = useState<string[]>([])
  const [scanning, setScanning] = useState(false)

  const load = async () => {
    setLoading(true); setError('')
    try {
      const [hardware, list] = await invoke<[Hardware, VariantInfo[]]>('list_runtime_variants')
      setHw(hardware); setVariants(list)
    } catch (e) {
      setError(String(e))
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => { load() }, [])
  useEffect(() => { invoke<string[]>('get_models_dirs').then(setModelsDirs).catch(() => {}) }, [])
  useEffect(() => {
    const un = listen<RuntimeProgress>('local_runtime_progress', e => setProgress(e.payload))
    return () => { un.then(f => f()) }
  }, [])

  const applyDirs = async (dirs: string[]) => {
    setError('')
    try {
      await invoke('set_models_dirs', { paths: dirs })
      setModelsDirs(await invoke<string[]>('get_models_dirs'))
      await fetchModels(LOCAL_PROVIDER_ID)
    } catch (e) { setError(String(e)) }
  }

  const addDir = async () => {
    const picked = await open({ directory: true, title: 'Add models folder' })
    if (typeof picked === 'string' && !modelsDirs.includes(picked)) {
      await applyDirs([...modelsDirs, picked])
    }
  }

  const removeDir = (dir: string) => applyDirs(modelsDirs.filter(d => d !== dir))

  const rescan = async () => {
    setScanning(true); setError('')
    try {
      await invoke('scan_local_models')
      await fetchModels(LOCAL_PROVIDER_ID)
    } catch (e) {
      setError(String(e))
    } finally {
      setScanning(false)
    }
  }

  const install = async (id: string) => {
    setInstalling(id); setProgress(null); setError('')
    try {
      await invoke('install_runtime_variant', { id })
      await load()
    } catch (e) {
      setError(String(e))
    } finally {
      setInstalling(null); setProgress(null)
    }
  }

  const pct = progress && progress.total > 0 ? Math.round((progress.downloaded / progress.total) * 100) : null

  return (
    <div className="h-full overflow-y-auto p-6 space-y-5">
      <div className="flex items-start justify-between gap-4">
        <div>
          <h3 className="text-sm font-semibold text-foreground">Inference Runtime</h3>
          <p className="text-xs text-muted-foreground mt-0.5">
            Download the llama.cpp backend that matches your hardware. Models run fully
            enclosed inside Demido Studio.
          </p>
        </div>
        <Button onClick={load} variant="ghost" size="icon-xs" title="Refresh" disabled={loading}>
          <RefreshCw size={14} className={loading ? 'animate-spin' : ''} />
        </Button>
      </div>

      {/* Detected hardware */}
      {hw && (
        <div className="rounded-lg border border-border bg-muted/30 px-3 py-2 text-xs text-muted-foreground">
          <span className="text-foreground font-medium">Detected:</span>{' '}
          {hw.os} · {hw.arch}
          {hw.gpus.length > 0 && <> · {hw.gpus.join(', ')}</>}
        </div>
      )}

      {/* Models folders */}
      <div className="rounded-lg border border-border p-3 space-y-2">
        <div className="flex items-center justify-between gap-2">
          <p className="text-xs font-medium text-foreground">Models folders</p>
          <div className="flex items-center gap-1.5 shrink-0">
            <Button onClick={rescan} variant="ghost" size="xs" disabled={scanning} className="gap-1">
              <RefreshCw size={12} className={scanning ? 'animate-spin' : ''} /> Rescan
            </Button>
            <Button onClick={addDir} size="xs" className="gap-1">
              <FolderOpen size={12} /> Add folder…
            </Button>
          </div>
        </div>
        <div className="space-y-1">
          {modelsDirs.map((dir, i) => (
            <div key={dir} className="flex items-center gap-2 rounded-md bg-muted/30 px-2 py-1">
              <span className="text-[11px] text-muted-foreground font-mono truncate flex-1" title={dir}>
                {dir}
              </span>
              {i === 0 && (
                <span className="text-[10px] text-primary shrink-0">downloads</span>
              )}
              {modelsDirs.length > 1 && (
                <Button
                  onClick={() => removeDir(dir)}
                  variant="ghost"
                  size="icon-xs"
                  title="Stop scanning this folder"
                  className="text-muted-foreground hover:text-destructive shrink-0"
                >
                  <X size={12} />
                </Button>
              )}
            </div>
          ))}
        </div>
        <p className="text-[11px] text-muted-foreground">
          Every folder is scanned for models organised as{' '}
          <span className="font-mono text-foreground/80">owner/name</span>. Downloads go to the
          first one.
        </p>
      </div>

      {error && (
        <p className="text-xs text-red-400 break-words rounded-lg border border-red-500/30 bg-red-500/10 px-3 py-2">{error}</p>
      )}

      {loading && !variants.length ? (
        <p className="text-xs text-muted-foreground">Checking latest release…</p>
      ) : (
        <div className="grid grid-cols-2 gap-3">
          {variants.map(v => {
            const Icon = ICONS[v.id] ?? Cpu
            const busy = installing === v.id
            return (
              <div
                key={v.id}
                className={`relative rounded-xl border p-4 transition-colors ${
                  v.recommended
                    ? 'border-primary/60 bg-primary/5'
                    : v.available
                      ? 'border-border'
                      : 'border-border/50 opacity-60'
                }`}
              >
                <div className="flex items-center gap-2.5 mb-2">
                  <Icon size={20} className={v.available ? ACCENT[v.id] : 'text-muted-foreground'} />
                  <span className="text-sm font-medium text-foreground">{v.label}</span>
                  {v.installed && (
                    <span className="flex items-center gap-1 text-[11px] text-green-400">
                      <Check size={12} /> Installed
                    </span>
                  )}
                </div>

                <p className={`text-xs flex items-start gap-1.5 min-h-[2.5rem] ${
                  v.available && !v.recommended && v.id !== 'cpu' ? 'text-amber-400/90' : 'text-muted-foreground'
                }`}>
                  {v.available && !v.recommended && v.id !== 'cpu' && (
                    <AlertTriangle size={12} className="mt-0.5 shrink-0" />
                  )}
                  {v.note}
                </p>

                {busy ? (
                  <div className="mt-2 space-y-1">
                    <div className="h-1.5 rounded-full bg-muted overflow-hidden">
                      <div className="h-full bg-primary transition-all" style={{ width: pct != null ? `${pct}%` : '100%' }} />
                    </div>
                    <p className="text-[11px] text-muted-foreground">
                      {progress?.stage ?? 'preparing'}…{' '}
                      {progress && progress.total > 0 && `${fmtSize(progress.downloaded)} / ${fmtSize(progress.total)}`}
                    </p>
                  </div>
                ) : (
                  <Button
                    onClick={() => install(v.id)}
                    disabled={!v.available || !!installing}
                    size="xs"
                    variant={v.recommended ? 'default' : 'ghost'}
                    className="mt-2 gap-1.5"
                  >
                    <Download size={12} />
                    {v.installed ? 'Reinstall' : 'Install'}
                  </Button>
                )}
              </div>
            )
          })}
        </div>
      )}
    </div>
  )
}
