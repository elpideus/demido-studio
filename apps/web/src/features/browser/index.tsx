/* The built-in browser. A drawn page in the preview, not a real browser.

   Added on the owner's direction (2026-11-20):
   - Annotation tools. You mark the page (point at an element, draw, highlight, box, arrow) and send the
     marked page to a session, so "this one, here" needs no words. The marks really draw in the preview.
   - An ad blocker, built in and on by default. Models know it exists and may loosen it for one site when a
     page breaks because of it. Every change is listed with who made it and why, and can be undone. */
import { useRef, useState } from 'react'
import type { PointerEvent as ReactPointerEvent } from 'react'
import { Popover } from '@base-ui/react/popover'
import { AppWindow, ArrowLeft, ArrowRight, ArrowUpRight, Eraser, Highlighter, MapPin, PenLine, Pencil, Plus, RotateCw, Send, ShieldCheck, ShieldOff, Square, Trash2, Undo2, X } from 'lucide-react'
import type { LucideIcon } from 'lucide-react'
import type { Feature } from '@/app/registry'
import { setSetting, toast, useSetting, useStore } from '@/data/store'
import { Button, Facts, Glyph, Icon, Menu, NotBuilt, Segmented, Switch, Tip, cx } from '@/ui'

const SITE = 'shop.example'
const NETWORK = [['GET', '/merino-crew-navy', '200', '18 KB'], ['GET', '/assets/app.js', '200', '212 KB'], ['GET', '/api/price?sku=4471', '200', '0.2 KB'], ['GET', 'pricewidget.example/embed.js', 'allowed for this site', '9 KB'], ['GET', 'ads.adnet.example/banner.js', 'blocked', ''], ['GET', 'metrics.tracker.example/t.gif', 'blocked', ''], ['GET', '/track.gif', 'blocked', '']]
const CONSOLE = [['log', 'price module ready'], ['warn', 'Deprecated API: sizeChart v1'], ['log', 'price rendered: 89.00']]
const BLOCKED = [['ads.adnet.example', 'Ads', 6], ['metrics.tracker.example', 'Trackers', 5], ['shop.example/track.gif', 'Trackers', 2], ['consent.cdn.example', 'Cookie banners', 1]] as const
const LISTS = ['EasyList', 'EasyPrivacy', 'uBlock filters', "Peter Lowe's list"]
/** What a model is told about the blocker. Written for models: short on purpose. */
const BLOCKER_GUIDE = `Browser has ad blocker. On by default.
Page broken, blank part, button dead -> read blocker log first.
blocker.status(site)             what was blocked
blocker.allow(domain, site, why) let one domain load on one site
blocker.pause(site, minutes, why)
blocker.add_rule(rule, why)
Use smallest change that fixes page. Say what you changed.
User sees every change. User can undo.
Never switch blocker off for all sites.`

interface Rule { id: number; what: string; by: 'model' | 'you'; why: string; when: string }
const RULES: Rule[] = [{ id: 1, what: `Allow pricewidget.example on ${SITE}`, by: 'model', why: 'The price did not load: its script was blocked as a tracker.', when: 'Today 11:02' }]

const ACTIONS = [
  { what: 'Opened', code: `${SITE}/merino-crew-navy`, meta: '1.2 s' },
  { what: 'Waited for', code: '.price', meta: 'timed out · 8 s', failed: true },
  { what: 'Read the blocker log: blocked', code: 'pricewidget.example', meta: '0.1 s' },
  { what: 'Allowed on this site:', code: 'pricewidget.example', meta: 'you can undo' },
  { what: 'Reloaded, read the price:', code: 'EUR 89.00', meta: '2.1 s' },
  { what: 'Clicked', code: 'button "Size M"', meta: '0.2 s' },
]

