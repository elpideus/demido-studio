/* What opens beside a session: its folder (here) and its browser (live-browser.tsx). Each takes the right part
   of the session's view, can take all of it, and closes back to the conversation.

   The folder is the session's files, as Files showed them: open ones as tabs, the tree on the right, and the
   files models gave (artifacts) under it. A page the model opened is a card in the conversation. */
import { useEffect, useMemo, useState } from 'react'
import { Copy, ExternalLink, FilePlus, FolderOpen, Gift, Maximize2, Minimize2, MoreHorizontal, Search, X } from 'lucide-react'
import type { CoreMessage } from '@/data/real-sessions'
import { CoreProblem, draftKey, newFile, useDrafts, watchFolders, type FolderEntry } from '@/data/session-files'
import { toast } from '@/data/store'
import { Button, Icon, Menu, Tip } from '@/ui'
import { Branch, DeletedFiles, NameIt, Viewer } from '@/features/files/real'
import { ArtifactList, ArtifactView, useArtifacts } from '@/features/files/artifact'

/** A page of the session's browser, as a step of the model's left it. */
export interface Shot { picture: string; address: string; title: string }

/** Every page the session's model saw in the browser, oldest first. */
export function shotsOf(messages: CoreMessage[]): Shot[] {
  return messages.flatMap((m) => (m.steps ?? []).filter((s) => s.picture).map((s) => ({ picture: s.picture as string, address: s.page?.address ?? (typeof s.args?.url === 'string' ? s.args.url : ''), title: s.page?.title ?? '' })))
}

export const pictureUrl = (session: string, name: string) => `/api/v1/sessions/${encodeURIComponent(session)}/browser/pictures/${encodeURIComponent(name)}`
const host = (address: string) => { try { return new URL(address).host } catch { return address } }

function PanelHead({ wide, onWide, onClose, children }: { wide: boolean; onWide: () => void; onClose: () => void; children: React.ReactNode }) {
  return (
    <header className="panel__head">
      {children}
      <Tip label={wide ? 'Beside the conversation' : 'Whole view'}><Button size="sm" variant="ghost" iconOnly aria-label={wide ? 'Beside the conversation' : 'Whole view'} onClick={onWide}><Icon icon={wide ? Minimize2 : Maximize2} size="sm" /></Button></Tip>
      <Tip label="Close"><Button size="sm" variant="ghost" iconOnly aria-label="Close" onClick={onClose}><Icon icon={X} size="sm" /></Button></Tip>
    </header>
  )
}

/** A page the model opened, in the conversation: its picture, its name, and the browser beside it. */
export function BrowserCard({ session, shot, onOpen }: { session: string; shot: Shot; onOpen: () => void }) {
  return (
    <figure className="page-card">
      <button type="button" className="page-card__shot" onClick={onOpen} aria-label={`Open ${shot.title || shot.address} beside the conversation`}>
        <img src={pictureUrl(session, shot.picture)} alt="" loading="lazy" />
      </button>
      <figcaption className="page-card__foot">
        <span className="page-card__text">
          <span className="page-card__title">{shot.title || host(shot.address)}</span>
          <span className="page-card__sub">{host(shot.address)} · Demido's browser</span>
        </span>
        <Button size="sm" onClick={onOpen}>Open</Button>
        <Menu align="end" trigger={<Button size="sm" variant="ghost" iconOnly aria-label="More"><Icon icon={MoreHorizontal} size="sm" /></Button>} items={[
          { label: 'In your own browser', lead: <Icon icon={ExternalLink} size="sm" />, onSelect: () => window.open(shot.address, '_blank', 'noopener,noreferrer') },
          { label: 'Copy the address', lead: <Icon icon={Copy} size="sm" />, onSelect: () => void navigator.clipboard.writeText(shot.address).then(() => toast('Copied.')) },
        ]} />
      </figcaption>
    </figure>
  )
}

type Tab = { kind: 'file'; entry: FolderEntry } | { kind: 'artifact'; id: string; n?: number; name: string }
const tabKey = (t: Tab) => (t.kind === 'file' ? `f:${t.entry.path}` : `a:${t.id}`)

