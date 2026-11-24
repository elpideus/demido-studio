/* Tools, for real (DECISIONS C6 to C12; research notes 08 and 34): the tools the person installed, each a package
   of a WebAssembly component checked against its publisher's signature, run with only what it asks for.

   Installing shows the publisher's key (to compare with the one they publish) and what the tool asks for, and
   installs only on a yes. An update must be signed with the same key; one that asks for more asks again. The
   guidance a model reads, and the line it sees in the list, are the person's to change, and an update keeps
   the change. Demido's own tools are listed too, as they are: built in.

   MCP servers and skills come in too (C14 to C17): a server is a program, run shut in the sandbox for each call,
   its tools listed once when it is added and asked about as any command a model runs; a skill is guidance only,
   its text given to a model as someone else's, its scripts never run. */
import { useEffect, useState } from 'react'
import { BookOpen, Download, PackagePlus, PenLine, Plug, RefreshCw, Trash2, Wrench } from 'lucide-react'
import { coreGet, coreSend } from '@/data/core'
import { toast } from '@/data/store'
import { Button, Dialog, Glyph, Icon, NotBuilt, Page, Section, Signal } from '@/ui'

interface Asks { folder: 'none' | 'read' | 'write'; network: string[] }
interface Manifest { name: string; version: string; publisher: string; publisher_key: string; about: string; does: string; args: { name: string; kind: string; optional: boolean }[]; asks: Asks; updates: string | null }
interface Installed { manifest: Manifest; key_id: string; installed_at: number; guidance: string; edited: boolean; does: string }
interface Checked { manifest: Manifest; key_id: string; replaces: string | null; asks_more: string[]; asks_said: string }
interface Latest { version: string; package: string; signature: string }
interface ServerTool { name: string; description: string }
interface McpServer { name: string; command: string; tools: ServerTool[]; added_at: number }
interface Skill { name: string; description: string; text: string; added_at: number }

const asksSaid = (a: Asks) => `${a.folder === 'write' ? 'reads and changes files in the folder' : a.folder === 'read' ? 'reads files in the folder' : 'nothing of the folder'}; ${a.network.length ? `pages from ${a.network.join(', ')}` : 'nothing from the internet'}`

const asBase64 = (file: File) => new Promise<string>((done, failed) => {
  const r = new FileReader()
  r.onload = () => done(String(r.result).split(',')[1] ?? '')
  r.onerror = () => failed(new Error('the file could not be read'))
  r.readAsDataURL(file)
})

