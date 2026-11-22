/* A session's folder, for real: what is in it, and each file, through the core.

   Nothing outside the session's folder is listed or sent; the core checks every path. A file is sent as what
   it is, except a page (HTML and the like), which comes as text so that it never runs with Demido's address.
   A text file comes with its version, and is saved only over that version (DECISIONS F6). */
import { create } from 'zustand'
import { coreGet, onCoreEvent } from './core'

/** `shown_as`: how the window shows a file. */
export type ShownAs = 'markdown' | 'html' | 'text' | 'image' | 'svg' | 'pdf' | 'video' | 'audio' | 'other'
export interface FolderEntry { name: string; path: string; kind: 'folder' | 'file'; shown_as: ShownAs | null; size: number; modified: number | null }
export interface Listing { folder: string; path: string; entries: FolderEntry[]; more: number }
export interface Saved { path: string; size: number; modified: number | null; version: string }

/** What is in a folder of the session's folder (`''` for itself). Throws with the core's words. */
export function listFolder(session: string, path = ''): Promise<Listing> {
  return coreGet<Listing>(`/sessions/${encodeURIComponent(session)}/files?path=${encodeURIComponent(path)}`)
}

/** Where a file is fetched from, for an image, a video, a PDF. The window's own sign-in goes with it. */
export function fileUrl(session: string, path: string): string {
  return `/api/v1/sessions/${encodeURIComponent(session)}/files/content?path=${encodeURIComponent(path)}`
}

/** The most of a text file that is shown: a longer one is shown up to here, says so, and is not edited. */
export const MOST_TEXT = 2_000_000

/** A problem the core named: `code` is its word for it (`changed`: the file was written since it was opened). */
export class CoreProblem extends Error {
  constructor(message: string, readonly code: string | null, readonly status: number) {
    super(message)
  }
}

async function problem(res: Response): Promise<CoreProblem> {
  const said = await res.json().catch(() => null) as { error?: { message?: string; code?: string } } | null
  return new CoreProblem(said?.error?.message ?? `The core answered ${res.status}.`, said?.error?.code ?? null, res.status)
}

/** A text file as it was read: its text (up to `MOST_TEXT` bytes), its version (`null`: it cannot be saved
    from here), and when it last changed. */
export interface Opened { text: string; version: string | null; modified: number | null }

/** Reads a text file. */
export async function readText(session: string, path: string, size: number): Promise<Opened> {
  const res = await fetch(fileUrl(session, path), { headers: size > MOST_TEXT ? { Range: `bytes=0-${MOST_TEXT - 1}` } : {} })
  if (!res.ok) throw await problem(res)
  const modified = Date.parse(res.headers.get('last-modified') ?? '')
  return { text: await res.text(), version: res.status === 206 ? null : res.headers.get('etag'), modified: Number.isNaN(modified) ? null : modified }
}

/** Saves a text file over `replaces`, the version it was opened at (`*`: over whatever is there). */
export async function saveText(session: string, path: string, text: string, replaces: string): Promise<Saved> {
  const res = await fetch(fileUrl(session, path), {
    method: 'PUT',
    headers: { accept: 'application/json', 'content-type': 'text/plain; charset=utf-8', 'if-match': replaces },
    body: text,
  })
  if (!res.ok) throw await problem(res)
  return res.json() as Promise<Saved>
}

/** An edit not saved yet: the text, and the version of the file it was made to. */
export interface Draft { text: string; version: string }

/** Edits not saved yet, kept while the window is open: moving to another file loses none of them. */
export const useDrafts = create<{ drafts: Record<string, Draft>; put: (key: string, draft: Draft | null) => void }>((set) => ({
  drafts: {},
  put: (key, draft) => set((s) => {
    const drafts = { ...s.drafts }
    if (draft) drafts[key] = draft
    else delete drafts[key]
    return { drafts }
  }),
}))

/** Which file a draft belongs to. */
export const draftKey = (session: string, path: string) => `${session}\u0000${path}`

/** How often each session's folder may have changed since the window opened. What is shown of a folder
    looks again when its count moves. */
export const useFolderTicks = create<{ ticks: Record<string, number>; bump: (session: string) => void }>((set) => ({
  ticks: {},
  bump: (session) => set((s) => ({ ticks: { ...s.ticks, [session]: (s.ticks[session] ?? 0) + 1 } })),
}))

let watching = false
/** Hears what can change a session's folder: a tool the model used, a turn's end (a program such as Claude
    Code works in the folder too), a turn undone, a file saved. Started by the Files screen, once. */
export function watchFolders() {
  if (watching) return
  watching = true
  for (const kind of ['tool.used', 'message.done', 'message.failed', 'message.stopped', 'change.undone', 'session.file_saved']) {
    onCoreEvent(kind, (e) => { if (e.subject) useFolderTicks.getState().bump(e.subject) })
  }
}

// Closing the window, or going to another address, with an edit not saved: the browser asks first.
if (typeof window !== 'undefined') {
  window.addEventListener('beforeunload', (e) => {
    if (Object.keys(useDrafts.getState().drafts).length) e.preventDefault()
  })
}
