/* Markets, for real (DECISIONS M2, M3, M7, M9, M15 to M19; research note 33).

   Prices from public feeds through the core (Kraken, Coinbase when Kraken does not answer, the ECB's daily
   rates), drawn by Demido's own chart. Every indicator is a script of Demido's own Pine-compatible engine, run
   by the core, the built-in ones too; the person's scripts are kept on this computer, and "Write with Demido"
   has a session's model write one, run it, and mend it from the engine's error. Nothing here places an order,
   and nothing reads TradingView. */
import { useEffect, useMemo, useRef, useState } from 'react'
import { useNavigate } from 'react-router'
import { Copy, Play, RefreshCw, Save, Sparkles, Trash2 } from 'lucide-react'
import { coreGet, coreSend } from '@/data/core'
import { toast, useSetting } from '@/data/store'
import { Button, Glyph, Icon, NotBuilt, Signal, Tip, Brief } from '@/ui'

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

const LABEL: Record<number, string> = { 1: '1m', 5: '5m', 15: '15m', 30: '30m', 60: '1h', 240: '4h', 1440: '1D', 10080: '1W' }
const SHOWN = 150
const MAIN_H = 340
const PANE_H = 110
const AXIS_W = 64

function useWidth() {
  const ref = useRef<HTMLDivElement>(null)
  const [w, setW] = useState(800)
  useEffect(() => {
    if (!ref.current) return
    const o = new ResizeObserver(([e]) => setW(Math.max(320, Math.floor(e.contentRect.width))))
    o.observe(ref.current)
    return () => o.disconnect()
  }, [])
  return { ref, w }
}

const fmt = (v: number) => (Math.abs(v) >= 1000 ? v.toLocaleString(undefined, { maximumFractionDigits: 1 }) : Math.abs(v) >= 1 ? v.toFixed(2) : v.toPrecision(4))

