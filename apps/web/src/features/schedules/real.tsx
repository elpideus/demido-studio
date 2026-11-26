/* Schedules, from the core (DECISIONS section J). Made from a sentence and shown back as "Understood as …"
   before anything is kept (J1), every part of it changeable there; each a session of its own, which runs on
   its own and asks nobody (J2). A monitor reads the shop's own price by code (J5). */
import { useEffect, useState } from 'react'
import { CalendarClock, BellOff, BellRing, Trash2, MessageSquare, Pencil, Pause, Play, Plus } from 'lucide-react'
import { useLocal } from '@/data/local-models'
import { useModelChoices } from '@/data/model-choices'
import { changeSchedule, deleteSchedule, desktopNotices, makeSchedule, runSchedule, scheduleRuns, setDesktopNotices, understand, useSchedules, whenFromNow, type Run, type Schedule, type Understood, type When } from '@/data/schedules'
import { Button, Dialog, Facts, Glyph, Icon, Page, Section, Segmented, Select, Signal, Tip, ContextMenu, Empty } from '@/ui'

const DAYS = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun']
const RUN_GLYPH: Record<Run['state'], [string, string]> = {
  running: ['task-running', 'Running'], done: ['task-done', 'Done'], failed: ['task-failed', 'Failed'], told: ['task-needs-input', 'Spoke up'], quiet: ['task-skipped', 'Nothing new'], skipped: ['task-skipped', 'Did not run'],
}
const when = (ms: number) => new Date(ms).toLocaleString([], { weekday: 'short', day: 'numeric', month: 'short', hour: '2-digit', minute: '2-digit', hourCycle: 'h23' })
const pad = (n: number) => String(n).padStart(2, '0')
/** A moment as a datetime-local input writes it, in this computer's time. */
const asLocal = (ms: number) => { const d = new Date(ms); return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}` }

/** When it runs, set by hand: every so often, every day, some days of the week, or once. */
function WhenEditor({ value, onChange }: { value: When; onChange: (w: When) => void }) {
  const at = value.kind === 'daily' || value.kind === 'weekly' ? value.at : '08:00'
  const hours = value.kind === 'every' && value.minutes % 60 === 0
  return (
    <div className="app-stack" style={{ gap: 'var(--d-space-8)' }}>
      <Segmented small label="How often" value={value.kind} onChange={(k) => onChange(
        k === 'every' ? { kind: 'every', minutes: 60 } : k === 'daily' ? { kind: 'daily', at } : k === 'weekly' ? { kind: 'weekly', days: [1, 2, 3, 4, 5], at } : { kind: 'once', at_ms: Date.now() + 3_600_000 })}
        options={[{ value: 'every', label: 'Every so often' }, { value: 'daily', label: 'Every day' }, { value: 'weekly', label: 'Some days' }, { value: 'once', label: 'Once' }]} />
      {value.kind === 'every' && (
        <div className="schedules__row">
          <span>Every</span>
          <input className="d-input schedules__number" type="number" min={hours ? 1 : 15} aria-label="How many" value={hours ? value.minutes / 60 : value.minutes}
            onChange={(e) => onChange({ kind: 'every', minutes: Math.max(1, Math.round(Number(e.target.value) || 0)) * (hours ? 60 : 1) })} />
          <Segmented small label="Unit" value={hours ? 'hours' : 'minutes'} onChange={(u) => onChange({ kind: 'every', minutes: u === 'hours' ? Math.max(60, Math.round(value.minutes / 60) * 60) : value.minutes })}
            options={[{ value: 'minutes', label: 'minutes' }, { value: 'hours', label: 'hours' }]} />
        </div>
      )}
      {value.kind === 'weekly' && (
        <div className="schedules__row" role="group" aria-label="Days">
          {DAYS.map((d, i) => {
            const on = value.days.includes(i + 1)
            return <Button key={d} size="sm" pressed={on} onClick={() => onChange({ ...value, days: on ? value.days.filter((x) => x !== i + 1) : [...value.days, i + 1].sort() })}>{d}</Button>
          })}
        </div>
      )}
      {(value.kind === 'daily' || value.kind === 'weekly') && (
        <label className="schedules__row"><span>At</span><input className="d-input schedules__time" type="time" value={value.at} onChange={(e) => onChange({ ...value, at: e.target.value })} /></label>
      )}
      {value.kind === 'once' && (
        <label className="schedules__row"><span>On</span><input className="d-input" type="datetime-local" value={asLocal(value.at_ms)} onChange={(e) => { const t = new Date(e.target.value).getTime(); if (!Number.isNaN(t)) onChange({ kind: 'once', at_ms: t }) }} /></label>
      )}
      {value.kind === 'every' && value.minutes < 15 && <Signal level="caution">Not more often than every 15 minutes.</Signal>}
    </div>
  )
}

const paid = (model: string) => model.includes('/')
/** The most a paid model may cost a schedule in a month, as typed: a number of dollars, or nothing. */
function LimitField({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  return (
    <label className="d-field"><span className="d-field__label">The most it may cost in a month, in dollars (empty: no limit of its own)</span>
      <input className="d-input schedules__number" type="number" min={0} step={0.5} inputMode="decimal" value={value} onChange={(e) => onChange(e.target.value)} /></label>
  )
}
const asLimit = (v: string) => (v.trim() === '' ? null : Math.max(0, Number(v)))

function NewSchedule({ onClose, onMade }: { onClose: () => void; onMade: () => void }) {
  const [text, setText] = useState('')
  const [read, setRead] = useState<Understood | null>(null)
  const [what, setWhat] = useState('')
  const [chosen, setChosen] = useState<When | null>(null)
  const [problem, setProblem] = useState<string | null>(null)
  const [making, setMaking] = useState(false)
  const choices = useModelChoices()
  const local = useLocal((s) => s.list?.models) ?? []
  const [model, setModel] = useState('')
  const [folder, setFolder] = useState('')
  const [limit, setLimit] = useState('')
  // A model that is loaded already, or else the first there is.
  useEffect(() => { if (!model && choices.length) setModel(local.find((m) => m.state === 'loaded')?.id ?? choices[0].value) }, [choices.length]) // eslint-disable-line react-hooks/exhaustive-deps
  // Read again a moment after the typing stops: what the sentence says, before anything is kept.
  useEffect(() => {
    if (!text.trim()) { setRead(null); return }
    const t = window.setTimeout(() => {
      understand(text).then((u) => { setRead(u); setWhat(u.what); setChosen(u.when ?? { kind: 'daily', at: '08:00' }); setProblem(null) }, (e: Error) => setProblem(e.message))
    }, 350)
    return () => window.clearTimeout(t)
  }, [text])
  const make = async () => {
    if (!read || !chosen) return
    setMaking(true)
    try {
      await makeSchedule({ said: text.trim(), what: what.trim(), when: chosen, watch: read.watch, model: model || null, folder: folder.trim() || null, limit_month_usd: paid(model) ? asLimit(limit) : null })
      onMade()
      onClose()
    } catch (e) {
      setProblem((e as Error).message)
    } finally {
      setMaking(false)
    }
  }
  const monitor = Boolean(read?.watch)
  return (
    <Dialog open wide onOpenChange={(o) => !o && onClose()} title="New schedule" text="Say it the way you would say it to a person: what, and when. To watch a price, give the shop's page."
      actions={<><Button onClick={onClose}>Cancel</Button><Button variant="primary" disabled={!read || !chosen || making || (!what.trim() && !monitor)} onClick={() => void make()}>{making ? 'Making…' : monitor ? 'Make the monitor' : 'Make the schedule'}</Button></>}>
      <div className="app-stack" style={{ marginBlockStart: 'var(--d-space-16)' }}>
        <textarea className="d-input" rows={2} autoFocus aria-label="What should happen, and when" placeholder="Give me my email report every morning at 7" value={text} onChange={(e) => setText(e.target.value)} />
        {read && chosen && (
          <div className="d-card"><div className="d-card__body app-stack">
            <p className="d-type-micro d-text-tertiary">Understood as</p>
            {monitor
              ? <Facts rows={[['Watches for', read.watch_said ?? ''], ['The page', <span className="d-type-code" key="u">{read.watch?.url}</span>], ['How', 'The shop\'s own price, from the data its page publishes for search engines, read and compared by Demido, not by a model. It tells you once, when it becomes true.']]} />
              : <label className="d-field"><span className="d-field__label">What to do, in your words</span><input className="d-input" value={what} onChange={(e) => setWhat(e.target.value)} /></label>}
            <div className="d-field"><span className="d-field__label">When</span><WhenEditor value={chosen} onChange={setChosen} /></div>
            {choices.length > 0 && <div className="d-field"><span className="d-field__label">{monitor ? 'The model, for a page that gives no price of its own' : 'The model its runs use'}</span><Select label="Model" value={model} onChange={setModel} options={choices} /></div>}
            {paid(model) && <LimitField value={limit} onChange={setLimit} />}
            {!monitor && <label className="d-field"><span className="d-field__label">A folder its tools may read and change, by its full path (empty: none)</span><input className="d-input d-input--mono" value={folder} onChange={(e) => setFolder(e.target.value)} placeholder="/home/you/Reports" /></label>}
            {read.notes.map((n) => <Signal key={n} level="note">{n}</Signal>)}
          </div></div>
        )}
        {!monitor && read && <Signal level="caution">It runs on its own, with every tool, and asks you nothing: a schedule has no permission level. What it reads from outside is cleaned first, but a small model can still do what a page or a mail tells it to. Everything a run did is in its session.</Signal>}
        {problem && <Signal level="caution">{problem}</Signal>}
      </div>
    </Dialog>
  )
}

function Runs({ schedule }: { schedule: Schedule }) {
  const [runs, setRuns] = useState<Run[] | null>(null)
  const load = () => { scheduleRuns(schedule.id).then((r) => setRuns(r.runs), () => setRuns([])) }
  // A run that ends while the list is open is shown.
  useEffect(() => { if (runs !== null) load() }, [schedule.last_run?.id, schedule.last_run?.state]) // eslint-disable-line react-hooks/exhaustive-deps
  return (
    <details className="d-disclosure" onToggle={(e) => { if (e.currentTarget.open && runs === null) load() }}>
      <summary>Runs</summary>
      <div className="d-disclosure__body">
        {runs === null ? <p className="d-type-caption d-text-tertiary">Reading…</p> : runs.length === 0 ? <p className="d-type-caption d-text-tertiary">No runs yet.</p> : (
          <ul className="d-tasks">
            {runs.map((r) => (
              <li key={r.id} className="d-task" data-state="done">
                <div className="d-task__row">
                  <Glyph name={RUN_GLYPH[r.state][0]} label={RUN_GLYPH[r.state][1]} busy={r.state === 'running'} tone={r.state === 'failed' ? 'danger' : r.state === 'told' ? 'caution' : undefined} />
                  <span className="d-task__label">{when(r.started_at)}{r.late ? ', late' : ''}{r.by_hand ? ', by you' : ''} · {r.note || RUN_GLYPH[r.state][1]}</span>
                </div>
              </li>
            ))}
          </ul>
        )}
      </div>
    </details>
  )
}

/** What a schedule does and when, changed: and for a paid model, its limit. */
function EditSchedule({ schedule, onClose, onChanged }: { schedule: Schedule; onClose: () => void; onChanged: () => void }) {
  const [what, setWhat] = useState(schedule.what)
  const [when, setWhen] = useState<When>(schedule.when)
  const [limit, setLimit] = useState(schedule.limit_month_usd?.toString() ?? '')
  const [problem, setProblem] = useState<string | null>(null)
  const monitor = Boolean(schedule.watch)
  const save = () => changeSchedule(schedule.id, { what: monitor ? undefined : what.trim(), when, limit_month_usd: paid(schedule.model) ? asLimit(limit) : undefined })
    .then(() => { onChanged(); onClose() }, (e: Error) => setProblem(e.message))
  return (
    <Dialog open wide onOpenChange={(o) => !o && onClose()} title={monitor ? 'Change the monitor' : 'Change the schedule'} text={`Made from: "${schedule.said}"`}
      actions={<><Button onClick={onClose}>Cancel</Button><Button variant="primary" disabled={!monitor && !what.trim()} onClick={() => void save()}>Save</Button></>}>
      <div className="app-stack" style={{ marginBlockStart: 'var(--d-space-16)' }}>
        {monitor ? <Facts rows={[['Watches for', schedule.watch_said ?? '']]} /> : <label className="d-field"><span className="d-field__label">What to do, in your words</span><input className="d-input" value={what} onChange={(e) => setWhat(e.target.value)} /></label>}
        <div className="d-field"><span className="d-field__label">When</span><WhenEditor value={when} onChange={setWhen} /></div>
        {paid(schedule.model) && <LimitField value={limit} onChange={setLimit} />}
        <p className="d-type-caption d-text-tertiary">Its model is its session's: {schedule.model}. It can be changed in the session.</p>
        {problem && <Signal level="caution">{problem}</Signal>}
      </div>
    </Dialog>
  )
}

function ScheduleCard({ schedule: s, running, onChanged }: { schedule: Schedule; running: boolean; onChanged: () => void }) {
  const [deleting, setDeleting] = useState(false)
  const [editing, setEditing] = useState(false)
  const [problem, setProblem] = useState<string | null>(null)
  const act = (p: Promise<unknown>) => p.then(onChanged, (e: Error) => setProblem(e.message))
  const monitor = Boolean(s.watch)
  const last = s.last_run
  const glyph = running ? 'task-running' : s.state === 'on' ? 'task-pending' : s.state === 'paused' ? 'task-failed' : 'task-done'
  return (
    <ContextMenu items={[
      { label: running ? 'Running…' : 'Run now', lead: <Icon icon={Play} size="sm" />, disabled: running, onSelect: () => void act(runSchedule(s.id)) },
      s.state === 'on'
        ? { label: 'Pause', lead: <Icon icon={Pause} size="sm" />, onSelect: () => void act(changeSchedule(s.id, { state: 'paused' })) }
        : { label: s.state === 'paused' ? 'Turn on' : 'Again', lead: <Icon icon={Play} size="sm" />, onSelect: () => void act(changeSchedule(s.id, { state: 'on' })) },
      { label: 'Edit…', lead: <Icon icon={Pencil} size="sm" />, onSelect: () => setEditing(true) },
      { label: 'Open its session', lead: <Icon icon={MessageSquare} size="sm" />, onSelect: () => { window.location.hash = `#/sessions/${s.session_id}` } },
      { kind: 'sep' },
      { label: 'Delete…', lead: <Icon icon={Trash2} size="sm" />, danger: true, disabled: running, onSelect: () => setDeleting(true) },
    ]}>
    <article className="d-card">
      <header className="d-card__head">
        <Glyph name={glyph} busy={running} tone={s.state === 'paused' ? 'caution' : undefined} size="1.25rem" style={{ marginTop: '0.125rem' }} label={running ? 'Running' : s.state === 'on' ? 'Waiting for its time' : s.state === 'paused' ? 'Paused' : 'Done'} />
        <div className="app-stack" style={{ gap: 'var(--d-space-2)', flex: 1, minWidth: 0 }}>
          <h3 className="d-card__title">{monitor ? s.watch_said : s.what}</h3>
          <span className="d-card__sub">{monitor ? 'Monitor' : 'Schedule'} · {s.when_said}</span>
        </div>
      </header>
      <div className="d-card__body app-stack">
        {s.state === 'paused' && s.paused_why && <Signal level="caution">{s.paused_why}</Signal>}
        <Facts rows={[
          ['Next run', running ? 'running now' : s.state === 'on' && s.next_at ? whenFromNow(s.next_at) : s.state === 'paused' ? 'paused' : 'not again'],
          ['Last run', last ? `${RUN_GLYPH[last.state][1]}, ${when(last.started_at)}${last.note ? `: ${last.note}` : ''}` : 'none yet'],
          ...(monitor && s.watched?.last != null ? [['Last price', `${s.watched.last}${s.watched.currency ? ` ${s.watched.currency}` : ''}`] as [string, string]] : []),
          ['Model', s.model],
          ...(s.limit_month_usd != null ? [['Its limit', `$${s.limit_month_usd} a month`] as [string, string]] : []),
        ]} />
        <Runs schedule={s} />
        {problem && <Signal level="caution">{problem}</Signal>}
      </div>
      <footer className="d-card__foot">
        <Button disabled={running} onClick={() => act(runSchedule(s.id))}>{running ? 'Running…' : 'Run now'}</Button>
        {s.state === 'on' ? <Button variant="ghost" onClick={() => act(changeSchedule(s.id, { state: 'paused' }))}>Pause</Button>
          : <Button variant="ghost" onClick={() => act(changeSchedule(s.id, { state: 'on' }))}>{s.state === 'paused' ? 'Turn on' : 'Again'}</Button>}
        <Button variant="ghost" onClick={() => setEditing(true)}>Edit</Button>
        <Button variant="ghost" onClick={() => { window.location.hash = `#/sessions/${s.session_id}` }}>Open its session</Button>
        <Button variant="ghost" disabled={running} onClick={() => setDeleting(true)}>Delete</Button>
      </footer>
      {editing && <EditSchedule schedule={s} onClose={() => setEditing(false)} onChanged={onChanged} />}
      {deleting && (
        <Dialog open onOpenChange={(o) => !o && setDeleting(false)} title="Delete this schedule?" text="It stops running, and its runs are forgotten. Its session stays, with everything its runs wrote in it."
          actions={<><Button onClick={() => setDeleting(false)}>Cancel</Button><Button variant="danger" onClick={() => { setDeleting(false); void act(deleteSchedule(s.id)) }}>Delete</Button></>} />
      )}
    </article>
    </ContextMenu>
  )
}