// ───────── annotations ─────────
type Tool = 'point' | 'pen' | 'mark' | 'box' | 'arrow' | 'erase'
type XY = [number, number]
interface Mark { id: number; type: Exclude<Tool, 'erase'>; color: string; pts: XY[]; n?: number; text?: string; target?: string }
const TOOLS: { value: Tool; label: string; icon: LucideIcon }[] = [
  { value: 'point', label: 'Point at something and add a note', icon: MapPin }, { value: 'pen', label: 'Draw', icon: PenLine }, { value: 'mark', label: 'Highlight', icon: Highlighter },
  { value: 'box', label: 'Box', icon: Square }, { value: 'arrow', label: 'Arrow', icon: ArrowUpRight }, { value: 'erase', label: 'Erase: click a mark', icon: Eraser },
]
const COLOURS = [['Red', 'var(--d-color-danger-base)'], ['Yellow', 'var(--d-color-caution-base)'], ['Blue', 'var(--d-color-info-base)']] as const
const path = (pts: XY[]) => pts.map(([x, y], i) => `${i ? 'L' : 'M'}${x.toFixed(1)} ${y.toFixed(1)}`).join('')

function MarkShape({ m, erase, onErase }: { m: Mark; erase: boolean; onErase: () => void }) {
  const [a, b] = [m.pts[0], m.pts[m.pts.length - 1]]
  const hit = { onPointerDown: (e: ReactPointerEvent) => { if (erase) { e.stopPropagation(); onErase() } }, style: { color: m.color, cursor: erase ? 'pointer' : undefined } }
  if (m.type === 'point')
    return <g {...hit} className="browser__pin"><circle cx={a[0]} cy={a[1]} r={12} /><text x={a[0]} y={a[1] + 4} textAnchor="middle">{m.n}</text></g>
  if (m.type === 'pen' || m.type === 'mark')
    return <path {...hit} className={m.type === 'mark' ? 'browser__hl' : 'browser__ink'} d={path(m.pts)} />
  if (m.type === 'box')
    return <rect {...hit} className="browser__ink" x={Math.min(a[0], b[0])} y={Math.min(a[1], b[1])} width={Math.abs(b[0] - a[0])} height={Math.abs(b[1] - a[1])} rx={4} />
  const ang = Math.atan2(b[1] - a[1], b[0] - a[0])
  const head = [b, [b[0] - 14 * Math.cos(ang - 0.42), b[1] - 14 * Math.sin(ang - 0.42)], [b[0] - 14 * Math.cos(ang + 0.42), b[1] - 14 * Math.sin(ang + 0.42)]] as XY[]
  return <g {...hit}><line className="browser__ink" x1={a[0]} y1={a[1]} x2={b[0]} y2={b[1]} /><polygon className="browser__head" points={head.map((p) => p.join(',')).join(' ')} /></g>
}