/** Demido's own chart: candles, volume, the scripts that draw over them, and a pane for each that does not. */
function Chart({ candles, results, interval }: { candles: Candle[]; results: Ran[]; interval: number }) {
  const { ref, w } = useWidth()
  const [at, setAt] = useState<number | null>(null)
  const style = useSetting<string>('markets.candles', 'classic')
  const up = style === 'hollow' ? 'var(--d-color-text-primary)' : 'var(--d-color-success-base)'
  const down = style === 'hollow' ? 'var(--d-color-text-secondary)' : 'var(--d-color-danger-base)'
  const first = Math.max(0, candles.length - SHOWN)
  const shown = candles.slice(first)
  const overlays = results.filter((r) => r.overlay)
  const panes = results.filter((r) => !r.overlay)
  const plotW = w - AXIS_W
  const step = plotW / Math.max(1, shown.length)
  const x = (i: number) => i * step + step / 2
  const values = (p: Plot) => p.values.slice(first)
  let lo = Math.min(...shown.map((c) => c.l))
  let hi = Math.max(...shown.map((c) => c.h))
  for (const r of overlays) for (const p of r.plots) for (const v of values(p)) if (v !== null) { lo = Math.min(lo, v); hi = Math.max(hi, v) }
  if (!Number.isFinite(lo) || lo === hi) { lo -= 1; hi += 1 }
  const pad = (hi - lo) * 0.05
  const y = (v: number) => MAIN_H - 8 - ((v - (lo - pad)) / (hi - lo + 2 * pad)) * (MAIN_H - 16)
  const vMax = Math.max(1, ...shown.map((c) => c.v))
  const line = (vals: (number | null)[], Y: (v: number) => number) => {
    let d = ''
    let pen = false
    vals.forEach((v, i) => { if (v === null) { pen = false; return } d += `${pen ? 'L' : 'M'}${x(i).toFixed(1)},${Y(v).toFixed(1)}`; pen = true })
    return d
  }
  const hover = at !== null ? shown[at] : shown[shown.length - 1]
  const height = MAIN_H + panes.length * PANE_H
  const ticks = 5
  return (
    <div ref={ref} className="mk-real__chart">
      {hover && (
        <div className="mk-real__legend d-mono d-type-caption">
          {new Date(hover.t).toLocaleString(undefined, interval >= 1440 ? { day: 'numeric', month: 'short', year: 'numeric' } : { day: 'numeric', month: 'short', hour: '2-digit', minute: '2-digit' })} · O {fmt(hover.o)} H {fmt(hover.h)} L {fmt(hover.l)} C {fmt(hover.c)}{hover.v ? ` · V ${fmt(hover.v)}` : ''}
          {overlays.flatMap((r) => r.plots).map((p) => { const v = values(p)[at ?? shown.length - 1]; return <span key={p.title} style={{ color: p.color }}> · {p.title} {v === null || v === undefined ? '–' : fmt(v)}</span> })}
        </div>
      )}
      <svg width={w} height={height} role="img" aria-label={`Candles, ${shown.length} of them`} onMouseMove={(e) => { const r = e.currentTarget.getBoundingClientRect(); const i = Math.floor((e.clientX - r.left) / step); setAt(i >= 0 && i < shown.length ? i : null) }} onMouseLeave={() => setAt(null)}>
        {Array.from({ length: ticks }, (_, k) => { const v = lo + ((hi - lo) * k) / (ticks - 1); return <g key={k}><line x1={0} x2={plotW} y1={y(v)} y2={y(v)} stroke="var(--d-color-border-hairline)" /><text x={plotW + 6} y={y(v) + 4} className="mk-real__axis">{fmt(v)}</text></g> })}
        {shown.map((c, i) => { const h = (c.v / vMax) * MAIN_H * 0.18; return <rect key={`v${c.t}`} x={x(i) - step * 0.35} y={MAIN_H - h} width={step * 0.7} height={h} fill={c.c >= c.o ? up : down} opacity={0.18} /> })}
        {shown.map((c, i) => {
          const rising = c.c >= c.o
          const top = y(Math.max(c.o, c.c))
          const body = Math.max(1, Math.abs(y(c.o) - y(c.c)))
          return (
            <g key={c.t} stroke={rising ? up : down} fill={rising ? (style === 'hollow' ? 'none' : up) : down}>
              <line x1={x(i)} x2={x(i)} y1={y(c.h)} y2={y(c.l)} />
              <rect x={x(i) - step * 0.35} y={top} width={Math.max(1, step * 0.7)} height={body} />
            </g>
          )
        })}
        {overlays.map((r) => r.plots.map((p) => <path key={r.title + p.title} d={line(values(p), y)} fill="none" stroke={p.color} strokeWidth={1.5} />))}
        {at !== null && <line x1={x(at)} x2={x(at)} y1={0} y2={height} stroke="var(--d-color-text-tertiary)" strokeDasharray="3 3" />}
        {panes.map((r, k) => {
          const top = MAIN_H + k * PANE_H
          const all = [...r.plots.flatMap((p) => values(p).filter((v): v is number => v !== null)), ...r.hlines.map((h) => h.price)]
          let a = Math.min(...all)
          let b = Math.max(...all)
          if (!Number.isFinite(a) || a === b) { a -= 1; b += 1 }
          const Y = (v: number) => top + PANE_H - 6 - ((v - a) / (b - a)) * (PANE_H - 22)
          return (
            <g key={r.title + k}>
              <line x1={0} x2={w} y1={top} y2={top} stroke="var(--d-color-border-hairline)" />
              <text x={4} y={top + 13} className="mk-real__axis">{r.title}</text>
              {r.hlines.map((h) => <line key={h.price} x1={0} x2={plotW} y1={Y(h.price)} y2={Y(h.price)} stroke={h.color} strokeDasharray="4 3" />)}
              {r.plots.map((p) => <path key={p.title} d={line(values(p), Y)} fill="none" stroke={p.color} strokeWidth={1.5} />)}
              <text x={plotW + 6} y={Y(b) + 4} className="mk-real__axis">{fmt(b)}</text>
              <text x={plotW + 6} y={Y(a) + 4} className="mk-real__axis">{fmt(a)}</text>
            </g>
          )
        })}
      </svg>
    </div>
  )
}

