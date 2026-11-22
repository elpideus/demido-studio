/* Schedules and monitors, from the core (DECISIONS section J). A schedule is a session of its own; it runs on
   its own, with every tool, and asks nobody anything. A monitor reads a shop's own price, by code, and says
   something only when what it watches for becomes true. What a run gave is a notice here, and a message in
   its session (J6). */
import { useEffect, useState } from 'react'
import { coreGet, coreSend, onCoreEvent } from './core'
import { toast } from './store'

export type When =
  | { kind: 'every'; minutes: number }
  | { kind: 'daily'; at: string }
  | { kind: 'weekly'; days: number[]; at: string }
  | { kind: 'once'; at_ms: number }
export type Price = { kind: 'below'; below: number } | { kind: 'above'; above: number } | { kind: 'drops' }
export interface Watch { url: string; price: Price }
export interface Understood { what: string; when: When | null; when_said: string | null; watch: Watch | null; watch_said: string | null; notes: string[] }
export interface Run {
  id: string; schedule_id: string; due_at: number; started_at: number; ended_at: number | null
  state: 'running' | 'done' | 'failed' | 'told' | 'quiet' | 'skipped'; late: boolean; by_hand: boolean; message_id: string | null; note: string
}
export interface Watched { first: number | null; last: number | null; currency: string | null; holds: boolean; line: string | null }
export interface Schedule {
  id: string; session_id: string; what: string; said: string; when: When; when_said: string; watch: Watch | null; watch_said: string | null
  state: 'on' | 'paused' | 'done'; paused_why: string | null; next_at: number | null; last_run_at: number | null; failures: number
  watched: Watched | null; limit_month_usd: number | null; created_at: number; updated_at: number; last_run: Run | null
}
export interface NewSchedule { said: string; what: string; when: When; watch?: Watch | null; model?: string | null; folder?: string | null; limit_month_usd?: number | null }

export const understand = (text: string) => coreSend<Understood>('POST', '/schedules/understand', { text })
export const makeSchedule = (s: NewSchedule) => coreSend<Schedule>('POST', '/schedules', s)
export const changeSchedule = (id: string, change: { what?: string; when?: When; state?: 'on' | 'paused'; limit_month_usd?: number | null }) => coreSend<Schedule>('PATCH', `/schedules/${id}`, change)
export const deleteSchedule = (id: string) => coreSend<Schedule>('DELETE', `/schedules/${id}`)
export const runSchedule = (id: string) => coreSend<Schedule>('POST', `/schedules/${id}/run`)
export const scheduleRuns = (id: string) => coreGet<{ schedule: Schedule; runs: Run[]; running: boolean }>(`/schedules/${id}`)

const KINDS = ['schedule.created', 'schedule.changed', 'schedule.deleted', 'schedule.started', 'schedule.ran']

/** Every schedule, read again whenever one is made, changed, deleted, starts or ends a run. */
export function useSchedules(): { list: Schedule[] | null; error: string | null; running: Set<string>; again: () => void } {
  const [list, setList] = useState<Schedule[] | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [running, setRunning] = useState<Set<string>>(new Set())
  const [tick, setTick] = useState(0)
  useEffect(() => {
    let alive = true
    coreGet<Schedule[]>('/schedules').then((l) => { if (alive) { setList(l); setError(null) } }, (e: Error) => { if (alive) setError(e.message) })
    return () => { alive = false }
  }, [tick])
  useEffect(() => {
    const offs = KINDS.map((kind) => onCoreEvent(kind, (e) => {
      if (kind === 'schedule.started' && e.subject) setRunning((r) => new Set(r).add(e.subject!))
      if (kind === 'schedule.ran' && e.subject) setRunning((r) => { const n = new Set(r); n.delete(e.subject!); return n })
      setTick((t) => t + 1)
    }))
    return () => offs.forEach((off) => off())
  }, [])
  return { list, error, running, again: () => setTick((t) => t + 1) }
}

const openSession = (id: string) => { window.location.hash = `#/sessions/${id}` }

/** A notice in the window for each run that has something to say (J6): what a task gave, a monitor that
    spoke up, a run that failed or was skipped, a schedule that paused itself. A monitor that found nothing
    new says nothing. Started once, when the window finds a core. */
let listening = false
export function startSchedules() {
  if (listening) return
  listening = true
  onCoreEvent('schedule.ran', (e) => {
    const d = e.data as { state: Run['state']; note: string; what: string; watch: string | null; session: string; late: boolean; paused: boolean; paused_why: string | null }
    const name = d.watch || d.what || 'A schedule'
    const open = { label: 'Open', run: () => openSession(d.session) }
    const late = d.late ? ' (late: the computer was asleep or Demido was not running at its time)' : ''
    if (d.paused) toast(`${name} paused itself: ${d.paused_why ?? d.note}`, open, 'task-failed')
    else if (d.state === 'told') toast(`${name}: ${d.note}`, open, 'task-needs-input')
    else if (d.state === 'done') toast(`${name}${late}: ${d.note}`, open, 'task-done')
    else if (d.state === 'failed') toast(`${name} failed${late}: ${d.note}`, open, 'task-failed')
    else if (d.state === 'skipped') toast(`${name} did not run: ${d.note}`, open, 'task-skipped')
  })
}

/** A time to come, as people say it: "in 20 minutes", "today at 07:00", "tomorrow at 07:00", "Tue 25 Nov at 09:00". */
export function whenFromNow(ms: number, now = Date.now()): string {
  const d = new Date(ms)
  const time = d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', hourCycle: 'h23' })
  const minutes = Math.round((ms - now) / 60_000)
  if (minutes <= 0) return 'now'
  if (minutes < 60) return `in ${minutes} minute${minutes === 1 ? '' : 's'}`
  const day = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime()
  const days = Math.round((day(d) - day(new Date(now))) / 86_400_000)
  if (days === 0) return `today at ${time}`
  if (days === 1) return `tomorrow at ${time}`
  return `${d.toLocaleDateString([], { weekday: 'short', day: 'numeric', month: 'short' })} at ${time}`
}