function InstallDialog({ onClose, onDone }: { onClose: () => void; onDone: () => void }) {
  const [pkg, setPkg] = useState<File | null>(null)
  const [sig, setSig] = useState<File | null>(null)
  const [checked, setChecked] = useState<Checked | null>(null)
  const [given, setGiven] = useState<{ package: string; signature: string } | null>(null)
  const [problem, setProblem] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const check = async () => {
    if (!pkg || !sig) return
    setBusy(true); setProblem(null)
    try {
      const g = { package: await asBase64(pkg), signature: await sig.text() }
      setChecked(await coreSend<Checked>('POST', '/tools/check', g))
      setGiven(g)
    } catch (e) {
      setProblem((e as Error).message)
    }
    setBusy(false)
  }
  const install = async () => {
    if (!given) return
    setBusy(true)
    try {
      const done = await coreSend<Installed>('POST', '/tools/install', { ...given, agreed: true })
      toast(`Installed: ${done.manifest.name} ${done.manifest.version}.`, undefined, 'task-done')
      onDone()
    } catch (e) {
      setProblem((e as Error).message)
    }
    setBusy(false)
  }
  return (
    <Dialog open wide onOpenChange={(o) => !o && onClose()} title={checked ? `Install ${checked.manifest.name} ${checked.manifest.version}?` : 'Install a tool'}
      text={checked ? undefined : 'A tool comes as a package (.demido-tool) and its publisher\'s signature (.minisig). Demido checks the signature first, then shows you who signed it and what the tool asks for.'}
      actions={checked
        ? <><Button onClick={onClose}>Cancel</Button><Button variant="primary" disabled={busy} onClick={() => void install()}>{busy ? 'Installing…' : 'Trust and install'}</Button></>
        : <><Button onClick={onClose}>Cancel</Button><Button variant="primary" disabled={busy || !pkg || !sig} onClick={() => void check()}>{busy ? 'Checking…' : 'Check it'}</Button></>}>
      <div className="app-stack" style={{ marginBlockStart: 'var(--d-space-16)' }}>
        {!checked && (
          <>
            <label className="d-field"><span className="d-field__label">The package</span><input className="d-input" type="file" accept=".demido-tool,.tar.gz,application/gzip" onChange={(e) => setPkg(e.target.files?.[0] ?? null)} /></label>
            <label className="d-field"><span className="d-field__label">Its signature</span><input className="d-input" type="file" accept=".minisig" onChange={(e) => setSig(e.target.files?.[0] ?? null)} /></label>
          </>
        )}
        {checked && (
          <>
            <p><strong>{checked.manifest.about}</strong></p>
            <p className="d-text-secondary">By {checked.manifest.publisher}, signed with the key <span className="d-mono">{checked.key_id}</span>. Compare it with the key the publisher shows on their own page: Demido can tell that the package was signed by this key and not changed since, not who holds the key. Updates must be signed with the same key.</p>
            <Signal level={checked.manifest.asks.folder === 'write' || checked.manifest.asks.network.length ? 'caution' : 'note'}>{checked.asks_said}</Signal>
            {checked.replaces && <p className="d-type-caption">It replaces version {checked.replaces}.{checked.asks_more.length ? ` It asks for more than that one: ${checked.asks_more.join(', ')}.` : ''}</p>}
            <p className="d-type-caption d-text-tertiary">It runs shut in, in Wasmtime: no files, no network, no programs, nothing of yours but what it asks for here, and for 30 seconds and 256 MB at most. What it gives back is treated as text from outside.</p>
          </>
        )}
        {problem && <Signal level="caution">{problem}</Signal>}
      </div>
    </Dialog>
  )
}

