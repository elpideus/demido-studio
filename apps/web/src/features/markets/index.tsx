/* Markets: a quieter TradingView. Demido's own chart, with search, timeframes, chart types, drawing tools,
   alerts and indicators.

   Indicators come from two places, and every one on the chart says which:
     Local        Demido works it out on this computer. No account, no limit. Pine scripts kept here too.
     TradingView  After you sign in, TradingView itself works it out, so the values are TradingView's own.
                  Your favourites, your scripts and your saved chart layouts are read from the account.
                  The plan's limit per chart applies to these, and only to these. Past the limit, Demido
                  works the extra ones out itself; nothing extra is loaded at TradingView (owner, 2026-11-20).

   Signing in breaks TradingView's terms. The owner decided (2026-11-20) to offer it behind a warning and
   then do everything automatically. See DECISIONS, section M. In the preview the account is made up. */
import { useEffect, useMemo, useRef, useState } from 'react'
import type { CSSProperties } from 'react'
import { Bell, ChartCandlestick, ChevronDown, Code, Copy, Eye, EyeOff, LineChart, Play, Plus, Search, Star, Trash2, X } from 'lucide-react'
import type { LucideIcon } from 'lucide-react'
import type { Feature } from '@/app/registry'
import { useCore } from '@/data/core'
import { RealMarkets } from './real'
import { setSetting, toast, useSetting } from '@/data/store'
import { Button, Dialog, Facts, Glyph, Icon, InspectorToggle, Menu, NotBuilt, Segmented, Select, Signal, Tip, cx, useInspector, Brief } from '@/ui'
import { CANDLE_OPTIONS, Chart, TOOLS, TYPES, fmt, useCandleColours } from './chart'
import type { CandleStyle, ChartType, Drawing, Shown, Tool } from './chart'
import { ACCOUNT, DRAW, KIND_LABEL, LOCAL, MODEL_RUN, MODEL_SCRIPT, PINE, QUICK_TF, SIGN_IN_RUN, TIMEFRAMES, TV_BUILT_IN, TV_BUILT_IN_TOTAL, TV_COMMUNITY, TV_FAVOURITES, TV_MINE, UNIVERSE, WATCHLIST_DEFAULT, series } from './data'
import type { Engine, Entry, Kind, OnChart, PineScript, RunStep, Shelf } from './data'

function CandleColours() {
  const { style, up, down } = useCandleColours()
  if (style !== 'custom') return <span className="d-type-caption d-text-tertiary">Set "Candle colours" to Custom first.</span>
  return (
    <span className="app-row">
      <input className="d-colorwell" type="color" aria-label="Colour of rising candles" value={up} onChange={(e) => setSetting('markets.up', e.target.value)} />
      <input className="d-colorwell" type="color" aria-label="Colour of falling candles" value={down} onChange={(e) => setSetting('markets.down', e.target.value)} />
    </span>
  )
}

function Steps({ steps }: { steps: RunStep[] }) {
  return (
    <ul className="mk__run" aria-live="polite">
      {steps.map((s, i) => (
        <li key={i}>
          <Glyph name={`task-${s.state}`} tone={s.state === 'failed' ? 'danger' : undefined} label={s.state === 'running' ? 'In progress' : s.state === 'done' ? 'Done' : 'Failed'} />
          <span>{s.text}{s.detail && <small>{s.detail}</small>}</span>
        </li>
      ))}
    </ul>
  )
}
/** Plays a list of timed steps; a running step is replaced by its result. */
function useRun() {
  const [steps, setSteps] = useState<RunStep[]>([])
  const timers = useRef<ReturnType<typeof setTimeout>[]>([])
  useEffect(() => () => timers.current.forEach(clearTimeout), [])
  const later = (ms: number, fn: () => void) => { timers.current.push(setTimeout(fn, ms)) }
  const push = (step: RunStep) => setSteps((prev) => (prev[prev.length - 1]?.state === 'running' ? [...prev.slice(0, -1), step] : [...prev, step]))
  return { steps, setSteps, later, push }
}

