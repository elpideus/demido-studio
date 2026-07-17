import { useEffect, useRef, useState } from 'react'
import { RefreshCw, ChevronLeft, ChevronRight, Pencil, X, MapPin, AlignLeft, Plus, Clock, CalendarDays } from 'lucide-react'
import { invoke } from '@tauri-apps/api/core'
import { Button } from '@/components/ui/button'
import { Switch } from '@/components/ui/switch'

interface CalendarEvent {
  id: string
  summary: string
  start: string
  end: string
  location: string | null
  description: string | null
  color: string | null
}

interface EventFormState {
  summary: string
  startDate: string
  startTime: string
  endDate: string
  endTime: string
  location: string
  description: string
  allDay: boolean
}

function isoDate(iso: string) {
  return iso.split('T')[0] || iso
}

function formatTime(iso: string) {
  try {
    return new Date(iso).toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' }).toLowerCase()
  } catch {
    return ''
  }
}

function formatDateTime(iso: string) {
  try {
    return new Date(iso).toLocaleString(undefined, { weekday: 'short', month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' })
  } catch {
    return iso
  }
}

const FALLBACK_COLORS = ['#4285f4','#0f9d58','#db4437','#f4b400','#ab47bc','#e91e63','#ff7043','#00acc1']

function eventColor(ev: CalendarEvent): string {
  if (ev.color) return ev.color
  let h = 0
  for (let i = 0; i < ev.summary.length; i++) h = (h * 31 + ev.summary.charCodeAt(i)) >>> 0
  return FALLBACK_COLORS[h % FALLBACK_COLORS.length]
}

const DOW_MON = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun']

function mondayFirst(jsDay: number) { return (jsDay + 6) % 7 }

function splitDateTime(iso: string): { date: string; time: string } {
  try {
    const d = new Date(iso)
    const pad = (n: number) => String(n).padStart(2, '0')
    return {
      date: `${d.getFullYear()}-${pad(d.getMonth()+1)}-${pad(d.getDate())}`,
      time: `${pad(d.getHours())}:${pad(d.getMinutes())}`,
    }
  } catch { return { date: '', time: '' } }
}

// ── Event Detail Modal ─────────────────────────────────────────────────────────

function EventDetail({
  event,
  onClose,
  onEdit,
}: {
  event: CalendarEvent
  onClose: () => void
  onEdit: () => void
}) {
  const allDay = !event.start.includes('T')
  return (
    <div className="absolute inset-0 z-20 flex items-center justify-center bg-black/40 backdrop-blur-sm" onClick={onClose}>
      <div
        className="bg-popover border border-border rounded-xl shadow-2xl w-80 p-4 flex flex-col gap-3"
        onClick={e => e.stopPropagation()}
      >
        <div className="flex items-start justify-between gap-2">
          <div className="flex items-center gap-2 min-w-0">
            <span className="w-2.5 h-2.5 rounded-full shrink-0 mt-0.5" style={{ backgroundColor: eventColor(event) }} />
            <h3 className="text-sm font-semibold text-foreground leading-tight">{event.summary}</h3>
          </div>
          <div className="flex items-center gap-1 shrink-0">
            <Button variant="ghost" size="icon-xs" onClick={onEdit} title="Edit">
              <Pencil size={13} />
            </Button>
            <Button variant="ghost" size="icon-xs" onClick={onClose}>
              <X size={13} />
            </Button>
          </div>
        </div>
        <div className="flex flex-col gap-1.5 text-xs text-muted-foreground">
          <p className="text-foreground/80">
            {allDay
              ? new Date(event.start + 'T12:00:00').toLocaleDateString(undefined, { weekday: 'long', month: 'long', day: 'numeric' })
              : `${formatDateTime(event.start)} – ${formatTime(event.end)}`}
          </p>
          {event.location && (
            <p className="flex items-center gap-1.5"><MapPin size={11} className="shrink-0" />{event.location}</p>
          )}
          {event.description && (
            <p className="flex items-start gap-1.5 whitespace-pre-wrap leading-relaxed">
              <AlignLeft size={11} className="shrink-0 mt-0.5" />{event.description}
            </p>
          )}
        </div>
      </div>
    </div>
  )
}

// ── Date Picker ───────────────────────────────────────────────────────────────

function DatePicker({ value, onChange }: { value: string; onChange: (d: string) => void }) {
  const [open, setOpen] = useState(false)
  const [editing, setEditing] = useState(false)
  const [draft, setDraft] = useState('')
  const ref = useRef<HTMLDivElement>(null)
  const inputRef = useRef<HTMLInputElement>(null)
  const parsed = value ? new Date(value + 'T12:00:00') : new Date()
  const [cur, setCur] = useState({ year: parsed.getFullYear(), month: parsed.getMonth() })

  useEffect(() => {
    if (!open) return
    const handler = (e: MouseEvent) => { if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false) }
    document.addEventListener('mousedown', handler)
    return () => document.removeEventListener('mousedown', handler)
  }, [open])

  const enterEdit = () => { setEditing(true); setOpen(false); setDraft(value); setTimeout(() => inputRef.current?.select(), 0) }

  const commitEdit = () => {
    const d = new Date(draft)
    if (!isNaN(d.getTime())) {
      const pad = (n: number) => String(n).padStart(2, '0')
      onChange(`${d.getFullYear()}-${pad(d.getMonth()+1)}-${pad(d.getDate())}`)
    }
    setEditing(false)
  }

  const firstDayJs = new Date(cur.year, cur.month, 1).getDay()
  const leading = mondayFirst(firstDayJs)
  const daysInMonth = new Date(cur.year, cur.month + 1, 0).getDate()
  const rows = Math.ceil((leading + daysInMonth) / 7)
  const pad = (n: number) => String(n).padStart(2, '0')
  const todayStr = (() => { const t = new Date(); return `${t.getFullYear()}-${pad(t.getMonth()+1)}-${pad(t.getDate())}` })()

  const displayDate = value
    ? new Date(value + 'T12:00:00').toLocaleDateString(undefined, { month: 'short', day: 'numeric', year: 'numeric' })
    : 'Pick date'

  if (editing) {
    return (
      <div className="relative flex-1">
        <input
          ref={inputRef}
          className="w-full bg-secondary border border-primary/50 rounded-md px-2 py-1.5 text-xs text-foreground outline-none ring-1 ring-primary/50"
          value={draft}
          onChange={e => setDraft(e.target.value)}
          onBlur={commitEdit}
          onKeyDown={e => { if (e.key === 'Enter') commitEdit(); if (e.key === 'Escape') setEditing(false) }}
          placeholder="YYYY-MM-DD or any date"
        />
      </div>
    )
  }

  return (
    <div ref={ref} className="relative flex-1">
      <div className="flex items-center bg-secondary border border-border rounded-md overflow-hidden hover:bg-accent/30 transition-colors w-full">
        <button
          type="button"
          onDoubleClick={enterEdit}
          onClick={() => setOpen(v => !v)}
          className="flex-1 flex items-center gap-1.5 px-2 py-1.5 text-xs text-foreground text-left"
        >
          <CalendarDays size={11} className="text-muted-foreground shrink-0" />
          {displayDate}
        </button>
        <button type="button" onClick={enterEdit} className="px-1.5 py-1.5 text-muted-foreground hover:text-foreground transition-colors border-l border-border shrink-0">
          <Pencil size={10} />
        </button>
      </div>
      {open && (
        <div className="absolute top-full mt-1 left-0 z-50 bg-popover border border-border rounded-xl shadow-2xl p-3 w-56">
          <div className="flex items-center justify-between mb-2">
            <button type="button" onClick={() => setCur(c => { const m = c.month===0?11:c.month-1; return { year: c.month===0?c.year-1:c.year, month: m } })} className="p-0.5 hover:bg-accent rounded"><ChevronLeft size={13}/></button>
            <span className="text-xs font-semibold">{new Date(cur.year, cur.month).toLocaleDateString(undefined, { month: 'long', year: 'numeric' })}</span>
            <button type="button" onClick={() => setCur(c => { const m = c.month===11?0:c.month+1; return { year: c.month===11?c.year+1:c.year, month: m } })} className="p-0.5 hover:bg-accent rounded"><ChevronRight size={13}/></button>
          </div>
          <div className="grid grid-cols-7 mb-1">
            {['M','T','W','T','F','S','S'].map((d,i) => (
              <div key={i} className="text-center text-[9px] font-semibold text-muted-foreground py-0.5">{d}</div>
            ))}
          </div>
          <div className="grid grid-cols-7 gap-0.5">
            {Array.from({ length: rows * 7 }, (_, i) => {
              const col = i - leading + 1
              if (col < 1 || col > daysInMonth) return <div key={i} />
              const ds = `${cur.year}-${pad(cur.month+1)}-${pad(col)}`
              const isSelected = ds === value
              const isToday = ds === todayStr
              return (
                <button
                  key={ds}
                  type="button"
                  onClick={() => { onChange(ds); setOpen(false) }}
                  className={[
                    'text-[11px] w-full aspect-square flex items-center justify-center rounded-full transition-colors',
                    isSelected ? 'bg-primary text-primary-foreground font-bold' : isToday ? 'ring-1 ring-primary text-primary font-semibold hover:bg-accent/50' : 'hover:bg-accent/50 text-foreground',
                  ].join(' ')}
                >
                  {col}
                </button>
              )
            })}
          </div>
          <button type="button" onClick={() => { onChange(todayStr); setOpen(false) }} className="mt-2 w-full text-[11px] text-primary hover:underline text-center">Today</button>
        </div>
      )}
    </div>
  )
}

// ── Time Picker ────────────────────────────────────────────────────────────────

const TIME_SLOTS = Array.from({ length: 96 }, (_, i) => {
  const h = Math.floor(i / 4)
  const m = (i % 4) * 15
  return `${String(h).padStart(2,'0')}:${String(m).padStart(2,'0')}`
})

function TimePicker({ value, onChange }: { value: string; onChange: (t: string) => void }) {
  const [open, setOpen] = useState(false)
  const [editing, setEditing] = useState(false)
  const [draft, setDraft] = useState('')
  const ref = useRef<HTMLDivElement>(null)
  const listRef = useRef<HTMLDivElement>(null)
  const inputRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    if (!open) return
    const handler = (e: MouseEvent) => { if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false) }
    document.addEventListener('mousedown', handler)
    setTimeout(() => {
      const el = listRef.current?.querySelector('[data-selected="true"]') as HTMLElement
      el?.scrollIntoView({ block: 'center' })
    }, 0)
    return () => document.removeEventListener('mousedown', handler)
  }, [open])

  const enterEdit = () => { setEditing(true); setOpen(false); setDraft(value); setTimeout(() => inputRef.current?.select(), 0) }

  const commitEdit = () => {
    const m = draft.match(/^(\d{1,2}):(\d{2})$/)
    if (m) {
      const h = Math.min(23, parseInt(m[1]))
      const min = Math.min(59, parseInt(m[2]))
      onChange(`${String(h).padStart(2,'0')}:${String(min).padStart(2,'0')}`)
    }
    setEditing(false)
  }

  if (editing) {
    return (
      <div className="w-28">
        <input
          ref={inputRef}
          className="w-full bg-secondary border border-primary/50 rounded-md px-2 py-1.5 text-xs text-foreground outline-none ring-1 ring-primary/50"
          value={draft}
          onChange={e => setDraft(e.target.value)}
          onBlur={commitEdit}
          onKeyDown={e => { if (e.key === 'Enter') commitEdit(); if (e.key === 'Escape') setEditing(false) }}
          placeholder="HH:MM"
        />
      </div>
    )
  }

  return (
    <div ref={ref} className="relative w-36">
      <div className="flex items-center bg-secondary border border-border rounded-md overflow-hidden hover:bg-accent/30 transition-colors w-full">
        <button
          type="button"
          onDoubleClick={enterEdit}
          onClick={() => setOpen(v => !v)}
          className="flex-1 flex items-center gap-1.5 px-2 py-1.5 text-xs text-foreground text-left"
        >
          <Clock size={11} className="text-muted-foreground shrink-0" />
          {value || '--:--'}
        </button>
        <button type="button" onClick={enterEdit} className="px-1.5 py-1.5 text-muted-foreground hover:text-foreground transition-colors border-l border-border shrink-0">
          <Pencil size={10} />
        </button>
      </div>
      {open && (
        <div ref={listRef} className="absolute top-full mt-1 left-0 z-50 bg-popover border border-border rounded-xl shadow-2xl overflow-y-auto max-h-48 w-28 py-1">
          {TIME_SLOTS.map(t => (
            <button
              key={t}
              type="button"
              data-selected={t === value}
              onClick={() => { onChange(t); setOpen(false) }}
              className={[
                'w-full text-left px-3 py-1 text-xs transition-colors',
                t === value ? 'bg-primary text-primary-foreground font-medium' : 'text-foreground hover:bg-accent/50',
              ].join(' ')}
            >
              {t}
            </button>
          ))}
        </div>
      )}
    </div>
  )
}

