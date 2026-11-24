import { useState } from 'react'
import { NavLink, Navigate, Route, Routes, useParams } from 'react-router'
import { AppWindow, Calendar, Cloud, FileText, Folder, GitBranch, Globe, Lock, Notebook, Plus, Search, Terminal, Wrench } from 'lucide-react'
import type { LucideIcon } from 'lucide-react'
import type { Feature } from '@/app/registry'
import { useCore } from '@/data/core'
import { RealTools } from './real'
import { auditTool, convertTool, installTool, resetToolFile, saveToolFile, setToolEnabled, toast, updateTool, useSetting, useStore } from '@/data/store'
import type { Tool, ToolKind } from '@/data/types'
import { Button, Dialog, Facts, Glyph, Icon, Page, Section, Signal, Switch } from '@/ui'
import { READER, filesOf, readerOf, tokens } from './files'

const ICONS: Record<string, LucideIcon> = { globe: Globe, folder: Folder, terminal: Terminal, 'app-window': AppWindow, 'git-branch': GitBranch, notebook: Notebook, cloud: Cloud, 'file-text': FileText, calendar: Calendar }
const KIND: Record<ToolKind, { label: string; badge: string; cls: string }> = {
  builtin: { label: 'Built in', badge: 'Built in', cls: 'd-badge' },
  tool: { label: 'Tools', badge: 'Tool', cls: 'd-badge' },
  external: { label: 'External', badge: 'External', cls: 'd-badge d-badge--outline' },
  mcp: { label: 'Deprecated', badge: 'Deprecated · MCP', cls: 'd-badge d-badge--dashed' },
  skill: { label: 'Deprecated', badge: 'Deprecated · skill', cls: 'd-badge d-badge--dashed' },
}
const EFFECT = { read: 'reads', write: 'writes', destructive: 'can destroy', external: 'outside this computer' } as const
const deprecated = (t: Tool) => t.kind === 'mcp' || t.kind === 'skill'

function ToolList() {
  const tools = useStore((s) => s.tools)
  const [q, setQ] = useState('')
  const needle = q.trim().toLowerCase()
  const shown = tools.filter((t) => !needle || `${t.name} ${t.summary} ${t.publisher}`.toLowerCase().includes(needle))
  const groups: [string, Tool[]][] = [
    ['On this computer', shown.filter((t) => t.installed && !deprecated(t))],
    ['Deprecated', shown.filter((t) => t.installed && deprecated(t))],
    ['Available', shown.filter((t) => !t.installed)],
  ]
  const cost = tools.filter((t) => t.installed && t.enabled).reduce((n, t) => n + t.indexTokens, 0)
  return (
    <aside className="app-side" aria-label="Tools">
      <div className="app-side__head"><span className="d-type-headline">Tools</span></div>
      <label className="d-input-wrap">
        <Icon icon={Search} />
        <input className="d-input" type="search" placeholder="Search tools" aria-label="Search tools" value={q} onChange={(e) => setQ(e.target.value)} />
      </label>
      <div className="app-side__scroll">
        {groups.map(([label, list]) =>
          list.length ? (
            <div key={label}>
              <p className="d-list__label">{label}</p>
              <ul className="d-list">
                {list.map((t) => (
                  <li key={t.id}>
                    <NavLink to={`/tools/${t.id}`} className="d-row">
                      <span className="d-row__lead"><Icon icon={ICONS[t.icon] ?? Wrench} /></span>
                      <span className="d-row__text">
                        <span className="d-row__title" style={t.enabled || !t.installed ? undefined : { color: 'var(--d-color-text-tertiary)' }}>{t.name}</span>
                        <span className="d-row__sub">{KIND[t.kind].badge}{t.update ? ' · update ready' : ''}{t.installed && !t.enabled ? ' · off' : ''}</span>
                      </span>
                      <span className="d-row__meta">{t.installed ? `${t.indexTokens.toLocaleString()} tok` : ''}</span>
                    </NavLink>
                  </li>
                ))}
              </ul>
            </div>
          ) : null,
        )}
      </div>
      <p className="app-side__foot">Switched-on tools add <strong className="d-mono">{cost.toLocaleString()} tokens</strong> to every conversation. A model sees one line per tool until it needs one.</p>
    </aside>
  )
}

