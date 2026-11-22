/* Sessions, for real: the conversations the core keeps, and saying something in one.

   The core runs every turn. This store hands over what was written, and then follows the core's news: the
   answer's words as they arrive (`message.delta`, told several times a second), and its end. Because it
   follows the news and not its own requests, an answer started somewhere else (the command line, another
   window) appears here too, as it is written. */
import { create } from 'zustand'
import { coreGet, coreSend, onCoreEvent } from './core'
import { toast } from './store'

/** `cost_usd`: what the answer cost at a provider, with everything asked for it; absent for a model on this computer. `cost_exact`: the provider's own figure, or Demido's count from tokens and prices. */
export interface CoreTimings { reused: number; read: number; written: number; per_second: number; took_ms: number; cost_usd?: number; cost_exact?: boolean }
/** One use of a tool. `waiting`: for the person's yes. `refused`: the request could not be used, or the person said no; nothing ran.
    `change`: the id of the change it made to a file, for undoing it. */
export interface CoreStep { tool: string; args: Record<string, unknown>; result: string; state: 'running' | 'waiting' | 'done' | 'failed' | 'refused'; took_ms: number; change?: string | null; output?: string | null }
export type Level = 'cautious' | 'balanced' | 'smart' | 'autonomous'
/** A use of a tool that is waiting for the person's yes. Every word of it is made by the core from the request, none by the model. */
export interface PermissionRequest { id: string; session_id: string; message_id: string; tool: string; args: Record<string, unknown>; reach: 'nothing' | 'reads' | 'changes' | 'runs' | 'network'; what: string; why: string; once_only?: boolean; asked_at: number }
/** A change a tool made to a file in the session's folder. `created`: the change made the file, so undoing removes it.
    `whole_folder`: it is everything a program did; the folder was saved before the program ran, and undoing puts the whole folder back. */
export interface CoreChange { id: string; session_id: string; message_id: string; path: string; created: boolean; whole_folder?: boolean; undone_at: number | null; created_at: number; diff_stat?: string | null }
/** The sandbox the core's computer has for the programs a model runs. `kind` is `none` when it has none, and `why_not` says why. */
export interface Sandbox { kind: string; program: string | null; why_not: string | null }
/** One task of work that was split into steps. `parent_id` is set for a sub-task. */
export interface CoreTask { id: string; session_id: string; message_id: string; parent_id: string | null; position: number; title: string; state: 'pending' | 'running' | 'done' | 'failed' | 'stopped'; result: string | null; steps: CoreStep[]; created_at: number; updated_at: number }
export interface CoreMessage { id: string; session_id: string; seq: number; role: 'user' | 'assistant'; content: string; thought: string | null; state: 'done' | 'answering' | 'stopped' | 'failed'; problem: string | null; model: string | null; timings: CoreTimings | null; steps: CoreStep[]; created_at: number }
/** `thinking`: off, on, or one of the model's own levels. */
/** `account`: the account at its model's provider it goes through; null or missing, the provider's default. */
export interface CoreSession { id: string; title: string; model: string; thinking: string; folder?: string | null; permission?: Level; account?: string | null; created_at: number; updated_at: number; messages: number }
/** An account of the older part of a session. The model is given it in place of the messages up to `upto_seq`. */
export interface CoreSummary { id: string; session_id: string; upto_seq: number; content: string; why: 'asked' | 'auto'; tokens_before: number; created_at: number }
/** How much of what the model can hold a session is using, in tokens. */
export interface ContextUse { used: number; limit: number; compact_at: number; auto: boolean }
/** `thinking_choices`: what thinking can be set to besides off. Empty: the model does not think. null: not known until it has been loaded once. */
/** `tasks` and `tools_told` are missing when the core is older than this window: a window may be newer than the server it talks to. */
export interface SessionDetail { session: CoreSession; messages: CoreMessage[]; instructions: string; answering: boolean; thinking_choices: string[] | null; summaries: CoreSummary[]; context: ContextUse; tasks?: CoreTask[]; tools_told?: string | null; asking?: PermissionRequest | null; need_not_ask?: string[]; changes?: CoreChange[]; sandbox?: Sandbox }

interface Real {
  /** `null` until the core has answered. */
  list: CoreSession[] | null
  /** The sessions that were opened, with everything said in them. */
  open: Record<string, SessionDetail>
  /** The answers being written right now, by message id: the words that have arrived so far. */
  writing: Record<string, { text: string; thought: string }>
  /** What the answers being written have done so far, by message id: the tools used, the tasks and how they stand. */
  working: Record<string, { steps?: CoreStep[]; tasks?: CoreTask[] }>
  /** The sessions whose older part is being written up right now. */
  shortening: Record<string, boolean>
  error: string | null
}

