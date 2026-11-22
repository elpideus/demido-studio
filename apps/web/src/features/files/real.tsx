/* Files, for real: the folder of a session, as it is on disk, and each file in it to look at (brief 4.24).

   Only a session's own folder, and only inside it: the core checks every path. Markdown is shown as it reads,
   and as its source; a page (HTML) in a frame that runs nothing; pictures, PDFs, videos and sound as
   themselves. Editing, highlighting and artifacts are not built yet, and say so. */
import { useEffect, useMemo, useState } from 'react'
import { ChevronDown, ChevronRight, File, FileCode, FileText, Film, Folder, Image, Music } from 'lucide-react'
import type { LucideIcon } from 'lucide-react'
import { useReal } from '@/data/real-sessions'
import { fileUrl, listFolder, MOST_TEXT, readText, type FolderEntry, type Listing, type ShownAs } from '@/data/session-files'
import { Glyph, Icon, NotBuilt, Segmented, Select, Signal } from '@/ui'
import { Prose } from '@/ui/prose'
import { PdfPages } from './pdf'

/** A file's size, in the unit that suits it. */
function size(bytes: number): string {
  if (bytes < 1000) return `${bytes} bytes`
  if (bytes < 1_000_000) return `${(bytes / 1000).toFixed(1)} kB`
  if (bytes < 1_000_000_000) return `${(bytes / 1_000_000).toFixed(1)} MB`
  return `${(bytes / 1_000_000_000).toFixed(1)} GB`
}

const ICON: Record<ShownAs, LucideIcon> = { markdown: FileText, html: FileCode, text: FileText, image: Image, svg: Image, pdf: File, video: Film, audio: Music, other: File }

/** One folder of the tree: listed when it is first opened. */
function Branch({ session, path, depth, open, onOpen }: { session: string; path: string; depth: number; open: string | null; onOpen: (e: FolderEntry) => void }) {
  const [listing, setListing] = useState<Listing | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [unfolded, setUnfolded] = useState<Record<string, boolean>>({})
  useEffect(() => {
    let alive = true
    listFolder(session, path).then((l) => { if (alive) setListing(l) }, (e: Error) => { if (alive) setError(e.message) })
    return () => { alive = false }
  }, [session, path])
  if (error) return <p className="app-empty" style={{ paddingInlineStart: `${depth}rem` }}>{error}</p>
  if (!listing) return <p className="app-empty" style={{ paddingInlineStart: `${depth}rem` }}><Glyph name="task-running" busy /> Reading…</p>
  if (!listing.entries.length && depth === 0) return <p className="app-empty">The folder is empty.</p>
  return (
    <ul className="d-list">
      {listing.entries.map((e) => {
        const folder = e.kind === 'folder'
        const out = unfolded[e.path]
        return (
          <li key={e.path}>
            <button type="button" className="d-row d-row--compact" style={{ paddingInlineStart: `calc(var(--d-space-8) + ${depth}rem)` }} aria-current={e.path === open ? 'true' : undefined} aria-expanded={folder ? Boolean(out) : undefined}
              onClick={() => (folder ? setUnfolded({ ...unfolded, [e.path]: !out }) : onOpen(e))}>
              <span className="d-row__lead"><Icon icon={folder ? (out ? ChevronDown : ChevronRight) : ICON[e.shown_as ?? 'other']} /></span>
              <span className="d-row__text"><span className="d-row__title">{e.name}</span></span>
              {!folder && <span className="d-row__meta">{size(e.size)}</span>}
            </button>
            {folder && out && <Branch session={session} path={e.path} depth={depth + 1} open={open} onOpen={onOpen} />}
          </li>
        )
      })}
      {listing.more > 0 && <li className="app-empty">{listing.more.toLocaleString()} more not listed.</li>}
    </ul>
  )
}

/** A file, shown as what it is. */
function Viewer({ session, entry, folder }: { session: string; entry: FolderEntry; folder: string }) {
  const kind = entry.shown_as ?? 'other'
  const textual = kind === 'markdown' || kind === 'html' || kind === 'text'
  const [text, setText] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [view, setView] = useState<'preview' | 'split' | 'source'>('preview')
  useEffect(() => {
    if (!textual) return
    let alive = true
    setText(null)
    setError(null)
    readText(session, entry.path, entry.size).then((t) => { if (alive) setText(t) }, (e: Error) => { if (alive) setError(e.message) })
    return () => { alive = false }
  }, [session, entry.path, entry.size, textual])
  const url = fileUrl(session, entry.path)
  const previewable = kind === 'markdown' || kind === 'html'
  const shown = previewable ? view : 'source'
  const preview = useMemo(() => {
    if (text === null) return null
    // A page in a frame that runs nothing and reaches nothing: no scripts, no forms, no address of its own.
    return kind === 'markdown' ? <Prose text={text} /> : <iframe className="files__frame" title={`${entry.name}, as a page`} sandbox="" srcDoc={text} />
  }, [kind, text, entry.name])
  return (
    <section className="chat" style={{ gridTemplateRows: 'auto minmax(0, 1fr)' }} aria-label="File">
      <header className="chat__head">
        <span className="chat__title">{entry.name}</span>
        <span className="d-type-caption d-text-tertiary">{size(entry.size)}{entry.modified ? ` · changed ${new Date(entry.modified).toLocaleString()}` : ''}</span>
        <span className="app-spacer" />
        {previewable && <Segmented small label="View" value={view} onChange={setView} options={[{ value: 'preview', label: kind === 'html' ? 'As a page' : 'As it reads' }, { value: 'split', label: 'Side by side' }, { value: 'source', label: 'Source' }]} />}
      </header>
      {error ? <div className="files__preview"><Signal level="caution">{error}</Signal></div>
        : textual ? (text === null ? <p className="app-empty"><Glyph name="task-running" busy /> Reading…</p> : (
          <div className="files__panes" data-view={shown === 'source' ? 'edit' : shown === 'preview' ? 'preview' : 'split'}>
            <pre className="files__edit files__source" aria-label={`${entry.name}, its text`}>{text}{entry.size > MOST_TEXT ? `\n\n… (the first ${size(MOST_TEXT)} of ${size(entry.size)} are shown)` : ''}</pre>
            {previewable && <div className="files__preview">{preview}</div>}
          </div>
        ))
        : kind === 'image' || kind === 'svg' ? <div className="files__preview files__media"><img src={url} alt={entry.name} /></div>
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
      {session && open ? <Viewer key={open.path} session={session.id} entry={open} folder={session.folder ?? ''} />
        : <section className="chat" aria-label="File"><div className="files__preview app-stack">
          <p className="app-empty">{session ? 'Pick a file on the left.' : 'The files of a session with a folder are shown here.'}</p>
          <NotBuilt block>Editing a file here, highlighting code, and artifacts (files a model makes and sends, with their earlier versions).</NotBuilt>
        </div></section>}
    </div>
  )
}
