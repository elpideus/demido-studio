import { useMemo, useState } from 'react'
import { File, FileCode, FileText, Folder, Image } from 'lucide-react'
import type { LucideIcon } from 'lucide-react'
import type { Feature } from '@/app/registry'
import { useCore } from '@/data/core'
import { showArtifact, toast, useStore } from '@/data/store'
import { RealFiles } from './real'
import { Button, Icon, NotBuilt, Segmented, Select } from '@/ui'

interface Entry { name: string; kind: 'md' | 'html' | 'code' | 'image' | 'pdf'; size: string; content?: string; artifact?: string }
const TREE: Entry[] = [
  { name: 'price-watch.md', kind: 'md', size: '1.2 KB', artifact: 'a-watch' },
  { name: 'notes.html', kind: 'html', size: '0.4 KB', content: '<h2>Sweater</h2>\n<p>Merino crew neck, <strong>navy</strong>.</p>\n<ul>\n  <li>Size M</li>\n  <li>Wait for a price under EUR 89</li>\n</ul>' },
  { name: 'schedule.json', kind: 'code', size: '0.2 KB', content: '{\n  "title": "Sweater price watch",\n  "kind": "monitor",\n  "when": "every day 07:00",\n  "tell_me_when": "price < 89.00"\n}' },
  { name: 'page.png', kind: 'image', size: '184 KB' },
  { name: 'receipt.pdf', kind: 'pdf', size: '62 KB' },
]
const ICON: Record<Entry['kind'], LucideIcon> = { md: FileText, html: FileCode, code: FileCode, image: Image, pdf: File }
const esc = (s: string) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')

/** A very small Markdown renderer, enough for the preview. The app uses a real one. */
function md(src: string): string {
  const out: string[] = []
  const lines = esc(src).split('\n')
  const inline = (s: string) => s.replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>').replace(/`(.+?)`/g, '<code>$1</code>')
  for (let i = 0; i < lines.length; i++) {
    const l = lines[i]
    if (/^\|/.test(l)) {
      const rows: string[] = []
      while (i < lines.length && /^\|/.test(lines[i])) rows.push(lines[i++])
      i--
      const cells = (r: string) => r.split('|').slice(1, -1).map((c) => c.trim())
      out.push(`<table><thead><tr>${cells(rows[0]).map((c) => `<th>${inline(c)}</th>`).join('')}</tr></thead><tbody>${rows.slice(2).map((r) => `<tr>${cells(r).map((c) => `<td>${inline(c)}</td>`).join('')}</tr>`).join('')}</tbody></table>`)
    } else if (/^## /.test(l)) out.push(`<h3>${inline(l.slice(3))}</h3>`)
    else if (/^# /.test(l)) out.push(`<h2>${inline(l.slice(2))}</h2>`)
    else if (l.trim()) out.push(`<p>${inline(l)}</p>`)
  }
  return out.join('')
}

function FilesPage() {
  const sessions = useStore((s) => s.sessions)
  const artifacts = useStore((s) => s.artifacts)
  const [sid, setSid] = useState('s-sweater')
  const [name, setName] = useState('price-watch.md')
  const [view, setView] = useState<'edit' | 'split' | 'preview'>('split')
  const [edits, setEdits] = useState<Record<string, string>>({})
  const session = sessions.find((s) => s.id === sid)!
  const files = sid === 's-sweater' ? TREE : []
  const file = files.find((f) => f.name === name)
  const artifact = artifacts.find((a) => a.id === file?.artifact)
  const base = artifact ? artifact.versions[artifact.versions.length - 1].content : (file?.content ?? '')
  const text = edits[name] ?? base
  const html = useMemo(() => (file?.kind === 'md' ? md(text) : file?.kind === 'html' ? text : ''), [file?.kind, text])
  const previewable = file?.kind === 'md' || file?.kind === 'html'
  return (
    <div className="app-split">
      <aside className="app-side" aria-label="Files">
        <div className="app-side__head"><span className="d-type-headline">Files</span></div>
        <Select label="Session" value={sid} onChange={setSid} options={sessions.slice(0, 6).map((s) => ({ value: s.id, label: s.title }))} />
        <div className="app-side__scroll">
          <p className="d-list__label"><Icon icon={Folder} size="sm" /> {session.folder.split('/').pop()}</p>
          <ul className="d-list">
            {files.map((f) => (
              <li key={f.name}>
                <button type="button" className="d-row d-row--compact" aria-current={f.name === name} onClick={() => setName(f.name)}>
                  <span className="d-row__lead"><Icon icon={ICON[f.kind]} /></span>
                  <span className="d-row__text"><span className="d-row__title">{f.name}{edits[f.name] !== undefined ? ' •' : ''}</span></span>
                  <span className="d-row__meta">{f.artifact ? 'artifact' : f.size}</span>
                </button>
              </li>
            ))}
          </ul>
          {!files.length && <p className="app-empty">Files this session creates or reads appear here. The preview has files for one session only.</p>}
        </div>
        <p className="app-side__foot">{session.folder}</p>
      </aside>
      <section className="chat" style={{ gridTemplateRows: 'auto minmax(0, 1fr)' }} aria-label="File">
        <header className="chat__head">
          <span className="chat__title">{file?.name ?? 'No file'}</span>
          {artifact && <span className="d-badge">Artifact · v{artifact.versions.length}</span>}
          <span className="app-spacer" />
          {previewable && <Segmented small label="View" value={view} onChange={setView} options={[{ value: 'edit', label: 'Edit' }, { value: 'split', label: 'Side by side' }, { value: 'preview', label: 'Preview' }]} />}
          {artifact && <Button size="sm" variant="ghost" onClick={() => showArtifact(artifact.id)}>Versions</Button>}
          {edits[name] !== undefined && <Button size="sm" variant="primary" onClick={() => { setEdits(({ [name]: _, ...rest }) => rest); toast('Saved. (Scripted: the preview keeps nothing.)', undefined, 'task-done') }}>Save</Button>}
        </header>
        {!file ? (
          <p className="app-empty">Pick a file on the left.</p>
        ) : file.kind === 'image' || file.kind === 'pdf' ? (
          <div className="app-empty" style={{ alignContent: 'center' }}>
            <Icon icon={ICON[file.kind]} size="xl" />
            <p>{file.name} · {file.size}</p>
            <NotBuilt>{file.kind === 'pdf' ? 'PDF viewer' : 'Image, video and music viewer'}: not built yet</NotBuilt>
          </div>
        ) : (
          <div className="files__panes" data-view={previewable ? view : 'edit'}>
            <textarea className="files__edit" aria-label={`Edit ${file.name}`} spellCheck={false} value={text} onChange={(e) => setEdits({ ...edits, [name]: e.target.value })} />
            {previewable && <div className="files__preview d-prose" dangerouslySetInnerHTML={{ __html: html }} />}
          </div>
        )}
      </section>
    </div>
  )
}

/** With a core behind the pages, a session's real folder; without one, the preview's. */
function Files() {
  const core = useCore()
  return core.state === 'ready' ? <RealFiles /> : <FilesPage />
}

// No place in the rail: a session's files open beside it (its folder button). The route stays, for links.
export const feature: Feature = { id: 'files', title: 'Files', icon: Folder, group: 'work', order: 2, path: 'files', Page: Files, nav: [] }