const NO_EDITS: Record<string, string> = {}

/** Every file of the tool, to read and to change: the instructions models get, and the rest. */
function ToolFiles({ tool }: { tool: Tool }) {
  const edits = useStore((s) => s.toolFiles[tool.id]) ?? NO_EDITS
  const published = filesOf(tool)
  const files = [
    ...published.map((f) => ({ ...f, text: edits[f.path] ?? f.text, edited: f.path in edits, added: false })),
    ...Object.keys(edits).filter((p) => !published.some((f) => f.path === p)).map((p) => ({ path: p, text: edits[p], reads: readerOf(p), locked: undefined, edited: true, added: true })),
  ]
  const [open, setOpen] = useState((files.find((f) => f.reads === 'picked' && !f.locked) ?? files[0]).path)
  const [draft, setDraft] = useState<string | null>(null)
  const [naming, setNaming] = useState<string | null>(null)
  const file = files.find((f) => f.path === open) ?? files[0]
  const text = draft ?? file.text
  const dirty = draft !== null && draft !== file.text
  const pick = (path: string) => { setOpen(path); setDraft(null) }
  const add = () => {
    const path = (naming ?? '').trim().replace(/^\/+/, '')
    if (!path || files.some((f) => f.path === path)) return
    saveToolFile(tool.id, path, '')
    setNaming(null)
    pick(path)
  }
  const whenRead = files.filter((f) => f.reads === 'picked').reduce((n, f) => n + tokens(f.text), 0)
  return (
    <>
      <div className="tools__files">
        <div className="tools__tree">
          <ul className="d-list">
            {files.map((f) => (
              <li key={f.path}>
                <button type="button" className="d-row d-row--compact" aria-current={f.path === file.path} onClick={() => pick(f.path)}>
                  <span className="d-row__lead"><Icon icon={f.locked ? Lock : FileText} size="sm" /></span>
                  <span className="d-row__text"><span className="d-row__title d-mono">{f.path}</span></span>
                  <span className="d-row__meta">{f.added ? 'yours' : f.edited ? 'edited' : ''}</span>
                </button>
              </li>
            ))}
          </ul>
          <Button variant="ghost" size="sm" onClick={() => setNaming('references/')}><Icon icon={Plus} size="sm" />Add a file</Button>
        </div>
        <div className="tools__edit">
          <div className="tools__edit-head">
            <span className="d-mono">{file.path}</span>
            <span className="d-type-caption d-text-tertiary">{READER[file.reads]}{file.reads === 'always' || file.reads === 'picked' ? ` · about ${tokens(text).toLocaleString()} tokens` : ''}</span>
          </div>
          <textarea className="files__edit tools__text" spellCheck={false} aria-label={file.path} value={text} readOnly={!!file.locked} onChange={(e) => setDraft(e.target.value)} />
          <div className="tools__edit-foot">
            {file.locked ? <span className="d-type-caption d-text-secondary">{file.locked}</span> : <span className="d-type-caption d-text-tertiary">Your edits are kept when the tool updates. Saving clears the audit.</span>}
            <span className="app-spacer" />
            {dirty && <Button variant="ghost" size="sm" onClick={() => setDraft(null)}>Discard</Button>}
            {dirty && <Button variant="primary" size="sm" onClick={() => { saveToolFile(tool.id, file.path, draft); setDraft(null); toast(`${file.path} saved. ${tool.installed ? 'The tool is "not audited" until you audit it again.' : ''}`) }}>Save</Button>}
            {!dirty && file.edited && <Button variant="ghost" size="sm" onClick={() => { resetToolFile(tool.id, file.path); setDraft(null); if (file.added) setOpen(published[0].path) }}>{file.added ? 'Delete the file' : 'Back to the published text'}</Button>}
          </div>
        </div>
      </div>
      <p className="d-type-caption d-text-tertiary">{files.length} files. A model that picks this tool reads about {whenRead.toLocaleString()} tokens of instructions; until then it sees one line.</p>
      {naming !== null && (
        <Dialog open onOpenChange={(o) => !o && setNaming(null)} title="Add a file" text="Markdown and text files are instructions: models read them when they pick the tool. Anything else is for the tool to run."
          actions={<><Button onClick={() => setNaming(null)}>Cancel</Button><Button variant="primary" disabled={!naming.trim() || naming.endsWith('/')} onClick={add}>Add</Button></>}>
          <form onSubmit={(e) => { e.preventDefault(); add() }} style={{ marginBlockStart: 'var(--d-space-16)' }}>
            <label className="d-field"><span className="d-field__label">Path inside the tool</span><input className="d-input d-input--mono" autoFocus value={naming} onChange={(e) => setNaming(e.target.value)} placeholder="references/examples.md" /></label>
          </form>
        </Dialog>
      )}
    </>
  )
}

