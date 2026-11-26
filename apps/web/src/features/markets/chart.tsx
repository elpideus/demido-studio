/* The chart. Drawn at its real size in plain SVG, so lines stay one pixel wide.
   Wheel zooms around the pointer, dragging moves through time, and the drawing tools draw on the price area.
   Indices are positions in the whole kept history (`all`), so drawings stay put when you zoom or move. */
import { useEffect, useMemo, useRef, useState } from 'react'
import type { PointerEvent as ReactPointerEvent, WheelEvent as ReactWheelEvent } from 'react'
import { Eraser, Eye, EyeOff, Minus, MousePointer2, Plus, RectangleHorizontal, Ruler, Slash, X } from 'lucide-react'
import type { CSSProperties } from 'react'
import type { LucideIcon } from 'lucide-react'
import { useSetting } from '@/data/store'
import { Icon } from '@/ui'
import { HISTORY, WARM_UP, heikin, timeLabel, timeOf } from './data'
import type { Candle, Drawer, Line, OnChart, Plot, Timeframe } from './data'

export type ChartType = 'candles' | 'bars' | 'line' | 'area' | 'heikin'
export type Tool = 'cursor' | 'trend' | 'hline' | 'rect' | 'measure' | 'erase'
export interface Point { i: number; p: number }
export interface Drawing { id: number; type: 'trend' | 'hline' | 'rect'; a: Point; b: Point }
export interface Shown extends OnChart { drawer: Drawer; refused?: boolean; canLocal?: boolean }
export const fmt = (v: number) => (Math.abs(v) < 10 ? v.toFixed(4) : Math.abs(v) < 1000 ? v.toFixed(2) : v.toLocaleString(undefined, { maximumFractionDigits: 0 }))

export type CandleStyle = 'classic' | 'hollow' | 'custom'
export const CANDLE_OPTIONS: { value: CandleStyle; label: string }[] = [
  { value: 'classic', label: 'Green and red' },
  { value: 'hollow', label: 'Hollow and solid' },
  { value: 'custom', label: 'Custom' },
]
export const TYPES: { value: ChartType; label: string }[] = [
  { value: 'candles', label: 'Candles' }, { value: 'bars', label: 'Bars' }, { value: 'line', label: 'Line' }, { value: 'area', label: 'Area' }, { value: 'heikin', label: 'Heikin Ashi' },
]
export const TOOLS: { value: Tool; label: string; icon: LucideIcon }[] = [
  { value: 'cursor', label: 'Move and zoom', icon: MousePointer2 }, { value: 'trend', label: 'Trend line: drag', icon: Slash }, { value: 'hline', label: 'Horizontal line: click', icon: Minus },
  { value: 'rect', label: 'Rectangle: drag', icon: RectangleHorizontal }, { value: 'measure', label: 'Measure: drag', icon: Ruler }, { value: 'erase', label: 'Erase: click a drawing', icon: Eraser },
]

/** The two candle colours, as CSS variables for the chart and the watchlist. */
export function useCandleColours() {
  const style = useSetting<CandleStyle>('markets.candles', 'classic')
  const up = useSetting('markets.up', '#26a69a')
  const down = useSetting('markets.down', '#ef5350')
  const vars =
    style === 'classic'
      ? { '--mk-up': 'var(--d-color-success-base)', '--mk-down': 'var(--d-color-danger-base)' }
      : style === 'custom'
        ? { '--mk-up': up, '--mk-down': down }
        : { '--mk-up': 'var(--d-color-text-primary)', '--mk-down': 'var(--d-color-text-secondary)' }
  return { style, up, down, vars: vars as CSSProperties }
}

type Scale = (v: number) => number

