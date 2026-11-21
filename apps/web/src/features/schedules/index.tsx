import { useState } from 'react'
import { CalendarClock, Plus } from 'lucide-react'
import type { Feature } from '@/app/registry'
import { addSchedule, modelById, runSchedule, setScheduleEnabled, toast, useStore } from '@/data/store'
import type { Schedule } from '@/data/types'
import { Button, Dialog, Facts, Glyph, Icon, Page, Section, Signal, Switch } from '@/ui'

const RESULT = { ok: ['task-done', 'Done'], 'no change': ['task-skipped', 'No change'], changed: ['task-needs-input', 'Changed: you were told'], failed: ['task-failed', 'Failed'] } as const

/** Reads a sentence the way the app will: a small scripted parser, enough to show the flow. */
function understand(text: string) {
  const t = text.toLowerCase()
  const time = t.match(/(\d{1,2})(?::(\d{2}))?\s*(am|pm|o'clock)?/)
  let hh = time ? Number(time[1]) : 7
  if (time?.[3] === 'pm' && hh < 12) hh += 12
  const at = `${String(hh).padStart(2, '0')}:${time?.[2] ?? '00'}`
  const monitor = /\b(monitor|watch|tell me when|notify|alert)\b/.test(t)
  const every = /\bweek/.test(t) ? 'Every week' : /\bhour/.test(t) ? 'Every hour' : 'Every day'
  return { kind: monitor ? ('monitor' as const) : ('schedule' as const), when: every === 'Every hour' ? every : `${every} at ${at}`, title: text.replace(/^(please\s+)?(give me|send me|monitor|watch|tell me when)\s+/i, '').replace(/\s+(every|each)\b.*$/i, '').replace(/^\w/, (c) => c.toUpperCase()).slice(0, 48) || 'New schedule' }
}

function NewSchedule({ onClose }: { onClose: () => void }) {
  const [text, setText] = useState('Give me my email report every morning at 7')
  const u = understand(text)
  return (
    <Dialog open wide onOpenChange={(o) => !o && onClose()} title="New schedule" text="Say it the way you would say it to a person."
      actions={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" disabled={!text.trim()} onClick={() => { addSchedule(u.title, u.when, u.kind, u.kind === 'monitor' ? 'Check the thing.\nChanged → tell user. Else stay quiet.' : 'Do the task.\nReport short. Answer first.'); toast('Schedule created. No model stays loaded for it.', undefined, 'task-done'); onClose() }}>
            Create {u.kind === 'monitor' ? 'monitor' : 'schedule'}
          </Button>
        </>
      }>
      <div className="app-stack" style={{ marginBlockStart: 'var(--d-space-16)' }}>
        <textarea className="d-input" rows={2} aria-label="What should happen, and when" value={text} onChange={(e) => setText(e.target.value)} />
        <div className="d-card"><div className="d-card__body">
          <p className="d-type-micro d-text-tertiary" style={{ marginBlockEnd: 'var(--d-space-8)' }}>Understood as</p>
          <Facts rows={[['Name', u.title], ['Kind', u.kind === 'monitor' ? 'Monitor: speaks up only on a change' : 'Schedule: reports every time'], ['When', u.when], ['Model', 'Default model, loaded for the run, then unloaded']]} />
        </div></div>
        <Signal level="note">Between runs nothing is loaded. If a session is using the model when a run is due, the session lends it, or waits its turn.</Signal>
      </div>
    </Dialog>
  )
}

function ScheduleCard({ s }: { s: Schedule }) {
  const model = modelById(s.modelId)
  return (
    <article className="d-card">
      <header className="d-card__head">
        <Glyph name={s.state === 'running' ? 'task-running' : s.enabled ? 'task-pending' : 'task-skipped'} size="1.25rem" style={{ marginTop: '0.125rem' }} label={s.state === 'running' ? 'Running' : s.enabled ? 'Waiting for its time' : 'Off'} />
        <div className="app-stack" style={{ gap: 'var(--d-space-2)', flex: 1 }}>
          <h3 className="d-card__title">{s.title}</h3>
          <span className="d-card__sub">{s.kind === 'monitor' ? 'Monitor' : 'Schedule'} · {s.when}</span>
        </div>
        <Switch label={`${s.title} on`} checked={s.enabled} onChange={(v) => setScheduleEnabled(s.id, v)} />
      </header>
      <div className="d-card__body app-stack">
        <Facts rows={[['Next run', s.state === 'running' ? 'running now' : s.enabled ? s.next : 'off'], ['Model', model?.name ?? s.modelId]]} />
        <details className="d-disclosure">
          <summary>What the model is told</summary>
          <div className="d-disclosure__body"><pre className="d-code">{s.prompt}</pre></div>
        </details>
        <details className="d-disclosure" open={s.runs.length > 0 && s.runs[0].at === 'Just now'}>
          <summary>Runs ({s.runs.length})</summary>
          <div className="d-disclosure__body">
            {s.runs.length === 0 && <p>No runs yet.</p>}
            <ul className="d-tasks">
              {s.runs.map((r, i) => (
                <li key={i} className="d-task" data-state="done">
                  <div className="d-task__row">
                    <Glyph name={RESULT[r.result][0]} label={RESULT[r.result][1]} tone={r.result === 'failed' ? 'danger' : r.result === 'changed' ? 'caution' : undefined} />
                    <span className="d-task__label">{r.at} · {r.note}</span>
                  </div>
                </li>
              ))}
            </ul>
          </div>
        </details>
      </div>
      <footer className="d-card__foot">
        <Button disabled={s.state === 'running'} onClick={() => void runSchedule(s.id)}>{s.state === 'running' ? 'Running…' : 'Run once now'}</Button>
        <Button variant="ghost" onClick={() => toast('Editing a schedule: not built in the preview.')}>Edit</Button>
      </footer>
    </article>
  )
}

function SchedulesPage() {
  const schedules = useStore((s) => s.schedules)
  const [adding, setAdding] = useState(false)
  return (
    <Page title="Schedules" lead="Things Demido does on its own: reports at a set time, and monitors that speak up only when something changes. A model is loaded for the run and unloaded after."
      actions={<Button variant="primary" onClick={() => setAdding(true)}><Icon icon={Plus} />New schedule</Button>}>
      <Section title={`${schedules.filter((s) => s.enabled).length} on · ${schedules.filter((s) => !s.enabled).length} off`}>
        <div className="app-grid">{schedules.map((s) => <ScheduleCard key={s.id} s={s} />)}</div>
      </Section>
      {adding && <NewSchedule onClose={() => setAdding(false)} />}
    </Page>
  )
}

export const feature: Feature = { id: 'schedules', title: 'Schedules', icon: CalendarClock, group: 'work', order: 1, path: 'schedules', Page: SchedulesPage }