export const useReal = create<Real>(() => ({ list: null, open: {}, writing: {}, working: {}, shortening: {}, error: null }))
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
    const [writing, working] = [{ ...get().writing }, { ...get().working }]
    for (const m of detail.messages) if (m.state !== 'answering') { delete writing[m.id]; delete working[m.id] }
    set({ open: { ...get().open, [id]: detail }, writing, working })
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

/** Says something in a session. The answer follows as news. Returns false, with the reason shown, when it was refused.
    `plan`: the model first lists the steps of the request, and then does them one at a time, as tasks. */
export async function say(id: string, content: string, plan = false): Promise<boolean> {
  try {
    const turn = await coreSend<{ said: CoreMessage; answer: CoreMessage }>('POST', `/sessions/${id}/messages`, { content, plan })
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

/** Makes a session shorter for the model: the older part is replaced by a short account. The messages stay. */
export async function makeShorter(id: string) {
  set({ shortening: { ...get().shortening, [id]: true } })
  try {
    await coreSend('POST', `/sessions/${id}/compact`)
  } catch (e) {
    toast(`Not made shorter. ${(e as Error).message}`, undefined, 'task-failed')
  }
  const shortening = { ...get().shortening }
  delete shortening[id]
  set({ shortening })
  await readSession(id)
}

/** Ends the thought the model is in, so that it goes on to its answer. */
export async function answerNow(id: string) {
  await coreSend('POST', `/sessions/${id}/answer-now`).catch((e: Error) => toast(e.message, undefined, 'task-failed'))
}

/** The person's answer to a use of a tool that is waiting: yes this once, yes for the rest of the session, or no. */
export async function answerPermission(id: string, request: string, said: 'once' | 'session' | 'no') {
  try {
    await coreSend('POST', `/sessions/${id}/permissions/${request}`, { said })
  } catch (e) {
    toast((e as Error).message, undefined, 'task-failed')
  }
  await readSession(id)
}

/** Undoes a change a tool made to a file: what the file held before is put back. */
/** What a change changed, as a unified diff. Throws with the core's own words when it is not kept. */
export async function changeDiff(id: string, change: string): Promise<string> {
  const res = await fetch(`/api/v1/sessions/${id}/changes/${change}/diff`)
  if (!res.ok) throw new Error((await res.json().catch(() => null))?.error?.message ?? `The core answered ${res.status}.`)
  return res.text()
}

export async function undoChange(id: string, change: string) {
  try {
    const undone = await coreSend<CoreChange>('POST', `/sessions/${id}/changes/${change}/undo`)
    toast(undone.whole_folder ? 'Put the folder back as it was before that command.' : undone.created ? `Removed ${undone.path}: the change had created it.` : `Put ${undone.path} back as it was.`)
  } catch (e) {
    toast(`Not undone. ${(e as Error).message}`, undefined, 'task-failed')
  }
  await readSession(id)
}

/** `folder`: a full path on the computer the core runs on; an empty text takes the folder away. `account`: an
    account's id at the model's provider; an empty text, the provider's default. */
export async function changeSession(id: string, change: { title?: string; model?: string; thinking?: string; folder?: string; permission?: Level; account?: string }) {
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
  // What an answer is doing before it is written: the tools it uses, and its tasks as they are done.
  onCoreEvent('message.steps', (e) => {
    const id = String(e.data.message)
    set({ working: { ...get().working, [id]: { ...get().working[id], steps: e.data.steps as CoreStep[] } } })
  })
  onCoreEvent('message.tasks', (e) => {
    const id = String(e.data.message)
    set({ working: { ...get().working, [id]: { ...get().working[id], tasks: e.data.tasks as CoreTask[] } } })
  })
  // A use of a tool is waiting for the person, or was answered; a change was undone.
  onCoreEvent('permission.asked', (e) => {
    const at = e.subject ? get().open[e.subject] : undefined
    if (e.subject && at) set({ open: { ...get().open, [e.subject]: { ...at, asking: e.data as unknown as PermissionRequest } } })
  })
  for (const kind of ['permission.answered', 'change.undone']) onCoreEvent(kind, (e) => { if (e.subject && get().open[e.subject]) void readSession(e.subject) })
  // Something was said, or an answer ended: the stored conversation is the truth again.
  // The older part of a session is being written up (by itself, before an answer, or because someone asked).
  onCoreEvent('session.compacting', (e) => { if (e.subject) set({ shortening: { ...get().shortening, [e.subject]: true } }) })
  onCoreEvent('session.compacted', (e) => {
    if (!e.subject) return
    const shortening = { ...get().shortening }
    delete shortening[e.subject]
    set({ shortening })
    if (get().open[e.subject]) void readSession(e.subject)
  })
  for (const kind of ['message.added', 'message.done', 'message.stopped', 'message.failed'])
    onCoreEvent(kind, (e) => {
      void readSessions()
      if (e.subject && get().open[e.subject]) void readSession(e.subject)
    })
}