// ───────── symbol search ─────────
function SymbolSearch({ watch, onOpen, onStar, onClose }: { watch: string[]; onOpen: (id: string) => void; onStar: (id: string) => void; onClose: () => void }) {
  const [q, setQ] = useState('')
  const [kind, setKind] = useState<Kind | 'all'>('all')
  const needle = q.trim().toLowerCase()
  const hits = UNIVERSE.filter((s) => (kind === 'all' || s.kind === kind) && (!needle || `${s.id} ${s.name} ${s.venue}`.toLowerCase().includes(needle)))
  return (
    <Dialog open wide onOpenChange={(o) => !o && onClose()} title="Find a symbol">
      <div className="mk__find">
        <label className="d-input-wrap"><Icon icon={Search} /><input className="d-input" type="search" autoFocus placeholder="Symbol, name or exchange" aria-label="Search symbols" value={q} onChange={(e) => setQ(e.target.value)} onKeyDown={(e) => { if (e.key === 'Enter' && hits[0]) onOpen(hits[0].id) }} /></label>
        <Segmented small label="Kind" value={kind} onChange={setKind} options={[{ value: 'all', label: 'All' }, ...(Object.keys(KIND_LABEL) as Kind[]).map((k) => ({ value: k, label: KIND_LABEL[k] }))]} />
        <ul className="d-list mk__results">
          {hits.map((s) => (
            <li key={s.id} className="mk__result">
              <button type="button" className="d-row" onClick={() => onOpen(s.id)}>
                <span className="d-row__text"><span className="d-row__title d-mono">{s.id}</span><span className="d-row__sub">{s.name} · {s.venue}</span></span>
                <span className="d-row__meta">{KIND_LABEL[s.kind]}</span>
              </button>
              <Tip label={watch.includes(s.id) ? 'Remove from the watchlist' : 'Add to the watchlist'}>
                <Button variant="ghost" size="sm" iconOnly pressed={watch.includes(s.id)} aria-label={`Watch ${s.id}`} onClick={() => onStar(s.id)}><Icon icon={Star} size="sm" /></Button>
              </Tip>
            </li>
          ))}
          {!hits.length && <li className="app-empty">No symbol matches.</li>}
        </ul>
        <p className="d-type-caption d-text-tertiary">{UNIVERSE.length} made-up listings. Real ones will come from the price feeds you connect.</p>
      </div>
    </Dialog>
  )
}

// ───────── signing in to TradingView ─────────
function SignIn({ onClose, onDone }: { onClose: () => void; onDone: () => void }) {
  const [stage, setStage] = useState<'warn' | 'page' | 'auto'>('warn')
  const [agreed, setAgreed] = useState(false)
  const run = useRun()
  const start = () => {
    setStage('auto')
    for (const e of SIGN_IN_RUN) run.later(e.at, () => { run.push(e.step); if (e.finish) run.later(900, onDone) })
  }
  return (
    <Dialog open wide onOpenChange={(o) => !o && stage !== 'auto' && onClose()}
      title={stage === 'warn' ? 'Before you sign in to TradingView' : stage === 'page' ? 'Sign in on TradingView' : 'Bringing your TradingView over'}
      actions={stage === 'warn' ? <><Button onClick={onClose}>Cancel</Button><Button variant="primary" disabled={!agreed} onClick={() => setStage('page')}>Continue to sign in</Button></> : stage === 'page' ? <><Button onClick={onClose}>Cancel</Button><Button variant="primary" onClick={start}>Pretend I signed in</Button></> : undefined}>
      <div className="app-stack" style={{ marginBlockStart: 'var(--d-space-16)' }}>
        {stage === 'warn' && (
          <>
            <Signal level="warning">TradingView's terms forbid reading an account by program, whatever the purpose. Accounts are banned for it: first for a time, then for good. Demido cannot make this safe.</Signal>
            <div className="app-grid" style={{ gridTemplateColumns: 'minmax(0, 1fr) minmax(0, 1fr)' }}>
              <div>
                <p className="d-list__label" style={{ paddingInline: 0 }}>What Demido does if you go on</p>
                <ol className="d-prose" style={{ margin: 0 }}>
                  <li>Opens TradingView's own sign-in page in a window of its own. You sign in there. Demido never sees your password.</li>
                  <li>Keeps the session in your system keyring.</li>
                  <li>Reads your favourites, your scripts and your saved chart layouts.</li>
                  <li>Loads your newest layout: its indicators and its candle colours.</li>
                  <li>Has TradingView work out as many of those indicators as your plan allows. Demido works out any extra ones itself; nothing extra is loaded at TradingView.</li>
                </ol>
              </div>
              <div>
                <p className="d-list__label" style={{ paddingInline: 0 }}>What it never does</p>
                <ul className="d-prose" style={{ margin: 0 }}>
                  <li>Save or publish anything to your account without asking.</li>
                  <li>Place a trade.</li>
                </ul>
                <p className="d-list__label" style={{ paddingInline: 0 }}>Before you decide</p>
                <Brief className="d-text-secondary" text={"If the account matters to you (a paid plan, published scripts, followers), think twice. Everything except TradingView's own indicators also works without signing in."} />
              </div>
            </div>
            <label className="d-check"><input className="d-checkbox" type="checkbox" checked={agreed} onChange={(e) => setAgreed(e.target.checked)} /><span className="d-check__text">I understand that TradingView may ban this account.</span></label>
          </>
        )}
        {stage === 'page' && (
          <>
            <div className="mk__window">
              <p className="d-mono d-text-tertiary">tradingview.com/accounts/signin</p>
              <p>TradingView's sign-in page opens here, in a window and a browser profile of its own. You type your password into TradingView, not into Demido. When TradingView has signed you in, the window closes by itself.</p>
            </div>
            <NotBuilt block label="Preview">There is no real sign-in and nothing is sent anywhere. The account that follows is made up.</NotBuilt>
          </>
        )}
        {stage === 'auto' && (
          <>
            <Steps steps={run.steps} />
            <p className="d-type-caption d-text-tertiary">Nothing else is needed from you. The window closes when this is done.</p>
          </>
        )}
      </div>
    </Dialog>
  )
}