function useSize<T extends HTMLElement>() {
  const ref = useRef<T>(null)
  const [size, setSize] = useState({ w: 0, h: 0 })
  useEffect(() => {
    if (!ref.current) return
    const ro = new ResizeObserver(([e]) => setSize({ w: Math.floor(e.contentRect.width), h: Math.floor(e.contentRect.height) }))
    ro.observe(ref.current)
    return () => ro.disconnect()
  }, [])
  return [ref, size] as const
}

function Series({ line, first, last, X, Y }: { line: Line; first: number; last: number; X: Scale; Y: Scale }) {
  if (line.colorAt) {
    const parts = []
    for (let i = first + 1; i < last; i++) {
      const a = line.values[i - 1], b = line.values[i]
      if (a == null || b == null) continue
      parts.push(<line key={i} x1={X(i - 1)} y1={Y(a)} x2={X(i)} y2={Y(b)} style={{ stroke: line.colorAt(i) }} />)
    }
    return <g className="mk__line">{parts}</g>
  }
  let d = ''
  let pen = false
  for (let i = first; i < last; i++) {
    const p = line.values[i]
    if (p == null) { pen = false; continue }
    d += `${pen ? 'L' : 'M'}${X(i).toFixed(1)} ${Y(p).toFixed(1)}`
    pen = true
  }
  return <path className="mk__line" d={d} style={{ stroke: line.color }} strokeDasharray={line.dash ? '4 4' : undefined} />
}

function Band({ band, first, last, X, Y }: { band: NonNullable<Plot['band']>; first: number; last: number; X: Scale; Y: Scale }) {
  const parts = []
  for (let i = first + 1; i < last; i++) {
    const u0 = band.upper[i - 1], u1 = band.upper[i], l0 = band.lower[i - 1], l1 = band.lower[i]
    if (u0 == null || u1 == null || l0 == null || l1 == null) continue
    parts.push(<polygon key={i} points={`${X(i - 1)},${Y(u0)} ${X(i)},${Y(u1)} ${X(i)},${Y(l1)} ${X(i - 1)},${Y(l0)}`} style={{ fill: band.colorAt ? band.colorAt(i) : band.color }} />)
  }
  return <g className="mk__band">{parts}</g>
}

function LegendRow({ item, value, split, onHide, onRemove, onLocal }: { item: Shown & { plot: Plot }; value: string; split: boolean; onHide: () => void; onRemove: () => void; onLocal: () => void }) {
  const colour = item.plot.lines?.[0]?.color ?? item.plot.band?.color ?? 'var(--mk-gray)'
  return (
    <div className="mk__row" data-hidden={item.hidden || item.refused ? '' : undefined}>
      <span className="mk__dot" style={{ background: colour }} />
      <span>{item.name}</span>
      {/* Signed in, some are worked out by TradingView and some by Demido: say which, on every one. */}
      {item.engine === 'tradingview' ? <span className="d-badge d-badge--outline">TradingView</span> : split ? <span className="d-badge">Demido</span> : item.entry.startsWith('PINE;') && <span className="d-badge d-badge--outline">Pine · local</span>}
      {item.refused ? (
        <>
          <span className="mk__refused">no room on your TradingView plan</span>
          {item.canLocal && <button type="button" className="mk__link" onClick={onLocal}>Work it out here instead</button>}
        </>
      ) : (
        <span className="d-mono d-text-secondary">{item.hidden ? 'hidden' : value}</span>
      )}
      {!item.refused && <button type="button" className="mk__mini" aria-label={`${item.hidden ? 'Show' : 'Hide'} ${item.name}`} onClick={onHide}><Icon icon={item.hidden ? EyeOff : Eye} size="sm" /></button>}
      <button type="button" className="mk__mini" aria-label={`Remove ${item.name}`} onClick={onRemove}><Icon icon={X} size="sm" /></button>
    </div>
  )
}

