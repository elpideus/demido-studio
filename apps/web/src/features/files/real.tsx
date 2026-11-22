/* Files, for real: the folder of a session, as it is on disk, and each file in it to look at and, for text,
   to edit (brief 4.24).

   Only a session's own folder, and only inside it: the core checks every path. Text opens in an editor that
   highlights it quietly; Markdown is shown as it reads, as its source, or side by side, as it is typed; a
   page (HTML) in a frame that runs nothing; pictures, PDFs, videos and sound as themselves. A save is made
   only over the version that was opened: when the model or another program wrote the file meanwhile, the
   person is told and chooses. Artifacts are not built yet, and say so. */
import { lazy, Suspense, useEffect, useMemo, useRef, useState } from 'react'
import { ChevronDown, ChevronRight, File, FileCode, FileText, Film, Folder, Image, Music } from 'lucide-react'
import type { LucideIcon } from 'lucide-react'
import { useReal } from '@/data/real-sessions'
import { CoreProblem, draftKey, fileUrl, listFolder, MOST_TEXT, readText, saveText, useDrafts, useFolderTicks, watchFolders, type FolderEntry, type Listing, type Opened, type ShownAs } from '@/data/session-files'
import { Button, Glyph, Icon, NotBuilt, Segmented, Select, Signal } from '@/ui'
import { Prose } from '@/ui/prose'
import { PdfPages } from './pdf'

/** The editor, loaded the first time a text file is opened. */
const Code = lazy(() => import('./code'))

/** A file's size, in the unit that suits it. */
function size(bytes: number): string {
  if (bytes < 1000) return `${bytes} bytes`
  if (bytes < 1_000_000) return `${(bytes / 1000).toFixed(1)} kB`
  if (bytes < 1_000_000_000) return `${(bytes / 1_000_000).toFixed(1)} MB`
  return `${(bytes / 1_000_000_000).toFixed(1)} GB`
}

const ICON: Record<ShownAs, LucideIcon> = { markdown: FileText, html: FileCode, text: FileText, image: Image, svg: Image, pdf: File, video: Film, audio: Music, other: File }

/** Text that reads as prose, whose long lines are wrapped. Code is not. */
const PROSE = /\.(txt|text|log)$/i

/** One folder of the tree: listed when it is first opened. */
function Branch({ session, path, depth, open, onOpen }: { session: string; path: string; depth: number; open: string | null; onOpen: (e: FolderEntry) => void }) {
  const [listing, setListing] = useState<Listing | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [unfolded, setUnfolded] = useState<Record<string, boolean>>({})
  const drafts = useDrafts((s) => s.drafts)
  // Listed again when the folder may have changed; what was listed stays until then.
  const tick = useFolderTicks((s) => s.ticks[session] ?? 0)
  useEffect(() => {
    let alive = true
    listFolder(session, path).then((l) => { if (alive) { setListing(l); setError(null) } }, (e: Error) => { if (alive) setError(e.message) })
    return () => { alive = false }
  }, [session, path, tick])
  if (error) return <p className="app-empty" style={{ paddingInlineStart: `${depth}rem` }}>{error}</p>
  if (!listing) return <p className="app-empty" style={{ paddingInlineStart: `${depth}rem` }}><Glyph name="task-running" busy /> Reading…</p>
  if (!listing.entries.length && depth === 0) return <p className="app-empty">The folder is empty.</p>
  return (
    <ul className="d-list">
      {listing.entries.map((e) => {
        const folder = e.kind === 'folder'
        const out = unfolded[e.path]
        const unsaved = !folder && Boolean(drafts[draftKey(session, e.path)])
        return (
          <li key={e.path}>
            <button type="button" className="d-row d-row--compact" style={{ paddingInlineStart: `calc(var(--d-space-8) + ${depth}rem)` }} aria-current={e.path === open ? 'true' : undefined} aria-expanded={folder ? Boolean(out) : undefined}
              onClick={() => (folder ? setUnfolded({ ...unfolded, [e.path]: !out }) : onOpen(e))}>
              <span className="d-row__lead"><Icon icon={folder ? (out ? ChevronDown : ChevronRight) : ICON[e.shown_as ?? 'other']} /></span>
              <span className="d-row__text"><span className="d-row__title">{e.name}</span></span>
              {unsaved && <span className="d-row__meta files__unsaved">Not saved</span>}
              {!folder && !unsaved && <span className="d-row__meta">{size(e.size)}</span>}
            </button>
            {folder && out && <Branch session={session} path={e.path} depth={depth + 1} open={open} onOpen={onOpen} />}
          </li>
        )
      })}
      {listing.more > 0 && <li className="app-empty">{listing.more.toLocaleString()} more not listed.</li>}
    </ul>
  )
}