export function RealMarkets() {
  const navigate = useNavigate()
  const [symbols, setSymbols] = useState<Symbol[]>([])
  const [symbol, setSymbol] = useState('BTC/EUR')
  const [interval, setInterval] = useState(1440)
  const [data, setData] = useState<Candles | null>(null)
  const [problem, setProblem] = useState<string | null>(null)
  const [builtIn, setBuiltIn] = useState<BuiltIn[]>([])
  const [on, setOn] = useState<string[]>(['ema20'])
  const [results, setResults] = useState<Record<string, Ran>>({})
  const [source, setSource] = useState('//@version=6\nindicator("My script", overlay=true)\nlen = input.int(10, "Length")\nplot(ta.ema(close, len), "EMA", color=color.green)')
  const [mine, setMine] = useState<Ran | null>(null)
  const [error, setError] = useState<PineError | string | null>(null)
  const [scripts, setScripts] = useState<Script[]>([])
  const [kept, setKept] = useState<Script | null>(null)
  const [request, setRequest] = useState('')
  const [writing, setWriting] = useState(false)
  const [written, setWritten] = useState<Written | null>(null)
  const sym = symbols.find((s) => s.id === symbol)

  useEffect(() => {
    void coreGet<Symbol[]>('/markets/symbols').then(setSymbols, () => {})
    void coreGet<BuiltIn[]>('/markets/built-in').then(setBuiltIn, () => {})
    void coreGet<Script[]>('/markets/scripts').then(setScripts, () => {})
  }, [])
  const read = (fresh = false) => coreGet<Candles>(`/markets/candles?symbol=${encodeURIComponent(symbol)}&interval=${interval}${fresh ? '&fresh=true' : ''}`).then((d) => { setData(d); setProblem(null) }, (e: Error) => setProblem(e.message))
  useEffect(() => {
    setData(null)
    void read()
    // Asked again while the screen is open: the core keeps an answer a quarter of its interval, 15 s at least.
    const every = window.setInterval(() => void read(), 30_000)
    return () => window.clearInterval(every)
  }, [symbol, interval])
  const run = (src: string) => coreSend<PineRan>('POST', '/markets/pine/run', { source: src, symbol, interval })
  useEffect(() => {
    if (!data) return
    for (const id of on) {
      const b = builtIn.find((x) => x.id === id)
      if (b) void run(b.source).then((r) => { if (r.ran) setResults((all) => ({ ...all, [id]: r.ran! })) }, () => {})
    }
  }, [data, on.join(','), builtIn.length])
  const runMine = async (src = source) => {
    try {
      const r = await run(src)
      setMine(r.ran)
      setError(r.error)
    } catch (e) {
      setError((e as Error).message)
    }
  }
  const keep = async () => {
    const name = (source.match(/indicator\(\s*["']([^"']+)/)?.[1]) ?? 'Script'
    try {
      const k = await coreSend<Script>('PUT', '/markets/scripts', { id: kept?.id, name, source })
      setKept(k)
      setScripts(await coreGet<Script[]>('/markets/scripts'))
      toast(`Kept on this computer: ${k.name}.`, undefined, 'task-done')
    } catch (e) {
      toast(`Not kept. ${(e as Error).message}`, undefined, 'task-failed')
    }
  }
  const write = async () => {
    if (!request.trim()) return
    setWriting(true); setWritten(null)
    try {
      const w = await coreSend<Written>('POST', '/markets/pine/write', { request, symbol, interval, source, session: written?.session })
      setWritten(w)
      setSource(w.source)
      setMine(w.ran)
      setError(w.error)
    } catch (e) {
      toast(`Not written. ${(e as Error).message}`, undefined, 'task-failed')
    }
    setWriting(false)
  }
  const shownResults = useMemo(() => [...on.map((id) => results[id]).filter((r): r is Ran => !!r), ...(mine ? [mine] : [])], [on, results, mine])
  const last = data?.candles[data.candles.length - 1]
  const before = data?.candles[data.candles.length - 2]
  const change = last && before ? ((last.c - before.c) / before.c) * 100 : null

  return (
    <div className="app-page"><div className="app-page__inner app-stack">
      <header className="app-row" style={{ flexWrap: 'wrap', gap: 'var(--d-space-8)', alignItems: 'baseline' }}>
        <h1 className="d-type-title-2" style={{ margin: 0 }}>{symbol}</h1>
        {last && <span className="d-mono">{fmt(last.c)}</span>}
        {change !== null && <span className="d-mono" style={{ color: change >= 0 ? 'var(--d-color-success-base)' : 'var(--d-color-danger-base)' }}>{change >= 0 ? '+' : ''}{change.toFixed(2)}%</span>}
        <span className="d-type-caption d-text-tertiary">{sym?.name}{data ? ` · from ${data.from}` : ''}</span>
        <span className="app-spacer" />
        <select className="d-input" aria-label="Symbol" value={symbol} onChange={(e) => { setSymbol(e.target.value); const s = symbols.find((x) => x.id === e.target.value); if (s && !s.intervals.includes(interval)) setInterval(s.intervals[s.intervals.length > 1 ? 6 : 0] ?? 1440) }}>
          {symbols.map((s) => <option key={s.id} value={s.id}>{s.id} · {s.name}</option>)}
        </select>
        <div className="app-row" role="group" aria-label="Timeframe" style={{ gap: 'var(--d-space-2)' }}>
          {(sym?.intervals ?? [1440]).map((i) => <Button key={i} size="sm" variant="ghost" pressed={i === interval} onClick={() => setInterval(i)}>{LABEL[i] ?? `${i}m`}</Button>)}
        </div>
        <Tip label="Ask the feed again now"><Button size="sm" variant="ghost" iconOnly aria-label="Ask again" onClick={() => void read(true)}><Icon icon={RefreshCw} size="sm" /></Button></Tip>
      </header>
      {problem && <Signal level="caution">{problem}</Signal>}
      {!data && !problem && <p className="app-empty"><Glyph name="task-running" busy /> Reading {symbol}…</p>}
      {data && data.candles.length > 0 && <Chart candles={data.candles} results={shownResults} interval={interval} />}
      <div className="app-row" style={{ flexWrap: 'wrap', gap: 'var(--d-space-4)' }}>
        {builtIn.map((b) => <Button key={b.id} size="sm" variant="ghost" pressed={on.includes(b.id)} onClick={() => setOn(on.includes(b.id) ? on.filter((x) => x !== b.id) : [...on, b.id])}>{b.name}</Button>)}
        <span className="d-type-caption d-text-tertiary">Worked out on this computer, by Demido's engine: as many as you like.</span>
      </div>
      <section className="d-card"><div className="d-card__body app-stack">
        <div className="app-row" style={{ gap: 'var(--d-space-8)', flexWrap: 'wrap' }}>
          <span className="d-card__title">A Pine-compatible script</span>
          <span className="app-spacer" />
          {scripts.length > 0 && (
            <select className="d-input" aria-label="Kept scripts" value={kept?.id ?? ''} onChange={(e) => { const s = scripts.find((x) => x.id === e.target.value); if (s) { setKept(s); setSource(s.source); void runMine(s.source) } }}>
              <option value="">Kept scripts…</option>
              {scripts.map((s) => <option key={s.id} value={s.id}>{s.name}</option>)}
            </select>
          )}
          <Button size="sm" variant="primary" onClick={() => void runMine()}><Icon icon={Play} size="sm" />Run on the chart</Button>
          <Button size="sm" variant="ghost" onClick={() => void keep()}><Icon icon={Save} size="sm" />Keep</Button>
          {kept && <Button size="sm" variant="ghost" onClick={() => void coreSend('DELETE', `/markets/scripts/${kept.id}`).then(async () => { setKept(null); setScripts(await coreGet<Script[]>('/markets/scripts')) })}><Icon icon={Trash2} size="sm" />Delete</Button>}
          <Tip label="Puts the script on the clipboard. Pasting it into TradingView is yours to do."><Button size="sm" variant="ghost" onClick={() => void navigator.clipboard.writeText(source).then(() => toast('The script is on the clipboard.'))}><Icon icon={Copy} size="sm" />Copy for TradingView</Button></Tip>
          {mine && <Button size="sm" variant="ghost" onClick={() => { setMine(null); setError(null) }}>Take it off the chart</Button>}
        </div>
        <textarea className="d-input d-input--mono mk-real__pine" spellCheck={false} rows={8} aria-label="The script" value={source} onChange={(e) => setSource(e.target.value)} />
        {error && <Signal level="caution">{typeof error === 'string' ? error : `Line ${error.line}, column ${error.col}: ${error.message}`}</Signal>}
        {mine && !error && <p className="d-type-caption d-text-secondary">{mine.title}: {mine.plots.length} {mine.plots.length === 1 ? 'line' : 'lines'}{mine.overlay ? ', over the candles' : ', in a pane of its own'}.</p>}
        <div className="app-row" style={{ gap: 'var(--d-space-8)', flexWrap: 'wrap' }}>
          <input className="d-input" style={{ flex: 1, minWidth: '16rem' }} placeholder="Ask Demido to write or change it: an RSI with a 9-bar smoothing line…" value={request} onChange={(e) => setRequest(e.target.value)} onKeyDown={(e) => { if (e.key === 'Enter') void write() }} />
          <Button size="sm" disabled={writing || !request.trim()} onClick={() => void write()}><Icon icon={writing ? RefreshCw : Sparkles} size="sm" />{writing ? 'Writing…' : 'Write with Demido'}</Button>
          {written && <Button size="sm" variant="ghost" onClick={() => navigate(`/sessions/${encodeURIComponent(written.session)}`)}>{written.tries} {written.tries === 1 ? 'try' : 'tries'}, in its session</Button>}
        </div>
        <Brief className="d-type-caption d-text-tertiary" text={"Demido's engine reads a first part of the language (research note 33) and is not TradingView's: a script can behave differently there. Pine Script is TradingView's trademark; Demido is not affiliated with TradingView. A model writes, Demido runs it, and hands the error back to be mended, three tries at most."} />
      </div></section>
      <NotBuilt block>Stocks (no public feed without a key or a check meant for people), live ticks (the candles are asked again every 30 s), drawing tools, alerts, saved layouts, comparing symbols, a logarithmic scale, if and for in scripts, and TradingView's sign-in (shown in the preview).</NotBuilt>
    </div></div>
  )
}