// ───────── the Indicators menu ─────────
const SHELVES: { id: Shelf | 'layouts'; label: string; account?: boolean }[] = [
  { id: 'favourites', label: 'Favourites', account: true }, { id: 'mine', label: 'My scripts', account: true }, { id: 'layouts', label: 'Chart layouts', account: true },
  { id: 'tv', label: 'TradingView built-ins' }, { id: 'community', label: 'Community scripts' }, { id: 'local', label: 'Demido built-ins' }, { id: 'pine', label: 'Pine, on this computer' },
]
const ACCESS = { open: 'open source', protected: 'closed source', invite: 'invite-only' } as const

function Indicators({ signedIn, entries, counts, first, onAdd, onLayout, onSignIn, onPine, onClose }: { signedIn: boolean; entries: Entry[]; counts: (id: string) => number; first: Shelf | 'layouts'; onAdd: (e: Entry) => void; onLayout: (id: string) => void; onSignIn: () => void; onPine: () => void; onClose: () => void }) {
  const [shelf, setShelf] = useState<Shelf | 'layouts'>(first)
  const [q, setQ] = useState('')
  const needle = q.trim().toLowerCase()
  const byId = (id: string) => entries.find((e) => e.id === id)
  const of = (s: Shelf | 'layouts'): Entry[] => (s === 'favourites' ? TV_FAVOURITES.flatMap((id) => byId(id) ?? []) : entries.filter((e) => e.shelf === s))
  const list = needle ? entries.filter((e) => `${e.name} ${e.author ?? ''}`.toLowerCase().includes(needle)) : of(shelf)
  const needsAccount = !needle && !signedIn && SHELVES.find((s) => s.id === shelf)?.account
  const sub = (e: Entry) =>
    e.shelf === 'tv' ? (signedIn ? 'Worked out by TradingView' : 'Worked out by Demido until you sign in')
    : e.shelf === 'local' ? 'Worked out by Demido, on this computer'
    : e.shelf === 'pine' ? 'Pine script, kept on this computer'
    : `by ${e.author}${e.access ? ` · ${ACCESS[e.access]}` : ''}`
  return (
    <Dialog open wide onOpenChange={(o) => !o && onClose()} title="Indicators">
      <div className="mk__menu">
        <label className="d-input-wrap mk__menu-search"><Icon icon={Search} /><input className="d-input" type="search" autoFocus placeholder="Search every shelf" aria-label="Search indicators" value={q} onChange={(e) => setQ(e.target.value)} /></label>
        <ul className="d-list mk__shelves">
          {SHELVES.map((s) => (
            <li key={s.id}><button type="button" className="d-row d-row--compact" aria-current={!needle && s.id === shelf} onClick={() => { setShelf(s.id); setQ('') }}>
              <span className="d-row__text"><span className="d-row__title">{s.label}</span></span>
              <span className="d-row__meta">{s.account && !signedIn ? 'sign in' : s.id === 'layouts' ? ACCOUNT.layouts.length : s.id === 'tv' ? TV_BUILT_IN_TOTAL : of(s.id).length}</span>
            </button></li>
          ))}
        </ul>
        <div className="mk__entries">
          {needsAccount ? (
            <div className="app-empty">
              <p>This shelf is read from your TradingView account.</p>
              <Button onClick={onSignIn}>Sign in to TradingView…</Button>
            </div>
          ) : !needle && shelf === 'layouts' ? (
            <ul className="d-list">
              {ACCOUNT.layouts.map((l) => (
                <li key={l.id}><button type="button" className="d-row" onClick={() => onLayout(l.id)}>
                  <span className="d-row__text"><span className="d-row__title">{l.name}</span><span className="d-row__sub">{l.symbol} · {l.tf} · {l.indicators.length} indicators · saved {l.saved}</span></span>
                  <span className="d-row__meta">Load</span>
                </button></li>
              ))}
            </ul>
          ) : (
            <ul className="d-list">
              {list.map((e) => (
                <li key={`${e.shelf}-${e.id}`}><button type="button" className="d-row" onClick={() => onAdd(e)}>
                  <span className="d-row__text"><span className="d-row__title">{e.name}</span><span className="d-row__sub">{sub(e)}</span></span>
                  <span className="d-row__meta">{counts(e.id) > 0 ? `on the chart${counts(e.id) > 1 ? ` ×${counts(e.id)}` : ''}` : 'Add'}</span>
                </button></li>
              ))}
              {!list.length && <li className="app-empty">Nothing matches.</li>}
              {!needle && shelf === 'tv' && <li className="d-type-caption d-text-tertiary mk__more">TradingView lists {TV_BUILT_IN_TOTAL} built-in indicators. The preview can draw these {TV_BUILT_IN.length}.</li>}
              {!needle && shelf === 'pine' && <li className="mk__more"><Button size="sm" onClick={onPine}><Icon icon={Code} size="sm" />Write or paste a script</Button></li>}
            </ul>
          )}
        </div>
      </div>
    </Dialog>
  )
}