function BrowserPage() {
  const sessions = useStore((s) => s.sessions)
  const [tab, setTab] = useState(0)
  const [panel, setPanel] = useState<'actions' | 'marks' | 'blocker' | 'network' | 'console'>('actions')
  const [driver, setDriver] = useState<'model' | 'you'>('model')
  const [step, setStep] = useState(ACTIONS.length)
  const [size, setSize] = useState('M')
  // the blocker
  const blockerOn = useSetting('browser.adblock', true)
  const [paused, setPaused] = useState(false)
  const [rules, setRules] = useState<Rule[]>(RULES)
  const blocking = blockerOn && !paused
  const total = BLOCKED.reduce((n, b) => n + b[2], 0)
  // the marks
  const [annotate, setAnnotate] = useState(false)
  const [tool, setTool] = useState<Tool>('point')
  const [colour, setColour] = useState<string>(COLOURS[0][1])
  const [marks, setMarks] = useState<Mark[]>([])
  const [draft, setDraft] = useState<Mark | null>(null)
  const layer = useRef<SVGSVGElement>(null)
  const notes = marks.filter((m) => m.type === 'point')

  const replay = () => {
    setStep(0)
    ACTIONS.forEach((_, i) => setTimeout(() => setStep(i + 1), 700 * (i + 1)))
  }
  const at = (e: ReactPointerEvent): XY => { const r = layer.current!.getBoundingClientRect(); return [e.clientX - r.left, e.clientY - r.top] }
  const down = (e: ReactPointerEvent<SVGSVGElement>) => {
    if (tool === 'erase') return
    const p = at(e)
    if (tool === 'point') {
      // What is under the pin? The model gets that too, not only the picture.
      const el = document.elementsFromPoint(e.clientX, e.clientY).find((x) => x.closest('.browser__shop') && !x.closest('svg') && x.textContent?.trim() && x.children.length === 0)
      const target = el ? `${el.tagName === 'BUTTON' ? 'button' : el.tagName === 'H2' ? 'heading' : 'text'} "${el.textContent!.trim().slice(0, 40)}"` : 'an empty part of the page'
      setMarks([...marks, { id: Date.now(), type: 'point', color: colour, pts: [p], n: notes.length + 1, text: '', target }])
      setPanel('marks')
      return
    }
    try { e.currentTarget.setPointerCapture(e.pointerId) } catch { /* a pointer that is already gone */ }
    setDraft({ id: Date.now(), type: tool, color: colour, pts: [p, p] })
  }
  const move = (e: ReactPointerEvent<SVGSVGElement>) => {
    if (!draft) return
    const p = at(e)
    setDraft({ ...draft, pts: draft.type === 'pen' || draft.type === 'mark' ? [...draft.pts, p] : [draft.pts[0], p] })
  }
  const up = () => {
    if (!draft) return
    const [a, b] = [draft.pts[0], draft.pts[draft.pts.length - 1]]
    if (draft.pts.length > 2 || Math.hypot(b[0] - a[0], b[1] - a[1]) > 6) setMarks([...marks, draft])
    setDraft(null)
  }
  const renumber = (list: Mark[]) => { let n = 0; return list.map((m) => (m.type === 'point' ? { ...m, n: ++n } : m)) }
  const send = (title: string) => toast(`Would send to "${title}": the page as a picture with your ${marks.length} ${marks.length === 1 ? 'mark' : 'marks'}, ${notes.length} ${notes.length === 1 ? 'note' : 'notes'} as text, and what each pin points at. Not built in the preview.`)

  const tabs = [`Merino crew neck, navy · ${SITE}`, 'New tab']
  return (
    <div className="browser" data-annotating={annotate ? '' : undefined}>
      <div className="browser__tabs" role="tablist" aria-label="Browser tabs">
        {tabs.map((t, i) => (
          <button key={t} type="button" role="tab" aria-selected={tab === i} className="browser__tab" onClick={() => setTab(i)}>
            <span>{t}</span>
            <Icon icon={X} size="sm" />
          </button>
        ))}
        <Tip label="New tab"><Button variant="ghost" size="sm" iconOnly aria-label="New tab" onClick={() => toast('New tabs: not built in the preview.')}><Icon icon={Plus} /></Button></Tip>
        <span className="app-spacer" />
        <NotBuilt>Preview: a drawn page, not a real browser</NotBuilt>
      </div>
      <div className="browser__bar">
        <Button variant="ghost" iconOnly aria-label="Back" disabled><Icon icon={ArrowLeft} /></Button>
        <Button variant="ghost" iconOnly aria-label="Forward" disabled><Icon icon={ArrowRight} /></Button>
        <Button variant="ghost" iconOnly aria-label="Reload" onClick={replay}><Icon icon={RotateCw} /></Button>
        <input className="d-input d-input--mono" aria-label="Address" readOnly value={tab === 0 ? `https://${SITE}/merino-crew-navy` : 'about:blank'} />
        <Popover.Root>
          <Tip label={blocking ? `The ad blocker stopped ${total} things on this page` : 'The ad blocker is off here'} side="bottom">
            <Popover.Trigger render={<Button variant="ghost" size="sm" aria-label="Ad blocker" />}>
              <Icon icon={blocking ? ShieldCheck : ShieldOff} size="sm" /><span className="d-mono">{blocking ? total : 'off'}</span>
            </Popover.Trigger>
          </Tip>
          <Popover.Portal>
            <Popover.Positioner side="bottom" align="end" sideOffset={8} className="app-layer">
              <Popover.Popup className="d-menu app-pop browser__shield">
                <Popover.Title className="d-type-headline">Ad blocker</Popover.Title>
                <label className="d-check"><Switch label={`Block on ${SITE}`} checked={blocking} disabled={!blockerOn} onChange={(v) => { setPaused(!v); if (!v) setRules([...rules, { id: Date.now(), what: `Paused on ${SITE}`, by: 'you', why: 'Switched off from the address bar.', when: 'Just now' }]) }} /><span className="d-check__text">Block ads and trackers on {SITE}<small>{blockerOn ? (blocking ? `${total} stopped on this page` : 'Paused for this site') : 'The blocker is off for every site, in Settings'}</small></span></label>
                {blocking && <Facts rows={(['Ads', 'Trackers', 'Cookie banners'] as const).map((k) => [k, String(BLOCKED.filter((b) => b[1] === k).reduce((n, b) => n + b[2], 0))])} />}
                <p className="d-type-caption d-text-secondary">Lists: {LISTS.join(', ')}. A model may loosen the blocker for one site when a page breaks; every change is listed.</p>
                <Button size="sm" onClick={() => setPanel('blocker')}>See what was blocked and changed</Button>
              </Popover.Popup>
            </Popover.Positioner>
          </Popover.Portal>
        </Popover.Root>
        <Tip label={annotate ? 'Stop marking the page' : 'Mark the page'} side="bottom"><Button variant="ghost" size="sm" pressed={annotate} onClick={() => setAnnotate(!annotate)}><Icon icon={Pencil} size="sm" />Annotate{marks.length > 0 && <span className="d-mono">{marks.length}</span>}</Button></Tip>
        <Segmented small label="Who is driving" value={driver} onChange={setDriver} options={[{ value: 'model', label: 'Model drives' }, { value: 'you', label: 'You drive' }]} />
      </div>
      {annotate && (
        <div className="browser__bar browser__marks" role="toolbar" aria-label="Annotation tools">
          {TOOLS.map((t) => <Tip key={t.value} label={t.label} side="bottom"><Button variant="ghost" size="sm" iconOnly pressed={tool === t.value} aria-label={t.label} onClick={() => setTool(t.value)}><Icon icon={t.icon} /></Button></Tip>)}
          <span className="browser__sep" />
          {COLOURS.map(([name, c]) => <button key={c} type="button" className="browser__swatch" aria-label={name} aria-pressed={colour === c} style={{ color: c }} onClick={() => setColour(c)} />)}
          <span className="browser__sep" />
          <Tip label="Undo the last mark" side="bottom"><Button variant="ghost" size="sm" iconOnly aria-label="Undo" disabled={!marks.length} onClick={() => setMarks(renumber(marks.slice(0, -1)))}><Icon icon={Undo2} /></Button></Tip>
          <Tip label="Remove every mark" side="bottom"><Button variant="ghost" size="sm" iconOnly aria-label="Remove every mark" disabled={!marks.length} onClick={() => setMarks([])}><Icon icon={Trash2} /></Button></Tip>
          <span className="app-spacer" />
          <span className="d-type-caption d-text-tertiary">The page holds still while you mark it.</span>
          <Menu align="end" trigger={<Button size="sm" variant="primary" disabled={!marks.length}><Icon icon={Send} size="sm" />Send to a session</Button>}
            items={[{ kind: 'label', label: 'Send the marked page to' }, ...sessions.slice(0, 5).map((s) => ({ label: s.title, onSelect: () => send(s.title) })), { kind: 'sep' }, { label: 'A new session', onSelect: () => send('a new session') }]} />
        </div>
      )}
      <div className="browser__body">
        <div className="browser__page" data-theme="light">
          {tab === 0 ? (
            <>
              {!blocking && <div className="browser__ad" aria-label="A drawn advertisement">WINTER SALE · UP TO 70% OFF · <u>adnet.example</u> · this is the kind of thing the blocker removes</div>}
              <div className="browser__shop">
                <div className="browser__photo" aria-hidden="true" />
                <div className="app-stack">
                  <p className="d-type-caption d-text-tertiary">{SITE} · Knitwear</p>
                  <h2 className="d-type-title-2">Merino crew neck, navy</h2>
                  <p className="d-type-title-3" data-hit={step === 5 ? '' : undefined}>EUR 89.00</p>
                  <div className="app-row">
                    {['S', 'M', 'L'].map((s) => (
                      <button key={s} type="button" className="d-button" aria-pressed={size === s} data-hit={step === 6 && s === 'M' ? '' : undefined} onClick={() => driver === 'you' ? setSize(s) : toast('The model is driving. Switch to "You drive" to click.')}>Size {s}</button>
                    ))}
                  </div>
                  <button type="button" className="d-button d-button--primary d-button--lg" onClick={() => toast(driver === 'you' ? 'A drawn page: nothing is bought.' : 'The model is driving. It would ask before buying anything.')}>Add to basket</button>
                </div>
              </div>
              <svg ref={layer} className="browser__layer" data-tool={tool} onPointerDown={down} onPointerMove={move} onPointerUp={up}>
                {marks.map((m) => <MarkShape key={m.id} m={m} erase={tool === 'erase'} onErase={() => setMarks(renumber(marks.filter((x) => x.id !== m.id)))} />)}
                {draft && <MarkShape m={draft} erase={false} onErase={() => {}} />}
              </svg>
            </>
          ) : (
            <p className="app-empty">Blank tab.</p>
          )}
        </div>
        <aside className="browser__side">
          <Segmented small fill label="Panel" value={panel} onChange={setPanel} options={[{ value: 'actions', label: 'Actions' }, { value: 'marks', label: `Marks${marks.length ? ` ${marks.length}` : ''}` }, { value: 'blocker', label: 'Blocker' }, { value: 'network', label: 'Network' }, { value: 'console', label: 'Console' }]} />
          {panel === 'actions' && (
            <>
              <ul className="d-tasks">
                {ACTIONS.map((a, i) => (
                  <li key={i} className="d-task" data-state={i < step ? (a.failed ? 'failed' : 'done') : i === step ? 'running' : 'pending'}>
                    <div className="d-task__row"><Glyph /><span className="d-task__label">{a.what} <code>{a.code}</code></span><span className="d-task__meta">{i < step ? a.meta : ''}</span></div>
                  </li>
                ))}
              </ul>
              <div className="app-row"><Button size="sm" onClick={replay}>Replay what the model did</Button></div>
              <p className="d-type-caption d-text-secondary">The model can do what you can: click, type, read, run scripts, take screenshots, open tabs. Here the price did not load, so it looked at the blocker, let one domain through for this site, and said so.</p>
              <span className="d-untrusted">Page text · treated as data, never as instructions</span>
            </>
          )}
          {panel === 'marks' && (
            <>
              {!marks.length && <p className="d-type-caption d-text-secondary">Nothing is marked. Press Annotate, then point at something or draw on the page. A model can then be told "this one" without a description.</p>}
              {notes.map((m) => (
                <label key={m.id} className="d-field">
                  <span className="d-field__label"><span className="browser__num" style={{ background: m.color }}>{m.n}</span> points at {m.target}</span>
                  <input className="d-input" placeholder="What about it?" value={m.text} autoFocus={m === notes[notes.length - 1]} onChange={(e) => setMarks(marks.map((x) => (x.id === m.id ? { ...x, text: e.target.value } : x)))} />
                </label>
              ))}
              {marks.length > notes.length && <p className="d-type-caption d-text-secondary">{marks.length - notes.length} drawn {marks.length - notes.length === 1 ? 'mark' : 'marks'}: lines, highlights, boxes or arrows.</p>}
              {marks.length > 0 && (
                <>
                  <Facts rows={[['The picture', 'the page with your marks on it'], ['The notes', `${notes.length} as text`], ['What each pin points at', notes.length ? 'the element and its text' : 'no pins yet']]} />
                  <p className="d-type-caption d-text-secondary">This is what a model gets when you send it. Models that cannot see images get the notes and the elements; Demido can lend them eyes for the rest.</p>
                </>
              )}
            </>
          )}
          {panel === 'blocker' && (
            <>
              <p className="d-list__label" style={{ padding: 0 }}>Stopped on this page</p>
              {blocking ? (
                <table className="app-table"><tbody>{BLOCKED.map((b) => <tr key={b[0]}><td className="d-mono">{b[0]}</td><td className="d-text-secondary">{b[1]}</td><td data-num>{b[2]}</td></tr>)}</tbody></table>
              ) : (
                <p className="d-type-caption d-text-secondary">Nothing: the blocker is {blockerOn ? 'paused on this site' : 'off for every site'}.</p>
              )}
              <p className="d-list__label" style={{ padding: 0 }}>Changes on {SITE}</p>
              {rules.map((r) => (
                <div key={r.id} className="browser__rule">
                  <span>{r.what}</span>
                  <span className="d-type-caption d-text-secondary">By {r.by === 'model' ? 'the model' : 'you'} · {r.when}. {r.why}</span>
                  <Button size="sm" variant="ghost" onClick={() => { setRules(rules.filter((x) => x.id !== r.id)); if (r.what.startsWith('Paused')) setPaused(false); toast('Undone.') }}>Undo</Button>
                </div>
              ))}
              {!rules.length && <p className="d-type-caption d-text-secondary">None. The lists apply as they are.</p>}
              <p className="d-list__label" style={{ padding: 0 }}>What models are told</p>
              <pre className="d-code" style={{ whiteSpace: 'pre-wrap' }}><code>{BLOCKER_GUIDE}</code></pre>
              <NotBuilt>Scripted: nothing is filtered in the preview</NotBuilt>
            </>
          )}
          {panel === 'network' && (
            <table className="app-table"><tbody>{NETWORK.map((r) => <tr key={r[1]}><td className="d-mono">{r[0]}</td><td className="d-mono" style={{ overflowWrap: 'anywhere' }}>{r[1]}</td><td data-num className={cx(r[2] !== '200' && 'browser__flag')}>{r[2] === 'blocked' && !blocking ? '200' : r[2]}</td><td data-num>{r[3]}</td></tr>)}</tbody></table>
          )}
          {panel === 'console' && (
            <ul className="d-tasks">{CONSOLE.map((r, i) => <li key={i} className="d-task" data-state={r[0] === 'warn' ? 'needs-input' : 'done'}><div className="d-task__row"><Glyph name={r[0] === 'warn' ? 'task-needs-input' : 'task-skipped'} tone={r[0] === 'warn' ? 'caution' : undefined} /><span className="d-task__label d-mono">{r[1]}</span></div></li>)}</ul>
          )}
        </aside>
      </div>
    </div>
  )
}

