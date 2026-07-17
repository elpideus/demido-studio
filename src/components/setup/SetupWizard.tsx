import { useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { AlertTriangle, Apple, Check, CircuitBoard, Cpu, Globe, Loader2 } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'

interface Hardware { os: string; arch: string; gpus: string[]; recommended: string }
interface VariantInfo {
  id: string
  label: string
  available: boolean
  installed: boolean
  recommended: boolean
  note: string
}
interface Progress { downloaded: number; total: number; stage: string }

const ICONS: Record<string, typeof Cpu> = { cuda: CircuitBoard, hip: CircuitBoard, metal: Apple, cpu: Cpu }
const ACCENT: Record<string, string> = {
  cuda: 'text-green-400',
  hip: 'text-red-400',
  metal: 'text-sky-300',
  cpu: 'text-amber-400',
}

function fmtSize(bytes: number): string {
  if (!bytes) return ''
  const gb = bytes / 1e9
  return gb >= 1 ? `${gb.toFixed(2)} GB` : `${(bytes / 1e6).toFixed(0)} MB`
}

/// One selectable install option: same shape for a runtime variant and for a Python tool.
function OptionRow({
  icon: Icon, accent, label, note, checked, disabled, onToggle, highlight,
}: {
  icon: typeof Cpu
  accent?: string
  label: string
  note: string
  checked: boolean
  disabled?: boolean
  onToggle: (v: boolean) => void
  highlight?: boolean
}) {
  return (
    <label
      className={`flex items-start gap-3 rounded-xl border p-3.5 transition-colors ${
        disabled
          ? 'border-border/50 opacity-50 cursor-not-allowed'
          : `cursor-pointer ${highlight ? 'border-primary/60 bg-primary/5' : 'border-border hover:bg-muted/40'}`
      }`}
    >
      <Checkbox
        checked={checked}
        disabled={disabled}
        onCheckedChange={v => onToggle(v === true)}
        className="mt-0.5"
      />
      <div className="min-w-0">
        <div className="flex items-center gap-2">
          <Icon size={16} className={disabled ? 'text-muted-foreground' : accent} />
          <span className="text-sm font-medium text-foreground">{label}</span>
        </div>
        <p className="text-[11px] text-muted-foreground mt-0.5 flex items-start gap-1.5">
          {!disabled && !highlight && note.includes('No matching GPU') && (
            <AlertTriangle size={11} className="mt-0.5 shrink-0 text-amber-400/90" />
          )}
          {note}
        </p>
      </div>
    </label>
  )
}

/// First-run setup: pick what to download now. Nothing has been fetched at this point;
/// the installer ships only the app, so every heavy payload (llama.cpp runtimes, the Python
/// runtime, SearXNG) is chosen here and pulled on demand.
export function SetupWizard({ onDone }: { onDone: () => void }) {
  const [step, setStep] = useState<0 | 1 | 2>(0)
  const [hw, setHw] = useState<Hardware | null>(null)
  const [variants, setVariants] = useState<VariantInfo[]>([])
  const [pyVersion, setPyVersion] = useState<string | null>(null)
  const [picked, setPicked] = useState<Record<string, boolean>>({})
  const [python, setPython] = useState(true)
  const [searxng, setSearxng] = useState(true)
  const [error, setError] = useState('')
  const [stage, setStage] = useState('')
  const [progress, setProgress] = useState<Progress | null>(null)
  const [done, setDone] = useState<string[]>([])

  useEffect(() => {
    invoke<[Hardware, VariantInfo[]]>('list_runtime_variants')
      .then(([hardware, list]) => {
        setHw(hardware)
        setVariants(list)
        // Pre-tick the detected-hardware match, which is the choice that works for most people.
        const rec = list.find(v => v.recommended && v.available)
        if (rec) setPicked({ [rec.id]: true })
      })
      .catch(e => setError(String(e)))
    invoke<string>('python_available_version').then(setPyVersion).catch(() => {})
  }, [])

  useEffect(() => {
    const uns = [
      listen<Progress>('local_runtime_progress', e => { setProgress(e.payload); setStage(e.payload.stage) }),
      listen<Progress>('python_install_progress', e => { setProgress(e.payload); setStage(e.payload.stage) }),
      listen<{ stage: string }>('searxng_install_progress', e => { setProgress(null); setStage(e.payload.stage) }),
    ]
    return () => { uns.forEach(u => u.then(f => f())) }
  }, [])

  const chosenRuntimes = variants.filter(v => picked[v.id] && v.available)

  const finish = async () => {
    setStep(2); setError(''); setDone([])
    try {
      for (const v of chosenRuntimes) {
        setStage(`Installing ${v.label}`); setProgress(null)
        await invoke('install_runtime_variant', { id: v.id })
        setDone(d => [...d, v.label])
      }
      // SearXNG's installer pulls the Python runtime itself, so asking for both is not two
      // downloads of Python: `install_searxng` no-ops on an already-present runtime.
      if (python && !searxng) {
        setStage('Installing Python runtime'); setProgress(null)
        await invoke('install_python')
        setDone(d => [...d, 'Python runtime'])
      }
      if (searxng) {
        setStage('Installing SearXNG'); setProgress(null)
        await invoke('install_searxng')
        setDone(d => [...d, python ? 'Python runtime + SearXNG' : 'SearXNG'])
        await invoke('set_setting', { key: 'websearch_searxng_enabled', value: JSON.stringify('true') })
        await invoke('start_searxng').catch(() => {})
      }
    } catch (e) {
      setError(String(e))
      return
    } finally {
      setProgress(null); setStage('')
    }
    await invoke('complete_setup').catch(() => {})
    onDone()
  }

  const skip = async () => {
    await invoke('complete_setup').catch(() => {})
    onDone()
  }

  const pct = progress && progress.total > 0 ? Math.round((progress.downloaded / progress.total) * 100) : null

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm">
      <div className="bg-background border border-border rounded-xl shadow-2xl w-full max-w-lg mx-4 p-6 flex flex-col gap-4">
        <div>
          <h2 className="text-lg font-semibold text-foreground">
            {step === 0 ? 'Inference Runtime' : step === 1 ? 'Python Runtime & Tools' : 'Installing'}
          </h2>
          <p className="text-xs text-muted-foreground mt-1">
            {step === 0
              ? 'Pick the llama.cpp backend to download for running models locally. Demido keeps the installer small by fetching only what you choose.'
              : step === 1
                ? 'Optional extras. You can install or remove these later in Settings > Engine > Python.'
                : 'Downloading your selections. This can take a few minutes.'}
          </p>
        </div>

        {hw && step === 0 && (
          <div className="rounded-lg border border-border bg-muted/30 px-3 py-2 text-xs text-muted-foreground">
            <span className="text-foreground font-medium">Detected:</span>{' '}
            {hw.os} · {hw.arch}
            {hw.gpus.length > 0 && <> · {hw.gpus.join(', ')}</>}
          </div>
        )}

        {error && (
          <p className="text-xs text-red-400 break-words rounded-lg border border-red-500/30 bg-red-500/10 px-3 py-2">{error}</p>
        )}

        {step === 0 && (
          <div className="space-y-2">
            {!variants.length && !error && (
              <p className="text-xs text-muted-foreground flex items-center gap-2">
                <Loader2 size={12} className="animate-spin" /> Detecting your hardware…
              </p>
            )}
            {variants.map(v => (
              <OptionRow
                key={v.id}
                icon={ICONS[v.id] ?? Cpu}
                accent={ACCENT[v.id]}
                label={v.label}
                note={v.note}
                checked={!!picked[v.id] && v.available}
                disabled={!v.available}
                highlight={v.recommended && v.available}
                onToggle={val => setPicked(p => ({ ...p, [v.id]: val }))}
              />
            ))}
          </div>
        )}

        {step === 1 && (
          <div className="space-y-2">
            <OptionRow
              icon={Cpu}
              accent="text-sky-300"
              label={`Python Runtime${pyVersion ? ` (${pyVersion})` : ''}`}
              note="Self-contained and portable, no system Python required. Powers the tools below."
              checked={python || searxng}
              // SearXNG runs on it, so it can't be dropped while SearXNG is selected.
              disabled={searxng}
              onToggle={setPython}
            />
            <OptionRow
              icon={Globe}
              accent="text-violet-300"
              label="SearXNG"
              note="Private metasearch over ~200 engines (Google, Bing, Brave…), running inside Demido on no network port. Needs the Python runtime."
              checked={searxng}
              onToggle={v => { setSearxng(v); if (v) setPython(true) }}
            />
          </div>
        )}

        {step === 2 && (
          <div className="space-y-3">
            {done.map(d => (
              <p key={d} className="text-xs text-green-400 flex items-center gap-1.5">
                <Check size={12} /> {d}
              </p>
            ))}
            {!error && (
              <div className="space-y-1">
                <div className="h-1.5 rounded-full bg-muted overflow-hidden">
                  <div className="h-full bg-primary transition-all" style={{ width: pct != null ? `${pct}%` : '100%' }} />
                </div>
                <p className="text-[11px] text-muted-foreground truncate">
                  {stage || 'Preparing'}…{' '}
                  {progress && progress.total > 0 && `${fmtSize(progress.downloaded)} / ${fmtSize(progress.total)}`}
                </p>
              </div>
            )}
          </div>
        )}

        <div className="flex items-center justify-between gap-2 mt-1">
          <Button onClick={skip} variant="ghost" size="sm" disabled={step === 2 && !error}>
            {step === 2 && error ? 'Continue anyway' : 'Skip for now'}
          </Button>
          <div className="flex items-center gap-2">
            {step === 1 && (
              <Button onClick={() => setStep(0)} variant="ghost" size="sm">Back</Button>
            )}
            {step === 0 && (
              <Button
                onClick={() => setStep(1)}
                size="sm"
                // Spec: no proceeding without a runtime. CPU is always available as the out.
                disabled={chosenRuntimes.length === 0}
                title={chosenRuntimes.length === 0 ? 'Select at least one runtime to continue' : undefined}
              >
                Next
              </Button>
            )}
            {step === 1 && <Button onClick={finish} size="sm">Install</Button>}
            {step === 2 && error && (
              <Button onClick={() => setStep(1)} variant="ghost" size="sm">Back</Button>
            )}
          </div>
        </div>
      </div>
    </div>
  )
}
