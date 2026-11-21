/* The model runtime, for real: what is installed on this computer, what its publisher offers, and installing it.
   Everything here goes through the core (core/crates/demido-core/src/runtime). Nothing is scripted. */
import { useCallback, useEffect, useState } from 'react'
import { coreGet, coreSend, onCoreEvent } from '@/data/core'
import type { CoreAvailable, CoreInstalled, CoreOffer, CoreProgress, CoreRuntimeStatus } from '@/data/core'
import { toast } from '@/data/store'
import { Button, Glyph, NotBuilt, Signal, Switch } from '@/ui'

const MB = 1024 * 1024
const mb = (bytes: number) => (bytes < MB ? 'under 1 MB' : bytes < 10 * MB ? `${(bytes / MB).toFixed(1)} MB` : `${Math.round(bytes / MB)} MB`)

function words(p: CoreProgress): string {
  switch (p.step) {
    case 'downloading': return p.total > 0 ? `Downloading ${p.file ?? ''} · ${mb(p.done)} of ${mb(p.total)}` : `Downloading ${p.file ?? ''} · ${mb(p.done)}`
    case 'unpacking': return 'Unpacking'
    case 'trying': return 'Trying that it starts'
    default: return 'Asking the publisher what it offers'
  }
}

function InstalledRow({ r, onGone }: { r: CoreInstalled; onGone: () => void }) {
  const [sure, setSure] = useState(false)
  const [busy, setBusy] = useState(false)
  const remove = async () => {
    setBusy(true)
    try {
      await coreSend('DELETE', `/runtime/installed/${encodeURIComponent(r.id)}`)
      toast(`Removed ${r.name}. ${mb(r.size)} freed.`, undefined, 'task-done')
      onGone()
    } catch (e) {
      toast(`Not removed. ${(e as Error).message}`, undefined, 'task-failed')
    }
    setBusy(false)
    setSure(false)
  }
  return (
    <div className="d-setting">
      <div className="d-setting__text">
        <span className="d-setting__label"><Glyph name={r.version ? 'task-done' : 'task-failed'} label={r.version ? 'starts' : 'does not start'} />{r.name}<span className="d-badge">Installed</span></span>
        <span className="d-setting__desc">Build {r.build} · {mb(r.size)} on disk · {r.checked ? 'every file matched the checksum its publisher lists' : 'NOTE: its publisher listed no checksum, so the download was not checked'}.</span>
        {r.version && <span className="d-setting__desc d-text-tertiary">It starts. It says: <span className="d-mono">{r.version}</span></span>}
        {r.problem && <span className="d-setting__desc">CAUTION: it does not start. <span className="d-mono">{r.problem}</span></span>}
      </div>
      <div className="d-setting__control">
        {sure
          ? <><Button size="sm" variant="ghost" onClick={() => setSure(false)}>Keep</Button><Button size="sm" variant="danger" disabled={busy} onClick={remove}>Remove {mb(r.size)}</Button></>
          : <Button size="sm" variant="ghost" onClick={() => setSure(true)}>Remove</Button>}
      </div>
    </div>
  )
}

function OfferRow({ o, installed, busy, onInstall }: { o: CoreOffer; installed: boolean; busy: boolean; onInstall: (extra: boolean) => void }) {
  const [extra, setExtra] = useState(o.needs_extra)
  const total = o.size + (extra ? o.extra_size : 0)
  return (
    <div className="d-setting">
      <div className="d-setting__text">
        <span className="d-setting__label"><Glyph name={installed ? 'task-done' : 'task-pending'} label={installed ? 'installed' : 'not installed'} />{o.name}{o.recommended && <span className="d-badge">Recommended</span>}</span>
        <span className="d-setting__desc">{o.why}</span>
        <span className="d-setting__desc d-text-tertiary"><span className="d-mono">{o.file.name}</span> · {mb(o.size)}</span>
        {o.extra.length > 0 && (
          <span className="d-setting__desc app-row" style={{ gap: 'var(--d-space-8)' }}>
            <Switch label={`Also download NVIDIA's libraries, ${mb(o.extra_size)}`} checked={extra} onChange={setExtra} />
            <span>Also download NVIDIA's libraries, {mb(o.extra_size)}. {o.needs_extra ? 'This system does not seem to have them.' : 'This system seems to have them, so they are left out.'}</span>
          </span>
        )}
      </div>
      <div className="d-setting__control">
        <Button size="sm" variant={o.recommended && !installed ? 'primary' : undefined} disabled={busy} onClick={() => onInstall(extra)}>{installed ? 'Install again' : 'Download'} · {mb(total)}</Button>
      </div>
    </div>
  )
}

