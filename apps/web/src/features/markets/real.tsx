/* Markets, for real (DECISIONS M2, M3, M7, M9, M15 to M20; research note 33), laid out as TradingView lays a
   chart out (M20): the symbol search and the timeframes along the top, the drawing tools down the left, the chart
   in the middle, the watchlist down the right with the symbol's details under it, and the Pine editor below.

   Prices from public feeds through the core (Kraken, Coinbase when Kraken does not answer, the ECB's daily
   rates), drawn by Demido's own chart. Every indicator is a script of Demido's own Pine-compatible engine, run
   by the core, the built-in ones too; the person's scripts are kept on this computer, and "Write with Demido"
   has a session's model write one, run it, and mend it from the engine's error. Nothing here places an order,
   and nothing reads TradingView. */
import { useEffect, useMemo, useRef, useState } from 'react'
import { useNavigate } from 'react-router'
import { ChartCandlestick, ChevronDown, Code, Copy, Eye, EyeOff, LineChart, PanelRight, PanelRightClose, Play, Plus, RefreshCw, Save, Search, Sparkles, Star, Trash2, X } from 'lucide-react'
import { coreGet, coreSend } from '@/data/core'
import { setSetting, toast, useSetting } from '@/data/store'
import { Brief, Button, ContextMenu, Empty, Facts, Icon, Menu, NotBuilt, Segmented, Signal, Skeleton, Tip, cx, type MenuEntry } from '@/ui'
import { Chart, TOOLS, TYPES, fmt, useCandleColours } from './chart'
import type { ChartType, Drawing, Shown, Tool } from './chart'
import type { Drawer, Timeframe } from './data'

interface Candle { t: number; o: number; h: number; l: number; c: number; v: number }
interface Symbol { id: string; name: string; kind: string; from: string; intervals: number[] }
interface Candles { symbol: string; interval: number; from: string; candles: Candle[]; age_ms: number }
interface Plot { title: string; color: string; values: (number | null)[] }
interface Hline { price: number; title: string; color: string }
interface Ran { title: string; overlay: boolean; plots: Plot[]; hlines: Hline[] }
interface PineError { line: number; col: number; message: string }
interface PineRan { ran: Ran | null; error: PineError | null; times: number[] }
interface BuiltIn { id: string; name: string; source: string }
interface Script { id: string; name: string; source: string; updated_at: number }
interface Written { session: string; source: string; tries: number; ran: Ran | null; error: string | null }
/** A symbol's last price, and how far it moved since the day before. */
interface Quote { last: number; change: number }
/** A script on the chart: a built-in, a kept one, or the one in the editor. */
interface OnChart { key: string; name: string; ran: Ran; times: number[] }

const LABEL: Record<number, string> = { 1: '1m', 5: '5m', 15: '15m', 30: '30m', 60: '1h', 240: '4h', 1440: '1D', 10080: '1W' }
const LONG: Record<number, string> = { 1: '1 minute', 5: '5 minutes', 15: '15 minutes', 30: '30 minutes', 60: '1 hour', 240: '4 hours', 1440: '1 day', 10080: '1 week' }
const QUICK = [5, 15, 60, 240, 1440, 10080]
const WATCH_FIRST = ['BTC/EUR', 'ETH/EUR', 'SOL/EUR', 'XRP/EUR', 'EUR/USD', 'GBP/USD']
const FIRST_SCRIPT = '//@version=6\nindicator("My script", overlay=true)\nlen = input.int(10, "Length")\nplot(ta.ema(close, len), "EMA", color=color.green)'

const timeframe = (minutes: number): Timeframe => ({ id: LABEL[minutes] ?? `${minutes}m`, label: LONG[minutes] ?? `${minutes} minutes`, minutes, swing: 1 })
const change = (c: Candle[]) => (c.length > 1 ? ((c[c.length - 1].c - c[c.length - 2].c) / c[c.length - 2].c) * 100 : 0)