// ───────── Pine editor ─────────
function PinePanel({ scripts, signedIn, engine, onEngine, onChart, onToggle, onModelDone }: { scripts: PineScript[]; signedIn: boolean; engine: Engine; onEngine: (e: Engine) => void; onChart: (id: string) => boolean; onToggle: (id: string, on?: boolean) => void; onModelDone: () => void }) {
  const [open, setOpen] = useState(scripts[0].id)
  const [sources, setSources] = useState<Record<string, string>>({})
  const [draft, setDraft] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [ask, setAsk] = useState('RSI with bands that follow it, so I can see when momentum fades')
  const run = useRun()
  const script = scripts.find((s) => s.id === open) ?? scripts[0]
  const writing = draft !== null
  const source = writing ? draft : (sources[script.id] ?? script.source)

  const test = () => {
    setBusy(true)
    run.setSteps([{ state: 'running', text: `Test run on this chart, by ${engine === 'local' ? 'Demido' : 'TradingView'}` }])
    run.later(900, () => { run.push({ state: 'done', text: 'Test run passed', detail: 'A value on every candle. Drawn on the chart.' }); onToggle(script.id, true); setBusy(false) })
  }
  const askModel = () => {
    setBusy(true)
    run.setSteps([])
    setDraft('')
    for (const e of MODEL_RUN) {
      run.later(e.at, () => {
        run.push(e.step)
        if (e.source) setDraft(e.source)
        if (e.finish) { onModelDone(); setOpen(MODEL_SCRIPT.id); setDraft(null); setBusy(false); toast('"RSI fade bands" is on the chart. It is kept on this computer and is in no account.') }
      })
    }
  }
  const copy = async () => {
    try { await navigator.clipboard.writeText(source); toast('Copied. Paste it into the Pine Editor on TradingView if you want it there too.') }
    catch { toast('Could not reach the clipboard. Select the text and copy it by hand.') }
  }
  return (
    <section className="mk__pine" aria-label="Pine editor">
      <div className="mk__pine-bar">
        <Select label="Script" value={writing ? '' : script.id} onChange={(id) => { setOpen(id); run.setSteps([]) }}
          options={[...(writing ? [{ value: '', label: 'New script, being written' }] : []), ...scripts.map((s) => ({ value: s.id, label: `${s.name} · ${s.by === 'model' ? 'written by a model' : 'yours'}` }))]} />
        <Button size="sm" variant="primary" disabled={busy} onClick={test}><Icon icon={Play} size="sm" />Run on this chart</Button>
        <Button size="sm" disabled={busy || writing} pressed={onChart(script.id)} onClick={() => onToggle(script.id)}>{onChart(script.id) ? 'On the chart' : 'Not on the chart'}</Button>
        <span className="app-spacer" />
        <NotBuilt>Scripted: Pine is not run here</NotBuilt>
        <Button size="sm" variant="ghost" onClick={() => void copy()}><Icon icon={Copy} size="sm" />Copy for TradingView</Button>
      </div>
      <textarea className="files__edit mk__code" spellCheck={false} aria-label="Pine Script source" value={source} readOnly={writing} onChange={(e) => setSources({ ...sources, [script.id]: e.target.value })} />
      <div className="mk__pine-side">
        <div className="d-field">
          <span className="d-field__label">Worked out by</span>
          <Segmented small fill label="Worked out by" value={engine} onChange={onEngine} options={[{ value: 'local', label: 'Demido' }, { value: 'tradingview', label: 'TradingView' }]} />
          <span className="d-field__help">{engine === 'local'
            ? 'On this computer. No account, no limit on how many. Demido\'s engine is not TradingView\'s: a script can behave differently there.'
            : signedIn ? 'TradingView compiles and runs it without saving anything to your account. Exact values. Counts toward your plan\'s limit per chart.' : 'Needs you to sign in to TradingView first.'}</span>
        </div>
        <label className="d-field">
          <span className="d-field__label">Ask a model for an indicator</span>
          <textarea className="d-input" rows={2} value={ask} onChange={(e) => setAsk(e.target.value)} />
        </label>
        <Button size="sm" disabled={busy || !ask.trim()} onClick={askModel}><Glyph name="model-loaded" tone="live" />Write and test · Qwen3 4B Instruct</Button>
        {run.steps.length > 0 && <Steps steps={run.steps} />}
        <Facts rows={[['Kept on', 'this computer'], ['Added to an account', 'never, unless you copy it there']]} />
        <Brief className="d-type-caption d-text-tertiary" text={"The samples are drawn by code written for the preview. Your edits are kept but not run. Pine Script is a trademark of TradingView; Demido is not affiliated with it."} />
      </div>
    </section>
  )
}

// ───────── page ─────────
const onChartOf = (e: Entry, engine: Engine): OnChart => ({ key: `${e.id}#${Math.random().toString(36).slice(2, 8)}`, entry: e.id, name: e.name, draw: e.draw, engine, hidden: false })

