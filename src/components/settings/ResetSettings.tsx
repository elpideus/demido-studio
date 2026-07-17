import { useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { AlertTriangle } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'

/// Typing the word is deliberate friction: this destroys data with no undo.
const CONFIRM_WORD = 'RESET'

type Scope = {
  key: string
  label: string
  hint: string
  /// Off by default: the big downloads. Losing them costs GBs to fetch again, and they hold
  /// nothing personal, so a "reset my stuff" almost never means "redownload everything".
  heavy?: boolean
}

const GROUPS: { title: string; scopes: Scope[] }[] = [
  {
    title: 'Your data',
    scopes: [
      { key: 'conversations',  label: 'Conversations',    hint: 'Every chat and its messages.' },
      { key: 'settings',       label: 'Settings',         hint: 'App preferences and the global system prompt.' },
      { key: 'providers',      label: 'Providers & API keys', hint: 'Cloud providers, model overrides and stored keys.' },
      { key: 'mcpServers',     label: 'MCP servers',      hint: 'Configured tool servers.' },
      { key: 'googleAccounts', label: 'Google accounts',  hint: 'Connected accounts and their tokens.' },
      { key: 'skills',         label: 'Skills',           hint: 'Installed skills.' },
      { key: 'firstRunDialogs', label: 'First-run dialogs', hint: 'The disclaimer and the setup wizard show again on next start.' },
    ],
  },
  {
    title: 'Downloads',
    scopes: [
      { key: 'inferenceRuntime', label: 'Inference runtime', hint: 'The downloaded llama.cpp runtime.', heavy: true },
      { key: 'pythonRuntime',    label: 'Python runtime',    hint: 'The portable Python install.', heavy: true },
      { key: 'pythonTools',      label: 'Python tools',      hint: 'SearXNG and anything built on the Python runtime.', heavy: true },
      { key: 'models',           label: 'Models',            hint: 'GGUF models downloaded into Demido. A models folder you chose yourself is left alone.', heavy: true },
    ],
  },
]

const ALL = GROUPS.flatMap(g => g.scopes)

const defaults = () => Object.fromEntries(ALL.map(s => [s.key, !s.heavy])) as Record<string, boolean>

export function ResetSettings() {
  const [sel, setSel] = useState<Record<string, boolean>>(defaults)
  const [confirming, setConfirming] = useState(false)
  const [typed, setTyped] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')

  const chosen = ALL.filter(s => sel[s.key])
  const toggle = (key: string, v: boolean) => setSel(p => ({ ...p, [key]: v }))

  const reset = async () => {
    setBusy(true); setError('')
    try {
      // The backend never sees localStorage, so anything stored there is cleared here, scoped
      // to what was picked. Deletes on the next boot: the app restarts out from under us.
      if (sel.firstRunDialogs || sel.settings) localStorage.removeItem('disclaimer_accepted')
      if (sel.settings) localStorage.removeItem('toolPopup:searchTools')
      await invoke('reset_app_data', { request: sel })
    } catch (e) {
      setError(String(e))
      setBusy(false)
    }
  }

  return (
    <div className="space-y-5 max-w-xl">
      <div>
        <h3 className="text-sm font-semibold text-foreground flex items-center gap-2">
          <AlertTriangle size={14} className="text-red-400" /> Reset data
        </h3>
        <p className="text-xs text-muted-foreground mt-1 leading-relaxed">
          Pick what to erase. Demido restarts afterwards and anything you erase comes back empty,
          as on a fresh install. It cannot be undone.
        </p>
      </div>

      {GROUPS.map(group => (
        <div key={group.title} className="space-y-2">
          <div className="text-xs font-medium text-muted-foreground uppercase tracking-wide">{group.title}</div>
          {group.scopes.map(s => (
            <label key={s.key} className="flex items-start gap-2.5 cursor-pointer rounded-lg border border-border p-3 hover:bg-accent/40 transition-colors">
              <Checkbox
                checked={!!sel[s.key]}
                onCheckedChange={v => toggle(s.key, v === true)}
                className="mt-0.5"
              />
              <span className="text-xs text-muted-foreground leading-relaxed">
                <span className="text-foreground font-medium">{s.label}.</span>{' '}{s.hint}
              </span>
            </label>
          ))}
        </div>
      ))}

      {error && (
        <p className="text-xs text-red-400 break-words rounded-lg border border-red-500/30 bg-red-500/10 px-3 py-2">{error}</p>
      )}

      {!confirming ? (
        <Button
          onClick={() => { setTyped(''); setConfirming(true) }}
          size="sm"
          disabled={chosen.length === 0}
          className="bg-red-500/90 text-white hover:bg-red-500"
        >
          Reset {chosen.length} {chosen.length === 1 ? 'item' : 'items'}…
        </Button>
      ) : (
        <div className="rounded-lg border border-red-500/30 bg-red-500/5 p-3 space-y-3">
          <p className="text-xs text-muted-foreground leading-relaxed">
            About to permanently erase:{' '}
            <span className="text-foreground">{chosen.map(s => s.label).join(', ')}</span>.
          </p>
          <div>
            <label className="text-xs text-muted-foreground">
              Type <span className="font-mono text-foreground">{CONFIRM_WORD}</span> to confirm
            </label>
            <input
              value={typed}
              onChange={e => setTyped(e.target.value)}
              autoFocus
              className="mt-1 w-full bg-background border border-border rounded-lg px-3 py-2 text-sm text-foreground outline-none focus:border-ring/50 font-mono"
            />
          </div>
          <div className="flex items-center gap-2">
            <Button
              onClick={reset}
              size="sm"
              disabled={typed !== CONFIRM_WORD || busy}
              className="bg-red-500/90 text-white hover:bg-red-500"
            >
              {busy ? 'Resetting…' : 'Erase and restart'}
            </Button>
            <Button onClick={() => setConfirming(false)} variant="ghost" size="sm" disabled={busy}>Cancel</Button>
          </div>
        </div>
      )}
    </div>
  )
}
