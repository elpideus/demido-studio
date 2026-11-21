/* Finding models on Hugging Face and downloading them, through the core.

   Searches and model pages are asked of the core each time; the core keeps them (ten minutes, an hour) so that
   asking again costs nothing. Downloads are the core's: this keeps the list right from the core's news. */
import { create } from 'zustand'
import { coreGet, coreSend, onCoreEvent } from './core'
import { readLocal } from './local-models'
import { toast } from './store'

export interface Machine { card_mb: number; ram_mb: number; margin_mb: number; context: number }
export interface HubModel {
  id: string; author: string; downloads: number; likes: number; trending: number; updated: string | null; params: number | null
  architecture: string | null; context_length: number | null; licence: string | null; gated: boolean; takes: string[]; does: string[]
  usual_size: number | null; fits: 'fits' | 'slow' | 'no' | null
}
export interface HubFile { path: string; size: number; sha256: string | null }
export interface HubQuant { name: string; files: HubFile[]; size: number; needs_mb: number; fits: 'fits' | 'slow' | 'no'; recommended: boolean; on_disk: boolean; local_id: string }
export interface HubDetail { model: HubModel; revision: string; quants: HubQuant[]; vision_part: HubFile | null; others: HubFile[]; card: string | null; page: string; machine: Machine }
export interface HubPage { models: HubModel[]; next: string | null; machine: Machine }
export interface Download { id: string; repo: string; quant: string; local_id: string; files: string[]; total: number; done: number; state: 'waiting' | 'downloading' | 'done' | 'failed' | 'stopped'; problem: string | null; started_at: number }

export interface Search { q: string; sort: 'trending' | 'downloads' | 'likes' | 'updated'; takes: string[]; does: string[]; fits: boolean }

export function searchHub(s: Search, cursor?: string | null): Promise<HubPage> {
  const p = new URLSearchParams({ q: s.q.trim(), sort: s.sort })
  if (s.takes.length) p.set('takes', s.takes.join(','))
  if (s.does.length) p.set('does', s.does.join(','))
  if (s.fits) p.set('fits', 'true')
  if (cursor) p.set('cursor', cursor)
  return coreGet<HubPage>(`/hub/models?${p}`)
}

export function hubModel(repo: string): Promise<HubDetail> {
  return coreGet<HubDetail>(`/hub/models/${repo.split('/').map(encodeURIComponent).join('/')}`)
}

interface Downloads { list: Download[] }
export const useDownloads = create<Downloads>(() => ({ list: [] }))
const set = useDownloads.setState
const get = useDownloads.getState

function put(d: Download) {
  const list = get().list.filter((x) => x.id !== d.id)
  set({ list: [...list, d].sort((a, b) => a.started_at - b.started_at) })
}

export async function readDownloads() {
  try {
    set({ list: await coreGet<Download[]>('/hub/downloads') })
  } catch { /* the core says why when asked again */ }
}

export async function startDownload(repo: string, quant: string): Promise<boolean> {
  try {
    put(await coreSend<Download>('POST', '/hub/downloads', { repo, quant }))
    return true
  } catch (e) {
    toast((e as Error).message, undefined, 'model-error')
    return false
  }
}

export async function stopDownload(id: string, discard = false) {
  try {
    await coreSend<Download>('DELETE', `/hub/downloads/${encodeURIComponent(id)}${discard ? '?discard=true' : ''}`)
  } catch (e) {
    toast((e as Error).message, undefined, 'model-error')
  }
  await readDownloads()
}

let started = false
/** Called once, when a core is found. */
export function startHub() {
  if (started) return
  started = true
  void readDownloads()
  onCoreEvent('model.download', (e) => put(e.data as unknown as Download))
  onCoreEvent('model.downloaded', (e) => {
    void readDownloads()
    void readLocal()
    toast(`${e.subject} is downloaded and checked. It is on this computer now.`, undefined, 'model-unloaded')
  })
  for (const kind of ['model.download.failed', 'model.download.stopped', 'model.download.started']) onCoreEvent(kind, () => void readDownloads())
}