/** The session's folder: its files to look at and edit, and the files models gave. */
export function FolderPanel({ session, folder, focus, wide, onWide, onClose, empty, leaving }: { session: string; folder: string | null; focus: { id: string; n: number } | null; wide: boolean; onWide: () => void; onClose: () => void; empty: React.ReactNode; leaving?: boolean }) {
  const artifacts = useArtifacts(session)
  const [tabs, setTabs] = useState<Tab[]>([])
  const [active, setActive] = useState<string | null>(null)
  const [filter, setFilter] = useState('')
  const [making, setMaking] = useState(false)
  const drafts = useDrafts((s) => s.drafts)
  useEffect(() => { watchFolders() }, [])
  useEffect(() => { setTabs([]); setActive(null) }, [session])
  const open = (t: Tab) => {
    setTabs((all) => (all.some((x) => tabKey(x) === tabKey(t)) ? all.map((x) => (tabKey(x) === tabKey(t) ? t : x)) : [...all, t]))
    setActive(tabKey(t))
  }
  // An artifact opened from the conversation.
  useEffect(() => {
    if (!focus) return
    const name = artifacts?.find((a) => a.id === focus.id)?.name ?? 'File'
    open({ kind: 'artifact', id: focus.id, n: focus.n, name })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [focus?.id, focus?.n, artifacts?.length])
  const close = (key: string) => {
    const at = tabs.findIndex((t) => tabKey(t) === key)
    const rest = tabs.filter((t) => tabKey(t) !== key)
    setTabs(rest)
    if (active === key) setActive(rest.length ? tabKey(rest[Math.min(at, rest.length - 1)]) : null)
  }
  const current = tabs.find((t) => tabKey(t) === active) ?? null
  const openFile = (entry: FolderEntry) => open({ kind: 'file', entry })
  const unsaved = useMemo(() => new Set(tabs.filter((t) => t.kind === 'file' && drafts[draftKey(session, t.entry.path)]).map(tabKey)), [tabs, drafts, session])
  return (
    <aside className="panel" aria-label="Files" data-leaving={leaving ? '' : undefined}>
      <PanelHead wide={wide} onWide={onWide} onClose={onClose}>
        <div className="panel__tabs" role="tablist" aria-label="Open files">
          {tabs.length === 0 && <span className="panel__name panel__name--quiet"><Icon icon={FolderOpen} size="sm" /> {folder ? folder.split(/[\\/]/).filter(Boolean).at(-1) : 'Files'}</span>}
          {tabs.map((t) => {
            const key = tabKey(t)
            const name = t.kind === 'file' ? t.entry.name : t.name
            return (
              <span key={key} className="panel__tab" role="tab" aria-selected={key === active} data-unsaved={unsaved.has(key) ? '' : undefined}>
                <button type="button" className="panel__tab-name" onClick={() => setActive(key)}>{t.kind === 'artifact' && <Icon icon={Gift} size="sm" />}{name}</button>
                <button type="button" className="panel__tab-close" aria-label={`Close ${name}`} onClick={() => close(key)}><Icon icon={X} size="sm" /></button>
              </span>
            )
          })}
        </div>
        <span className="app-spacer" />
        {folder && <Tip label="New file"><Button size="sm" variant="ghost" iconOnly aria-label="New file" onClick={() => setMaking(true)}><Icon icon={FilePlus} size="sm" /></Button></Tip>}
      </PanelHead>
      <div className="panel__body folder-panel">
        <div className="folder-panel__view">
          {current?.kind === 'file' ? <Viewer key={`${session}\u0000${current.entry.path}`} bare session={session} entry={current.entry} folder={folder ?? ''} onRenamed={(e) => { close(tabKey(current)); openFile(e) }} onDeleted={() => close(tabKey(current))} />
            : current?.kind === 'artifact' ? <ArtifactView key={`${current.id}:${current.n ?? 'last'}`} session={session} id={current.id} n={current.n} folder={folder} onPut={openFile} />
            : (
              <div className="panel__empty">
                <span className="panel__empty-art"><Icon icon={FolderOpen} size="xl" /></span>
                {folder ? <p>Pick a file.</p> : empty}
              </div>
            )}
        </div>
        <nav className="folder-panel__tree" aria-label="The folder">
          {folder && (
            <label className="folder-panel__filter">
              <Icon icon={Search} size="sm" />
              <input className="d-input" placeholder="Filter" aria-label="Filter the files by name" value={filter} onChange={(e) => setFilter(e.target.value)} spellCheck={false} />
            </label>
          )}
          <div className="folder-panel__scroll">
            {folder && <Branch key={session} session={session} path="" depth={0} open={current?.kind === 'file' ? current.entry.path : null} onOpen={openFile} filter={filter} />}
            {artifacts && artifacts.length > 0 && <>
              <p className="d-list__label"><Icon icon={Gift} size="sm" /> Given to you</p>
              <ArtifactList artifacts={artifacts} open={current?.kind === 'artifact' ? current.id : null} onOpen={(a) => open({ kind: 'artifact', id: a.id, name: a.name })} />
            </>}
            {folder && <DeletedFiles session={session} onBack={openFile} />}
          </div>
        </nav>
      </div>
      {making && <NameIt title="New file" text="In the session's folder." initial="notes.md" action="Make it"
        done={async (path) => { try { openFile(await newFile(session, path)); return null } catch (e) { return e instanceof CoreProblem && e.status === 409 ? `${path} is there already.` : (e as Error).message } }} onClose={() => setMaking(false)} />}
    </aside>
  )
}
