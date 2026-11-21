import { useEffect } from 'react'
import { NavLink, Navigate, Route, Routes, useLocation } from 'react-router'
import { Popover } from '@base-ui/react/popover'
import { applyAppearance, dismissToast, memoryBudget, startCore, useStore } from '@/data/store'
import { Budget, Button, Glyph, MODEL_STATE_LABEL, Tip, TooltipProvider } from '@/ui'
import { allFeatures, GROUPS } from './registry'
import logo from '@brand/logo/demido-symbol-small.svg?raw'

export function App() {
  useEffect(() => applyAppearance(), [])
  // If a core is behind these pages, its settings become the real ones.
  useEffect(() => void startCore(), [])
  const features = allFeatures()
  const home = features.find((f) => f.id === 'sessions')
  const { pathname } = useLocation()
  // A feature may ask for the whole window (voice mode does). The rail steps aside; the feature shows the way back.
  const immersive = features.some((f) => f.immersive?.some((p) => pathname === `/${p}` || pathname.startsWith(`/${p}/`)))
  return (
    <TooltipProvider delay={500} closeDelay={0}>
      <div className="app" data-immersive={immersive ? '' : undefined}>
        {!immersive && <Rail />}
        <main className="app-main" onPointerDownCapture={(e) => { if (!(e.target as Element).closest('.app-side')) document.documentElement.classList.remove('side-open') }}>
          <Routes>
            {features.map((f) => (
              <Route key={f.id} path={`${f.path}/*`} element={<f.Page />} />
            ))}
            <Route path="*" element={<Navigate to={home?.home ?? `/${features[0].path}`} replace />} />
          </Routes>
        </main>
        <Toasts />
      </div>
    </TooltipProvider>
  )
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
  return (
    <nav className="app-rail" aria-label="Demido Studio">
      <NavLink to="/" className="app-rail__brand" aria-label="Demido Studio" dangerouslySetInnerHTML={{ __html: logo }} />
      <div className="app-rail__scroll">
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
function Now() {
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
        <div key={t.id} className="d-toast app-toast" role="status">
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
