/* An artifact: a file a model gave in the session, with every version (DECISIONS F10 to F14). Shown as what
   it is, at any of its versions, and downloaded under its name. A page in it runs nothing, as a page in the
   folder does not (F2); a drawing (SVG) is shown as a picture, in which it runs nothing either. */
import { lazy, Suspense, useEffect, useMemo, useState } from 'react'
import { FileCode, FileText, Image } from 'lucide-react'
import type { LucideIcon } from 'lucide-react'
import { listArtifacts, putIntoFolder, readArtifact, readVersion, versionFileUrl, type Artifact, type ArtifactShownAs, type Version, type VersionFacts } from '@/data/artifacts'
import { CoreProblem, useFolderTicks, type FolderEntry } from '@/data/session-files'
import { Button, Dialog, Glyph, Icon, Segmented, Select, Signal } from '@/ui'
import { Prose } from '@/ui/prose'

const Code = lazy(() => import('./code'))

const ICON: Record<ArtifactShownAs, LucideIcon> = { markdown: FileText, html: FileCode, svg: Image, text: FileCode }

const when = (ms: number) => new Date(ms).toLocaleString()

/** A session's artifacts, read again when a tool was used in it (the model may have given another). */
export function useArtifacts(session: string | undefined): Artifact[] | null {
  const tick = useFolderTicks((s) => (session ? s.ticks[session] ?? 0 : 0))
  // Whose list it is: another session's is not shown while this one's is read.
  const [read, setRead] = useState<{ session: string; list: Artifact[] } | null>(null)
  useEffect(() => {
    if (!session) return
    let alive = true
    listArtifacts(session).then((list) => { if (alive) setRead({ session, list }) }, () => { if (alive) setRead({ session, list: [] }) })
    return () => { alive = false }
  }, [session, tick])
  return session && read?.session === session ? read.list : null
}

export function ArtifactList({ artifacts, open, onOpen }: { artifacts: Artifact[]; open: string | null; onOpen: (a: Artifact) => void }) {
  return (
    <ul className="d-list">
      {artifacts.map((a) => (
        <li key={a.id}>
          <button type="button" className="d-row d-row--compact" aria-current={a.id === open ? 'true' : undefined} onClick={() => onOpen(a)}>
            <span className="d-row__lead"><Icon icon={ICON[a.shown_as]} /></span>
            <span className="d-row__text"><span className="d-row__title">{a.name}</span></span>
            <span className="d-row__meta">{a.versions === 1 ? '1 version' : `${a.versions} versions`}</span>
          </button>
        </li>
      ))}
    </ul>
  )
}

/** A drawing, as a picture: an SVG in an image runs nothing. */
function Drawing({ text, name }: { text: string; name: string }) {
  const url = useMemo(() => URL.createObjectURL(new Blob([text], { type: 'image/svg+xml' })), [text])
  useEffect(() => () => URL.revokeObjectURL(url), [url])
  return <div className="files__media"><img src={url} alt={name} /></div>
}

/** Puts a version into the session's folder, as a file of its own on disk. A file that is there is replaced
    only when the person says so. */
function IntoFolder({ session, artifact, n, onPut }: { session: string; artifact: Artifact; n: number; onPut: (e: FolderEntry) => void }) {
  const [open, setOpen] = useState(false)
  const [path, setPath] = useState(artifact.name)
  const [there, setThere] = useState(false)
  const [problem, setProblem] = useState<string | null>(null)
  const put = async (replace: boolean) => {
    if (!path.trim()) return
    try {
      const entry = await putIntoFolder(session, artifact.id, n, path.trim(), replace)
      setOpen(false)
      onPut(entry)
    } catch (e) {
      if (e instanceof CoreProblem && e.status === 409) setThere(true)
      else setProblem((e as Error).message)
    }
  }
  return (
    <>
      <Button size="sm" onClick={() => { setPath(artifact.name); setThere(false); setProblem(null); setOpen(true) }}>Put in the folder</Button>
      {open && (
        <Dialog open onOpenChange={(o) => !o && setOpen(false)} title="Put it in the session's folder"
          text={`Version ${n} of ${artifact.name}, as a file of its own in the folder. The file the model gave stays here too.`}
          actions={<><Button onClick={() => setOpen(false)}>Cancel</Button>{there
            ? <Button variant="danger" onClick={() => void put(true)}>Replace the file</Button>
            : <Button variant="primary" disabled={!path.trim()} onClick={() => void put(false)}>Put it there</Button>}</>}>
          <form className="app-stack" style={{ marginBlockStart: 'var(--d-space-16)' }} onSubmit={(e) => { e.preventDefault(); void put(false) }}>
            <label className="d-field"><span className="d-field__label">Where, in the folder</span><input className="d-input d-input--mono" autoFocus value={path} onChange={(e) => { setPath(e.target.value); setThere(false) }} /></label>
            {there && <Signal level="caution">There is a file {path.trim()} in the folder already. Replacing it loses what it holds now; or give another name.</Signal>}
            {problem && <Signal level="caution">{problem}</Signal>}
          </form>
        </Dialog>
      )}
    </>
  )
}

