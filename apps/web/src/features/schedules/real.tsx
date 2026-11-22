/* Schedules, from the core (DECISIONS section J). Made from a sentence and shown back as "Understood as …"
   before anything is kept (J1), every part of it changeable there; each a session of its own, which runs on
   its own and asks nobody (J2). A monitor reads the shop's own price by code (J5). */
import { useEffect, useState } from 'react'
import { Plus } from 'lucide-react'
import { changeSchedule, deleteSchedule, makeSchedule, runSchedule, scheduleRuns, understand, useSchedules, whenFromNow, type Run, type Schedule, type Understood, type When } from '@/data/schedules'
import { Button, Dialog, Facts, Glyph, Icon, Page, Section, Segmented, Signal } from '@/ui'

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

function NewSchedule({ onClose, onMade }: { onClose: () => void; onMade: () => void }) {
  const [text, setText] = useState('')
  const [read, setRead] = useState<Understood | null>(null)
  const [what, setWhat] = useState('')
  const [chosen, setChosen] = useState<When | null>(null)
  const [problem, setProblem] = useState<string | null>(null)
  const [making, setMaking] = useState(false)
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
      await makeSchedule({ said: text.trim(), what: what.trim(), when: chosen, watch: read.watch })
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

function ScheduleCard({ schedule: s, running, onChanged }: { schedule: Schedule; running: boolean; onChanged: () => void }) {
  const [deleting, setDeleting] = useState(false)
  const [problem, setProblem] = useState<string | null>(null)
  const act = (p: Promise<unknown>) => p.then(onChanged, (e: Error) => setProblem(e.message))
  const monitor = Boolean(s.watch)
  const last = s.last_run
  const glyph = running ? 'task-running' : s.state === 'on' ? 'task-pending' : s.state === 'paused' ? 'task-failed' : 'task-done'
  return (
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
        ]} />
        <Runs schedule={s} />
        {problem && <Signal level="caution">{problem}</Signal>}
      </div>
      <footer className="d-card__foot">
        <Button disabled={running} onClick={() => act(runSchedule(s.id))}>{running ? 'Running…' : 'Run now'}</Button>
        {s.state === 'on' ? <Button variant="ghost" onClick={() => act(changeSchedule(s.id, { state: 'paused' }))}>Pause</Button>
          : <Button variant="ghost" onClick={() => act(changeSchedule(s.id, { state: 'on' }))}>{s.state === 'paused' ? 'Turn on' : 'Again'}</Button>}
        <Button variant="ghost" onClick={() => { window.location.hash = `#/sessions/${s.session_id}` }}>Open its session</Button>
        <Button variant="ghost" disabled={running} onClick={() => setDeleting(true)}>Delete</Button>
      </footer>
      {deleting && (
        <Dialog open onOpenChange={(o) => !o && setDeleting(false)} title="Delete this schedule?" text="It stops running, and its runs are forgotten. Its session stays, with everything its runs wrote in it."
          actions={<><Button onClick={() => setDeleting(false)}>Cancel</Button><Button variant="danger" onClick={() => { setDeleting(false); void act(deleteSchedule(s.id)) }}>Delete</Button></>} />
      )}
    </article>
  )
}

export function RealSchedules() {
  const { list, error, running, again } = useSchedules()
  const [adding, setAdding] = useState(false)
  const on = list?.filter((s) => s.state === 'on').length ?? 0
  return (
    <Page title="Schedules" lead="Work Demido does on its own: tasks at a time you set, and monitors that tell you when a price is what you wait for. Each is a session of its own, which runs with every tool and asks you nothing. The model is loaded for a run, and unloaded when it has been idle."
      actions={<Button variant="primary" onClick={() => setAdding(true)}><Icon icon={Plus} />New schedule</Button>}>
      {error && <Signal level="caution">{error}</Signal>}
      {list === null && !error && <p className="app-empty"><Glyph name="task-running" busy /> Reading…</p>}
      {list && list.length === 0 && <p className="app-empty">No schedules yet. Make one from a sentence: "Give me my email report every morning at 7".</p>}
      {list && list.length > 0 && (
        <Section title={`${on} on · ${list.length - on} not`}>
          <div className="app-grid">{list.map((s) => <ScheduleCard key={s.id} schedule={s} running={running.has(s.id) || s.last_run?.state === 'running'} onChanged={again} />)}</div>
        </Section>
      )}
      {adding && <NewSchedule onClose={() => setAdding(false)} onMade={again} />}
    </Page>
  )
}