interface Props {
  /** The whole kept history, warm-up included. */
  all: Candle[]
  tf: Timeframe
  type: ChartType
  /** Rising candles are drawn hollow. */
  hollow: boolean
  shown: Shown[]
  tool: Tool
  onToolDone: () => void
  drawings: Drawing[]
  onDrawings: (d: Drawing[]) => void
  hideDrawings: boolean
  alerts: { id: number; price: number }[]
  /** Where alerts can be set: without it, the chart offers none. */
  onAlert?: (price: number) => void
  title: string
  /** Signed in to TradingView: indicators are split between two engines, so each one is labelled. */
  split: boolean
  onHide: (key: string) => void
  onRemove: (key: string) => void
  onLocal: (key: string) => void
  /** Real candles: when each began (ms), for the time axis in the person's own time. Without it, made-up times. */
  times?: number[]
  /** Candles before the first one shown, kept only so slow indicators have values (the preview's warm-up). */
  warm?: number
  /** Real prices, said so to a screen reader. */
  real?: boolean
}

/** A real candle's time, in the person's own time zone, as the axis and the crosshair say it. */
function realLabel(t: number, minutes: number, long = false): string {
  const d = new Date(t)
  const day = d.toLocaleDateString(undefined, { day: 'numeric', month: 'short' })
  const clock = d.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' })
  if (minutes >= 10080) return d.toLocaleDateString(undefined, { day: 'numeric', month: 'short', year: '2-digit' })
  if (minutes >= 1440) return long ? d.toLocaleDateString(undefined, { day: 'numeric', month: 'short', year: 'numeric' }) : day
  return long ? `${day}, ${clock}` : clock === '00:00' ? day : clock
}

