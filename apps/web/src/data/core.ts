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

/** Asks the core. Throws with the core's own words when it refuses. */
export async function coreGet<T>(path: string): Promise<T> {
  const res = await fetch(`/api/v1${path}`, { headers: { accept: 'application/json' } })
  const body = await res.json().catch(() => null)
  if (!res.ok) throw new Error(body?.error?.message ?? `The core answered ${res.status}.`)
  return body as T
}

/** `none`: these pages are not served by a core (the preview). `locked`: a core is here but this browser is not signed in. */
export type CoreState = { state: 'looking' | 'none' } | { state: 'locked' | 'ready'; version: string }

let found: Promise<CoreState> | null = null
function look(): Promise<CoreState> {
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
    coreGet<T>(path).then((d) => { if (alive) { setData(d); setError(null) } }, (e: Error) => { if (alive) setError(e.message) })
    return () => { alive = false }
  }, [path, ready, turn])
  return { data, error, again: () => setTurn((n) => n + 1) }
}