// ── DateTime row (date + time side by side) ────────────────────────────────────

function DateTimeRow({
  label,
  dateValue,
  timeValue,
  allDay,
  onDateChange,
  onTimeChange,
}: {
  label: string
  dateValue: string
  timeValue: string
  allDay: boolean
  onDateChange: (d: string) => void
  onTimeChange: (t: string) => void
}) {
  return (
    <div className="flex flex-col gap-1">
      <span className="text-[10px] font-medium text-muted-foreground uppercase tracking-wider">{label}</span>
      <div className="flex items-center gap-1.5">
        <DatePicker value={dateValue} onChange={onDateChange} />
        {!allDay && <TimePicker value={timeValue} onChange={onTimeChange} />}
      </div>
    </div>
  )
}

// ── Event Form Modal ───────────────────────────────────────────────────────────

function EventForm({
  initial,
  defaultDate,
  onClose,
  onSaved,
}: {
  initial?: CalendarEvent
  defaultDate?: string
  onClose: () => void
  onSaved: (ev: CalendarEvent) => void
}) {
  const [form, setForm] = useState<EventFormState>(() => {
    if (initial) {
      const allDay = !initial.start.includes('T')
      const s = allDay ? { date: isoDate(initial.start), time: '09:00' } : splitDateTime(initial.start)
      const e = allDay ? { date: isoDate(initial.end), time: '10:00' } : splitDateTime(initial.end)
      return { summary: initial.summary, startDate: s.date, startTime: s.time, endDate: e.date, endTime: e.time, location: initial.location ?? '', description: initial.description ?? '', allDay }
    }
    const base = defaultDate ?? isoDate(new Date().toISOString())
    return { summary: '', startDate: base, startTime: '09:00', endDate: base, endTime: '10:00', location: '', description: '', allDay: false }
  })
  const [saving, setSaving] = useState(false)
  const [err, setErr] = useState<string | null>(null)

  const set = <K extends keyof EventFormState>(k: K, v: EventFormState[K]) => setForm(f => ({ ...f, [k]: v }))

  const save = async () => {
    if (!form.summary.trim()) { setErr('Title required'); return }
    setSaving(true); setErr(null)
    try {
      const startIso = form.allDay ? form.startDate : new Date(`${form.startDate}T${form.startTime}`).toISOString()
      const endIso   = form.allDay ? form.endDate   : new Date(`${form.endDate}T${form.endTime}`).toISOString()
      const args = { summary: form.summary.trim(), start: startIso, end: endIso, location: form.location || null, description: form.description || null, allDay: form.allDay }
      const saved: CalendarEvent = initial
        ? await invoke('update_calendar_event', { eventId: initial.id, ...args })
        : await invoke('create_calendar_event', args)
      onSaved(saved)
    } catch (e) {
      setErr(String(e))
    } finally {
      setSaving(false)
    }
  }

  const inputCls = 'w-full bg-secondary border border-border rounded-md px-2 py-1.5 text-xs text-foreground outline-none focus:ring-1 focus:ring-primary/50'

  return (
    <div className="absolute inset-0 z-20 flex items-center justify-center bg-black/40 backdrop-blur-sm" onClick={onClose}>
      <div
        className="bg-popover border border-border rounded-xl shadow-2xl w-80 p-4 flex flex-col gap-3"
        onClick={e => e.stopPropagation()}
      >
        <div className="flex items-center justify-between">
          <h3 className="text-sm font-semibold">{initial ? 'Edit Event' : 'New Event'}</h3>
          <Button variant="ghost" size="icon-xs" onClick={onClose}><X size={13} /></Button>
        </div>

        <input
          className={inputCls}
          placeholder="Title"
          value={form.summary}
          onChange={e => set('summary', e.target.value)}
          autoFocus
        />

        <div className="flex items-center justify-between">
          <span className="text-xs text-muted-foreground">All day</span>
          <Switch checked={form.allDay} onCheckedChange={v => set('allDay', v)} />
        </div>

        <DateTimeRow label="Start" dateValue={form.startDate} timeValue={form.startTime} allDay={form.allDay}
          onDateChange={d => set('startDate', d)} onTimeChange={t => set('startTime', t)} />
        <DateTimeRow label="End" dateValue={form.endDate} timeValue={form.endTime} allDay={form.allDay}
          onDateChange={d => set('endDate', d)} onTimeChange={t => set('endTime', t)} />

        <input
          className={inputCls}
          placeholder="Location (optional)"
          value={form.location}
          onChange={e => set('location', e.target.value)}
        />

        <textarea
          className={`${inputCls} resize-none h-16`}
          placeholder="Description (optional)"
          value={form.description}
          onChange={e => set('description', e.target.value)}
        />

        {err && <p className="text-xs text-red-400">{err}</p>}

        <div className="flex justify-end gap-2">
          <Button variant="ghost" size="sm" onClick={onClose}>Cancel</Button>
          <Button size="sm" onClick={save} disabled={saving}>{saving ? 'Saving…' : 'Save'}</Button>
        </div>
      </div>
    </div>
  )
}