function PreviewMarkets() {
  const [symId, setSymId] = useState('BTC/EUR')
  const [tfId, setTfId] = useState('1D')
  const [type, setType] = useState<ChartType>('candles')
  const [watch, setWatch] = useState(WATCHLIST_DEFAULT)
  const [filter, setFilter] = useState('')
  const [scripts, setScripts] = useState<PineScript[]>(PINE)
  const [onChart, setOnChart] = useState<OnChart[]>(() => ['L;ema20', 'L;ema50', 'L;rsi'].map((id) => onChartOf(LOCAL.find((e) => e.id === id)!, 'local')))
  const [tool, setTool] = useState<Tool>('cursor')
  const [drawings, setDrawings] = useState<Record<string, Drawing[]>>({})
  const [hideDrawings, setHideDrawings] = useState(false)
  const [alerts, setAlerts] = useState<{ id: number; sym: string; price: number }[]>([])
  const [dialog, setDialog] = useState<null | 'symbols' | 'signin' | { shelf: Shelf | 'layouts' }>(null)
  const [pine, setPine] = useState(false)
  const [pineEngine, setPineEngine] = useState<Engine>('local')
  const signedIn = useSetting('markets.tradingview', false)
  const [inspector, toggleInspector] = useInspector('markets')
  const colours = useCandleColours()

  const sym = UNIVERSE.find((s) => s.id === symId)!
  const tf = TIMEFRAMES.find((t) => t.id === tfId)!
  const all = useMemo(() => series(sym, tf), [sym, tf])
  const last = all[all.length - 1], prev = all[all.length - 2]
  const change = ((last.c - prev.c) / prev.c) * 100

  const entries = useMemo<Entry[]>(() => [...TV_MINE, ...TV_BUILT_IN, ...TV_COMMUNITY, ...LOCAL, ...scripts.map((s): Entry => ({ id: `PINE;${s.id}`, name: s.name, shelf: 'pine', draw: s.draw, author: s.by === 'model' ? 'a model' : 'you' }))], [scripts])
  const entryOf = (id: string) => entries.find((e) => e.id === id)
  const counts = (id: string) => onChart.filter((o) => o.entry === id).length

  // TradingView works out as many as the plan allows, in order; the rest are refused, as it would refuse them.
  let used = 0
  const shown: Shown[] = onChart.map((o) => ({ ...o, drawer: DRAW[o.draw], refused: o.engine === 'tradingview' && ++used > ACCOUNT.limit, canLocal: !!entryOf(o.entry)?.localToo || o.entry.startsWith('PINE;') }))

  const add = (e: Entry) => {
    if (e.access === 'invite') return toast(`"${e.name}" is invite-only, and ${signedIn ? 'your TradingView account has' : 'no account here has'} access to it.`)
    if (e.shelf === 'local') return setOnChart((p) => [...p, onChartOf(e, 'local')])
    if (e.shelf === 'pine') {
      if (pineEngine === 'tradingview' && !signedIn) return setDialog('signin')
      return setOnChart((p) => [...p, onChartOf(e, pineEngine)])
    }
    if (signedIn) {
      // TradingView works out as many as the plan allows. Demido takes the extra ones itself.
      if (onChart.filter((o) => o.engine === 'tradingview').length < ACCOUNT.limit) return setOnChart((p) => [...p, onChartOf(e, 'tradingview')])
      if (!e.localToo) return toast(`"${e.name}" is closed source, so only TradingView can work it out, and your plan has no room left on this chart. Remove one of TradingView's indicators first.`)
      setOnChart((p) => [...p, onChartOf(e, 'local')])
      return toast(`Your TradingView plan works out ${ACCOUNT.limit} indicators per chart. Demido works "${e.name}" out itself. Nothing is added on TradingView.`)
    }
    if (!e.localToo) return setDialog('signin')
    setOnChart((p) => [...p, onChartOf(e, 'local')])
    toast(`${e.name}: worked out by Demido, since you are not signed in to TradingView.`)
  }
  const loadLayout = (id: string, quiet = false) => {
    const l = ACCOUNT.layouts.find((x) => x.id === id)!
    const found = l.indicators.flatMap((i) => entryOf(i) ?? []).filter((e) => e.access !== 'invite')
    let room = ACCOUNT.limit
    setOnChart(found.map((e) => onChartOf(e, room-- > 0 || !e.localToo ? 'tradingview' : 'local')))
    setSymId(l.symbol)
    setTfId(l.tf)
    setSetting('markets.candles', 'custom')
    setSetting('markets.up', l.up)
    setSetting('markets.down', l.down)
    setDialog(null)
    if (!quiet) toast(`"${l.name}" loaded: ${found.length} indicators and its candle colours. TradingView works out ${Math.min(ACCOUNT.limit, found.length)}; Demido works out the rest.`)
  }
  const signedInNow = () => {
    setSetting('markets.tradingview', true)
    loadLayout(ACCOUNT.layouts[0].id, true)
    toast(`Signed in as ${ACCOUNT.user}. "Swing setup" is on the chart: TradingView works out ${ACCOUNT.limit} indicators, Demido the rest. 1 invite-only script was skipped.`)
  }
  const signOut = () => {
    setSetting('markets.tradingview', false)
    setPineEngine('local')
    // What Demido can work out itself stays; the rest needs TradingView and goes.
    setOnChart((p) => p.flatMap((o) => (o.engine !== 'tradingview' ? [o] : entryOf(o.entry)?.localToo || o.entry.startsWith('PINE;') ? [{ ...o, engine: 'local' as Engine }] : [])))
    toast('Signed out of TradingView. The session is deleted from this computer.')
  }
  const togglePine = (id: string, on?: boolean) => setOnChart((p) => {
    const there = p.some((o) => o.entry === `PINE;${id}`)
    const want = on ?? !there
    if (want === there) return p
    const s = scripts.find((x) => x.id === id) ?? MODEL_SCRIPT
    return want ? [...p, onChartOf({ id: `PINE;${id}`, name: s.name, shelf: 'pine', draw: s.draw }, signedIn ? pineEngine : 'local')] : p.filter((o) => o.entry !== `PINE;${id}`)
  })
  const star = (id: string) => setWatch((w) => (w.includes(id) ? w.filter((x) => x !== id) : [...w, id]))
  const needle = filter.trim().toLowerCase()
  const myAlerts = alerts.filter((a) => a.sym === symId)

  return (
    <div className="app-split" data-inspector={inspector ? '' : undefined} style={colours.vars}>
      <aside className="app-side" aria-label="Watchlist">
        <div className="app-side__head"><span className="d-type-headline">Markets</span><Tip label="Find a symbol"><Button variant="ghost" size="sm" iconOnly aria-label="Find a symbol" onClick={() => setDialog('symbols')}><Icon icon={Plus} /></Button></Tip></div>
        <label className="d-input-wrap"><Icon icon={Search} /><input className="d-input" type="search" placeholder="Filter the watchlist" aria-label="Filter the watchlist" value={filter} onChange={(e) => setFilter(e.target.value)} /></label>
        <div className="app-side__scroll">
          <ul className="d-list">
            {watch.map((id) => UNIVERSE.find((s) => s.id === id)!).filter((s) => !needle || `${s.id} ${s.name}`.toLowerCase().includes(needle)).map((s) => {
              const d = series(s, TIMEFRAMES.find((t) => t.id === '1D')!)
              const a = d[d.length - 1], b = d[d.length - 2]
              const ch = ((a.c - b.c) / b.c) * 100
              return (
                <li key={s.id} className="mk__watch">
                  <button type="button" className="d-row" aria-current={s.id === symId} onClick={() => setSymId(s.id)}>
                    <span className="d-row__text"><span className="d-row__title d-mono">{s.id}</span><span className="d-row__sub">{s.name}</span></span>
                    <span className="d-row__meta" style={{ display: 'grid', justifyItems: 'end' }}><span className="d-mono">{fmt(a.c)}</span><span className="mk__change" data-up={ch >= 0 ? '' : undefined}>{ch >= 0 ? '▲' : '▼'} {Math.abs(ch).toFixed(2)}%</span></span>
                  </button>
                  <button type="button" className="mk__mini" aria-label={`Remove ${s.id} from the watchlist`} onClick={() => star(s.id)}><Icon icon={X} size="sm" /></button>
                </li>
              )
            })}
          </ul>
          {!watch.length && <p className="app-empty">The watchlist is empty.</p>}
          <Button variant="ghost" size="sm" onClick={() => setDialog('symbols')}><Icon icon={Search} size="sm" />Find a symbol</Button>
        </div>
        <p className="app-side__foot">Prices in this preview are made up from a fixed seed. They are not market data.</p>
      </aside>

      <section className={cx('chat', 'mk')} data-pine={pine ? '' : undefined} aria-label="Chart">
        <header className="chat__head">
          <Button variant="ghost" onClick={() => setDialog('symbols')}><span className="chat__title d-mono">{sym.id}</span><span className="d-text-tertiary">{sym.name} · {sym.venue}</span><Icon icon={ChevronDown} size="sm" /></Button>
          <span className="d-mono">{fmt(last.c)}</span>
          <span className="d-badge mk__change" data-up={change >= 0 ? '' : undefined}>{change >= 0 ? '▲' : '▼'} {Math.abs(change).toFixed(2)}%</span>
          <span className="app-spacer" />
          {signedIn ? (
            <Menu align="end" trigger={<Button size="sm" variant="ghost"><Glyph name="task-done" tone="live" />TradingView: {ACCOUNT.user}</Button>}
              items={[{ kind: 'label', label: `${ACCOUNT.plan} plan · ${ACCOUNT.limit} indicators per chart` }, { label: 'Load a chart layout…', onSelect: () => setDialog({ shelf: 'layouts' }) }, { label: 'Read the account again', onSelect: () => toast('Favourites, scripts and layouts read again.') }, { kind: 'sep' }, { label: 'Sign out and forget the session', danger: true, onSelect: signOut }]} />
          ) : (
            <Button size="sm" onClick={() => setDialog('signin')}>Sign in to TradingView</Button>
          )}
          <InspectorToggle open={inspector} onToggle={toggleInspector} />
        </header>
        <div className="mk__bar">
          <Segmented small label="Timeframe" value={QUICK_TF.includes(tfId) ? tfId : ''} onChange={setTfId} options={QUICK_TF.map((t) => ({ value: t, label: t }))} />
          <Menu trigger={<Button size="sm" variant="ghost" pressed={!QUICK_TF.includes(tfId)}>{QUICK_TF.includes(tfId) ? 'More' : tf.label}<Icon icon={ChevronDown} size="sm" /></Button>}
            items={[{ kind: 'label', label: 'Timeframe' }, ...TIMEFRAMES.map((t) => ({ label: t.label, hint: t.id, checked: t.id === tfId, onSelect: () => setTfId(t.id) }))]} />
          <Menu trigger={<Button size="sm" variant="ghost"><Icon icon={type === 'line' || type === 'area' ? LineChart : ChartCandlestick} size="sm" />{TYPES.find((t) => t.value === type)!.label}<Icon icon={ChevronDown} size="sm" /></Button>}
            items={[{ kind: 'label', label: 'Chart type' }, ...TYPES.map((t) => ({ label: t.label, checked: t.value === type, onSelect: () => setType(t.value) }))]} />
          <Button size="sm" onClick={() => setDialog({ shelf: signedIn ? 'favourites' : 'local' })}><Icon icon={Plus} size="sm" />Indicators<span className="d-text-tertiary">{onChart.length}</span></Button>
          <Button size="sm" variant="ghost" pressed={pine} onClick={() => setPine(!pine)}><Icon icon={Code} size="sm" />Pine editor</Button>
          <span className="app-spacer" />
          <Tip label="Point at a price on the chart and press +"><Button size="sm" variant="ghost" onClick={() => setAlerts((a) => [...a, { id: Date.now(), sym: symId, price: last.c * 1.02 }])}><Icon icon={Bell} size="sm" />Alert</Button></Tip>
        </div>
        <div className="mk__stage">
          <div className="mk__tools" role="toolbar" aria-label="Drawing tools" aria-orientation="vertical">
            {TOOLS.map((t) => <Tip key={t.value} side="right" label={t.label}><Button variant="ghost" size="sm" iconOnly pressed={tool === t.value} aria-label={t.label} onClick={() => setTool(t.value)}><Icon icon={t.icon} /></Button></Tip>)}
            <hr />
            <Tip side="right" label={hideDrawings ? 'Show drawings' : 'Hide drawings'}><Button variant="ghost" size="sm" iconOnly pressed={hideDrawings} aria-label="Hide drawings" onClick={() => setHideDrawings(!hideDrawings)}><Icon icon={hideDrawings ? EyeOff : Eye} /></Button></Tip>
            <Tip side="right" label="Remove every drawing on this symbol"><Button variant="ghost" size="sm" iconOnly aria-label="Remove every drawing" disabled={!(drawings[symId]?.length)} onClick={() => setDrawings({ ...drawings, [symId]: [] })}><Icon icon={Trash2} /></Button></Tip>
          </div>
          <Chart key={`${symId}@${tfId}`} all={all} tf={tf} type={type} hollow={colours.style === 'hollow'} shown={shown} tool={tool} onToolDone={() => setTool('cursor')}
            drawings={drawings[symId] ?? []} onDrawings={(d) => setDrawings({ ...drawings, [symId]: d })} hideDrawings={hideDrawings}
            alerts={myAlerts} onAlert={(price) => { setAlerts((a) => [...a, { id: Date.now(), sym: symId, price }]); toast(`Alert set on ${symId} at ${fmt(price)}.`) }}
            title={`${sym.id}, ${tf.label}`} split={signedIn}
            onHide={(key) => setOnChart((p) => p.map((o) => (o.key === key ? { ...o, hidden: !o.hidden } : o)))}
            onRemove={(key) => setOnChart((p) => p.filter((o) => o.key !== key))}
            onLocal={(key) => setOnChart((p) => p.map((o) => (o.key === key ? { ...o, engine: 'local' } : o)))} />
        </div>
        {pine && <PinePanel scripts={scripts} signedIn={signedIn} engine={signedIn ? pineEngine : 'local'} onEngine={(e) => (e === 'tradingview' && !signedIn ? setDialog('signin') : setPineEngine(e))}
          onChart={(id) => onChart.some((o) => o.entry === `PINE;${id}`)} onToggle={togglePine}
          onModelDone={() => { setScripts((p) => (p.some((s) => s.id === MODEL_SCRIPT.id) ? p : [...p, MODEL_SCRIPT])); togglePine(MODEL_SCRIPT.id, true) }} />}
      </section>

      {inspector && (
        <aside className="app-insp" aria-label="Market tools">
          <section className="app-insp__panel">
            <h4 className="app-insp__title">{sym.id} <span>{tf.label}</span></h4>
            <Facts rows={[['Open', fmt(last.o)], ['High', fmt(last.h)], ['Low', fmt(last.l)], ['Close', fmt(last.c)], ['Highest kept', fmt(Math.max(...all.map((d) => d.h)))], ['Lowest kept', fmt(Math.min(...all.map((d) => d.l)))]]} />
          </section>
          <section className="app-insp__panel">
            <h4 className="app-insp__title">Alerts <span>{myAlerts.length}</span></h4>
            {myAlerts.length === 0 && <p className="d-type-caption d-text-secondary">None on this symbol. Point at a price on the chart and press +.</p>}
            {myAlerts.map((a) => (
              <div key={a.id} className="app-row"><Icon icon={Bell} size="sm" /><span className="d-mono">{fmt(a.price)}</span><span className="d-type-caption d-text-tertiary">{a.price >= last.c ? 'when it rises to' : 'when it falls to'}</span><span className="app-spacer" /><button type="button" className="mk__mini" style={{ opacity: 1 }} aria-label="Remove the alert" onClick={() => setAlerts((x) => x.filter((y) => y.id !== a.id))}><Icon icon={X} size="sm" /></button></div>
            ))}
            <NotBuilt>Alerts do not fire in the preview</NotBuilt>
          </section>
          <section className="app-insp__panel">
            <h4 className="app-insp__title">Indicators <span>{onChart.length} on the chart</span></h4>
            <Facts rows={[['Worked out here', String(onChart.filter((o) => o.engine === 'local').length)], ['By TradingView', signedIn ? `${Math.min(used, ACCOUNT.limit)} of ${ACCOUNT.limit}` : 'not signed in']]} />
            <p className="d-type-caption d-text-secondary">{signedIn ? `TradingView works out up to ${ACCOUNT.limit} per chart on your plan. Demido works out every one beyond that itself, on this computer. Nothing is added to your charts on TradingView.` : 'All of them are worked out on this computer. There is no limit.'}</p>
          </section>
          <section className="app-insp__panel">
            <h4 className="app-insp__title">Candles</h4>
            <Select label="Candle colours" value={colours.style} onChange={(v) => setSetting('markets.candles', v)} options={CANDLE_OPTIONS} />
            {colours.style === 'custom' && <div className="app-row"><CandleColours /><span className="d-type-caption d-text-secondary">rising · falling</span></div>}
            {colours.style === 'hollow' && <p className="d-type-caption d-text-secondary">Hollow rises, solid falls. Readable without telling red from green.</p>}
          </section>
          <section className="app-insp__panel">
            <h4 className="app-insp__title">Data and tools</h4>
            <p className="d-type-caption d-text-secondary">Prices will come from public exchange feeds: free, allowed, no login.</p>
            <Button disabled>Download history</Button>
            <Button disabled>Backtest a strategy</Button>
            <NotBuilt block>Live prices, history download, backtesting, saved layouts of your own, comparing symbols, a logarithmic scale.</NotBuilt>
            <Signal level="note">Demido can chart and test. It does not place trades and does not give investment advice.</Signal>
          </section>
        </aside>
      )}

      {dialog === 'symbols' && <SymbolSearch watch={watch} onStar={star} onOpen={(id) => { setSymId(id); setDialog(null) }} onClose={() => setDialog(null)} />}
      {dialog === 'signin' && <SignIn onClose={() => setDialog(null)} onDone={signedInNow} />}
      {dialog && typeof dialog === 'object' && <Indicators signedIn={signedIn} entries={entries} counts={counts} first={dialog.shelf} onAdd={add} onLayout={loadLayout} onSignIn={() => setDialog('signin')} onPine={() => { setDialog(null); setPine(true) }} onClose={() => setDialog(null)} />}
    </div>
  )
}