export function RealSchedules() {
  const { list, error, running, again } = useSchedules()
  const [adding, setAdding] = useState(false)
  const [desktop, setDesktop] = useState(desktopNotices())
  const on = list?.filter((s) => s.state === 'on').length ?? 0
  const desktopTip = desktop === 'shell' ? 'The desktop app shows a notice on the desktop for each run that has something to say, while its window is not in front. A click on it opens the run.'
    : desktop === 'none' ? 'This browser cannot show notices on the desktop from this address.'
    : desktop === 'blocked' ? "Notices are blocked for this page in the browser's own settings."
    : 'A notice on the desktop for each run that has something to say, while this window is not in front. The window shows its own either way.'
  return (
    <Page title="Schedules" lead="Tasks at a time you set, and monitors that watch a price."
      about={<><p>Work Demido does on its own. A monitor tells you when a price is what you wait for.</p><p>Each is a session of its own, which runs with every tool and asks you nothing. The model is loaded for a run, and unloaded when it has been idle.</p></>}
      actions={<>
        <Tip label={desktopTip}><span><Button variant="ghost" iconOnly aria-label={desktop === 'on' || desktop === 'shell' ? 'Desktop notices: on' : 'Desktop notices: off'} pressed={desktop === 'on' || desktop === 'shell'} disabled={desktop === 'none' || desktop === 'blocked' || desktop === 'shell'} onClick={() => void setDesktopNotices(desktop !== 'on').then(setDesktop)}><Icon icon={desktop === 'on' || desktop === 'shell' ? BellRing : BellOff} /></Button></span></Tip>
        <Button variant="primary" onClick={() => setAdding(true)}><Icon icon={Plus} />New schedule</Button>
      </>}>
      {error && <Signal level="caution">{error}</Signal>}
      {list === null && !error && <p className="app-empty"><Glyph name="task-running" busy /> Reading…</p>}
      {list && list.length === 0 && <Empty icon={CalendarClock} title="No schedules yet" text={<>Make one from a sentence, like “Give me my email report every morning at 7”.</>} action={<Button onClick={() => setAdding(true)}><Icon icon={Plus} />New schedule</Button>} />}
      {list && list.length > 0 && (
        <Section title={`${on} on · ${list.length - on} not`}>
          <div className="app-grid">{list.map((s) => <ScheduleCard key={s.id} schedule={s} running={running.has(s.id) || s.last_run?.state === 'running'} onChanged={again} />)}</div>
        </Section>
      )}
      {adding && <NewSchedule onClose={() => setAdding(false)} onMade={again} />}
    </Page>
  )
}