/** What the engine drew, as the chart draws it: its values put on the candles they belong to, by time. */
function drawerOf(on: OnChart, candles: Candle[]): Drawer {
  const index = new Map(candles.map((c, i) => [c.t, i]))
  const placed = on.ran.plots.map((p) => {
    const values: (number | null)[] = candles.map(() => null)
    on.times.forEach((t, k) => { const i = index.get(t); if (i !== undefined) values[i] = p.values[k] ?? null })
    return { ...p, values }
  })
  return {
    where: on.ran.overlay ? 'price' : 'pane',
    plot: () => ({
      lines: placed.map((p) => ({ values: p.values, color: p.color })),
      guides: on.ran.hlines.map((h) => h.price),
      read: (i) => placed.map((p) => (p.values[i] == null ? '–' : fmt(p.values[i] as number))).join(' · '),
    }),
  }
}

/** The search along the top: a symbol by its name, as you type; the star puts it in the watchlist. */
function Find({ symbols, watch, onOpen, onStar }: { symbols: Symbol[]; watch: string[]; onOpen: (id: string) => void; onStar: (id: string) => void }) {
  const [q, setQ] = useState('')
  const [open, setOpen] = useState(false)
  const [at, setAt] = useState(0)
  const box = useRef<HTMLDivElement>(null)
  const needle = q.trim().toLowerCase()
  const hits = symbols.filter((s) => !needle || `${s.id} ${s.name} ${s.kind}`.toLowerCase().includes(needle)).slice(0, 9)
  useEffect(() => {
    const away = (e: PointerEvent) => { if (!box.current?.contains(e.target as Node)) setOpen(false) }
    document.addEventListener('pointerdown', away)
    return () => document.removeEventListener('pointerdown', away)
  }, [])
  const go = (id: string) => { onOpen(id); setQ(''); setOpen(false); (document.activeElement as HTMLElement | null)?.blur() }
  return (
    <div className="mkr__find" ref={box}>
      <label className="d-input-wrap">
        <Icon icon={Search} />
        <input className="d-input" type="search" placeholder="Symbol or name" aria-label="Find a symbol" role="combobox" aria-expanded={open} aria-controls="mkr-hits" value={q}
          onFocus={() => setOpen(true)} onChange={(e) => { setQ(e.target.value); setAt(0); setOpen(true) }}
          onKeyDown={(e) => {
            if (e.key === 'ArrowDown') { e.preventDefault(); setAt((a) => Math.min(hits.length - 1, a + 1)) }
            else if (e.key === 'ArrowUp') { e.preventDefault(); setAt((a) => Math.max(0, a - 1)) }
            else if (e.key === 'Enter' && hits[at]) go(hits[at].id)
            else if (e.key === 'Escape') { setOpen(false); e.currentTarget.blur() }
          }} />
      </label>
      {open && (
        <ul className="d-menu app-pop mkr__hits" id="mkr-hits" role="listbox" aria-label="Symbols">
          {hits.map((s, i) => (
            <li key={s.id} role="option" aria-selected={i === at} className="mkr__hit" data-at={i === at ? '' : undefined} onPointerEnter={() => setAt(i)}>
              <button type="button" className="mkr__hit-name" onClick={() => go(s.id)}>
                <span className="d-mono">{s.id}</span><span className="d-text-tertiary">{s.name}</span>
                <span className="d-badge d-badge--outline">{s.kind === 'crypto' ? 'Crypto' : 'Currency'}</span>
              </button>
              <Tip label={watch.includes(s.id) ? 'In the watchlist' : 'Add to the watchlist'}>
                <Button size="sm" variant="ghost" iconOnly pressed={watch.includes(s.id)} aria-label={`Watch ${s.id}`} onClick={() => onStar(s.id)}><Icon icon={Star} size="sm" /></Button>
              </Tip>
            </li>
          ))}
          {!hits.length && <li className="mkr__none">No symbol matches. Stocks have no public feed yet.</li>}
        </ul>
      )}
    </div>
  )
}

