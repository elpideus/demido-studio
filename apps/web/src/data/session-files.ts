/* A session's folder, for real: what is in it, and each file, through the core.

   Nothing outside the session's folder is listed or sent; the core checks every path. A file is sent as what
   it is, except a page (HTML and the like), which comes as text so that it never runs with Demido's address. */
import { coreGet } from './core'

/** `shown_as`: how the window shows a file. */
export type ShownAs = 'markdown' | 'html' | 'text' | 'image' | 'svg' | 'pdf' | 'video' | 'audio' | 'other'
export interface FolderEntry { name: string; path: string; kind: 'folder' | 'file'; shown_as: ShownAs | null; size: number; modified: number | null }
export interface Listing { folder: string; path: string; entries: FolderEntry[]; more: number }

/** What is in a folder of the session's folder (`''` for itself). Throws with the core's words. */
export function listFolder(session: string, path = ''): Promise<Listing> {
  return coreGet<Listing>(`/sessions/${encodeURIComponent(session)}/files?path=${encodeURIComponent(path)}`)
}

/** Where a file is fetched from, for an image, a video, a PDF. The window's own sign-in goes with it. */
export function fileUrl(session: string, path: string): string {
  return `/api/v1/sessions/${encodeURIComponent(session)}/files/content?path=${encodeURIComponent(path)}`
}

/** The most of a text file that is shown: a longer one is shown up to here, and says so. */
export const MOST_TEXT = 2_000_000

/** A text file's text, up to `MOST_TEXT` bytes. */
export async function readText(session: string, path: string, size: number): Promise<string> {
  const res = await fetch(fileUrl(session, path), { headers: size > MOST_TEXT ? { Range: `bytes=0-${MOST_TEXT - 1}` } : {} })
  if (!res.ok && res.status !== 206) {
    const said = await res.json().catch(() => null) as { error?: { message?: string } } | null
    throw new Error(said?.error?.message ?? `The core answered ${res.status}.`)
  }
  return res.text()
}