export const feature: Feature = {
  id: 'browser', title: 'Browser', icon: AppWindow, group: 'connect', order: 0, path: 'browser', Page: BrowserPage,
  settings: [
    {
      id: 'browser', title: 'Browser', order: 6.5,
      settings: [
        {
          id: 'browser.adblock', type: 'switch', label: 'Block ads and trackers', default: true,
          desc: 'In the built-in browser, for you and for models.',
          details: 'Default on. Pages load faster, models read less junk, and fewer third parties see what is opened. Off: every site loads everything. You can also pause it for one site from the address bar.',
        },
        {
          id: 'browser.adblockModels', type: 'select', label: 'Models may change the blocker', default: 'site',
          options: [{ value: 'never', label: 'Never: they only read its log' }, { value: 'site', label: 'For one site, and they say so' }, { value: 'ask', label: 'Only after asking me' }],
          desc: 'What a model may do when a page breaks because something was blocked.',
          details: 'Default: a model may let one domain through, or pause the blocker, on the one site it is working on. Each change is listed in the browser with its reason and an Undo. A model can never switch the blocker off for all sites.',
        },
        {
          id: 'browser.adblockLists', type: 'select', label: 'Filter lists', default: 'standard',
          options: [{ value: 'standard', label: 'Standard: ads and trackers' }, { value: 'strict', label: 'Strict: also cookie banners and annoyances' }, { value: 'custom', label: 'My own choice of lists' }],
          desc: 'Which lists decide what is blocked. They are downloaded from the people who keep them.', notBuilt: true,
        },
      ],
    },
  ],
}
