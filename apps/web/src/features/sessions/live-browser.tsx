/* A session's browser, live (DECISIONS Z12 to Z16): Demido's own headless browser, drawn here as it draws the
   page, and used from here as a browser is: tabs, back, forward, reload, an address, the mouse and the keys. The
   model's tools act on the tab in front, so what it does is seen as it happens.

   Marks: point at something, draw round it, box it, or leave a note; each is described by the page itself (what
   it is, its words, where it leads, a selector to find it again) and sent to the message box as words, for the
   model, which reads text and not pictures. */
import { useEffect, useRef, useState } from 'react'
import { ArrowLeft, ArrowRight, Copy, Download, ExternalLink, Globe, Maximize2, MessageSquarePlus, Minimize2, MousePointer2, Pencil, Plus, RotateCw, Square, SquareDashedMousePointer, Trash2, Type, X } from 'lucide-react'
import { coreGet, coreSend } from '@/data/core'
import { toast } from '@/data/store'
import { Button, ContextMenu, Glyph, Icon, Tip, cx } from '@/ui'
import { useInsert } from './draft'

interface Meta { deviceWidth: number; deviceHeight: number; offsetTop: number; pageScaleFactor: number; scrollOffsetX: number; scrollOffsetY: number }
interface TabFacts { id: string; address: string; title: string }
interface Described { tag: string; role: string | null; label: string | null; text: string; href: string | null; selector: string; box: [number, number, number, number] }
type Tool = 'browse' | 'pick' | 'pen' | 'box' | 'note'
/** A mark, where it is on the page (scroll included), and what the page says is there. */
interface Mark { n: number; kind: Exclude<Tool, 'browse'>; box: [number, number, number, number]; path?: [number, number][]; note?: string; element: Described | null }

const host = (address: string) => { try { return new URL(address).host } catch { return address } }
const blank = (address: string) => !address || address === 'about:blank'
const modifiers = (e: { altKey: boolean; ctrlKey: boolean; metaKey: boolean; shiftKey: boolean }) => (e.altKey ? 1 : 0) | (e.ctrlKey ? 2 : 0) | (e.metaKey ? 4 : 0) | (e.shiftKey ? 8 : 0)

const NAMES: Record<string, string> = {
  p: 'paragraph', a: 'link', img: 'picture', svg: 'picture', picture: 'picture', video: 'video', canvas: 'drawing', button: 'button',
  input: 'field', textarea: 'field', select: 'choice', label: 'label', form: 'form', li: 'list item', ul: 'list', ol: 'list',
  table: 'table', tr: 'table row', td: 'table cell', th: 'table heading', figure: 'figure', nav: 'menu', header: 'top of the page',
  footer: 'foot of the page', article: 'article', main: 'main part', aside: 'side part', code: 'code', pre: 'code', span: 'text',
}

/** An element, in a few words a model can find it by. */
function words(el: Described | null): string {
  if (!el) return 'nothing there'
  const what = el.role && el.role !== 'generic' ? el.role : /^h[1-6]$/.test(el.tag) ? 'heading' : NAMES[el.tag] ?? 'part of the page'
  const said = el.label || el.text
  return `the ${what}${said ? ` "${said.length > 120 ? `${said.slice(0, 120)}…` : said}"` : ''}${el.href ? ` → ${el.href}` : ''} (\`${el.selector}\`)`
}

function asText(marks: Mark[], tab: TabFacts | undefined): string {
  const lead = { pick: 'Pointed at', pen: 'Drawn round', box: 'Boxed', note: 'A note' } as const
  const lines = marks.map((m) => `${m.n}. ${m.kind === 'note' ? `A note, "${m.note ?? ''}", on` : lead[m.kind]} ${words(m.element)}`)
  return `On ${tab?.title ? `"${tab.title}"` : 'the page'} (${tab?.address ?? ''}), I marked:\n${lines.join('\n')}`
}

const TOOLS: { tool: Tool; icon: typeof Pencil; label: string }[] = [
  { tool: 'browse', icon: MousePointer2, label: 'Use the page' },
  { tool: 'pick', icon: SquareDashedMousePointer, label: 'Point at something' },
  { tool: 'pen', icon: Pencil, label: 'Draw round it' },
  { tool: 'box', icon: Square, label: 'Box it' },
  { tool: 'note', icon: Type, label: 'A note' },
]