/** With a core behind the pages, the real one; without, the preview of where it is going. */
function MarketsPage() {
  const core = useCore()
  if (core.state === 'looking') return null
  return core.state === 'ready' ? <RealMarkets /> : <PreviewMarkets />
}

export const feature: Feature = {
  id: 'markets', title: 'Markets', icon: ChartCandlestick, group: 'connect', order: 3, path: 'markets', Page: MarketsPage,
  settings: [
    {
      id: 'markets', title: 'Markets', order: 7,
      settings: [
        {
          id: 'markets.candles', type: 'select', label: 'Candle colours', default: 'classic', options: CANDLE_OPTIONS,
          desc: 'How rising and falling candles are drawn.',
          details: 'Default: green rises, red falls. "Hollow and solid" tells them apart by shape, for anyone who cannot tell red from green. "Custom" uses your own two colours. Signing in to TradingView sets Custom to the colours of your newest chart layout.',
        },
        { id: 'markets.colours', type: 'custom', label: 'Custom candle colours', desc: 'Rising, then falling. Used when Candle colours is set to Custom.', default: null, Control: CandleColours },
        {
          id: 'markets.tradingview', type: 'switch', label: 'Signed in to TradingView (preview)', default: false,
          desc: 'On after you sign in from Markets. Turn it off to sign out.',
          details: 'In the real app this is not a switch: you sign in on TradingView\'s own page, after a warning. Reading an account by program breaks TradingView\'s terms and can get the account banned.',
        },
      ],
    },
  ],
}
