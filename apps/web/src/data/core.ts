/* The real core, when there is one.

   The preview runs without it: every screen has scripted data. When these same pages are served by
   `demido serve`, they can ask the core instead. This file is the one place that talks to it. Screens move
   from scripted to real one at a time, and each says which it is showing.

   The API is described by the core itself at /api/v1/openapi.json. */
import { useEffect, useState } from 'react'

export interface CoreGpu { vendor: string; name: string; vram_gb: number | null; vram_used_gb: number | null; shares_memory: boolean; driver: string | null; cuda: string | null; found_by: string }
export interface CoreBuild { id: string; name: string; file: string; why: string }
export interface CoreSystem {
  os: { name: string; version: string; kernel: string; arch: string; family: string }
  cpu: { model: string; threads: number; cores: number | null; features: string[] }
  memory: { total_gb: number; available_gb: number }
  gpus: CoreGpu[]
  runtime: { runtime: string; source: string; recommended: CoreBuild; others: CoreBuild[] }
}

/** How a setting is drawn and what it takes. The core declares it (core/crates/demido-core/src/settings.rs). */
export type CoreKind =
  | { type: 'switch' }
  | { type: 'select'; options: { value: string; label: string }[] }
  | { type: 'slider'; min: number; max: number; step: number; unit: string }
  | { type: 'text'; secret: boolean; code: boolean }
  | { type: 'colour' }
export interface CoreSettingDef { id: string; section: string; label: string; desc: string; details: string | null; kind: CoreKind; default: unknown; built: boolean }
export interface CoreSetting { def: CoreSettingDef; value: unknown; changed: boolean }
export interface CoreSection { id: string; title: string; lead: string | null }
export interface CoreSettings { sections: CoreSection[]; settings: CoreSetting[] }
/** A file as its publisher lists it. `sha256` is null when the publisher lists none. */
export interface CoreRemoteFile { name: string; url: string; size: number; sha256: string | null }
/** A model runtime on this computer. `version` is what it said when tried; `problem` is why it does not start. */
export interface CoreInstalled { id: string; runtime: string; build: string; backend: string; name: string; files: CoreRemoteFile[]; checked: boolean; server: string | null; version: string | null; problem: string | null; devices: string[]; caution: string | null; installed_at: number; size: number }
/** An installation under way. `step`: looking, downloading, unpacking, trying. Bytes over all files. */
export interface CoreProgress { id: string; name: string; step: string; file: string | null; done: number; total: number }
export interface CoreRuntimeStatus { runtime: string; folder: string; installed: CoreInstalled[]; installing: CoreProgress | null }
/** One build of the runtime that suits this computer. `extra`: files it may also need (NVIDIA's libraries). */
export interface CoreOffer { id: string; name: string; why: string; recommended: boolean; file: CoreRemoteFile; extra: CoreRemoteFile[]; size: number; extra_size: number; needs_extra: boolean }
/** `stale`: null normally. When the publisher could not be reached, why; the rest is then its last answer, as old as `asked_at` says. */
export interface CoreAvailable { runtime: string; release: string; build: string; offers: CoreOffer[]; asked_at: number; stale: string | null }
/** The desktop's look, as the core reads it on its own computer. `supported` is false where reading it is not built yet. */
export interface CoreAppearance { accent: string | null; source: string | null; supported: boolean }
/** One line of the core's log of what happened. `seq` is its number; `subject` is what it is about. */
export interface CoreEvent { seq: number; id: string; at: number; kind: string; subject: string | null; data: Record<string, unknown> }

async function ask<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await fetch(`/api/v1${path}`, {
    method,
    headers: body === undefined ? { accept: 'application/json' } : { accept: 'application/json', 'content-type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  })
  const answer = await res.json().catch(() => null)
  if (!res.ok) throw new Error(answer?.error?.message ?? `The core answered ${res.status}.`)
  return answer as T
}

/** Asks the core. Throws with the core's own words when it refuses. */
export function coreGet<T>(path: string): Promise<T> {
  return ask<T>('GET', path)
}

/** Tells the core to change something. Throws with the core's own words when it refuses. */
export function coreSend<T>(method: 'PUT' | 'POST' | 'PATCH' | 'DELETE', path: string, body?: unknown): Promise<T> {
  return ask<T>(method, path, body)
}

/* The core's log, live. One connection for the whole page, opened when the first listener arrives. The browser
   reconnects by itself and tells the core the last number it saw, so nothing is missed in between. */
let stream: EventSource | null = null
const heard = new Map<string, Set<(e: CoreEvent) => void>>()

/** Calls `listener` for every event of this kind, for example "setting.changed". Returns how to stop. */
export function onCoreEvent(kind: string, listener: (e: CoreEvent) => void): () => void {
  stream ??= new EventSource('/api/v1/events/stream')
  let listeners = heard.get(kind)
  if (!listeners) {
    listeners = new Set()
    heard.set(kind, listeners)
    const all = listeners
    stream.addEventListener(kind, (message) => {
      let event: CoreEvent
      try {
        event = JSON.parse((message as MessageEvent<string>).data)
      } catch {
        return
      }
      all.forEach((l) => l(event))
    })
  }
  listeners.add(listener)
  return () => { listeners.delete(listener) }
}

/** `none`: these pages are not served by a core (the preview). `locked`: a core is here but this browser is not signed in. */
export type CoreState = { state: 'looking' | 'none' } | { state: 'locked' | 'ready'; version: string }

let found: Promise<CoreState> | null = null
/** Looks for a core behind these pages. Asked once per visit; later calls get the same answer. */
export function look(): Promise<CoreState> {
  found ??= (async (): Promise<CoreState> => {
    try {
      const health = await fetch('/api/v1/health', { headers: { accept: 'application/json' }, signal: AbortSignal.timeout(1500) })
      const body = await health.json()
      if (!health.ok || body?.status !== 'ok') return { state: 'none' }
      // Health needs no key. Whether this browser is let in shows on the first thing that does.
      const inside = await fetch('/api/v1/settings/server.address', { headers: { accept: 'application/json' } })
      return { state: inside.ok ? 'ready' : 'locked', version: String(body.version) }
    } catch {
      return { state: 'none' } // the dev server answers with a page, not JSON
    }
  })()
  return found
}

/** Whether a core is behind these pages. Looked up once per visit. */
export function useCore(): CoreState {
  const [core, setCore] = useState<CoreState>({ state: 'looking' })
  useEffect(() => {
    let alive = true
    void look().then((c) => { if (alive) setCore(c) })
    return () => { alive = false }
  }, [])
  return core
}

/** One thing read from the core, once it is known to be there. */
export function useCoreData<T>(path: string, ready: boolean): { data: T | null; error: string | null; again: () => void } {
  const [data, setData] = useState<T | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [turn, setTurn] = useState(0)
  useEffect(() => {
    if (!ready) return
    let alive = true
    // The core keeps some answers for a few seconds. "Again" is a person asking to look now, so it says so.
    const asked = turn > 0 ? `${path}${path.includes('?') ? '&' : '?'}fresh=true` : path
    coreGet<T>(asked).then((d) => { if (alive) { setData(d); setError(null) } }, (e: Error) => { if (alive) setError(e.message) })
    return () => { alive = false }
  }, [path, ready, turn])
  return { data, error, again: () => setTurn((n) => n + 1) }
}
