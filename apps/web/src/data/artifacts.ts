/* Artifacts, for real: the files models gave in a session, with every version, through the core
   (DECISIONS F10 to F14). They are the session's own, not files in its folder. */
import { create } from 'zustand'
import { coreGet, coreSend } from './core'
import { CoreProblem, type FolderEntry } from './session-files'

export type ArtifactShownAs = 'markdown' | 'html' | 'svg' | 'text'
export interface Artifact { id: string; session_id: string; name: string; shown_as: ArtifactShownAs; versions: number; size: number; created_at: number; updated_at: number }
export interface VersionFacts { n: number; size: number; made_at: number; message_id: string | null }
export interface Version { n: number; text: string; made_at: number; message_id: string | null }

const at = (session: string, artifact = '') => `/sessions/${encodeURIComponent(session)}/artifacts${artifact ? `/${encodeURIComponent(artifact)}` : ''}`

/** The artifacts of a session, the first made first. */
export function listArtifacts(session: string): Promise<Artifact[]> {
  return coreGet<Artifact[]>(at(session))
}

/** One artifact, and its versions without their text. */
export function readArtifact(session: string, artifact: string): Promise<{ artifact: Artifact; versions: VersionFacts[] }> {
  return coreGet(at(session, artifact))
}

/** One version, with its text. */
export function readVersion(session: string, artifact: string, n: number): Promise<Version> {
  return coreGet<Version>(`${at(session, artifact)}/versions/${n}`)
}

/** Where a version is downloaded from, under the artifact's name. */
export function versionFileUrl(session: string, artifact: string, n: number): string {
  return `/api/v1${at(session, artifact)}/versions/${n}/file`
}

/** Puts a version into the session's folder, at `path` (its own name when left out). A file that is there is
    replaced only with `replace`; without it the core answers 409, as a `CoreProblem`. */
export async function putIntoFolder(session: string, artifact: string, n: number, path: string, replace: boolean): Promise<FolderEntry> {
  const res = await fetch(`/api/v1${at(session, artifact)}/versions/${n}/into-folder`, {
    method: 'POST',
    headers: { accept: 'application/json', 'content-type': 'application/json' },
    body: JSON.stringify({ path, replace }),
  })
  if (!res.ok) {
    const said = await res.json().catch(() => null) as { error?: { message?: string; code?: string } } | null
    throw new CoreProblem(said?.error?.message ?? `The core answered ${res.status}.`, said?.error?.code ?? null, res.status)
  }
  return res.json() as Promise<FolderEntry>
}

/** Keeps text as a file of the session's own, as the person asks. The same name again is a new version. */
export function keepAsFile(session: string, name: string, text: string, messageId?: string): Promise<{ artifact: Artifact; n: number }> {
  return coreSend('POST', at(session), { name, text, message_id: messageId })
}

/** A code block's language, as the ending of a file. */
const ENDING: Record<string, string> = {
  python: 'py', py: 'py', javascript: 'js', js: 'js', typescript: 'ts', ts: 'ts', tsx: 'tsx', jsx: 'jsx', html: 'html', css: 'css',
  rust: 'rs', rs: 'rs', go: 'go', java: 'java', c: 'c', cpp: 'cpp', 'c++': 'cpp', csharp: 'cs', cs: 'cs', bash: 'sh', sh: 'sh',
  shell: 'sh', zsh: 'sh', json: 'json', yaml: 'yaml', yml: 'yaml', toml: 'toml', sql: 'sql', markdown: 'md', md: 'md', svg: 'svg',
  xml: 'xml', ruby: 'rb', php: 'php', kotlin: 'kt', swift: 'swift', lua: 'lua',
}

/** What an answer would be kept as: its one block of code, when that is most of it, under the ending of its
    language; otherwise the whole answer, as Markdown, named after its first heading. */
export function asAFile(answer: string): { name: string; text: string } {
  const blocks = [...answer.matchAll(/```([\w+#-]*)[^\n]*\n([\s\S]*?)```/g)]
  if (blocks.length === 1 && blocks[0][2].length > answer.length * 0.5) {
    const language = blocks[0][1].toLowerCase()
    return { name: `file.${ENDING[language] ?? 'txt'}`, text: blocks[0][2].replace(/\n$/, '') }
  }
  const heading = answer.match(/^#{1,3}\s+(.+)$/m)?.[1]
  const name = (heading ?? 'answer').replace(/[\\/:*?"<>|#`]/g, '').replace(/\*\*/g, '').trim().slice(0, 60) || 'answer'
  return { name: `${name}.md`, text: answer }
}

/** What the Files screen is asked to show when it opens: an artifact a step gave, from the conversation. */
export const useFilesFocus = create<{ focus: { session: string; artifact: string; n: number } | null; clear: () => void }>((set) => ({
  focus: null,
  clear: () => set({ focus: null }),
}))

/** Opens an artifact, at a version, in the Files screen. */
export function openArtifact(session: string, artifact: string, n: number) {
  useFilesFocus.setState({ focus: { session, artifact, n } })
  window.location.hash = '#/files'
}