/** The Pine editor, under the chart: the script, run it, keep it, or have a model write it. */
function PinePanel({ symbol, interval, onRan, onClose }: { symbol: string; interval: number; onRan: (r: OnChart | null) => void; onClose: () => void }) {
  const navigate = useNavigate()
  const [source, setSource] = useState(FIRST_SCRIPT)
  const [error, setError] = useState<PineError | string | null>(null)
  const [ran, setRan] = useState<Ran | null>(null)
  const [scripts, setScripts] = useState<Script[]>([])
  const [kept, setKept] = useState<Script | null>(null)
  const [request, setRequest] = useState('')
  const [writing, setWriting] = useState(false)
  const [written, setWritten] = useState<Written | null>(null)
  useEffect(() => { void coreGet<Script[]>('/markets/scripts').then(setScripts, () => {}) }, [])
  const run = async (src = source) => {
    try {
      const r = await coreSend<PineRan>('POST', '/markets/pine/run', { source: src, symbol, interval })
      setRan(r.ran)
      setError(r.error)
      onRan(r.ran ? { key: 'editor', name: r.ran.title, ran: r.ran, times: r.times } : null)
    } catch (e) { setError((e as Error).message) }
  }
  const keep = async () => {
    const name = (source.match(/indicator\(\s*["']([^"']+)/)?.[1]) ?? 'Script'
    try {
      const k = await coreSend<Script>('PUT', '/markets/scripts', { id: kept?.id, name, source })
      setKept(k)
      setScripts(await coreGet<Script[]>('/markets/scripts'))
      toast(`Kept on this computer: ${k.name}.`, undefined, 'task-done')
    } catch (e) { toast(`Not kept. ${(e as Error).message}`, undefined, 'task-failed') }
  }
  const write = async () => {
    if (!request.trim()) return
    setWriting(true); setWritten(null)
    try {
      const w = await coreSend<Written>('POST', '/markets/pine/write', { request, symbol, interval, source, session: written?.session })
      setWritten(w)
      setSource(w.source)
      setError(w.error)
      if (w.ran) { setRan(w.ran); void run(w.source) }
    } catch (e) { toast(`Not written. ${(e as Error).message}`, undefined, 'task-failed') }
    setWriting(false)
  }
  return (
    <section className="mk__pine" aria-label="Pine editor">
      <div className="mk__pine-bar">
        <span className="d-type-headline">Pine editor</span>
        {scripts.length > 0 && (
          <select className="d-input" aria-label="Kept scripts" value={kept?.id ?? ''} onChange={(e) => { const s = scripts.find((x) => x.id === e.target.value); if (s) { setKept(s); setSource(s.source); void run(s.source) } }}>
            <option value="">Kept scripts…</option>
            {scripts.map((s) => <option key={s.id} value={s.id}>{s.name}</option>)}
          </select>
        )}
        <Button size="sm" variant="primary" onClick={() => void run()}><Icon icon={Play} size="sm" />Run on the chart</Button>
        <Button size="sm" variant="ghost" onClick={() => void keep()}><Icon icon={Save} size="sm" />Keep</Button>
        {kept && <Button size="sm" variant="ghost" onClick={() => void coreSend('DELETE', `/markets/scripts/${kept.id}`).then(async () => { setKept(null); setScripts(await coreGet<Script[]>('/markets/scripts')) })}><Icon icon={Trash2} size="sm" />Delete</Button>}
        {ran && <Button size="sm" variant="ghost" onClick={() => { setRan(null); setError(null); onRan(null) }}>Off the chart</Button>}
        <span className="app-spacer" />
        <Tip label="Puts the script on the clipboard. Pasting it into TradingView is yours to do."><Button size="sm" variant="ghost" iconOnly aria-label="Copy for TradingView" onClick={() => void navigator.clipboard.writeText(source).then(() => toast('The script is on the clipboard.'))}><Icon icon={Copy} size="sm" /></Button></Tip>
        <Tip label="Close the editor"><Button size="sm" variant="ghost" iconOnly aria-label="Close the editor" onClick={onClose}><Icon icon={X} size="sm" /></Button></Tip>
      </div>
      <textarea className="files__edit mk__code" spellCheck={false} aria-label="The script" value={source} onChange={(e) => setSource(e.target.value)}
        onKeyDown={(e) => { if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') { e.preventDefault(); void run() } }} />
      <div className="mk__pine-side">
        {error && <Signal level="caution">{typeof error === 'string' ? error : `Line ${error.line}, column ${error.col}: ${error.message}`}</Signal>}
        {ran && !error && <p className="d-type-caption d-text-secondary">{ran.title}: {ran.plots.length} {ran.plots.length === 1 ? 'line' : 'lines'}{ran.overlay ? ', over the candles' : ', in a pane of its own'}.</p>}
        <label className="d-field">
          <span className="d-field__label">Ask Demido to write or change it</span>
          <textarea className="d-input" rows={3} placeholder="An RSI with a 9-bar smoothing line…" value={request} onChange={(e) => setRequest(e.target.value)}
            onKeyDown={(e) => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); void write() } }} />
        </label>
        <Button size="sm" disabled={writing || !request.trim()} onClick={() => void write()}><Icon icon={writing ? RefreshCw : Sparkles} size="sm" />{writing ? 'Writing…' : 'Write with Demido'}</Button>
        {written && <Button size="sm" variant="ghost" onClick={() => navigate(`/sessions/${encodeURIComponent(written.session)}`)}>{written.tries} {written.tries === 1 ? 'try' : 'tries'}, in its session</Button>}
        <Brief className="d-type-caption d-text-tertiary" text={"Demido's engine reads a first part of the language (research note 33) and is not TradingView's: a script can behave differently there. Pine Script is TradingView's trademark; Demido is not affiliated with TradingView. A model writes, Demido runs it, and hands the error back to be mended, three tries at most. Ctrl+Enter runs the script."} />
      </div>
    </section>
  )
}

export function RealMarkets() {
  const [symbols, setSymbols] = useState<Symbol[]>([])
  const [symbol, setSymbolKept] = useState(() => { try { return localStorage.getItem('demido.markets.symbol') ?? 'BTC/EUR' } catch { return 'BTC/EUR' } })
  const [interval, setInterval] = useState(1440)
  const type = useSetting<ChartType>('markets.type', 'candles')
  const watch = useSetting<string[]>('markets.watch', WATCH_FIRST)
  const [quotes, setQuotes] = useState<Record<string, Quote>>({})
  const [data, setData] = useState<Candles | null>(null)
  const [problem, setProblem] = useState<string | null>(null)
  const [builtIn, setBuiltIn] = useState<BuiltIn[]>([])
  const on = useSetting<string[]>('markets.on', ['ema20'])
  const [results, setResults] = useState<Record<string, OnChart>>({})
  const [editor, setEditor] = useState<OnChart | null>(null)
  const [hidden, setHidden] = useState<string[]>([])
  const [tool, setTool] = useState<Tool>('cursor')
  const [drawings, setDrawings] = useState<Record<string, Drawing[]>>({})
  const [hideDrawings, setHideDrawings] = useState(false)
  const [pine, setPine] = useState(false)
  const [side, setSide] = useState(true)
  const colours = useCandleColours()
  const sym = symbols.find((s) => s.id === symbol)

  const setSymbol = (id: string) => {
    setSymbolKept(id)
    try { localStorage.setItem('demido.markets.symbol', id) } catch { /* no storage: it is chosen for now only */ }
    const s = symbols.find((x) => x.id === id)
    if (s && !s.intervals.includes(interval)) setInterval(s.intervals.includes(1440) ? 1440 : s.intervals[0])
  }
  const star = (id: string) => setSetting('markets.watch', watch.includes(id) ? watch.filter((x) => x !== id) : [...watch, id])

  useEffect(() => {
    void coreGet<Symbol[]>('/markets/symbols').then(setSymbols, () => {})
    void coreGet<BuiltIn[]>('/markets/built-in').then(setBuiltIn, () => {})
  }, [])
  const read = (fresh = false) => coreGet<Candles>(`/markets/candles?symbol=${encodeURIComponent(symbol)}&interval=${interval}${fresh ? '&fresh=true' : ''}`).then((d) => { setData(d); setProblem(null) }, (e: Error) => setProblem(e.message))
  useEffect(() => {
    setData(null)
    void read()
    // Asked again while the screen is open: the core keeps an answer a quarter of its interval, 15 s at least.
    const every = window.setInterval(() => void read(), 30_000)
    return () => window.clearInterval(every)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [symbol, interval])
  // The watchlist's prices: each symbol's last two days, from the same feeds, kept by the core.
  useEffect(() => {
    let alive = true
    const quote = () => watch.forEach((id) => void coreGet<Candles>(`/markets/candles?symbol=${encodeURIComponent(id)}&interval=1440`).then((d) => {
      const c = d.candles
      if (alive && c.length) setQuotes((q) => ({ ...q, [id]: { last: c[c.length - 1].c, change: change(c) } }))
    }, () => {}))
    quote()
    const every = window.setInterval(quote, 60_000)
    return () => { alive = false; window.clearInterval(every) }
  }, [watch.join(',')])
  // The built-in indicators on the chart, worked out by the engine each time the candles change.
  useEffect(() => {
    if (!data) return
    for (const id of on) {
      const b = builtIn.find((x) => x.id === id)
      if (b) void coreSend<PineRan>('POST', '/markets/pine/run', { source: b.source, symbol, interval }).then((r) => { if (r.ran) setResults((all) => ({ ...all, [id]: { key: id, name: b.name, ran: r.ran!, times: r.times } })) }, () => {})
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [data, on.join(','), builtIn.length])

  const candles = data?.candles ?? []
  const shown: Shown[] = useMemo(() => {
    const all = [...on.map((id) => results[id]).filter((r): r is OnChart => !!r), ...(editor ? [editor] : [])]
    return all.map((o) => ({ key: o.key, entry: o.key, name: o.name, draw: '', engine: 'local' as const, hidden: hidden.includes(o.key), drawer: drawerOf(o, candles) }))
  }, [on, results, editor, hidden, candles])
  const last = candles[candles.length - 1]
  const moved = change(candles)
  const turn = (id: string) => setSetting('markets.on', on.includes(id) ? on.filter((x) => x !== id) : [...on, id])
  const indicators: MenuEntry[] = [
    { kind: 'label', label: "Demido's built-ins, worked out here" },
    ...builtIn.map((b) => ({ label: b.name, checked: on.includes(b.id), onSelect: () => turn(b.id) })),
    { kind: 'sep' },
    { label: 'Write or paste a script…', lead: <Icon icon={Code} size="sm" />, onSelect: () => setPine(true) },
  ]
  const tf = timeframe(interval)
  const myDrawings = drawings[symbol] ?? []

  return (
    <div className="mkr" data-side={side ? '' : undefined} style={colours.vars}>
      <section className="chat mk" data-pine={pine ? '' : undefined} aria-label="Chart">
        <header className="mkr__top">
          <Find symbols={symbols} watch={watch} onOpen={setSymbol} onStar={star} />
          <span className="mkr__name"><span className="chat__title d-mono">{symbol}</span><span className="d-text-tertiary">{sym?.name}</span></span>
          {last && <span className="d-mono mkr__price">{fmt(last.c)}</span>}
          {last && <span className="d-badge mk__change" data-up={moved >= 0 ? '' : undefined}>{moved >= 0 ? '▲' : '▼'} {Math.abs(moved).toFixed(2)}%</span>}
          <span className="app-spacer" />
          <Tip label="Ask the feed again now"><Button size="sm" variant="ghost" iconOnly aria-label="Ask the feed again" onClick={() => void read(true)}><Icon icon={RefreshCw} size="sm" /></Button></Tip>
          <Tip label={side ? 'Hide the watchlist' : 'Show the watchlist'}><Button size="sm" variant="ghost" iconOnly aria-label={side ? 'Hide the watchlist' : 'Show the watchlist'} pressed={side} onClick={() => setSide(!side)}><Icon icon={side ? PanelRightClose : PanelRight} size="sm" /></Button></Tip>
        </header>
        <div className="mk__bar">
          {/* The kit's control holds six: the common ones, and the rest in a menu, as TradingView keeps favourites. */}
          {(() => {
            const all = sym?.intervals ?? [1440]
            const quick = all.length <= 6 ? all : all.filter((i) => QUICK.includes(i))
            const rest = all.filter((i) => !quick.includes(i))
            return (
              <>
                <Segmented small label="Timeframe" value={quick.includes(interval) ? String(interval) : ''} onChange={(v) => setInterval(Number(v))} options={quick.map((i) => ({ value: String(i), label: LABEL[i] ?? `${i}m` }))} />
                {rest.length > 0 && <Menu trigger={<Button size="sm" variant="ghost" pressed={!quick.includes(interval)}>{quick.includes(interval) ? 'More' : LABEL[interval]}<Icon icon={ChevronDown} size="sm" /></Button>}
                  items={[{ kind: 'label', label: 'Timeframe' }, ...all.map((i) => ({ label: LONG[i] ?? `${i} minutes`, hint: LABEL[i], checked: i === interval, onSelect: () => setInterval(i) }))]} />}
              </>
            )
          })()}
          <Menu trigger={<Button size="sm" variant="ghost"><Icon icon={type === 'line' || type === 'area' ? LineChart : ChartCandlestick} size="sm" />{TYPES.find((t) => t.value === type)?.label}<Icon icon={ChevronDown} size="sm" /></Button>}
            items={[{ kind: 'label', label: 'Chart type' }, ...TYPES.map((t) => ({ label: t.label, checked: t.value === type, onSelect: () => setSetting('markets.type', t.value) }))]} />
          <Menu trigger={<Button size="sm" variant="ghost"><Icon icon={Plus} size="sm" />Indicators<span className="d-text-tertiary">{shown.length}</span></Button>} items={indicators} />
          <Button size="sm" variant="ghost" pressed={pine} onClick={() => setPine(!pine)}><Icon icon={Code} size="sm" />Pine editor</Button>
        </div>
        <div className="mk__stage">
          <div className="mk__tools" role="toolbar" aria-label="Drawing tools" aria-orientation="vertical">
            {TOOLS.map((t) => <Tip key={t.value} side="right" label={t.label}><Button variant="ghost" size="sm" iconOnly pressed={tool === t.value} aria-label={t.label} onClick={() => setTool(t.value)}><Icon icon={t.icon} /></Button></Tip>)}
            <hr />
            <Tip side="right" label={hideDrawings ? 'Show the drawings' : 'Hide the drawings'}><Button variant="ghost" size="sm" iconOnly pressed={hideDrawings} aria-label="Hide the drawings" onClick={() => setHideDrawings(!hideDrawings)}><Icon icon={hideDrawings ? EyeOff : Eye} /></Button></Tip>
            <Tip side="right" label="Remove every drawing on this symbol"><Button variant="ghost" size="sm" iconOnly aria-label="Remove every drawing" disabled={!myDrawings.length} onClick={() => setDrawings({ ...drawings, [symbol]: [] })}><Icon icon={Trash2} /></Button></Tip>
          </div>
          {problem ? <div className="mkr__cover"><Signal level="caution">{problem}</Signal></div>
            : !data ? <div className="mkr__cover"><Skeleton rows={4} /></div>
            : candles.length === 0 ? <div className="mkr__cover"><Empty icon={ChartCandlestick} title="No candles" text="The feed gave none for this timeframe." /></div>
            : <Chart key={`${symbol}@${interval}`} all={candles} tf={tf} type={type} hollow={colours.style === 'hollow'} shown={shown} tool={tool} onToolDone={() => setTool('cursor')}
                drawings={myDrawings} onDrawings={(d) => setDrawings({ ...drawings, [symbol]: d })} hideDrawings={hideDrawings} alerts={[]}
                title={`${symbol}, ${tf.label}`} split={false} times={candles.map((c) => c.t)} warm={0} real
                onHide={(key) => setHidden((h) => (h.includes(key) ? h.filter((x) => x !== key) : [...h, key]))}
                onRemove={(key) => (key === 'editor' ? setEditor(null) : turn(key))} onLocal={() => {}} />}
        </div>
        {pine && <PinePanel symbol={symbol} interval={interval} onRan={setEditor} onClose={() => setPine(false)} />}
      </section>

      {side && (
        <aside className="mkr__side" aria-label="Watchlist">
          <div className="app-side__head"><span className="d-type-headline">Watchlist</span><span className="d-type-caption d-text-tertiary">1D</span></div>
          <div className="mkr__watch">
            {watch.length === 0 && <Empty icon={Star} title="Nothing watched" text="Star a symbol in the search above." />}
            <ul className="d-list">
              {watch.map((id) => {
                const s = symbols.find((x) => x.id === id)
                const q = quotes[id]
                return (
                  <li key={id} className="mk__watch">
                    <ContextMenu items={[
                      { label: 'Open', lead: <Icon icon={ChartCandlestick} size="sm" />, onSelect: () => setSymbol(id) },
                      { label: 'Remove from the watchlist', lead: <Icon icon={X} size="sm" />, onSelect: () => star(id) },
                    ]}>
                      <button type="button" className={cx('d-row', 'mkr__row')} aria-current={id === symbol} onClick={() => setSymbol(id)}>
                        <span className="d-row__text"><span className="d-row__title d-mono">{id}</span><span className="d-row__sub">{s?.name ?? ''}</span></span>
                        <span className="mkr__quote">
                          <span className="d-mono">{q ? fmt(q.last) : '…'}</span>
                          {q && <span className="mk__change" data-up={q.change >= 0 ? '' : undefined}>{q.change >= 0 ? '+' : '−'}{Math.abs(q.change).toFixed(2)}%</span>}
                        </span>
                      </button>
                    </ContextMenu>
                    <button type="button" className="mk__mini" aria-label={`Remove ${id} from the watchlist`} onClick={() => star(id)}><Icon icon={X} size="sm" /></button>
                  </li>
                )
              })}
            </ul>
          </div>
          {last && (
            <section className="mkr__details" aria-label={`${symbol} in detail`}>
              <p className="mkr__details-head"><span className="d-mono">{symbol}</span><span className="d-text-tertiary">{tf.label}</span></p>
              <p className="mkr__big d-mono">{fmt(last.c)} <span className="mk__change" data-up={moved >= 0 ? '' : undefined}>{moved >= 0 ? '+' : '−'}{Math.abs(moved).toFixed(2)}%</span></p>
              <Facts rows={[
                ['Open', fmt(last.o)], ['High', fmt(last.h)], ['Low', fmt(last.l)], ['Volume', last.v ? fmt(last.v) : '–'],
                ['Highest kept', fmt(Math.max(...candles.map((c) => c.h)))], ['Lowest kept', fmt(Math.min(...candles.map((c) => c.l)))],
                ['From', data?.from ?? '', 'Public feeds, read by the core: no account, no key. Asked again every 30 seconds while this is open.'],
              ]} />
              <NotBuilt>Stocks, alerts, saved layouts, comparing symbols</NotBuilt>
              <Signal level="note">Demido charts and tests. It does not place trades and gives no investment advice.</Signal>
            </section>
          )}
        </aside>
      )}
    </div>
  )
}
