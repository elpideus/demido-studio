import { useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { Check, Download, RefreshCw, Trash2 } from 'lucide-react'
import { Button } from '@/components/ui/button'

interface InstallProgress { downloaded: number; total: number; stage: string }
interface SearxngProgress { stage: string }
interface PythonStatus {
  ready: boolean
  version: string | null
  searxngInstalled: boolean
  searxngRunning: boolean
}

function fmtSize(bytes: number): string {
  if (!bytes) return ''
  const mb = bytes / 1e6
  return mb >= 1000 ? `${(mb / 1000).toFixed(2)} GB` : `${mb.toFixed(0)} MB`
}

export function PythonTab() {
  const [status, setStatus] = useState<PythonStatus | null>(null)
  const [busy, setBusy] = useState<'' | 'python' | 'searxng'>('')
  const [progress, setProgress] = useState<InstallProgress | null>(null)
  const [stage, setStage] = useState('')
  const [error, setError] = useState('')

  const load = async () => {
    try {
      setStatus(await invoke<PythonStatus>('python_status'))
    } catch (e) {
      setError(String(e))
    }
  }

  useEffect(() => { load() }, [])
  useEffect(() => {
    const uns = [
      listen<InstallProgress>('python_install_progress', e => { setProgress(e.payload); setStage(e.payload.stage) }),
      listen<SearxngProgress>('searxng_install_progress', e => { setProgress(null); setStage(e.payload.stage) }),
    ]
    return () => { uns.forEach(u => u.then(f => f())) }
  }, [])

  // Every button here is one `run(kind, command)`: same busy/progress/error lifecycle, same
  // refresh afterwards, only the command name and which section spins differ.
  const run = async (kind: 'python' | 'searxng', command: string) => {
    setBusy(kind); setProgress(null); setStage(''); setError('')
    try {
      await invoke(command)
    } catch (e) {
      setError(String(e))
    } finally {
      setBusy(''); setProgress(null); setStage('')
      await load()
    }
  }

  const pct = progress && progress.total > 0 ? Math.round((progress.downloaded / progress.total) * 100) : null
  const ready = status?.ready ?? false

  const Progress = () => (
    <div className="space-y-1">
      <div className="h-1.5 rounded-full bg-muted overflow-hidden">
        <div className="h-full bg-primary transition-all" style={{ width: pct != null ? `${pct}%` : '100%' }} />
      </div>
      <p className="text-[11px] text-muted-foreground truncate">
        {stage || 'preparing'}…{' '}
        {progress && progress.total > 0 && `${fmtSize(progress.downloaded)} / ${fmtSize(progress.total)}`}
      </p>
    </div>
  )

  return (
    <div className="h-full overflow-y-auto p-6 space-y-6">
      {error && (
        <p className="text-xs text-red-400 break-words rounded-lg border border-red-500/30 bg-red-500/10 px-3 py-2">{error}</p>
      )}

      {/* Runtime: the foundation everything below runs on, so it reads as a status bar
          rather than as one more card in the tool grid. */}
      <section className="rounded-xl border border-border bg-muted/30 p-4">
        <div className="flex items-center justify-between gap-4 flex-wrap">
          <div className="min-w-0">
            <div className="flex items-center gap-2.5">
              <h3 className="text-sm font-semibold text-foreground">Python Runtime</h3>
              {ready ? (
                <span className="flex items-center gap-1 text-[11px] text-green-400">
                  <Check size={12} /> {status?.version ? `Python ${status.version}` : 'Installed'}
                </span>
              ) : status && (
                <span className="text-[11px] text-muted-foreground">Not installed</span>
              )}
            </div>
            <p className="text-xs text-muted-foreground mt-0.5">
              Self-contained and portable, no system Python required. Installs itself on first
              launch and powers the tools below.
            </p>
          </div>

          <div className="flex items-center gap-2 shrink-0">
            <Button onClick={load} variant="ghost" size="icon-xs" title="Refresh" disabled={!status}>
              <RefreshCw size={14} />
            </Button>
            <Button onClick={() => run('python', 'install_python')} disabled={!status || !!busy} size="xs" className="gap-1.5">
              <Download size={12} />
              {ready ? 'Reinstall' : 'Install'}
            </Button>
            {ready && (
              <Button
                onClick={() => run('python', 'uninstall_python')}
                disabled={!!busy}
                size="xs"
                variant="ghost"
                className="gap-1.5 text-red-400 hover:text-red-400"
                title="Removes the runtime and everything installed into it, including the tools' dependencies"
              >
                <Trash2 size={12} /> Uninstall
              </Button>
            )}
          </div>
        </div>

        {busy === 'python' && <div className="mt-3"><Progress /></div>}
      </section>

      <section className="space-y-2.5">
        <h3 className="text-sm font-semibold text-foreground">Tools</h3>
        {/* auto-fill so the column count follows the panel width instead of a fixed breakpoint */}
        <div className="grid gap-3" style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(260px, 1fr))' }}>
          <div className="rounded-xl border border-border p-4 flex flex-col">
            <div className="flex items-center gap-2.5 mb-1">
              <span className="text-sm font-medium text-foreground">SearXNG</span>
              {status?.searxngInstalled && (
                <span className="flex items-center gap-1 text-[11px] text-green-400">
                  <Check size={12} /> {status.searxngRunning ? 'Running' : 'Installed'}
                </span>
              )}
            </div>
            <p className="text-xs text-muted-foreground mb-3 flex-1">
              Private metasearch over ~200 engines, running inside Demido on no network port.
              Enable it as a search provider in Tools &gt; Web Browsing.
            </p>

            {busy === 'searxng' ? <Progress /> : (
              <div className="flex items-center gap-2 flex-wrap">
                <Button onClick={() => run('searxng', 'install_searxng')} disabled={!status || !!busy} size="xs" className="gap-1.5">
                  <Download size={12} />
                  {status?.searxngInstalled ? 'Reinstall' : 'Install'}
                </Button>
                {status?.searxngInstalled && (
                  <>
                    {/* No Stop: the worker is owned by the app (it exits with it) and is what
                        `web_search` falls back to; stopping it by hand only breaks search.
                        Switch it off in Tools > Web Browsing instead. */}
                    {!status.searxngRunning && (
                      <Button onClick={() => run('searxng', 'start_searxng')} disabled={!!busy} size="xs" variant="secondary">
                        Start
                      </Button>
                    )}
                    <Button
                      onClick={() => run('searxng', 'uninstall_searxng')}
                      disabled={!!busy}
                      size="xs"
                      variant="ghost"
                      className="gap-1.5 text-red-400 hover:text-red-400"
                    >
                      <Trash2 size={12} /> Uninstall
                    </Button>
                  </>
                )}
              </div>
            )}
          </div>
        </div>
      </section>
    </div>
  )
}