/** A text file, in the editor: saved over the version it was opened at. */
function TextFile({ session, entry }: { session: string; entry: FolderEntry }) {
  const kind = entry.shown_as ?? 'text'
  const key = draftKey(session, entry.path)
  const draft = useDrafts((s) => s.drafts[key])
  const put = useDrafts((s) => s.put)
  const tick = useFolderTicks((s) => s.ticks[session] ?? 0)
  // The file as it was last read from disk.
  const [opened, setOpened] = useState<Opened | null>(null)
  const seen = useRef<Opened | null>(null)
  const take = (o: Opened | null) => { seen.current = o; setOpened(o) }
  // What the editor was started with: a new `n` starts it again.
  const [start, setStart] = useState({ text: '', n: 0 })
  const [error, setError] = useState<string | null>(null)
  // A new round opens the file again.
  const [round, setRound] = useState(0)
  const [saving, setSaving] = useState(false)
  const savingNow = useRef(false)
  // Tells what was typed in the editor and not yet told: a save takes all of it.
  const flush = useRef<(() => void) | null>(null)
  const [refused, setRefused] = useState<{ message: string; changed: boolean } | null>(null)
  const [view, setView] = useState<'preview' | 'split' | 'source'>(draft ? 'split' : 'preview')

  useEffect(() => {
    let alive = true
    take(null)
    setError(null)
    readText(session, entry.path, entry.size).then((o) => {
      if (!alive) return
      take(o)
      setStart((s) => ({ text: useDrafts.getState().drafts[key]?.text ?? o.text, n: s.n + 1 }))
    }, (e: Error) => { if (alive) setError(e.message) })
    return () => { alive = false }
  }, [session, entry.path, entry.size, key, round])

  // The model, a program or a save may have changed the folder: read the file again. An edit not saved
  // stays in the editor (and is told when the file changed under it); with none, the editor shows the file
  // as it is now.
  useEffect(() => {
    if (!tick) return
    let alive = true
    readText(session, entry.path, entry.size).then((o) => {
      const was = seen.current
      if (!alive || savingNow.current || !was || (was.version === o.version && was.text === o.text)) return
      take(o)
      if (!useDrafts.getState().drafts[key]) setStart((s) => ({ text: o.text, n: s.n + 1 }))
    }, () => {})
    return () => { alive = false }
  }, [tick, session, entry.path, entry.size, key])

  const partial = entry.size > MOST_TEXT
  const editable = Boolean(opened?.version) && !partial
  // Written since it was edited here, before the edit was saved.
  const stale = Boolean(draft && opened?.version && draft.version !== opened.version)
  const shown = draft?.text ?? opened?.text ?? ''
  const previewable = kind === 'markdown' || kind === 'html'
  const panes = previewable ? view : 'source'
  const bytes = useMemo(() => (opened && !partial ? new TextEncoder().encode(opened.text).length : entry.size), [opened, partial, entry.size])
  const modified = opened?.modified ?? entry.modified

  const changed = (text: string) => {
    const now = seen.current
    if (!now?.version || partial) return
    const base = useDrafts.getState().drafts[key]?.version ?? now.version
    put(key, text === now.text && base === now.version ? null : { text, version: base })
  }
  const save = async (replaces?: string) => {
    flush.current?.()
    const d = useDrafts.getState().drafts[key]
    if (!d || savingNow.current) return
    savingNow.current = true
    setSaving(true)
    setRefused(null)
    try {
      const saved = await saveText(session, entry.path, d.text, replaces ?? d.version)
      take({ text: d.text, version: saved.version, modified: saved.modified })
      // What was typed while it was saving stays an edit, made to the version just saved.
      const now = useDrafts.getState().drafts[key]
      put(key, now && now.text !== d.text ? { text: now.text, version: saved.version } : null)
    } catch (e) {
      setRefused({ message: (e as Error).message, changed: e instanceof CoreProblem && e.code === 'changed' })
    } finally {
      savingNow.current = false
      setSaving(false)
    }
  }
  const dropMine = () => {
    put(key, null)
    setRefused(null)
    setRound((r) => r + 1)
  }

  const preview = useMemo(() => {
    if (opened === null) return null
    // A page in a frame that runs nothing and reaches nothing: no scripts, no forms, no address of its own.
    return kind === 'markdown' ? <Prose text={shown} /> : <iframe className="files__frame" title={`${entry.name}, as a page`} sandbox="" srcDoc={shown} />
  }, [kind, shown, opened, entry.name])

  const conflict = Boolean(refused?.changed || stale)
  return (
    <section className="chat" style={{ gridTemplateRows: refused || stale ? 'auto auto minmax(0, 1fr)' : 'auto minmax(0, 1fr)' }} aria-label="File">
      <header className="chat__head">
        <span className="chat__title">{entry.name}</span>
        <span className="d-type-caption d-text-tertiary">
          {size(bytes)}{modified ? ` · changed ${new Date(modified).toLocaleString()}` : ''}
          {partial ? ` · the first ${size(MOST_TEXT)} are shown, so it is not edited here` : ''}
        </span>
        <span className="app-spacer" />
        {previewable && <Segmented small label="View" value={view} onChange={setView} options={[{ value: 'preview', label: kind === 'html' ? 'As a page' : 'As it reads' }, { value: 'split', label: 'Side by side' }, { value: 'source', label: 'Source' }]} />}
        {editable && (
          <>
            <span className="d-type-caption d-text-secondary" aria-live="polite">{saving ? 'Saving…' : draft ? 'Not saved' : ''}</span>
            <Button size="sm" variant="primary" disabled={!draft || saving} onClick={() => void save()} title="Save (Ctrl+S, or ⌘S on a Mac)">Save</Button>
          </>
        )}
      </header>
      {(refused || stale) && (
        <div className="files__notice">
          {conflict ? (
            <Signal level="caution" actions={<>
              <Button size="sm" onClick={() => void save('*')} disabled={saving}>Save mine over it</Button>
              <Button size="sm" variant="ghost" onClick={dropMine}>Drop mine and open it again</Button>
            </>}>
              {entry.name} was written by the model or another program after you began editing it here, so your edit is not saved.
            </Signal>
          ) : <Signal level="error">The file was not saved: {refused?.message}</Signal>}
        </div>
      )}
      {error ? <div className="files__preview"><Signal level="caution">{error}</Signal></div>
        : opened === null ? <p className="app-empty"><Glyph name="task-running" busy /> Reading…</p>
        : (
          <div className="files__panes" data-view={panes === 'source' ? 'edit' : panes === 'preview' ? 'preview' : 'split'}>
            <Suspense fallback={<p className="app-empty files__edit"><Glyph name="task-running" busy /> Opening the editor…</p>}>
              <Code key={start.n} name={entry.name} text={start.text} readOnly={!editable} wrap={kind === 'markdown' || PROSE.test(entry.name)} onChange={changed} onSave={() => void save()} flush={flush} />
            </Suspense>
            {previewable && <div className="files__preview">{preview}</div>}
          </div>
        )}
    </section>
  )
}

