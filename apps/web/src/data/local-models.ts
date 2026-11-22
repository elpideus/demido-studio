/* The models on this computer, for real: what the core lists, and loading and unloading them.

   Read once when a core is found, and again only when the core says something changed (a model was loaded
   or unloaded, the runtime started or stopped) or the window gets the focus back. Nothing is asked on a
   timer: the countdown to "unloaded for being idle" is worked out here from the last answer. */
import { create } from 'zustand'
import { coreGet, coreSend, onCoreEvent } from './core'
import { toast } from './store'

export interface LocalModel { id: string; file: string; size: number; state: 'unloaded' | 'loading' | 'loaded'; context_length: number | null; needs_mb: number | null; idle_for_s: number | null; unloads_in_s: number | null; loads_in_ms: number | null; tokens_per_second: number | null; thinking: string[] | null; takes: string[] | null }
export interface EngineStatus { running: boolean; runtime: string | null; context: number; margin_mb: number; idle_unload_min: number | null }
export interface ModelList { folder: string; models: LocalModel[]; engine: EngineStatus }

interface Local {
  /** `null` until the core has answered. */
  list: ModelList | null
  /** When `list` was read, in milliseconds. The countdowns in it are counted from here. */
  readAt: number
  error: string | null
  /** Models this window asked to load or unload and is waiting for. */
  waiting: Record<string, 'loading' | 'unloading'>
  /** How far a load has come, 0 to 1, as the runtime tells it. */
  progress: Record<string, number>
}

export const useLocal = create<Local>(() => ({ list: null, readAt: 0, error: null, waiting: {}, progress: {} }))
const set = useLocal.setState
const get = useLocal.getState

let reading: Promise<void> | null = null
let again = false
/** Asks the core for the list. Several askers in the same moment share one answer; one who asks while an
    answer is on its way gets one more, read after theirs, so that news that came meanwhile is not missed. */
export function readLocal(): Promise<void> {
  if (reading) {
    again = true
    return reading
  }
  reading = (async () => {
    do {
      again = false
      try {
        set({ list: await coreGet<ModelList>('/models'), readAt: Date.now(), error: null })
      } catch (e) {
        set({ error: (e as Error).message })
      }
    } while (again)
  })().finally(() => { reading = null })
  return reading
}

function waitFor(id: string, what: 'loading' | 'unloading' | null) {
  const waiting = { ...get().waiting }
  if (what) waiting[id] = what
  else delete waiting[id]
  set({ waiting })
}

export async function loadLocal(id: string) {
  waitFor(id, 'loading')
  try {
    await coreSend('POST', `/models/${encodeURIComponent(id)}/load`)
  } catch (e) {
    toast(`${id} was not loaded. ${(e as Error).message}`, undefined, 'model-error')
  }
  waitFor(id, null)
  set({ progress: { ...get().progress, [id]: 0 } })
  await readLocal()
}

export async function unloadLocal(id: string) {
  waitFor(id, 'unloading')
  try {
    await coreSend('POST', `/models/${encodeURIComponent(id)}/unload`)
  } catch (e) {
    toast(`${id} was not unloaded. ${(e as Error).message}`, undefined, 'model-error')
  }
  waitFor(id, null)
  await readLocal()
}

/** Deletes a model from the models folder; a loaded one is unloaded first. True when it is gone. */
export async function deleteLocal(id: string): Promise<boolean> {
  try {
    const gone = await coreSend<LocalModel>('DELETE', `/models/${encodeURIComponent(id)}`)
    toast(`${gone.file} is deleted.`, undefined, 'task-done')
    await readLocal()
    return true
  } catch (e) {
    toast(`${id} was not deleted. ${(e as Error).message}`, undefined, 'task-failed')
    return false
  }
}

let started = false
/** Called once, when a core is found: reads the list and keeps it right from the core's news. */
export function startLocal() {
  if (started) return
  started = true
  void readLocal()
  for (const kind of ['model.loaded', 'model.unloaded', 'model.deleted', 'model.downloaded', 'engine.started', 'engine.stopped', 'model.measured']) onCoreEvent(kind, () => void readLocal())
  onCoreEvent('model.loading', (e) => {
    const value = Number(e.data.progress)
    if (e.subject && Number.isFinite(value)) set({ progress: { ...get().progress, [e.subject]: value } })
  })
  onCoreEvent('model.unloaded', (e) => {
    if (e.data.crashed) toast(`${e.subject} stopped by itself. Its memory is free again.`, undefined, 'model-error')
  })
  document.addEventListener('visibilitychange', () => { if (!document.hidden) void readLocal() })
}

/** Seconds until a loaded model is unloaded for being idle, counted down from the last answer. `null`: never, or not loaded. */
export function unloadsIn(m: LocalModel, readAt: number, now: number): number | null {
  // `now` may be a clock that ticks every few seconds and so lags behind `readAt`: never count backwards.
  return m.unloads_in_s === null ? null : Math.max(0, m.unloads_in_s - Math.round(Math.max(0, now - readAt) / 1000))
}

/** How a model picks each word. Each is missing where nothing is said. */
export interface Sampling { temperature?: number; top_p?: number; top_k?: number; min_p?: number; presence_penalty?: number; repeat_penalty?: number }
/** One way of answering: what Demido sends (empty: nothing), what is in effect in the end, and why. */
export interface Chosen { sent: Sampling; in_effect: Sampling; from: 'yours' | 'maker' | 'file' | 'runtime' }
export interface MakerSays { who: string; source: string; read_on: string | null; how: 'card' | 'generation_config'; thinking: Sampling; plain: Sampling }
export interface ModelSettings { model: string; thinking: Chosen; plain: Chosen; maker: MakerSays | null; yours: { thinking: Sampling; plain: Sampling; changed_at: number }; in_file: Sampling; runtime: Sampling }

/** A model's settings, thinking and not. Throws with the core's words. */
export function readModelSettings(id: string): Promise<ModelSettings> {
  return coreGet<ModelSettings>(`/models/${encodeURIComponent(id)}/settings`)
}

/** Keeps the person's own settings for a model; both empty, it follows its maker again. The settings now, or null with the reason shown. */
export async function saveModelSettings(id: string, thinking: Sampling, plain: Sampling): Promise<ModelSettings | null> {
  try {
    const now = await coreSend<ModelSettings>('PUT', `/models/${encodeURIComponent(id)}/settings`, { thinking, plain })
    toast(`Kept: ${id} is asked with your settings from its next answer.`, undefined, 'task-done')
    return now
  } catch (e) {
    toast(`Not kept. ${(e as Error).message}`, undefined, 'task-failed')
    return null
  }
}

export async function resetModelSettings(id: string): Promise<ModelSettings | null> {
  try {
    const now = await coreSend<ModelSettings>('DELETE', `/models/${encodeURIComponent(id)}/settings`)
    toast(`${id} follows its maker's settings again.`, undefined, 'task-done')
    return now
  } catch (e) {
    toast(`Not changed. ${(e as Error).message}`, undefined, 'task-failed')
    return null
  }
}