function EditDialog({ tool, onClose, onDone }: { tool: Installed; onClose: () => void; onDone: () => void }) {
  const [does, setDoes] = useState(tool.does)
  const [guidance, setGuidance] = useState(tool.guidance)
  const save = async (back = false) => {
    try {
      await coreSend('PUT', `/tools/installed/${encodeURIComponent(tool.manifest.name)}/guidance`, back ? { guidance: null, does: null } : { guidance, does })
      toast(back ? `${tool.manifest.name}: its package's words are back.` : `${tool.manifest.name}: your words are kept, and an update keeps them.`, undefined, 'task-done')
      onDone()
    } catch (e) {
      toast(`Not kept. ${(e as Error).message}`, undefined, 'task-failed')
    }
  }
  return (
    <Dialog open wide onOpenChange={(o) => !o && onClose()} title={`What a model is told about ${tool.manifest.name}`}
      actions={<>{tool.edited && <Button variant="ghost" onClick={() => void save(true)}>Its package's words</Button>}<Button onClick={onClose}>Cancel</Button><Button variant="primary" onClick={() => void save()}>Keep</Button></>}>
      <div className="app-stack" style={{ marginBlockStart: 'var(--d-space-16)' }}>
        <label className="d-field"><span className="d-field__label">The line in the list of tools</span><input className="d-input" value={does} maxLength={400} onChange={(e) => setDoes(e.target.value)} /><span className="d-field__help">One or two short sentences, saying what a model cannot do without it. Small models read this to choose.</span></label>
        <label className="d-field"><span className="d-field__label">Its guidance</span><textarea className="d-input d-input--mono" rows={10} value={guidance} onChange={(e) => setGuidance(e.target.value)} /><span className="d-field__help">Read when a model picks the tool.</span></label>
      </div>
    </Dialog>
  )
}

function ToolCard({ tool, onChanged }: { tool: Installed; onChanged: () => void }) {
  const [editing, setEditing] = useState(false)
  const [removing, setRemoving] = useState(false)
  const [latest, setLatest] = useState<Latest | null>(null)
  const [busy, setBusy] = useState(false)
  const m = tool.manifest
  const lookForUpdate = async () => {
    setBusy(true)
    try {
      const l = await coreGet<Latest>(`/tools/installed/${encodeURIComponent(m.name)}/latest`)
      setLatest(l)
      if (l.version === m.version) toast(`${m.name} ${m.version} is the newest.`)
    } catch (e) {
      toast(`Not looked at. ${(e as Error).message}`, undefined, 'task-failed')
    }
    setBusy(false)
  }
  const update = async () => {
    setBusy(true)
    try {
      const done = await coreSend<Installed>('POST', `/tools/installed/${encodeURIComponent(m.name)}/update`, { agreed: false })
      toast(`${m.name} is now ${done.manifest.version}.`, undefined, 'task-done')
      onChanged()
    } catch (e) {
      // An update that asks for more says so; the person installs it from its package, where it is shown whole.
      toast(`Not updated. ${(e as Error).message}`, undefined, 'task-failed')
    }
    setBusy(false)
  }
  return (
    <article className="d-card">
      <header className="d-card__head">
        <Glyph name="task-done" size="1.25rem" style={{ marginTop: '0.125rem' }} label="Installed" />
        <div className="app-stack" style={{ gap: 'var(--d-space-2)', flex: 1 }}>
          <h3 className="d-card__title"><span className="d-mono">{m.name}</span> {m.version}</h3>
          <span className="d-card__sub">by {m.publisher} · key {tool.key_id} · {asksSaid(m.asks)}{tool.edited ? ' · its guidance is yours' : ''}</span>
        </div>
        <Button size="sm" variant="ghost" onClick={() => setEditing(true)}><Icon icon={PenLine} size="sm" />Guidance</Button>
        {m.updates && <Button size="sm" variant="ghost" disabled={busy} onClick={() => void lookForUpdate()}><Icon icon={RefreshCw} size="sm" />Update?</Button>}
        <Button size="sm" variant="ghost" onClick={() => setRemoving(true)}><Icon icon={Trash2} size="sm" />Remove</Button>
      </header>
      <div className="d-card__body app-stack">
        <p className="d-text-secondary">{m.about}</p>
        <p className="d-type-caption"><span className="d-text-tertiary">A model sees:</span> <span className="d-mono">{m.name}({m.args.map((a) => `${a.name}${a.optional ? '?' : ''}: ${a.kind}`).join(', ')})</span>: {tool.does}</p>
        {latest && latest.version !== m.version && <Signal level="note" actions={<Button size="sm" disabled={busy} onClick={() => void update()}><Icon icon={Download} size="sm" />Update to {latest.version}</Button>}>Version {latest.version} is announced. It is checked against the same publisher's key before anything is replaced.</Signal>}
      </div>
      {editing && <EditDialog tool={tool} onClose={() => setEditing(false)} onDone={() => { setEditing(false); onChanged() }} />}
      {removing && <Dialog open onOpenChange={(o) => !o && setRemoving(false)} title={`Remove ${m.name}?`} text="Models are no longer offered it, and your edits of its guidance go with it." actions={<><Button onClick={() => setRemoving(false)}>Cancel</Button><Button variant="danger" onClick={() => { setRemoving(false); void coreSend('DELETE', `/tools/installed/${encodeURIComponent(m.name)}`).then(onChanged, (e: Error) => toast(`Not removed. ${e.message}`, undefined, 'task-failed')) }}>Remove</Button></>} />}
    </article>
  )
}

function AddServerDialog({ onClose, onDone }: { onClose: () => void; onDone: () => void }) {
  const [name, setName] = useState('')
  const [command, setCommand] = useState('')
  const [problem, setProblem] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const add = async () => {
    setBusy(true); setProblem(null)
    try {
      const done = await coreSend<McpServer>('POST', '/tools/mcp', { name, command })
      toast(`Added: ${done.name}, with ${done.tools.length} tool${done.tools.length === 1 ? '' : 's'}.`, undefined, 'task-done')
      onDone()
    } catch (e) {
      setProblem((e as Error).message)
    }
    setBusy(false)
  }
  return (
    <Dialog open wide onOpenChange={(o) => !o && onClose()} title="Add an MCP server"
      text="A server is a program. Demido starts it in the sandbox, with no network and nothing of yours but the session's folder (or a folder of its own), asks it for its tools once, and keeps that list: a server cannot change its tools later without you adding it again."
      actions={<><Button onClick={onClose}>Cancel</Button><Button variant="primary" disabled={busy || name.trim().length < 2 || !command.trim()} onClick={() => void add()}>{busy ? 'Starting it…' : 'Add'}</Button></>}>
      <div className="app-stack" style={{ marginBlockStart: 'var(--d-space-16)' }}>
        <label className="d-field"><span className="d-field__label">Its name</span><input className="d-input" value={name} maxLength={40} placeholder="notes" onChange={(e) => setName(e.target.value)} /><span className="d-field__help">Small letters, digits and _. Its tools are offered as mcp_name_tool.</span></label>
        <label className="d-field"><span className="d-field__label">The command that starts it</span><input className="d-input d-input--mono" value={command} placeholder="python3 server.py" onChange={(e) => setCommand(e.target.value)} /><span className="d-field__help">Run in Demido's folder mcp/name when the session has none. A server that needs the internet to start (npx, uvx) cannot fetch itself inside the sandbox: install it first.</span></label>
        {problem && <Signal level="caution">{problem}</Signal>}
      </div>
    </Dialog>
  )
}

function AddSkillDialog({ onClose, onDone }: { onClose: () => void; onDone: () => void }) {
  const [text, setText] = useState('')
  const [problem, setProblem] = useState<string | null>(null)
  const add = async () => {
    setProblem(null)
    try {
      const done = await coreSend<Skill>('POST', '/tools/skills', { text })
      toast(`Brought in: ${done.name}.`, undefined, 'task-done')
      onDone()
    } catch (e) {
      setProblem((e as Error).message)
    }
  }
  return (
    <Dialog open wide onOpenChange={(o) => !o && onClose()} title="Bring in a skill"
      text="A skill is a SKILL.md: a name and a description at its top, then instructions. A model is offered it by its description and, when it picks it, is given its text to follow, as text someone else wrote. Its scripts are not run."
      actions={<><Button onClick={onClose}>Cancel</Button><Button variant="primary" disabled={!text.trim()} onClick={() => void add()}>Bring it in</Button></>}>
      <div className="app-stack" style={{ marginBlockStart: 'var(--d-space-16)' }}>
        <label className="d-field"><span className="d-field__label">Its SKILL.md</span><input className="d-input" type="file" accept=".md,text/markdown,text/plain" onChange={(e) => { const f = e.target.files?.[0]; if (f) void f.text().then(setText) }} /></label>
        <label className="d-field"><span className="d-field__label">Or its text</span><textarea className="d-input d-input--mono" rows={10} value={text} placeholder={'---\nname: release-notes\ndescription: Write release notes from a changelog.\n---\n\nRead CHANGELOG.md first…'} onChange={(e) => setText(e.target.value)} /></label>
        {problem && <Signal level="caution">{problem}</Signal>}
      </div>
    </Dialog>
  )
}

function Removable({ what, name, path, onChanged }: { what: string; name: string; path: string; onChanged: () => void }) {
  const [removing, setRemoving] = useState(false)
  return (
    <>
      <Button size="sm" variant="ghost" onClick={() => setRemoving(true)}><Icon icon={Trash2} size="sm" />Remove</Button>
      {removing && <Dialog open onOpenChange={(o) => !o && setRemoving(false)} title={`Remove ${name}?`} text={`Models are no longer offered this ${what}.`} actions={<><Button onClick={() => setRemoving(false)}>Cancel</Button><Button variant="danger" onClick={() => { setRemoving(false); void coreSend('DELETE', path).then(onChanged, (e: Error) => toast(`Not removed. ${e.message}`, undefined, 'task-failed')) }}>Remove</Button></>} />}
    </>
  )
}

function External() {
  const [servers, setServers] = useState<McpServer[] | null>(null)
  const [skills, setSkills] = useState<Skill[] | null>(null)
  const [adding, setAdding] = useState<'server' | 'skill' | null>(null)
  const read = () => {
    void coreGet<McpServer[]>('/tools/mcp').then(setServers, () => setServers([]))
    void coreGet<Skill[]>('/tools/skills').then(setSkills, () => setSkills([]))
  }
  useEffect(read, [])
  return (
    <>
      <Section title="MCP servers" hint="Programs of other people's that offer tools. Each call starts the server shut in the sandbox; at Balanced every use is asked about, as any command a model runs is. Without a sandbox none is run.">
        <div className="app-stack">
          {servers?.map((s) => (
            <article key={s.name} className="d-card">
              <header className="d-card__head">
                <Glyph name="task-done" size="1.25rem" style={{ marginTop: '0.125rem' }} label="Added" />
                <div className="app-stack" style={{ gap: 'var(--d-space-2)', flex: 1 }}>
                  <h3 className="d-card__title"><span className="d-mono">{s.name}</span></h3>
                  <span className="d-card__sub d-mono">{s.command}</span>
                </div>
                <Removable what="server's tools" name={s.name} path={`/tools/mcp/${encodeURIComponent(s.name)}`} onChanged={read} />
              </header>
              <div className="d-card__body app-stack">
                {s.tools.map((t) => <p key={t.name} className="d-type-caption"><span className="d-mono">{t.name}</span> <span className="d-text-secondary">{t.description}</span></p>)}
              </div>
            </article>
          ))}
          {servers && servers.length === 0 && <p className="app-empty"><Icon icon={Plug} /> No MCP server is added.</p>}
          <div><Button onClick={() => setAdding('server')}><Icon icon={Plug} />Add an MCP server…</Button></div>
        </div>
      </Section>
      <Section title="Skills" hint="Instructions other people wrote for a kind of task. Only their text is given to a model, fenced as someone else's; nothing of theirs is run.">
        <div className="app-stack">
          {skills?.map((s) => (
            <article key={s.name} className="d-card">
              <header className="d-card__head">
                <Glyph name="task-done" size="1.25rem" style={{ marginTop: '0.125rem' }} label="Brought in" />
                <div className="app-stack" style={{ gap: 'var(--d-space-2)', flex: 1 }}>
                  <h3 className="d-card__title">{s.name}</h3>
                  <span className="d-card__sub">{s.description || 'No description'}</span>
                </div>
                <Removable what="skill" name={s.name} path={`/tools/skills/${encodeURIComponent(s.name)}`} onChanged={read} />
              </header>
            </article>
          ))}
          {skills && skills.length === 0 && <p className="app-empty"><Icon icon={BookOpen} /> No skill is brought in.</p>}
          <div><Button onClick={() => setAdding('skill')}><Icon icon={BookOpen} />Bring in a skill…</Button></div>
        </div>
      </Section>
      {adding === 'server' && <AddServerDialog onClose={() => setAdding(null)} onDone={() => { setAdding(null); read() }} />}
      {adding === 'skill' && <AddSkillDialog onClose={() => setAdding(null)} onDone={() => { setAdding(null); read() }} />}
    </>
  )
}

export function RealTools() {
  const [installed, setInstalled] = useState<Installed[] | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [adding, setAdding] = useState(false)
  const read = () => coreGet<Installed[]>('/tools/installed').then((t) => { setInstalled(t); setError(null) }, (e: Error) => setError(e.message))
  useEffect(() => { void read() }, [])
  return (
    <Page title="Tools" lead="What a model can ask Demido to do. Demido's own tools are built in; the ones you install are packages of other people's, each shut in and given only what it asks for.">
      {error && <Signal level="caution">The core did not say. {error}</Signal>}
      <Section title="Installed by you" hint="A package is checked against its publisher's signature before anything else; you are shown their key and what the tool asks for, and it is installed only on your yes.">
        <div className="app-stack">
          {installed?.map((t) => <ToolCard key={t.manifest.name} tool={t} onChanged={() => void read()} />)}
          {installed && installed.length === 0 && <p className="app-empty"><Icon icon={Wrench} /> No tool is installed yet.</p>}
          <div><Button variant="primary" onClick={() => setAdding(true)}><Icon icon={PackagePlus} />Install a tool…</Button></div>
        </div>
      </Section>
      <External />
      <NotBuilt block>A list of tools to browse and install from (a registry), MCP servers over the network (HTTP) and their prompts and resources, a skill's scripts, a tool's files besides its guidance, and an audit of a tool by a model.</NotBuilt>
      {adding && <InstallDialog onClose={() => setAdding(false)} onDone={() => { setAdding(false); void read() }} />}
    </Page>
  )
}