/** A file, shown as what it is. */
function Viewer({ session, entry, folder }: { session: string; entry: FolderEntry; folder: string }) {
  const kind = entry.shown_as ?? 'other'
  if (kind === 'markdown' || kind === 'html' || kind === 'text') return <TextFile session={session} entry={entry} />
  const url = fileUrl(session, entry.path)
  return (
    <section className="chat" style={{ gridTemplateRows: 'auto minmax(0, 1fr)' }} aria-label="File">
      <header className="chat__head">
        <span className="chat__title">{entry.name}</span>
        <span className="d-type-caption d-text-tertiary">{size(entry.size)}{entry.modified ? ` · changed ${new Date(entry.modified).toLocaleString()}` : ''}</span>
      </header>
      {kind === 'image' || kind === 'svg' ? <div className="files__preview files__media"><img src={url} alt={entry.name} /></div>
        : kind === 'pdf' ? <PdfPages url={url} name={entry.name} />
        : kind === 'video' ? <div className="files__preview files__media"><video controls src={url} /></div>
        : kind === 'audio' ? <div className="files__preview files__media"><audio controls src={url} /></div>
        : <p className="app-empty">Demido does not show this kind of file. It is at <span className="d-mono">{folder}/{entry.path}</span>.</p>}
    </section>
  )
}

export function RealFiles() {
  const list = useReal((s) => s.list)
  const withFolder = (list ?? []).filter((s) => s.folder)
  const [chosen, setChosen] = useState<string | null>(null)
  const [open, setOpen] = useState<FolderEntry | null>(null)
  const session = withFolder.find((s) => s.id === chosen) ?? withFolder[0]
  useEffect(() => { setOpen(null) }, [session?.id])
  useEffect(() => { watchFolders() }, [])
  return (
    <div className="app-split">
      <aside className="app-side" aria-label="Files">
        <div className="app-side__head"><span className="d-type-headline">Files</span></div>
        {!list ? <p className="app-empty"><Glyph name="task-running" busy /> Reading the sessions…</p>
          : !session ? <p className="app-empty">No session has a folder yet. Give one a folder in its header, and its files appear here.</p>
          : <>
            <Select label="Session" value={session.id} onChange={setChosen} options={withFolder.map((s) => ({ value: s.id, label: s.title }))} />
            <div className="app-side__scroll">
              <p className="d-list__label"><Icon icon={Folder} size="sm" /> {session.folder?.split(/[\\/]/).filter(Boolean).at(-1)}</p>
              <Branch key={session.id} session={session.id} path="" depth={0} open={open?.path ?? null} onOpen={setOpen} />
            </div>
            <p className="app-side__foot">{session.folder}</p>
          </>}
      </aside>
      {session && open ? <Viewer key={`${session.id}\u0000${open.path}`} session={session.id} entry={open} folder={session.folder ?? ''} />
        : <section className="chat" aria-label="File"><div className="files__preview app-stack">
          <p className="app-empty">{session ? 'Pick a file on the left.' : 'The files of a session with a folder are shown here.'}</p>
          <NotBuilt block>Artifacts (files a model makes and sends, with their earlier versions), and making, renaming or deleting a file here.</NotBuilt>
        </div></section>}
    </div>
  )
}