export function LiveBrowser({ session, wide, onWide, onClose, leaving }: { session: string; wide: boolean; onWide: () => void; onClose: () => void; leaving?: boolean }) {
  const socket = useRef<WebSocket | null>(null)
  const canvas = useRef<HTMLCanvasElement>(null)
  const view = useRef<HTMLDivElement>(null)
  const addressBox = useRef<HTMLInputElement>(null)
  const meta = useRef<Meta | null>(null)
  const [scroll, setScroll] = useState<[number, number]>([0, 0])
  const [state, setState] = useState<'connecting' | 'open' | 'closed' | 'none'>('connecting')
  const [problem, setProblem] = useState<string | null>(null)
  const [tabs, setTabs] = useState<TabFacts[]>([])
  const [front, setFront] = useState<string | null>(null)
  const [address, setAddress] = useState('')
  const [editing, setEditing] = useState(false)
  const [password, setPassword] = useState(false)
  const [pageCursor, setPageCursor] = useState('default')
  const [tool, setTool] = useState<Tool>('browse')
  const [marks, setMarks] = useState<Mark[]>([])
  const [hover, setHover] = useState<[number, number, number, number] | null>(null)
  const [drawing, setDrawing] = useState<{ from: [number, number]; to: [number, number]; path: [number, number][] } | null>(null)
  const [noting, setNoting] = useState<{ at: [number, number]; element: Described | null } | null>(null)
  const [round, setRound] = useState(0)
  const asks = useRef(new Map<number, (d: Described | null) => void>())
  const nextAsk = useRef(1)
  const put = useInsert((s) => s.put)
  const tab = tabs.find((t) => t.id === front)

  const send = (message: object) => { if (socket.current?.readyState === WebSocket.OPEN) socket.current.send(JSON.stringify(message)) }
  const describe = (x: number, y: number, w = 0, h = 0, ask?: number) => new Promise<Described | null>((done) => {
    const id = ask ?? nextAsk.current++
    asks.current.set(id, done)
    send({ type: 'describe', x, y, w, h, ask: id })
    setTimeout(() => { if (asks.current.delete(id)) done(null) }, 4000)
  })

  // The socket: the page's pictures, the tabs, and what the browser says.
  useEffect(() => {
    setState('connecting')
    setProblem(null)
    const ws = new WebSocket(`${location.protocol === 'https:' ? 'wss' : 'ws'}://${location.host}/api/v1/sessions/${encodeURIComponent(session)}/browser/live`)
    socket.current = ws
    ws.onopen = () => { setState('open'); sizeNow() }
    ws.onclose = () => setState((s) => (s === 'none' ? 'none' : 'closed'))
    ws.onmessage = async (event) => {
      const said = JSON.parse(event.data as string)
      if (said.type === 'frame') {
        meta.current = said.meta
        setScroll([said.meta.scrollOffsetX ?? 0, said.meta.scrollOffsetY ?? 0])
        const bytes = Uint8Array.from(atob(said.data), (c) => c.charCodeAt(0))
        const picture = await createImageBitmap(new Blob([bytes], { type: 'image/jpeg' }))
        const c = canvas.current
        if (!c) return
        if (c.width !== picture.width || c.height !== picture.height) { c.width = picture.width; c.height = picture.height }
        c.getContext('2d')?.drawImage(picture, 0, 0)
        picture.close()
      } else if (said.type === 'tabs') {
        setTabs(said.tabs)
        setFront(said.front)
      } else if (said.type === 'problem') {
        if (/not downloaded/i.test(said.message)) setState('none')
        setProblem(said.message)
      } else if (said.type === 'cursor') {
        setPageCursor(String(said.cursor))
      } else if (said.type === 'focus') {
        setPassword(Boolean(said.password))
      } else if (said.type === 'dialog') {
        toast(`The page said: ${said.message}`)
      } else if (said.type === 'described') {
        const done = asks.current.get(said.ask)
        if (done) { asks.current.delete(said.ask); done(said.element) }
      }
    }
    return () => { ws.close(); socket.current = null }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [session, round])

  // Watching is a word too: the core lets a quiet view go after two minutes, and a view shown again comes back.
  useEffect(() => {
    const here = setInterval(() => { if (document.visibilityState === 'visible') send({ type: 'here' }) }, 30_000)
    const back = () => { if (document.visibilityState === 'visible' && socket.current?.readyState !== WebSocket.OPEN) setRound((r) => r + 1) }
    document.addEventListener('visibilitychange', back)
    return () => { clearInterval(here); document.removeEventListener('visibilitychange', back) }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  // The page is laid out at the size it is shown at.
  const sizeNow = () => {
    const el = view.current
    if (!el) return
    const r = el.getBoundingClientRect()
    send({ type: 'size', width: Math.round(r.width), height: Math.round(r.height), scale: Math.min(window.devicePixelRatio || 1, 2) })
  }
  useEffect(() => {
    const el = view.current
    if (!el) return
    let wait: ReturnType<typeof setTimeout> | undefined
    const seen = new ResizeObserver(() => { clearTimeout(wait); wait = setTimeout(sizeNow, 150) })
    seen.observe(el)
    return () => { seen.disconnect(); clearTimeout(wait) }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])
  useEffect(() => { if (!editing) setAddress(tab && !blank(tab.address) ? tab.address : '') }, [tab?.address, editing])
  // Marks are of one page: another page, or another tab, starts with none.
  useEffect(() => { setMarks([]); setHover(null) }, [tab?.address, front])

  /** A point on the view, in the page's own pixels. */
  const at = (e: { clientX: number; clientY: number }): [number, number] => {
    const r = canvas.current!.getBoundingClientRect()
    const m = meta.current
    return [((e.clientX - r.left) / r.width) * (m?.deviceWidth ?? r.width), ((e.clientY - r.top) / r.height) * (m?.deviceHeight ?? r.height)]
  }
  const mouse = (event: string, e: React.MouseEvent | React.WheelEvent, extra: object = {}) => {
    const [x, y] = at(e)
    send({ type: 'mouse', event, x, y, button: ['left', 'middle', 'right'][(e as React.MouseEvent).button] ?? 'none', buttons: (e as React.MouseEvent).buttons, clickCount: (e as React.MouseEvent).detail || 1, modifiers: modifiers(e), ...extra })
  }
  const moving = useRef(false)
  const lastHover = useRef(0)
  const add = (m: Omit<Mark, 'n'>) => setMarks((all) => [...all, { ...m, n: (all.at(-1)?.n ?? 0) + 1 }])
  const toPage = ([x, y]: [number, number]): [number, number] => [x + scroll[0], y + scroll[1]]

  const down = (e: React.MouseEvent) => {
    view.current?.focus()
    if (tool === 'browse') return mouse('mousePressed', e)
    if (tool === 'pen' || tool === 'box') { const p = at(e); setDrawing({ from: p, to: p, path: [p] }) }
  }
  const move = (e: React.MouseEvent) => {
    if (tool === 'browse') {
      if (moving.current) return
      moving.current = true
      requestAnimationFrame(() => { moving.current = false; mouse('mouseMoved', e, { button: e.buttons ? 'left' : 'none' }) })
      return
    }
    if (drawing) { const p = at(e); setDrawing({ ...drawing, to: p, path: [...drawing.path, p] }); return }
    if (tool === 'pick' || tool === 'note') {
      const now = Date.now()
      if (now - lastHover.current < 120) return
      lastHover.current = now
      const [x, y] = at(e)
      void describe(x, y).then((d) => setHover(d ? d.box : null))
    }
  }
  const up = async (e: React.MouseEvent) => {
    if (tool === 'browse') return mouse('mouseReleased', e)
    const [x, y] = at(e)
    if (tool === 'pick') { const d = await describe(x, y); if (d) add({ kind: 'pick', box: [d.box[0] + scroll[0], d.box[1] + scroll[1], d.box[2], d.box[3]], element: d }) }
    if (tool === 'note') { const d = await describe(x, y); setNoting({ at: toPage([x, y]), element: d }) }
    if ((tool === 'pen' || tool === 'box') && drawing) {
      const xs = drawing.path.map((p) => p[0]).concat(drawing.to[0]); const ys = drawing.path.map((p) => p[1]).concat(drawing.to[1])
      const box: [number, number, number, number] = [Math.min(...xs), Math.min(...ys), Math.max(...xs) - Math.min(...xs), Math.max(...ys) - Math.min(...ys)]
      setDrawing(null)
      if (box[2] < 4 && box[3] < 4) return
      const d = await describe(box[0], box[1], box[2], box[3])
      add({ kind: tool, box: [box[0] + scroll[0], box[1] + scroll[1], box[2], box[3]], path: tool === 'pen' ? drawing.path.map(toPage) : undefined, element: d })
    }
  }
  const key = (e: React.KeyboardEvent, kind: 'down' | 'up') => {
    if (tool !== 'browse') return
    const paste = (e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'v'
    if (paste) return
    e.preventDefault()
    const base = { type: 'key', key: e.key, code: e.code, keyCode: e.keyCode, modifiers: modifiers(e) }
    if (kind === 'up') return send({ ...base, event: 'keyUp' })
    const printable = e.key.length === 1 && !e.ctrlKey && !e.metaKey
    send(printable ? { ...base, event: 'keyDown', text: e.key } : e.key === 'Enter' ? { ...base, event: 'keyDown', text: '\r' } : { ...base, event: 'rawKeyDown' })
  }

  const go = (to: string) => { if (to.trim()) { send({ type: 'go', address: to.trim() }); setEditing(false); view.current?.focus() } }
  const shown = (b: [number, number, number, number]) => [b[0] - scroll[0], b[1] - scroll[1], b[2], b[3]] as const
  const vw = meta.current?.deviceWidth ?? 1280, vh = meta.current?.deviceHeight ?? 800
  const download = async () => {
    try { await coreSend('POST', '/browser'); toast("Demido's browser is downloaded.", undefined, 'task-done'); setRound((r) => r + 1) } catch (e) { toast(`Not downloaded. ${(e as Error).message}`, undefined, 'task-failed') }
  }
  const [offered, setOffered] = useState(true)
  useEffect(() => { if (state === 'none') coreGet<{ offered: boolean }>('/browser').then((s) => setOffered(s.offered), () => {}) }, [state])

  return (
    <aside className="panel lb" aria-label="Browser" data-leaving={leaving ? '' : undefined}>
      <header className="lb__tabs">
        <div className="lb__tablist" role="tablist" aria-label="Tabs">
          {tabs.map((t) => (
            <ContextMenu key={t.id} items={[
              { label: 'New tab', lead: <Icon icon={Plus} size="sm" />, onSelect: () => { send({ type: 'newTab' }); setAddress(''); addressBox.current?.focus() } },
              { label: 'Reload', lead: <Icon icon={RotateCw} size="sm" />, onSelect: () => { if (t.id !== front) send({ type: 'activate', id: t.id }); send({ type: 'reload' }) } },
              { kind: 'sep' },
              ...(!blank(t.address) ? [
                { label: 'Copy the address', lead: <Icon icon={Copy} size="sm" />, onSelect: () => void navigator.clipboard.writeText(t.address).then(() => toast('Copied.')) },
                { label: 'In your own browser', lead: <Icon icon={ExternalLink} size="sm" />, onSelect: () => window.open(t.address, '_blank', 'noopener,noreferrer') },
                { kind: 'sep' as const },
              ] : []),
              { label: 'Close the others', disabled: tabs.length < 2, onSelect: () => tabs.filter((o) => o.id !== t.id).forEach((o) => send({ type: 'closeTab', id: o.id })) },
              { label: 'Close', lead: <Icon icon={X} size="sm" />, onSelect: () => send({ type: 'closeTab', id: t.id }) },
            ]}>
            <span className="lb__tab" role="tab" aria-selected={t.id === front} onAuxClick={(e) => { if (e.button === 1) send({ type: 'closeTab', id: t.id }) }}>
              <button type="button" className="lb__tab-name" onClick={() => send({ type: 'activate', id: t.id })} title={t.address}>
                <Icon icon={Globe} size="sm" /><span>{blank(t.address) ? 'New tab' : t.title || host(t.address)}</span>
              </button>
              <button type="button" className="lb__tab-close" aria-label={`Close ${t.title || 'the tab'}`} onClick={() => send({ type: 'closeTab', id: t.id })}><Icon icon={X} size="sm" /></button>
            </span>
            </ContextMenu>
          ))}
          <Tip label="New tab"><button type="button" className="lb__tab-new" aria-label="New tab" onClick={() => { send({ type: 'newTab' }); setAddress(''); addressBox.current?.focus() }}><Icon icon={Plus} size="sm" /></button></Tip>
        </div>
        <Tip label={wide ? 'Beside the conversation' : 'Whole view'}><Button size="sm" variant="ghost" iconOnly aria-label={wide ? 'Beside the conversation' : 'Whole view'} onClick={onWide}><Icon icon={wide ? Minimize2 : Maximize2} size="sm" /></Button></Tip>
        <Tip label="Close"><Button size="sm" variant="ghost" iconOnly aria-label="Close" onClick={onClose}><Icon icon={X} size="sm" /></Button></Tip>
      </header>
      <div className="lb__bar">
        <Tip label="Back"><Button size="sm" variant="ghost" iconOnly aria-label="Back" onClick={() => send({ type: 'back' })}><Icon icon={ArrowLeft} size="sm" /></Button></Tip>
        <Tip label="Forward"><Button size="sm" variant="ghost" iconOnly aria-label="Forward" onClick={() => send({ type: 'forward' })}><Icon icon={ArrowRight} size="sm" /></Button></Tip>
        <Tip label="Reload"><Button size="sm" variant="ghost" iconOnly aria-label="Reload" onClick={() => send({ type: 'reload' })}><Icon icon={RotateCw} size="sm" /></Button></Tip>
        <form className="lb__address" onSubmit={(e) => { e.preventDefault(); go(address) }}>
          <input ref={addressBox} className="d-input" aria-label="Address" placeholder="Search or type an address" value={address} spellCheck={false}
            onFocus={(e) => { setEditing(true); e.currentTarget.select() }} onBlur={() => setEditing(false)} onChange={(e) => setAddress(e.target.value)}
            onKeyDown={(e) => { if (e.key === 'Escape') { setEditing(false); e.currentTarget.blur() } }} />
        </form>
        {tab && !blank(tab.address) && <Tip label="In your own browser"><a className="d-button d-button--ghost d-button--sm d-button--icon" aria-label="In your own browser" href={tab.address} target="_blank" rel="noopener noreferrer"><Icon icon={ExternalLink} size="sm" /></a></Tip>}
      </div>
      {password && <p className="lb__note">What you sign in to here, the model can use in this session.</p>}
      <div className="lb__view" ref={view} tabIndex={0} data-tool={tool} style={tool === 'browse' && state === 'open' ? { cursor: pageCursor } : undefined}
        onMouseDown={down} onMouseMove={move} onMouseUp={(e) => void up(e)} onMouseLeave={() => setHover(null)}
        onWheel={(e) => { if (tool === 'browse') mouse('mouseWheel', e, { deltaX: e.deltaX, deltaY: e.deltaY, button: 'none' }) }}
        onContextMenu={(e) => e.preventDefault()} onKeyDown={(e) => key(e, 'down')} onKeyUp={(e) => key(e, 'up')}
        onPaste={(e) => { const text = e.clipboardData.getData('text'); if (text) { e.preventDefault(); send({ type: 'text', text }) } }}>
        <canvas ref={canvas} className="lb__page" aria-label={tab?.title || 'The page'} />
        <svg className="lb__marks" viewBox={`0 0 ${vw} ${vh}`} preserveAspectRatio="none" aria-hidden="true">
          {hover && tool !== 'browse' && <rect className="lb__hover" x={hover[0]} y={hover[1]} width={hover[2]} height={hover[3]} />}
          {marks.map((m) => {
            const [x, y, w, h] = shown(m.box)
            return (
              <g key={m.n} className="lb__mark" data-kind={m.kind}>
                {m.kind === 'pen' && m.path ? <polyline points={m.path.map(([px, py]) => `${px - scroll[0]},${py - scroll[1]}`).join(' ')} />
                  : m.kind === 'note' ? <circle cx={x} cy={y} r={6} />
                  : <rect x={x} y={y} width={w} height={h} rx={m.kind === 'pick' ? 3 : 2} />}
                <circle className="lb__badge" cx={m.kind === 'note' ? x + 12 : x} cy={m.kind === 'note' ? y - 12 : y} r={10} />
                <text x={m.kind === 'note' ? x + 12 : x} y={(m.kind === 'note' ? y - 12 : y) + 4}>{m.n}</text>
              </g>
            )
          })}
          {drawing && (tool === 'pen'
            ? <polyline className="lb__drawing" points={drawing.path.map(([px, py]) => `${px},${py}`).join(' ')} />
            : <rect className="lb__drawing" x={Math.min(drawing.from[0], drawing.to[0])} y={Math.min(drawing.from[1], drawing.to[1])} width={Math.abs(drawing.to[0] - drawing.from[0])} height={Math.abs(drawing.to[1] - drawing.from[1])} />)}
        </svg>
        {noting && (
          <form className="lb__noting" style={{ left: `${((noting.at[0] - scroll[0]) / vw) * 100}%`, top: `${((noting.at[1] - scroll[1]) / vh) * 100}%` }}
            onSubmit={(e) => { e.preventDefault(); const text = (new FormData(e.currentTarget).get('note') as string).trim(); if (text) add({ kind: 'note', box: [noting.at[0], noting.at[1], 0, 0], note: text, element: noting.element }); setNoting(null) }}>
            <input name="note" className="d-input" autoFocus placeholder="A note" aria-label="A note" onKeyDown={(e) => { e.stopPropagation(); if (e.key === 'Escape') setNoting(null) }} onKeyUp={(e) => e.stopPropagation()} />
          </form>
        )}
        {state !== 'open' && (
          <div className="panel__empty lb__cover">
            <span className="panel__empty-art"><Icon icon={Globe} size="xl" /></span>
            {state === 'connecting' ? <p><Glyph name="task-running" busy /></p>
              : state === 'none' ? (offered ? <><p>No browser yet.</p><Button size="sm" onClick={() => void download()}><Icon icon={Download} size="sm" />Download (about 130 MB)</Button></> : <p>Not on this system yet.</p>)
              : <><p>{problem ?? 'The browser stopped.'}</p><Button size="sm" onClick={() => setRound((r) => r + 1)}>Open again</Button></>}
          </div>
        )}
        {state === 'open' && tab && blank(tab.address) && !editing && <div className="lb__blank"><Icon icon={Globe} size="xl" /><p>Type an address above.</p></div>}
      </div>
      {state === 'open' && (
        <div className="lb__tools" role="toolbar" aria-label="Marks">
          {TOOLS.map((t) => (
            <Tip key={t.tool} label={t.label}><button type="button" className={cx('lb__tool', tool === t.tool && 'is-on')} aria-pressed={tool === t.tool} aria-label={t.label} onClick={() => { setTool(t.tool); setHover(null); setNoting(null) }}><Icon icon={t.icon} size="sm" /></button></Tip>
          ))}
          <span className="lb__tools-sep" />
          <Tip label="Into the message box, as words"><button type="button" className="lb__tool lb__tool--send" disabled={marks.length === 0} aria-label="Put the marks into the message box" onClick={() => { put(session, asText(marks, tab)); toast(`${marks.length} ${marks.length === 1 ? 'mark' : 'marks'} in the message box.`) }}><Icon icon={MessageSquarePlus} size="sm" /><span>{marks.length}</span></button></Tip>
          <Tip label="Clear the marks"><button type="button" className="lb__tool" disabled={marks.length === 0} aria-label="Clear the marks" onClick={() => setMarks([])}><Icon icon={Trash2} size="sm" /></button></Tip>
        </div>
      )}
      {problem && state === 'open' && <p className="lb__problem" role="status" onAnimationEnd={() => setProblem(null)}>{problem}</p>}
    </aside>
  )
}
