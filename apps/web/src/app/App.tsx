import { Component, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react'
import { NavLink, Navigate, Route, Routes, useLocation, useNavigate } from 'react-router'
import { newSession } from '@/data/real-sessions'
import { firstModel } from '@/features/sessions/real'
import { Popover } from '@base-ui/react/popover'
import { coreGet, useCore, type CoreSystem } from '@/data/core'
import { unloadLocal, useLocal } from '@/data/local-models'
import { applyAppearance, dismissToast, memoryBudget, startCore, useStore } from '@/data/store'
import { Budget, Button, Glyph, MODEL_STATE_LABEL, Signal, Tip, TooltipProvider } from '@/ui'
import { allFeatures, GROUPS } from './registry'
import logo from '@brand/logo/demido-symbol-small.svg?raw'

export function App() {
  useEffect(() => applyAppearance(), [])
  // A right click is the window's: what is under it offers its own menu (ContextMenu), and where nothing does, no
  // web page menu (back, reload, inspect) appears. Fields, links and selected text keep the browser's own.
  useEffect(() => {
    const own = (e: MouseEvent) => {
      const at = e.target instanceof Element ? e.target : null
      if (e.defaultPrevented || at?.closest('input, textarea, select, [contenteditable="true"], a[href]') || window.getSelection()?.toString()) return
      e.preventDefault()
    }
    document.addEventListener('contextmenu', own)
    return () => document.removeEventListener('contextmenu', own)
  }, [])
  // If a core is behind these pages, its settings become the real ones.
  useEffect(() => void startCore(), [])
  const features = allFeatures()
  const home = features.find((f) => f.id === 'sessions')
  const { pathname } = useLocation()
  // A feature may ask for the whole window (voice mode does). The rail steps aside; the feature shows the way back.
  const immersive = features.some((f) => f.immersive?.some((p) => pathname === `/${p}` || pathname.startsWith(`/${p}/`)))
  // A core is here, but this browser is not signed in to it: every screen then shows the scripted preview, and
  // the window says so on every screen, not only on Setup.
  const core = useCore()
  const locked = core.state === 'locked'
  const navigate = useNavigate()
  // The window is named for the screen it shows (a session names it itself, with its title).
  useEffect(() => {
    const here = features.find((f) => pathname === `/${f.path}` || pathname.startsWith(`/${f.path}/`))
    if (here && (here.id !== 'sessions' || pathname === '/sessions')) document.title = `${here.title} · Demido Studio`
    else if (!here) document.title = 'Demido Studio'
  }, [pathname, features])
  // A few keys, as a desktop app has them: Ctrl+, opens Settings; Ctrl+Shift+O starts a session; / goes to the
  // message box. None of them while typing, except the first two.
  useEffect(() => {
    const keys = async (e: KeyboardEvent) => {
      // A key something took already (the live browser's page, an editor) is not the window's.
      if (e.defaultPrevented) return
      const typing = e.target instanceof Element && e.target.closest('input, textarea, select, [contenteditable="true"], .lb__view')
      const mod = e.ctrlKey || e.metaKey
      if (mod && !e.shiftKey && e.key === ',') { e.preventDefault(); navigate('/settings') }
      else if (mod && e.shiftKey && e.key.toLowerCase() === 'o') {
        e.preventDefault()
        if (core.state === 'ready') { const id = await newSession(firstModel()); if (id) navigate(`/sessions/${id}`) } else navigate('/sessions')
      } else if (!typing && !mod && !e.altKey && e.key === '/') {
        const box = document.querySelector<HTMLTextAreaElement>('.chat__input textarea')
        if (box) { e.preventDefault(); box.focus() }
      }
    }
    window.addEventListener('keydown', keys)
    return () => window.removeEventListener('keydown', keys)
  }, [navigate, core.state])
  return (
    <TooltipProvider delay={500} closeDelay={0}>
      <div className="app" data-immersive={immersive ? '' : undefined}>
        {!immersive && <Rail />}
        <main className="app-main" data-notice={locked ? '' : undefined} onPointerDownCapture={(e) => { if (!(e.target as Element).closest('.app-side')) document.documentElement.classList.remove('side-open') }}>
          {locked && (
            <div className="app-locked">
              <Signal level="caution" actions={<Button size="sm" onClick={() => window.location.reload()}>Look again</Button>}>
                Demido {core.version} is running on this computer, but this browser is not signed in to it, so every screen shows the scripted preview. Run <code>demido open</code> in a terminal to sign this browser in.
              </Signal>
            </div>
          )}
          {/* Another screen comes in with a short fade; moving within one screen does not. */}
          <div className="app-route" key={pathname.split('/')[1] ?? ''}>
            <Fallen key={pathname}>
            <Routes>
              {features.map((f) => (
                <Route key={f.id} path={`${f.path}/*`} element={<f.Page />} />
              ))}
              <Route path="*" element={<Navigate to={home?.home ?? `/${features[0].path}`} replace />} />
            </Routes>
            </Fallen>
          </div>
        </main>
        <Toasts />
      </div>
    </TooltipProvider>
  )
}

/** A screen that failed while it was drawn: said, with a way on, instead of an empty window. Another screen (or
    the same one opened again) starts over. */
class Fallen extends Component<{ children: ReactNode }, { problem: Error | null }> {
  state = { problem: null as Error | null }
  static getDerivedStateFromError(problem: Error) { return { problem } }
  render() {
    const { problem } = this.state
    if (!problem) return this.props.children
    const parts = /dynamically imported module|Importing a module script failed/i.test(problem.message)
    return (
      <div className="app-page"><div className="app-page__inner app-page__inner--narrow">
        <div className="d-empty">
          <span className="d-empty__art" aria-hidden="true"><Glyph name="task-failed" size="1.75rem" /></span>
          <p className="d-empty__title">{parts ? 'Demido was updated' : 'This screen stopped'}</p>
          <p className="d-empty__text">{parts ? 'A part of this window has a newer version. Reloading brings it.' : problem.message}</p>
          <div className="d-empty__action"><Button variant="primary" onClick={() => window.location.reload()}>Reload</Button></div>
        </div>
      </div></div>
    )
  }
}

function Rail() {
  const features = allFeatures()
  const { pathname } = useLocation()
  // In a narrow window the list beside each screen is tucked away. Pressing the icon of the screen you
  // are on brings it back, so nothing becomes unreachable.
  useEffect(() => document.documentElement.classList.remove('side-open'), [pathname])
  const toggleSide = (path: string) => {
    if (pathname.startsWith(`/${path}`)) document.documentElement.classList.toggle('side-open')
  }
  // The selection is one pill that glides to the screen chosen, on the default spring, instead of jumping.
  const scroll = useRef<HTMLDivElement>(null)
  const [pill, setPill] = useState<{ x: number; y: number; w: number; h: number; moved: boolean } | null>(null)
  useLayoutEffect(() => {
    const place = () => {
      const on = scroll.current?.querySelector<HTMLElement>('.app-rail__item.active')
      setPill((was) => (on ? { x: on.offsetLeft, y: on.offsetTop, w: on.offsetWidth, h: on.offsetHeight, moved: was !== null } : null))
    }
    place()
    const seen = new ResizeObserver(place)
    if (scroll.current) seen.observe(scroll.current)
    return () => seen.disconnect()
  }, [pathname])
  return (
    <nav className="app-rail" aria-label="Demido Studio">
      <NavLink to="/" className="app-rail__brand" aria-label="Demido Studio" dangerouslySetInnerHTML={{ __html: logo }} />
      <div className="app-rail__scroll" ref={scroll} data-pill={pill ? '' : undefined}>
        {pill && <span className="app-rail__pill" aria-hidden="true" data-moved={pill.moved ? '' : undefined} style={{ width: pill.w, height: pill.h, transform: `translate(${pill.x}px, ${pill.y}px)` }} />}
        {GROUPS.map((g) => {
          const items = features.filter((f) => f.group === g)
          if (!items.length) return null
          return (
            <div key={g} className="app-rail__group" data-group={g}>
              {items.flatMap((f) => f.nav ?? [{ title: f.title, icon: f.icon, path: f.path, home: f.home }]).map((n: { title: string; icon: typeof items[number]['icon']; path: string; home?: string }) => (
                <NavLink key={n.path} to={pathname.startsWith(`/${n.path}`) ? pathname : (n.home ?? `/${n.path}`)} className="app-rail__item" onClick={() => toggleSide(n.path)}>
                  <span className="d-icon d-icon--lg" aria-hidden="true">
                    <n.icon />
                  </span>
                  <span>{n.title}</span>
                </NavLink>
              ))}
            </div>
          )
        })}
      </div>
      <Now />
      {features.map((f) => (f.rail ? <f.rail key={f.id} /> : null))}
    </nav>
  )
}

/** "Now": what is in memory and what is running, one press away from anywhere. */
/** What is loaded now, for real: the core's list of models. Shown instead of the scripted one when a core is behind the pages. */
function NowReal() {
  const list = useLocal((s) => s.list)!
  const readAt = useLocal((s) => s.readAt)
  const waiting = useLocal((s) => s.waiting)
  // The graphics card as the core reads it. Read again whenever the list of models changed: that is when it moved.
  const [card, setCard] = useState<{ name: string; total: number; used: number } | null>(null)
  useEffect(() => {
    let alive = true
    coreGet<CoreSystem>('/system?fresh=true').then((sys) => {
      const g = sys.gpus.find((x) => x.vram_gb !== null && x.vram_used_gb !== null)
      if (alive) setCard(g ? { name: g.name, total: g.vram_gb!, used: g.vram_used_gb! } : null)
    }, () => {})
    return () => { alive = false }
  }, [readAt])
  const loaded = list.models.filter((m) => m.state === 'loaded')
  const loading = list.models.filter((m) => m.state === 'loading' || waiting[m.id] === 'loading')
  const glyph = loading.length ? 'model-loading' : loaded.length ? 'model-loaded' : 'model-unloaded'
  const inUse = loaded.reduce((sum, m) => sum + (m.needs_mb ?? 0), 0) / 1024
  const says = loading.length ? `${loading[0].id}: loading` : loaded.length ? `${loaded.map((m) => m.id).join(', ')}: loaded` : 'No model loaded'
  return (
    <Popover.Root>
      <Tip side="right" label={says}>
        <Popover.Trigger className="app-rail__now" aria-label="What is loaded now">
          <Glyph name={glyph} busy={loading.length > 0} tone={loaded.length && !loading.length ? 'live' : undefined} size="1.25rem" />
          <span className="d-mono">{inUse.toFixed(1)} GB</span>
        </Popover.Trigger>
      </Tip>
      <Popover.Portal>
        <Popover.Positioner side="right" align="end" sideOffset={10} className="app-layer">
          <Popover.Popup className="d-menu app-pop app-now">
            <Popover.Title className="d-type-headline">Now</Popover.Title>
            <p className="d-type-caption d-text-secondary">{list.engine.runtime ?? 'No model runtime is installed'}{list.engine.runtime ? (list.engine.running ? ' · running' : ' · not running') : ''}</p>
            <p className="d-menu__label">Loaded</p>
            {loaded.length === 0 && loading.length === 0 && <p className="d-text-tertiary app-now__empty">No model is loaded. Models unload on their own when idle.</p>}
            {loading.map((m) => <p key={m.id} className="app-now__row"><Glyph name="model-loading" busy /> {m.id} is loading</p>)}
            {loaded.map((m) => (
              <p key={m.id} className="app-now__row"><Glyph name="model-loaded" tone="live" /> {m.id}{m.needs_mb !== null ? ` · ${(m.needs_mb / 1024).toFixed(1)} GB` : ''}<span className="app-spacer" /><Button size="sm" variant="ghost" disabled={Boolean(waiting[m.id])} onClick={() => unloadLocal(m.id)}>Unload</Button></p>
            ))}
            {card && (
              <>
                <p className="d-menu__label">Graphics memory</p>
                <div className="d-meter" role="meter" aria-valuenow={Math.round((card.used / card.total) * 100)} aria-valuemin={0} aria-valuemax={100} aria-label="Graphics memory in use"><span style={{ '--d-meter': `${Math.min(100, (card.used / card.total) * 100)}%` } as React.CSSProperties} /></div>
                <p className="d-type-caption d-text-secondary">{card.used.toFixed(1)} of {card.total.toFixed(1)} GB in use on {card.name}: Demido's models about {inUse.toFixed(1)} GB, everything else on this computer {Math.max(0, card.used - inUse).toFixed(1)} GB.</p>
              </>
            )}
            <p className="d-type-caption d-text-tertiary">What a model needs is as the runtime measured it. {(list.engine.margin_mb / 1024).toFixed(1)} GB is kept free: a model that would not fit beside the others makes the one used longest ago unload first.</p>
          </Popover.Popup>
        </Popover.Positioner>
      </Popover.Portal>
    </Popover.Root>
  )
}

function Now() {
  const real = useLocal((s) => s.list !== null)
  return real ? <NowReal /> : <NowScripted />
}

function NowScripted() {
  const models = useStore((s) => s.models)
  const agents = useStore((s) => s.agents)
  const schedules = useStore((s) => s.schedules)
  const b = memoryBudget(models, agents)
  const order = ['generating', 'lent', 'loading', 'queued', 'loaded']
  const top = [...b.live].sort((a, c) => order.indexOf(a.state) - order.indexOf(c.state))[0]
  const glyph = !top ? 'model-unloaded' : top.state === 'loading' ? 'model-loading' : top.state === 'lent' ? 'model-lent' : 'model-loaded'
  const running = schedules.filter((s) => s.state === 'running')
  return (
    <Popover.Root>
      <Tip side="right" label={top ? `${top.name}: ${MODEL_STATE_LABEL[top.state].toLowerCase()}` : 'No model loaded'}>
        <Popover.Trigger className="app-rail__now" aria-label="What is loaded and running now">
          <Glyph name={glyph} busy={top?.state === 'generating'} tone={top && top.state !== 'loading' ? 'live' : undefined} size="1.25rem" />
          <span className="d-mono">{(b.total - b.free).toFixed(1)} GB</span>
        </Popover.Trigger>
      </Tip>
      <Popover.Portal>
        <Popover.Positioner side="right" align="end" sideOffset={10} className="app-layer">
          <Popover.Popup className="d-menu app-pop app-now">
            <Popover.Title className="d-type-headline">Now</Popover.Title>
            <p className="d-type-caption d-text-secondary">Memory on this computer · {b.total.toFixed(1)} GB</p>
            <Budget />
            <p className="d-menu__label">Running</p>
            {agents.length === 0 && running.length === 0 && !models.some((m) => m.state === 'generating') && <p className="d-text-tertiary app-now__empty">Nothing is running. Models unload on their own when idle.</p>}
            {models.filter((m) => m.state === 'generating').map((m) => (
              <p key={m.id} className="app-now__row"><Glyph name="model-loaded" busy tone="live" /> {m.name} is generating</p>
            ))}
            {agents.map((a) => (
              <p key={a.id} className="app-now__row"><Glyph name="task-delegated" /> Agent 1: {a.label.toLowerCase()}</p>
            ))}
            {running.map((s) => (
              <p key={s.id} className="app-now__row"><Glyph name="task-running" /> {s.title}</p>
            ))}
          </Popover.Popup>
        </Popover.Positioner>
      </Popover.Portal>
    </Popover.Root>
  )
}

function Toasts() {
  const toasts = useStore((s) => s.toasts)
  return (
    <div className="d-toasts" aria-live="polite">
      {toasts.map((t) => (
        <div key={t.id} className="d-toast app-toast" role="status" data-leaving={t.leaving ? '' : undefined}>
          {t.glyph && <Glyph name={t.glyph} />}
          <span>{t.text}</span>
          {t.action && (
            <Button size="sm" onClick={() => { t.action!.run(); dismissToast(t.id) }}>
              {t.action.label}
            </Button>
          )}
        </div>
      ))}
    </div>
  )
}