/** One artifact, at the version asked for; with none asked, its last, and the next one when the model gives it.
    In a session with a folder, it can be put there (`onPut` opens the file it became). */
export function ArtifactView({ session, id, n: asked, folder, onPut }: { session: string; id: string; n?: number; folder: string | null; onPut: (e: FolderEntry) => void }) {
  const tick = useFolderTicks((s) => s.ticks[session] ?? 0)
  const [facts, setFacts] = useState<{ artifact: Artifact; versions: VersionFacts[] } | null>(null)
  // `null`: the last version, whichever that is.
  const [n, setN] = useState<number | null>(asked ?? null)
  const [version, setVersion] = useState<Version | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [view, setView] = useState<'preview' | 'split' | 'source'>('preview')
  useEffect(() => {
    let alive = true
    readArtifact(session, id).then((f) => { if (alive) { setFacts(f); setError(null) } }, (e: Error) => { if (alive) setError(e.message) })
    return () => { alive = false }
  }, [session, id, tick])
  const last = facts?.artifact.versions ?? null
  const shown = n ?? last
  useEffect(() => {
    if (shown === null) return
    let alive = true
    readVersion(session, id, shown).then((v) => { if (alive) setVersion(v) }, (e: Error) => { if (alive) setError(e.message) })
    return () => { alive = false }
  }, [session, id, shown])

  if (error) return <section className="chat" aria-label="Artifact"><div className="files__preview"><Signal level="caution">{error}</Signal></div></section>
  if (!facts || !version || version.n !== shown) return <section className="chat" aria-label="Artifact"><p className="app-empty"><Glyph name="task-running" busy /> Reading…</p></section>
  const a = facts.artifact
  const previewable = a.shown_as !== 'text'
  const panes = previewable ? view : 'source'
  const made = facts.versions.find((v) => v.n === version.n)
  const size = (bytes: number) => (bytes < 1000 ? `${bytes} bytes` : bytes < 1_000_000 ? `${(bytes / 1000).toFixed(1)} kB` : `${(bytes / 1_000_000).toFixed(1)} MB`)
  return (
    <section className="chat" style={{ gridTemplateRows: 'auto minmax(0, 1fr)' }} aria-label={`${a.name}, a file the model gave you`}>
      <header className="chat__head">
        <span className="chat__title">{a.name}</span>
        <span className="d-type-caption d-text-tertiary">{made ? `${size(made.size)} · made ${when(made.made_at)}` : ''}</span>
        <span className="app-spacer" />
        {facts.versions.length > 1 && (
          <Select label="Version" value={String(version.n)} onChange={(v) => setN(Number(v) === last ? null : Number(v))}
            options={[...facts.versions].reverse().map((v) => ({ value: String(v.n), label: v.n === last ? `Version ${v.n}, the latest` : `Version ${v.n}` }))} />
        )}
        {previewable && <Segmented small label="View" value={view} onChange={setView} options={[{ value: 'preview', label: a.shown_as === 'html' ? 'As a page' : a.shown_as === 'svg' ? 'As a picture' : 'As it reads' }, { value: 'split', label: 'Side by side' }, { value: 'source', label: 'Source' }]} />}
        {folder && <IntoFolder session={session} artifact={a} n={version.n} onPut={onPut} />}
        <a className="d-button d-button--sm" href={versionFileUrl(session, id, version.n)} download={a.name}>Download</a>
      </header>
      <div className="files__panes" data-view={panes === 'source' ? 'edit' : panes === 'preview' ? 'preview' : 'split'}>
        <Suspense fallback={<p className="app-empty files__edit"><Glyph name="task-running" busy /> Opening…</p>}>
          <Code key={`${id}:${version.n}`} name={a.name} text={version.text} readOnly wrap={a.shown_as === 'markdown'} onChange={() => {}} onSave={() => {}} />
        </Suspense>
        {previewable && (
          <div className="files__preview">
            {a.shown_as === 'markdown' ? <Prose text={version.text}/>
              : a.shown_as === 'svg' ? <Drawing text={version.text} name={a.name} />
              // A page in a frame that runs nothing and reaches nothing: no scripts, no forms, no address of its own.
              : <iframe className="files__frame" title={`${a.name}, as a page`} sandbox="" srcDoc={version.text} />}
          </div>
        )}
      </div>
    </section>
  )
}