export function Chart({ all, tf, type, hollow, shown, tool, onToolDone, drawings, onDrawings, hideDrawings, alerts, onAlert, title, split, onHide, onRemove, onLocal, times, warm = WARM_UP, real }: Props) {
  const history = times ? Math.max(24, all.length) : HISTORY
  const label = (i: number, long = false) => (times ? realLabel(times[Math.max(0, Math.min(times.length - 1, i))], tf.minutes, long) : timeLabel(timeOf(i, all.length, tf), tf, long))
  const [box, size] = useSize<HTMLDivElement>()
  const [view, setView] = useState({ end: all.length + 6, count: 120 })
  const [hover, setHover] = useState<{ x: number; y: number } | null>(null)
  const [draft, setDraft] = useState<{ type: 'trend' | 'rect' | 'measure'; a: Point; b: Point } | null>(null)
  const [measure, setMeasure] = useState<{ a: Point; b: Point } | null>(null)
  const drag = useRef<{ x: number; end: number } | null>(null)
  const data = useMemo(() => (type === 'heikin' ? heikin(all) : all), [all, type])
  const plots = useMemo(() => shown.map((s) => ({ ...s, plot: s.drawer.plot(all) })), [shown, all])
  useEffect(() => { setDraft(null); if (tool !== 'cursor') setMeasure(null) }, [tool])

  const AXIS = 68, TIME = 22
  const W = Math.max(1, size.w - AXIS)
  const live = plots.filter((p) => !p.hidden && !p.refused)
  const overlays = live.filter((p) => p.drawer.where === 'price')
  const panes = live.filter((p) => p.drawer.where === 'pane')
  const listed = plots.filter((p) => p.drawer.where === 'price' || p.hidden || p.refused)
  const paneH = Math.round(Math.max(60, Math.min(112, (size.h - TIME) * 0.2)))
  const mainH = Math.max(90, size.h - TIME - panes.length * paneH)
  const start = view.end - view.count
  const step = W / view.count
  const first = Math.max(start, warm), last = Math.max(first + 1, Math.min(view.end, all.length))
  const X: Scale = (i) => (i - start + 0.5) * step
  const vis: number[] = []
  for (let i = first; i < last; i++) vis.push(i)
  let lo = Math.min(...vis.map((i) => data[i].l))
  let hi = Math.max(...vis.map((i) => data[i].h))
  const pad = (hi - lo) * 0.08 || hi * 0.01
  lo -= pad
  hi += pad
  const Y: Scale = (v) => ((hi - v) / (hi - lo)) * mainH
  const P: Scale = (y) => hi - (y / mainH) * (hi - lo)
  const bw = Math.max(1, step * 0.64)
  const ticks = Array.from({ length: 5 }, (_, k) => lo + ((hi - lo) * (k + 0.5)) / 5)
  const newest = data[all.length - 1]

  const clamp = (end: number, count: number) => {
    const c = Math.round(Math.max(24, Math.min(history, count)))
    return { count: c, end: Math.round(Math.max(warm + Math.min(c, 24), Math.min(all.length + Math.round(c * 0.4), end))) }
  }
  const local = (e: ReactPointerEvent | ReactWheelEvent) => {
    const r = box.current!.getBoundingClientRect()
    return { x: e.clientX - r.left, y: e.clientY - r.top }
  }
  const capture = (e: ReactPointerEvent<HTMLDivElement>) => { try { e.currentTarget.setPointerCapture(e.pointerId) } catch { /* a pointer that is already gone */ } }
  const pointAt = (x: number, y: number): Point => ({ i: start + x / step - 0.5, p: P(Math.max(0, Math.min(mainH, y))) })

  const down = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (e.button !== 0 || (e.target as Element).closest('button')) return
    const { x, y } = local(e)
    if (x > W) return
    if (measure) setMeasure(null)
    if (tool === 'cursor') {
      capture(e)
      drag.current = { x, end: view.end }
    } else if (y <= mainH && tool === 'hline') {
      onDrawings([...drawings, { id: Date.now(), type: 'hline', a: pointAt(x, y), b: pointAt(x, y) }])
      onToolDone()
    } else if (y <= mainH && tool !== 'erase' && tool !== 'hline') {
      capture(e)
      setDraft({ type: tool, a: pointAt(x, y), b: pointAt(x, y) })
    }
  }
  const move = (e: ReactPointerEvent<HTMLDivElement>) => {
    const { x, y } = local(e)
    setHover({ x, y })
    if (drag.current) { const d = drag.current; setView((v) => clamp(d.end - Math.round((x - d.x) / step), v.count)) }
    else if (draft) setDraft({ ...draft, b: pointAt(x, y) })
  }
  const up = () => {
    drag.current = null
    if (!draft) return
    const moved = Math.abs(draft.a.i - draft.b.i) > 0.4 || Math.abs(Y(draft.a.p) - Y(draft.b.p)) > 3
    if (draft.type === 'measure') { if (moved) setMeasure({ a: draft.a, b: draft.b }) }
    else if (moved) onDrawings([...drawings, { id: Date.now(), type: draft.type, a: draft.a, b: draft.b }])
    setDraft(null)
    onToolDone()
  }
  const wheel = (e: ReactWheelEvent<HTMLDivElement>) => {
    const f = Math.min(local(e).x, W) / W
    const zoom = e.deltaY > 0 ? 1.18 : 1 / 1.18
    setView((v) => {
      const count = Math.max(24, Math.min(history, v.count * zoom))
      const under = v.end - v.count + f * v.count
      return clamp(under - f * count + count, count)
    })
  }

  const inside = hover && hover.x <= W ? hover : null
  const at = inside ? Math.max(first, Math.min(last - 1, Math.round(start + inside.x / step - 0.5))) : last - 1
  const cur = data[at]
  const everyLabel = Math.max(1, Math.ceil(96 / step))
  const m = draft?.type === 'measure' ? draft : measure
  const erase = tool === 'erase'

  return (
    <div className="mk__box" ref={box} data-candles={hollow && type === 'candles' ? 'hollow' : undefined} data-tool={tool} onPointerDown={down} onPointerMove={move} onPointerUp={up} onPointerLeave={() => setHover(null)} onWheel={wheel}>
      {size.w > 0 && (
        <svg className="mk__svg" width={size.w} height={size.h} role="img" aria-label={`${title}: chart of ${real ? '' : 'made-up '}prices with ${shown.length} indicators`}>
          <defs>
            <clipPath id="mk-main"><rect width={W} height={mainH} /></clipPath>
            <linearGradient id="mk-area" x1="0" x2="0" y1="0" y2="1"><stop offset="0" stopColor="currentColor" stopOpacity="0.28" /><stop offset="1" stopColor="currentColor" stopOpacity="0" /></linearGradient>
          </defs>
          {ticks.map((t) => (
            <g key={t}>
              <line className="mk__grid" x1={0} x2={W} y1={Y(t)} y2={Y(t)} />
              <text className="mk__tick" x={W + 8} y={Y(t) + 4}>{fmt(t)}</text>
            </g>
          ))}
          <g clipPath="url(#mk-main)">
            {overlays.filter((p) => p.plot.band).map((p) => <Band key={p.key} band={p.plot.band!} first={first} last={last} X={X} Y={Y} />)}
            {(type === 'candles' || type === 'heikin') && vis.map((i) => {
              const d = data[i]
              return (
                <g key={i} className="mk__candle" data-up={d.c >= d.o ? '' : undefined}>
                  <line x1={X(i)} x2={X(i)} y1={Y(d.h)} y2={Y(d.l)} />
                  <rect x={X(i) - bw / 2} width={bw} y={Y(Math.max(d.o, d.c))} height={Math.max(1, Math.abs(Y(d.o) - Y(d.c)))} rx={bw > 4 ? 1 : 0} />
                </g>
              )
            })}
            {type === 'bars' && vis.map((i) => {
              const d = data[i]
              return (
                <g key={i} className="mk__candle" data-up={d.c >= d.o ? '' : undefined}>
                  <line x1={X(i)} x2={X(i)} y1={Y(d.h)} y2={Y(d.l)} />
                  <line x1={X(i) - bw / 2} x2={X(i)} y1={Y(d.o)} y2={Y(d.o)} />
                  <line x1={X(i)} x2={X(i) + bw / 2} y1={Y(d.c)} y2={Y(d.c)} />
                </g>
              )
            })}
            {(type === 'line' || type === 'area') && (() => {
              const d = vis.map((i, k) => `${k ? 'L' : 'M'}${X(i).toFixed(1)} ${Y(data[i].c).toFixed(1)}`).join('')
              return (
                <g className="mk__close">
                  {type === 'area' && <path d={`${d}L${X(last - 1)} ${mainH}L${X(first)} ${mainH}Z`} fill="url(#mk-area)" />}
                  <path className="mk__line" d={d} />
                </g>
              )
            })()}
            {overlays.flatMap((p) => (p.plot.lines ?? []).map((l, k) => <Series key={`${p.key}-${k}`} line={l} first={first} last={last} X={X} Y={Y} />))}

            {alerts.map((a) => <line key={a.id} className="mk__alert" x1={0} x2={W} y1={Y(a.price)} y2={Y(a.price)} />)}
            {!hideDrawings && drawings.map((d) => {
              const hit = { onPointerDown: (e: ReactPointerEvent) => { if (erase) { e.stopPropagation(); onDrawings(drawings.filter((x) => x.id !== d.id)) } } }
              const x1 = X(d.a.i), y1 = Y(d.a.p), x2 = X(d.b.i), y2 = Y(d.b.p)
              return (
                <g key={d.id} className="mk__drawing" data-erase={erase ? '' : undefined} {...hit}>
                  {d.type === 'hline' && <><line x1={0} x2={W} y1={y1} y2={y1} /><line className="mk__hit" x1={0} x2={W} y1={y1} y2={y1} /></>}
                  {d.type === 'trend' && <><line x1={x1} y1={y1} x2={x2} y2={y2} /><line className="mk__hit" x1={x1} y1={y1} x2={x2} y2={y2} /><circle cx={x1} cy={y1} r={3} /><circle cx={x2} cy={y2} r={3} /></>}
                  {d.type === 'rect' && <rect x={Math.min(x1, x2)} y={Math.min(y1, y2)} width={Math.abs(x2 - x1)} height={Math.abs(y2 - y1)} />}
                </g>
              )
            })}
            {draft && draft.type !== 'measure' && (
              <g className="mk__drawing">
                {draft.type === 'trend' ? <line x1={X(draft.a.i)} y1={Y(draft.a.p)} x2={X(draft.b.i)} y2={Y(draft.b.p)} /> : <rect x={Math.min(X(draft.a.i), X(draft.b.i))} y={Math.min(Y(draft.a.p), Y(draft.b.p))} width={Math.abs(X(draft.b.i) - X(draft.a.i))} height={Math.abs(Y(draft.b.p) - Y(draft.a.p))} />}
              </g>
            )}
            {m && (() => {
              const x1 = X(m.a.i), y1 = Y(m.a.p), x2 = X(m.b.i), y2 = Y(m.b.p)
              const diff = m.b.p - m.a.p
              const text = `${diff >= 0 ? '+' : '−'}${fmt(Math.abs(diff))} (${((diff / m.a.p) * 100).toFixed(2)}%) · ${Math.round(Math.abs(m.b.i - m.a.i))} candles`
              return (
                <g className="mk__measure" data-up={diff >= 0 ? '' : undefined}>
                  <rect x={Math.min(x1, x2)} y={Math.min(y1, y2)} width={Math.abs(x2 - x1)} height={Math.abs(y2 - y1)} />
                  <text x={(x1 + x2) / 2} y={Math.min(y1, y2) - 6} textAnchor="middle">{text}</text>
                </g>
              )
            })()}
          </g>
          {!hideDrawings && drawings.filter((d) => d.type === 'hline').map((d) => (
            <g key={d.id}><rect className="mk__tag" x={W} y={Y(d.a.p) - 9} width={AXIS} height={18} rx={3} /><text className="mk__tick mk__tick--tag" x={W + 8} y={Y(d.a.p) + 4}>{fmt(d.a.p)}</text></g>
          ))}
          {alerts.map((a) => (
            <g key={a.id}><rect className="mk__tag mk__tag--alert" x={W} y={Y(a.price) - 9} width={AXIS} height={18} rx={3} /><text className="mk__tick mk__tick--last" x={W + 8} y={Y(a.price) + 4}>{fmt(a.price)}</text></g>
          ))}
          <line className="mk__now" x1={0} x2={W} y1={Y(newest.c)} y2={Y(newest.c)} data-up={newest.c >= newest.o ? '' : undefined} />
          <rect className="mk__last" x={W} y={Y(newest.c) - 9} width={AXIS} height={18} rx={3} data-up={newest.c >= newest.o ? '' : undefined} />
          <text className="mk__tick mk__tick--last" x={W + 8} y={Y(newest.c) + 4}>{fmt(newest.c)}</text>

          {panes.map((p, k) => {
            const top = mainH + k * paneH
            const vals: number[] = []
            for (const i of vis) {
              for (const l of p.plot.lines ?? []) { const v = l.values[i]; if (v != null) vals.push(v) }
              const b = p.plot.bars?.values[i]
              if (b != null) vals.push(b)
            }
            let min = p.plot.range?.[0] ?? Math.min(...vals, p.plot.bars ? 0 : Infinity)
            let max = p.plot.range?.[1] ?? Math.max(...vals, p.plot.bars ? 0 : -Infinity)
            if (!p.plot.range) { const g = (max - min) * 0.1 || 1; max += g; if (min < 0 || !p.plot.bars) min -= g }
            const PY: Scale = (v) => top + 20 + ((max - v) / (max - min)) * (paneH - 26)
            return (
              <g key={p.key}>
                <line className="mk__rule" x1={0} x2={size.w} y1={top} y2={top} />
                {(p.plot.guides ?? []).map((g) => (
                  <g key={g}>
                    <line className="mk__grid" x1={0} x2={W} y1={PY(g)} y2={PY(g)} strokeDasharray="3 4" />
                    <text className="mk__tick" x={W + 8} y={PY(g) + 4}>{g}</text>
                  </g>
                ))}
                {p.plot.bars && vis.map((i) => {
                  const v = p.plot.bars!.values[i]
                  if (v == null) return null
                  return <rect key={i} className="mk__bar" x={X(i) - bw / 2} width={bw} y={PY(Math.max(v, 0))} height={Math.max(1, Math.abs(PY(v) - PY(0)))} style={{ fill: p.plot.bars!.colorAt(i) }} />
                })}
                {p.plot.band && <Band band={p.plot.band} first={first} last={last} X={X} Y={PY} />}
                {(p.plot.lines ?? []).map((l, j) => <Series key={j} line={l} first={first} last={last} X={X} Y={PY} />)}
              </g>
            )
          })}

          <line className="mk__rule" x1={0} x2={size.w} y1={size.h - TIME} y2={size.h - TIME} />
          {vis.filter((i) => i % everyLabel === 0).map((i) => <text key={i} className="mk__tick" x={X(i)} y={size.h - 7} textAnchor="middle">{label(i)}</text>)}

          {inside && !drag.current && (
            <g className="mk__cross">
              <line x1={X(at)} x2={X(at)} y1={0} y2={size.h - TIME} />
              <rect className="mk__tag" x={Math.max(0, Math.min(W - 112, X(at) - 56))} y={size.h - TIME + 2} width={112} height={18} rx={3} />
              <text className="mk__tick mk__tick--tag" x={Math.max(56, Math.min(W - 56, X(at)))} y={size.h - 7} textAnchor="middle">{label(at, true)}</text>
              {inside.y <= mainH && (
                <>
                  <line x1={0} x2={W} y1={inside.y} y2={inside.y} />
                  <rect className="mk__tag" x={W} y={inside.y - 9} width={AXIS} height={18} rx={3} />
                  <text className="mk__tick mk__tick--tag" x={W + 8} y={inside.y + 4}>{fmt(P(inside.y))}</text>
                </>
              )}
            </g>
          )}
        </svg>
      )}
      {inside && inside.y <= mainH && tool === 'cursor' && !drag.current && onAlert && (
        <button type="button" className="mk__plus" style={{ left: W - 22, top: inside.y - 10 }} aria-label={`Add an alert at ${fmt(P(inside.y))}`} onClick={() => onAlert(P(inside.y))}><Icon icon={Plus} size="sm" /></button>
      )}
      <div className="mk__legend">
        <p className="d-mono mk__ohlc">O {fmt(cur.o)}  H {fmt(cur.h)}  L {fmt(cur.l)}  C {fmt(cur.c)}  <span className="mk__change" data-up={cur.c >= cur.o ? '' : undefined}>{cur.c >= cur.o ? '+' : '−'}{Math.abs(((cur.c - cur.o) / cur.o) * 100).toFixed(2)}%</span></p>
        {listed.map((p) => <LegendRow key={p.key} item={p} value={p.plot.read(at)} split={split} onHide={() => onHide(p.key)} onRemove={() => onRemove(p.key)} onLocal={() => onLocal(p.key)} />)}
      </div>
      {panes.map((p, k) => (
        <div key={p.key} className="mk__legend" style={{ top: mainH + k * paneH + 2 }}>
          <LegendRow item={p} value={p.plot.read(at)} split={split} onHide={() => onHide(p.key)} onRemove={() => onRemove(p.key)} onLocal={() => onLocal(p.key)} />
        </div>
      ))}
    </div>
  )
}