// ── Title Bar Action (Add Event button) ────────────────────────────────────────

let _openAddEvent: (() => void) | null = null
export function CalendarTitleBarActions() {
  return (
    <Button
      size="sm"
      variant="secondary"
      className="gap-1.5 text-muted-foreground h-7 px-2 text-xs bg-[#2a2a2a] hover:bg-[#333] border-0"
      onClick={() => _openAddEvent?.()}
    >
      <Plus size={13} /> Add Event
    </Button>
  )
}

// ── Main Component ─────────────────────────────────────────────────────────────

// ponytail: module-scope cache so reopening the window shows the last
// fetched events instantly instead of a blank loading state.
let eventsCache: CalendarEvent[] | null = null

export function CalendarWindow() {
  const [events, setEvents] = useState<CalendarEvent[]>(eventsCache ?? [])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [cursor, setCursor] = useState(() => {
    const now = new Date()
    return { year: now.getFullYear(), month: now.getMonth() }
  })
  const [detail, setDetail] = useState<CalendarEvent | null>(null)
  const [editEvent, setEditEvent] = useState<CalendarEvent | null | 'new'>(null)
  const [newDefaultDate, setNewDefaultDate] = useState<string | undefined>()

  // Wire up title bar button
  useEffect(() => {
    _openAddEvent = () => { setNewDefaultDate(undefined); setEditEvent('new') }
    return () => { _openAddEvent = null }
  }, [])

  const load = async () => {
    if (!eventsCache) setLoading(true)
    setError(null)
    try {
      const now = new Date()
      const daysBehind = Math.ceil((now.getTime() - new Date(now.getFullYear(), now.getMonth() - 1, 1).getTime()) / 86400000)
      const list = await invoke<CalendarEvent[]>('fetch_calendar_events', { daysAhead: 120, daysBehind, maxResults: 200 })
      setEvents(list)
      eventsCache = list
    } catch (e) {
      setError(String(e))
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => { load() }, [])

  const { year, month } = cursor

  const firstDayJs = new Date(year, month, 1).getDay()
  const leadingBlanks = mondayFirst(firstDayJs)
  const daysInMonth = new Date(year, month + 1, 0).getDate()
  const prevMonthDays = new Date(year, month, 0).getDate()

  const today = new Date()
  const todayStr = `${today.getFullYear()}-${String(today.getMonth() + 1).padStart(2, '0')}-${String(today.getDate()).padStart(2, '0')}`

  const byDay: Record<string, CalendarEvent[]> = {}
  for (const ev of events) {
    const d = isoDate(ev.start)
    byDay[d] = byDay[d] ?? []
    byDay[d].push(ev)
  }

  function cellDate(col: number): { dateStr: string; day: number; isCurrentMonth: boolean } {
    if (col < leadingBlanks) {
      const day = prevMonthDays - leadingBlanks + col + 1
      const m = month === 0 ? 12 : month
      const y = month === 0 ? year - 1 : year
      return { dateStr: `${y}-${String(m).padStart(2, '0')}-${String(day).padStart(2, '0')}`, day, isCurrentMonth: false }
    }
    const d = col - leadingBlanks + 1
    if (d <= daysInMonth) {
      return { dateStr: `${year}-${String(month + 1).padStart(2, '0')}-${String(d).padStart(2, '0')}`, day: d, isCurrentMonth: true }
    }
    const overflow = d - daysInMonth
    const m = month === 11 ? 1 : month + 2
    const y = month === 11 ? year + 1 : year
    return { dateStr: `${y}-${String(m).padStart(2, '0')}-${String(overflow).padStart(2, '0')}`, day: overflow, isCurrentMonth: false }
  }

  const totalCells = leadingBlanks + daysInMonth
  const rows = Math.ceil(totalCells / 7)

  const prevMonth = () => setCursor(c => ({ year: c.month === 0 ? c.year - 1 : c.year, month: c.month === 0 ? 11 : c.month - 1 }))
  const nextMonth = () => setCursor(c => ({ year: c.month === 11 ? c.year + 1 : c.year, month: c.month === 11 ? 0 : c.month + 1 }))

  const handleSaved = (saved: CalendarEvent) => {
    setEvents(evs => {
      const idx = evs.findIndex(e => e.id === saved.id)
      if (idx >= 0) { const next = [...evs]; next[idx] = saved; return next }
      return [...evs, saved].sort((a, b) => a.start.localeCompare(b.start))
    })
    setEditEvent(null)
    setDetail(saved)
  }

  const MAX_VISIBLE = 3

  return (
    <div className="relative flex flex-col h-full select-none">
      {/* Header */}
      <div className="flex items-center justify-between px-3 py-2 border-b border-border shrink-0">
        <div className="flex items-center gap-1">
          <Button variant="ghost" size="icon-xs" onClick={prevMonth}><ChevronLeft size={13} /></Button>
          <span className="text-xs font-semibold w-36 text-center">
            {new Date(year, month).toLocaleDateString(undefined, { month: 'long', year: 'numeric' })}
          </span>
          <Button variant="ghost" size="icon-xs" onClick={nextMonth}><ChevronRight size={13} /></Button>
        </div>
        <Button variant="ghost" size="icon-xs" onClick={load} disabled={loading} title="Refresh">
          <RefreshCw size={13} className={loading ? 'animate-spin' : ''} />
        </Button>
      </div>

      {error && (
        <div className="mx-3 mt-2 px-3 py-1.5 rounded-lg bg-red-950/30 border border-red-800/40 text-red-400 text-xs shrink-0">
          {error.includes('No calendar account') ? (
            <span>No calendar account connected. Add one in <strong>Accounts</strong>.</span>
          ) : error}
        </div>
      )}

      {/* Grid */}
      <div className="flex-1 flex flex-col min-h-0 overflow-hidden">
        <div className="grid grid-cols-7 border-b border-border shrink-0">
          {DOW_MON.map(d => (
            <div key={d} className="text-center text-[10px] font-semibold uppercase tracking-wider text-muted-foreground py-1.5">
              {d}
            </div>
          ))}
        </div>

        <div className="flex-1 grid min-h-0" style={{ gridTemplateRows: `repeat(${rows}, 1fr)` }}>
          {Array.from({ length: rows }, (_, row) => (
            <div key={row} className="grid grid-cols-7 border-b border-border last:border-b-0 min-h-0">
              {Array.from({ length: 7 }, (_, col) => {
                const idx = row * 7 + col
                const { dateStr, day, isCurrentMonth } = cellDate(idx)
                const dayEvents = byDay[dateStr] ?? []
                const isToday = dateStr === todayStr
                const visible = dayEvents.slice(0, MAX_VISIBLE)
                const overflow = dayEvents.length - MAX_VISIBLE

                return (
                  <div
                    key={dateStr}
                    className={['border-r border-border last:border-r-0 p-1 overflow-hidden flex flex-col gap-0.5 min-h-0 cursor-pointer hover:bg-accent/10 transition-colors', isCurrentMonth ? '' : 'opacity-40'].join(' ')}
                    onClick={() => { setNewDefaultDate(dateStr); setEditEvent('new') }}
                  >
                    <div className="flex items-center mb-0.5">
                      <span className={['text-[11px] font-medium w-5 h-5 flex items-center justify-center rounded-full', isToday ? 'bg-primary text-primary-foreground font-bold' : 'text-foreground'].join(' ')}>
                        {day}
                      </span>
                    </div>

                    {visible.map(ev => {
                      const allDay = !ev.start.includes('T')
                      const dot = eventColor(ev)
                      return (
                        <div
                          key={ev.id}
                          title={ev.summary}
                          className="flex items-center gap-1 px-1 py-0.5 rounded text-[10px] leading-tight bg-accent/30 hover:bg-accent/70 transition-colors cursor-pointer truncate"
                          onClick={e => { e.stopPropagation(); setDetail(ev) }}
                        >
                          <span className="w-1.5 h-1.5 rounded-full shrink-0" style={{ backgroundColor: dot }} />
                          <span className="truncate text-foreground">
                            {!allDay && <span className="text-muted-foreground mr-0.5">{formatTime(ev.start)}</span>}
                            {ev.summary}
                          </span>
                        </div>
                      )
                    })}

                    {overflow > 0 && (
                      <div className="text-[10px] text-muted-foreground px-1">+{overflow} more</div>
                    )}
                  </div>
                )
              })}
            </div>
          ))}
        </div>
      </div>

      {/* Modals */}
      {detail && !editEvent && (
        <EventDetail
          event={detail}
          onClose={() => setDetail(null)}
          onEdit={() => { setEditEvent(detail); setDetail(null) }}
        />
      )}
      {editEvent && (
        <EventForm
          initial={editEvent === 'new' ? undefined : editEvent}
          defaultDate={newDefaultDate}
          onClose={() => setEditEvent(null)}
          onSaved={handleSaved}
        />
      )}
    </div>
  )
}
