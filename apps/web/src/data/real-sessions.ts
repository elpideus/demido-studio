/* Sessions, for real: the conversations the core keeps, and saying something in one.

   The core runs every turn. This store hands over what was written, and then follows the core's news: the
   answer's words as they arrive (`message.delta`, told several times a second), and its end. Because it
   follows the news and not its own requests, an answer started somewhere else (the command line, another
   window) appears here too, as it is written. */
import { create } from 'zustand'
import { coreGet, coreSend, onCoreEvent } from './core'
import { toast } from './store'

export interface CoreTimings { reused: number; read: number; written: number; per_second: number; took_ms: number }
export interface CoreMessage { id: string; session_id: string; seq: number; role: 'user' | 'assistant'; content: string; thought: string | null; state: 'done' | 'answering' | 'stopped' | 'failed'; problem: string | null; model: string | null; timings: CoreTimings | null; created_at: number }
export interface CoreSession { id: string; title: string; model: string; created_at: number; updated_at: number; messages: number }
export interface SessionDetail { session: CoreSession; messages: CoreMessage[]; instructions: string; answering: boolean }

interface Real {
  /** `null` until the core has answered. */
  list: CoreSession[] | null
  /** The sessions that were opened, with everything said in them. */
  open: Record<string, SessionDetail>
  /** The answers being written right now, by message id: the words that have arrived so far. */
  writing: Record<string, { text: string; thought: string }>
  error: string | null
}

export const useReal = create<Real>(() => ({ list: null, open: {}, writing: {}, error: null }))
const set = useReal.setState
const get = useReal.getState

export async function readSessions() {
  try {
    set({ list: await coreGet<CoreSession[]>('/sessions'), error: null })
  } catch (e) {
    set({ error: (e as Error).message })
  }
}

export async function readSession(id: string) {
  try {
    const detail = await coreGet<SessionDetail>(`/sessions/${id}`)
    // Words that arrived for a message that is stored whole by now are no longer needed.
    const writing = { ...get().writing }
    for (const m of detail.messages) if (m.state !== 'answering') delete writing[m.id]
    set({ open: { ...get().open, [id]: detail }, writing })
  } catch (e) {
    // It was deleted, here or somewhere else.
    const open = { ...get().open }
    delete open[id]
    set({ open })
    if (!/no session/i.test((e as Error).message)) toast((e as Error).message, undefined, 'task-failed')
  }
}

/** Starts a session and returns its id, or null with the reason shown. */
export async function newSession(model?: string): Promise<string | null> {
  try {
    const made = await coreSend<CoreSession>('POST', '/sessions', model ? { model } : {})
    await readSessions()
    return made.id
  } catch (e) {
    toast(`No session was started. ${(e as Error).message}`, undefined, 'task-failed')
    return null
  }
}

/** Says something in a session. The answer follows as news. Returns false, with the reason shown, when it was refused. */
export async function say(id: string, content: string): Promise<boolean> {
  try {
    const turn = await coreSend<{ said: CoreMessage; answer: CoreMessage }>('POST', `/sessions/${id}/messages`, { content })
    const at = get().open[id]
    // Shown at once, without waiting for the news to come round.
    if (at && !at.messages.some((m) => m.id === turn.said.id)) set({ open: { ...get().open, [id]: { ...at, answering: true, messages: [...at.messages, turn.said, turn.answer] } } })
    return true
  } catch (e) {
    toast(`Not sent. ${(e as Error).message}`, undefined, 'task-failed')
    return false
  }
}

export async function stopAnswer(id: string) {
  await coreSend('POST', `/sessions/${id}/stop`).catch((e: Error) => toast(e.message, undefined, 'task-failed'))
}

export async function changeSession(id: string, change: { title?: string; model?: string }) {
  try {
    await coreSend('PATCH', `/sessions/${id}`, change)
    await Promise.all([readSessions(), readSession(id)])
  } catch (e) {
    toast(`Not changed. ${(e as Error).message}`, undefined, 'task-failed')
  }
}

export async function deleteSession(id: string) {
  try {
    await coreSend('DELETE', `/sessions/${id}`)
    await readSessions()
  } catch (e) {
    toast(`Not deleted. ${(e as Error).message}`, undefined, 'task-failed')
  }
}

let started = false
/** Called once, when a core is found. */
export function startSessions() {
  if (started) return
  started = true
  void readSessions()
  for (const kind of ['session.created', 'session.deleted', 'session.changed']) onCoreEvent(kind, () => void readSessions())
  onCoreEvent('message.delta', (e) => {
    const id = String(e.data.message)
    const so_far = get().writing[id] ?? { text: '', thought: '' }
    set({ writing: { ...get().writing, [id]: { text: so_far.text + String(e.data.text ?? ''), thought: so_far.thought + String(e.data.thought ?? '') } } })
  })
  // Something was said, or an answer ended: the stored conversation is the truth again.
  for (const kind of ['message.added', 'message.done', 'message.stopped', 'message.failed'])
    onCoreEvent(kind, (e) => {
      void readSessions()
      if (e.subject && get().open[e.subject]) void readSession(e.subject)
    })
}