function ToolPage() {
  const { id } = useParams()
  const tool = useStore((s) => s.tools.find((t) => t.id === id))
  const auditFirst = useSetting('tools.auditBeforeInstall', true)
  if (!tool) return <Navigate to="/tools" replace />
  const k = KIND[tool.kind]
  return (
    <Page key={tool.id} title={tool.name}
      lead={<>{tool.summary}</>}
      actions={
        tool.installed ? (
          <label className="d-check"><Switch label={`Use ${tool.name}`} checked={tool.enabled} onChange={(v) => setToolEnabled(tool.id, v)} /><span className="d-check__text">{tool.enabled ? 'On' : 'Off'}</span></label>
        ) : (
          <Button variant="primary" disabled={tool.audit === 'running'} onClick={() => void installTool(tool.id)}>{tool.audit === 'running' ? 'Auditing…' : auditFirst ? 'Audit, then install' : 'Install'}</Button>
        )
      }>
      <div className="app-row">
        <span className={k.cls}>{k.badge}</span>
        <span className="d-badge d-badge--outline">{tool.publisher} · {tool.version}</span>
        {tool.audit === 'passed' && <span className="d-badge" data-tone="success"><Glyph name="task-done" />Audit passed</span>}
        {tool.audit === 'failed' && <span className="d-badge" data-tone="danger"><Glyph name="task-failed" />Audit failed</span>}
        {tool.audit === 'none' && <span className="d-badge d-badge--outline">Not audited</span>}
        {tool.audit === 'running' && <span className="d-badge"><Glyph name="task-running" />Auditing with Qwen3 4B</span>}
      </div>

      {deprecated(tool) && (
        <Signal level="caution" actions={<Button size="sm" variant="primary" disabled={tool.audit === 'running'} onClick={() => void convertTool(tool.id)}>{tool.audit === 'running' ? 'Converting…' : 'Convert to a Demido tool'}</Button>}>
          {tool.kind === 'mcp' ? 'A classic MCP server. It works, but it loads all of its descriptions into every conversation and runs all the time.' : 'A classic skill. It works, but its long instructions cost context, and it brings no functions.'} Converting keeps what it does, writes it a short index line and short guidance, and starts it only when used. It then shows as External.
        </Signal>
      )}
      {tool.update && (
        <Signal level="note" actions={<Button size="sm" disabled={tool.audit === 'running'} onClick={() => void updateTool(tool.id)}>{auditFirst ? `Audit, then update to ${tool.update}` : `Update to ${tool.update}`}</Button>}>
          Version {tool.update} is available. Automatic updates are off, so nothing changes until you say so.
        </Signal>
      )}
      {tool.auditNote && <Signal level={tool.audit === 'failed' ? 'warning' : 'note'}>Audit by Qwen3 4B: {tool.auditNote} An audit by a model is a second pair of eyes, not a guarantee.</Signal>}

      <Section title="What it may do">
        <div className="d-card"><div className="d-card__body">
          <Facts rows={[['Can', tool.can.join(', ') || 'nothing on its own'], ['Cannot', tool.cannot.join(', ') || 'not declared'], ['Cost in every conversation', `${tool.indexTokens.toLocaleString()} tokens${tool.rawTokens && tool.rawTokens !== tool.indexTokens ? ` (was ${tool.rawTokens.toLocaleString()})` : ''}`]]} />
        </div></div>
      </Section>

      {tool.functions.length > 0 && (
        <Section title="Functions" hint="A model sees these only after it picks this tool.">
          <div className="d-card"><div className="d-card__body">
            <table className="app-table">
              <tbody>
                {tool.functions.map((f) => (
                  <tr key={f.name}>
                    <td className="d-mono">{f.name}</td>
                    <td className="d-text-secondary">{f.summary}</td>
                    <td style={{ textAlign: 'end', paddingInlineEnd: 0 }}><span className="d-badge" data-tone={f.effect === 'destructive' ? 'danger' : undefined}>{f.effect === 'destructive' && <Glyph name="task-failed" />}{EFFECT[f.effect]}</span></td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div></div>
        </Section>
      )}

      <Section title="Instructions and files" hint="Everything this tool is made of. All of it is yours to read and change.">
        <ToolFiles key={tool.id} tool={tool} />
        <div className="app-row">
          <span className="app-spacer" />
          {tool.installed && !deprecated(tool) && <Button disabled={tool.audit === 'running'} onClick={() => void auditTool(tool.id)}>{tool.audit === 'running' ? 'Auditing…' : 'Audit now'}</Button>}
        </div>
      </Section>
    </Page>
  )
}

function PreviewTools() {
  return (
    <div className="app-split">
      <ToolList />
      <Routes>
        <Route path=":id" element={<ToolPage />} />
        <Route path="*" element={<Navigate to="/tools/web-search" replace />} />
      </Routes>
    </div>
  )
}

/** With a core behind the pages, the real one; without, the preview of where it is going. */
function ToolsPage() {
  const core = useCore()
  if (core.state === 'looking') return null
  return core.state === 'ready' ? <RealTools /> : <PreviewTools />
}

export const feature: Feature = {
  id: 'tools', title: 'Tools', icon: Wrench, group: 'library', order: 1, path: 'tools', Page: ToolsPage,
  settings: [
    {
      id: 'tools', title: 'Tools', lead: 'Skills and MCP servers are both Tools here.', order: 5,
      settings: [
        /* The same two ids as the core's registry: with a core, these two rows are drawn from its description, first. */
        { id: 'tools.use', type: 'switch', label: 'Let models use tools', desc: 'Before it answers, a model is asked whether it needs a tool.', default: true },
        { id: 'tools.plan', type: 'select', label: 'Split work into tasks', desc: 'When a request is split into a list of tasks, which the model then does one at a time.', default: 'asked', options: [{ value: 'asked', label: 'When I ask' }, { value: 'always', label: 'Always' }] },
        { id: 'tools.auditBeforeInstall', type: 'switch', label: 'Audit before installing or updating', desc: 'A model you choose reads every tool and its changes first.', default: true,
          details: 'Default on. The auditor sees the source or the change, the permissions asked for, and every description a model would read. It catches the obvious; it is not a guarantee.' },
        { id: 'tools.auditModel', type: 'select', label: 'Audit with', desc: 'The model that does the audit.', default: 'qwen3-4b', options: [{ value: 'qwen3-4b', label: 'Qwen3 4B Instruct' }, { value: 'gemma-3n-e4b', label: 'Gemma 3n E4B' }, { value: 'claude', label: 'Claude, through the Anthropic API' }] },
        { id: 'tools.autoUpdate', type: 'switch', label: 'Update tools automatically', desc: 'Installs new versions without asking.', default: false,
          details: 'Default off. On means you accept updates you have not looked at. With auditing on, each update is still audited first and held back if the audit fails.' },
      ],
    },
  ],
}