export function RealRuntime() {
  const [status, setStatus] = useState<CoreRuntimeStatus | null>(null)
  const [offered, setOffered] = useState<CoreAvailable | null>(null)
  const [asking, setAsking] = useState(false)
  const [problem, setProblem] = useState<string | null>(null)
  /** Progress as the core tells it, several times a second. Newer than `status.installing`. */
  const [told, setTold] = useState<CoreProgress | null>(null)

  const read = useCallback(async () => {
    try {
      setStatus(await coreGet<CoreRuntimeStatus>('/runtime'))
    } catch (e) {
      setProblem((e as Error).message)
    }
  }, [])

  useEffect(() => {
    void read()
    const ended = (say?: (message: string) => void) => (e: { data: Record<string, unknown> }) => {
      setTold(null)
      void read()
      say?.(String(e.data.message ?? ''))
    }
    const stops = [
      onCoreEvent('runtime.install.progress', (e) => setTold(e.data as unknown as CoreProgress)),
      onCoreEvent('runtime.installed', ended(() => toast('The runtime is installed.', undefined, 'task-done'))),
      onCoreEvent('runtime.install.failed', ended((message) => setProblem(message))),
      onCoreEvent('runtime.install.stopped', ended((message) => toast(message))),
      onCoreEvent('runtime.removed', ended()),
    ]
    return () => stops.forEach((stop) => stop())
  }, [read])

  const ask = async (fresh: boolean) => {
    setAsking(true)
    setProblem(null)
    try {
      setOffered(await coreGet<CoreAvailable>(`/runtime/available?fresh=${fresh}`))
    } catch (e) {
      setProblem((e as Error).message)
    }
    setAsking(false)
  }
  const install = async (o: CoreOffer, extra: boolean) => {
    setProblem(null)
    try {
      setTold(await coreSend<CoreProgress>('POST', '/runtime/install', { backend: o.id, extra }))
    } catch (e) {
      setProblem((e as Error).message)
    }
  }
  const stop = async () => {
    try {
      await coreSend('DELETE', '/runtime/install')
    } catch (e) {
      setProblem((e as Error).message)
    }
  }

  if (!status) return problem ? <Signal level="caution" actions={<Button size="sm" onClick={() => { setProblem(null); void read() }}>Try again</Button>}>{problem}</Signal> : <p className="d-text-secondary"><Glyph name="task-running" /> Looking for installed runtimes…</p>
  const installing = told ?? status.installing
  const percent = installing && installing.total > 0 ? Math.min(100, Math.round((installing.done / installing.total) * 100)) : null
  return (
    <div className="app-stack">
      {problem && <Signal level="caution" actions={<Button size="sm" variant="ghost" onClick={() => setProblem(null)}>Dismiss</Button>}>{problem}</Signal>}
      <div className="d-settings">
        {status.installed.map((r) => <InstalledRow key={r.id} r={r} onGone={read} />)}
        {status.installed.length === 0 && !installing && (
          <div className="d-setting"><div className="d-setting__text">
            <span className="d-setting__label"><Glyph name="task-pending" label="none" />No runtime is installed</span>
            <span className="d-setting__desc">Models cannot run without one. It is downloaded from the people who make it; Demido does not ship it.</span>
          </div></div>
        )}
        {installing && (
          <div className="d-setting">
            <div className="d-setting__text">
              <span className="d-setting__label"><Glyph name="task-running" busy label="installing" />{installing.name}</span>
              <span className="d-setting__desc">{words(installing)}{percent !== null && installing.step === 'downloading' ? ` · ${percent}%` : ''}</span>
            </div>
            <div className="d-setting__control"><Button size="sm" onClick={stop}>Stop</Button></div>
            <div className="d-meter" style={{ gridColumn: '1 / -1' }} role="meter" aria-valuenow={percent ?? 0} aria-valuemin={0} aria-valuemax={100} aria-label={`Installing ${installing.name}`}><span style={{ '--d-meter': `${percent ?? 0}%` } as React.CSSProperties} /></div>
          </div>
        )}
      </div>

      {!offered && (
        <div className="app-row">
          <Button variant={status.installed.length === 0 ? 'primary' : undefined} disabled={asking} onClick={() => ask(false)}>{asking ? 'Asking…' : 'See what is offered'}</Button>
          <span className="d-type-caption d-text-tertiary">Asks github.com which build of {status.runtime} is current. Nothing is downloaded until you choose.</span>
        </div>
      )}
      {offered && (
        <div>
          <div className="app-row">
            <p className="d-list__label" style={{ paddingInline: 0 }}>{offered.runtime} {offered.release} (build {offered.build}), as its publisher offers it for this computer</p>
            <span className="app-spacer" />
            <Button size="sm" variant="ghost" disabled={asking} onClick={() => ask(true)}>{asking ? 'Asking…' : 'Ask again'}</Button>
          </div>
          <div className="d-settings">
            {offered.offers.map((o) => <OfferRow key={o.id} o={o} busy={Boolean(installing)} installed={status.installed.some((r) => r.build === offered.build && r.backend === o.id)} onInstall={(extra) => install(o, extra)} />)}
            {offered.offers.length === 0 && <p className="app-empty">Nothing in this build suits this computer.</p>}
          </div>
          <p className="d-type-caption d-text-tertiary">Each file is checked against the SHA-256 its publisher lists, and a download that stops goes on from where it stopped. Kept in <span className="d-mono">{status.folder}</span>.</p>
        </div>
      )}
      <NotBuilt block>Starting the runtime with a model, and stopping it when idle. Installing it is real; nothing uses it yet.</NotBuilt>
    </div>
  )
}
