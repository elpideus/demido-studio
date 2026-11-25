/* Sessions, for real: conversations with a model on this computer, or one chosen elsewhere, kept and run by the core.

   The conversation leads. What the session works with sits where it is used: the model, its thinking, the
   permission level and the folder under the message box; the browser and the folder open beside the
   conversation (panels.tsx), from the header and from the card of each page the model opened. Each message has
   its own actions under it: copy, and for the person's own, edit and send again. */
import { useEffect, useRef, useState } from 'react'
import { Popover } from '@base-ui/react/popover'
import { Prose } from '@/ui/prose'
import { ArrowUp, Brain, CalendarClock, ChevronDown, Copy, FilePlus, Folder, FolderOpen, Globe, Info, ListChecks, Loader, Mic, MicOff, MoreHorizontal, Pencil, Plus, RotateCcw, ShieldCheck, ShieldEllipsis, ShieldHalf, ShieldOff, Square, Trash2, Volume2, Wifi, WifiOff, type LucideIcon } from 'lucide-react'
import { Dictation, ReadAloud } from '@/data/voice'
import { Navigate, Route, Routes, useNavigate, useParams } from 'react-router'
import { useLocal } from '@/data/local-models'
import { chosenModels, dollars, useProviders } from '@/data/providers'
import { answerNow, answerPermission, changeDiff, changeSession, deleteSession, makeShorter, newSession, readSession, say, sayAgain, setUpSandbox, stopAnswer, undoChange, useReal, type ContextUse, type CoreChange, type CoreMessage, type CoreStep, type CoreTask, type Level, type PermissionRequest } from '@/data/real-sessions'
import { Button, Dialog, Glyph, Icon, Menu, Signal, StepRow, Tip, cx, type MenuEntry } from '@/ui'
import { asAFile, keepAsFile, openArtifact, useFilesFocus } from '@/data/artifacts'
import { onCoreEvent } from '@/data/core'
import { toast, useSetting } from '@/data/store'
import { runSchedule, scheduleRuns, whenFromNow, type Schedule } from '@/data/schedules'
import { BrowserCard, BrowserPanel, FolderPanel, shotsOf, type Shot } from './panels'
import logo from '@brand/logo/demido-symbol-small.svg?raw'

/** How long ago, short: "now", "5 min", "3 h", "2 d". */
function ago(at: number): string {
  const min = Math.max(0, Math.round((Date.now() - at) / 60_000))
  return min < 1 ? 'now' : min < 60 ? `${min} min` : min < 60 * 48 ? `${Math.round(min / 60)} h` : `${Math.round(min / 60 / 24)} d`
}

/** The model a new session starts with: the core picks one on this computer. With none here, the first one
    the person chose elsewhere, named, so that it is their choice and not the core's. */
function firstModel(): string | undefined {
  if (useLocal.getState().list?.models.length) return undefined
  return chosenModels(useProviders.getState().providers)[0]?.id
}

/** The provider a session's model runs at, by name. `null` for a model on this computer. */
function elsewhere(model: string | null | undefined): string | null {
  if (!model || !model.includes('/')) return null
  const id = model.split('/')[0]
  return useProviders.getState().providers?.find((p) => p.id === id)?.name ?? id
}

/** Whether a session's model is run by a program on this computer (Claude Code, OpenCode). */
function byProgram(model: string | null | undefined): boolean {
  return Boolean(model && ['claude-code/', 'opencode/'].some((p) => model.startsWith(p)))
}

const folderName = (folder: string) => folder.split(/[\\/]/).filter(Boolean).at(-1) ?? folder

function List() {
  const list = useReal((s) => s.list)
  const { '*': rest } = useParams()
  const current = rest?.split('/')[0]
  const navigate = useNavigate()
  const start = async () => {
    const id = await newSession(firstModel())
    if (id) navigate(`/sessions/${id}`)
  }
  return (
    <aside className="app-side" aria-label="Sessions">
      <div className="app-side__head">
        <span className="d-type-headline">Sessions</span>
        <Tip label="New session"><Button size="sm" variant="ghost" iconOnly aria-label="New session" onClick={start}><Icon icon={Plus} /></Button></Tip>
      </div>
      <div className="app-side__scroll">
        {!list && <p className="app-empty"><Glyph name="task-running" busy /></p>}
        {list && !list.length && <p className="app-empty">No sessions yet.</p>}
        <ul className="d-list">
          {list?.map((s) => (
            <li key={s.id}>
              <button type="button" className="d-row" aria-current={s.id === current ? 'true' : undefined} onClick={() => navigate(`/sessions/${s.id}`)}>
                {s.schedule_id && <span className="d-row__lead"><Icon icon={CalendarClock} size="sm" label="A schedule's session" /></span>}
                <span className="d-row__text"><span className="d-row__title">{s.title}</span><span className="d-row__sub">{s.model}</span></span>
                <span className="d-row__meta">{ago(s.updated_at)}</span>
              </button>
            </li>
          ))}
        </ul>
      </div>
    </aside>
  )
}

/** What a tool is called when it is used, in plain words, and which of its arguments says what it was used on. */
const TOOL_SAID: Record<string, [string, string]> = {
  time: ['Read the clock', ''], calculate: ['Calculated', 'expression'], list_files: ['Listed the files', ''], read_file: ['Read', 'path'], search_files: ['Searched the files for', 'text'],
  add_to_file: ['Added to', 'path'], write_file: ['Wrote', 'path'], edit_file: ['Changed', 'path'], run_command: ['Ran', 'command'],
  message_session: ['Told another session', 'to'],
  search_web: ['Searched for', 'query'], give_file: ['Gave you', 'file_name'], read_page: ['Read', 'url'], read_output: ['Read more of the output, from line', 'from_line'],
  // Demido's browser (DECISIONS Z).
  open_page: ['Opened', 'url'], press: ['Pressed', 'number'], type_text: ['Typed', 'text'],
  // Claude Code's own tools, by their names in its stream.
  Read: ['Read', 'file_path'], Write: ['Wrote', 'file_path'], Edit: ['Changed', 'file_path'], MultiEdit: ['Changed', 'file_path'], NotebookEdit: ['Changed', 'notebook_path'],
  Bash: ['Ran', 'command'], Glob: ['Looked for files', 'pattern'], Grep: ['Searched the files for', 'pattern'], WebFetch: ['Read', 'url'], WebSearch: ['Searched for', 'query'],
  TodoWrite: ['Wrote its to-do list', ''], Task: ['Gave part of it to a helper', 'description'], Agent: ['Gave part of it to a helper', 'description'],
  // OpenCode's own tools, by their names in its updates.
  read: ['Read', 'path'], write: ['Wrote', 'path'], edit: ['Changed', 'path'], shell: ['Ran', 'command'], bash: ['Ran', 'command'], glob: ['Looked for files', 'pattern'],
  grep: ['Searched the files for', 'pattern'], list: ['Listed the files', 'path'], webfetch: ['Read', 'url'], websearch: ['Searched for', 'query'], todowrite: ['Wrote its to-do list', ''],
  // Everything a program changed in the folder in one turn, undone in one go.
  all_it_changed: ['Changed files in the folder', ''],
}
/** The four levels. One shield each, so the level reads from the icon alone: whole, half, deciding, off. */
const LEVELS: Record<Level, { label: string; hint: string; text: string; icon: LucideIcon }> = {
  cautious: { label: 'Cautious', icon: ShieldCheck, hint: 'Asks before every tool', text: 'Asks you before every use of a tool that reaches the folder or the internet, reading included.' },
  balanced: { label: 'Balanced', icon: ShieldHalf, hint: 'Asks before programs', text: 'Reads and changes files in the folder without asking: a change can be undone. Asks before every program, before an address the model wrote itself, and, once the session read the internet, before a change to a file.' },
  smart: { label: 'Smart', icon: ShieldEllipsis, hint: 'The model decides', text: 'The model decides when to ask you, wherever Balanced would ask. A page or a file can talk a small model into going ahead; the folder, the sandbox and Undo hold as at Balanced.' },
  autonomous: { label: 'Autonomous', icon: ShieldOff, hint: 'Never asks', text: 'Never asks. A model that obeys something it read can then send what it has read to any address.' },
}
const flat = (text: string, most: number) => { const one = text.replace(/\s+/g, ' ').trim(); return one.length > most ? `${one.slice(0, most)}…` : one }
const copy = (text: string) => void navigator.clipboard.writeText(text).then(() => toast('Copied.'), () => toast('Not copied: this browser would not let the page write to the clipboard.', undefined, 'task-failed'))

/** What a change changed, fetched when it is opened: the lines that went, and the lines that came. */
function ChangeShown({ change }: { change: CoreChange }) {
  const [text, setText] = useState<string | null>(null)
  const [failed, setFailed] = useState<string | null>(null)
  const load = () => {
    if (text !== null || failed) return
    changeDiff(change.session_id, change.id).then(setText, (e: Error) => setFailed(e.message))
  }
  const kind = (line: string) =>
    line.startsWith('+++') || line.startsWith('---') ? 'file' : line.startsWith('+') ? 'add' : line.startsWith('-') ? 'del' : line.startsWith('@@') ? 'hunk' : undefined
  return (
    <details className="d-disclosure chat__diff" onToggle={(e) => { if (e.currentTarget.open) load() }}>
      <summary>Changes <span className="d-text-tertiary">{change.diff_stat}</span></summary>
      <div className="d-disclosure__body">
        {failed ? <p className="d-type-caption d-text-secondary">{failed}</p>
          : text === null ? <p className="d-type-caption d-text-tertiary"><Glyph name="task-running" busy /></p>
          : <pre className="d-code chat__diff-text"><code>{text.replace(/\n$/, '').split('\n').map((line, i) => <span key={i} data-line={kind(line)}>{line}{'\n'}</span>)}</code></pre>}
      </div>
    </details>
  )
}

/** One use of a tool: what was asked for and what came back. Opened, it shows the request as the model made it.
    A use that changed a file can be undone from here, and shows what it changed. */
function ToolUse({ step, at, change, session, program }: { step: CoreStep; at: string; change?: CoreChange; session: string; program?: boolean }) {
  const [said, on] = TOOL_SAID[step.tool] ?? [step.tool, '']
  const value = (name: string) => { const v = (step.args ?? {})[name]; return typeof v === 'string' ? v : v === undefined ? '' : JSON.stringify(v) }
  const shown = on ? value(on) : Object.keys(step.args ?? {}).map(value).join(', ')
  const denied = step.state === 'refused' && step.result === 'the person said no'
  const refused = step.state === 'refused' && !denied
  return (
    <div className="chat__use">
      <div className="chat__step">
        <StepRow step={{
          id: at,
          state: step.state === 'running' ? 'running' : step.state === 'waiting' ? 'needs-input' : step.state === 'done' ? 'done' : 'failed',
          what: refused ? (program ? `${said}: not allowed` : `Asked for ${step.tool} in a way that cannot be used`) : denied ? `${said}: you said no` : said,
          code: refused ? undefined : flat(shown, 80) || undefined,
          meta: step.state === 'done' ? flat(step.result, 60) : step.state === 'running' ? '' : step.state === 'waiting' ? 'waiting for you' : step.state === 'refused' ? 'not run' : 'could not',
          request: JSON.stringify({ tool: step.tool, args: step.args }, null, 1),
          output: step.state === 'running' || step.state === 'waiting' ? undefined : step.result,
        }} />
        {step.artifact && step.state === 'done' && <Button variant="ghost" size="sm" onClick={() => { if (step.artifact) openArtifact(session, step.artifact.id, step.artifact.n) }}>Open</Button>}
        {step.output && <Tip label="Everything it printed"><a className="d-button d-button--ghost d-button--sm" href={`/api/v1/sessions/${session}/outputs/${step.output}`} target="_blank" rel="noopener">Output</a></Tip>}
        {step.model_chose === 'go' && <Tip label="At Smart the model chose not to ask you, both ways round."><span className="d-type-caption d-text-tertiary">model's call</span></Tip>}
        {change && (change.undone_at
          ? <span className="d-type-caption d-text-tertiary">Undone</span>
          : <Tip label={change.whole_folder ? 'Puts the whole folder back as it was before this command ran.' : change.created ? `Removes ${change.path}: this created it.` : `Puts ${change.path} back as it was.`}><Button variant="ghost" size="sm" onClick={() => void undoChange(change.session_id, change.id)}>Undo</Button></Tip>)}
      </div>
      {change?.diff_stat && <ChangeShown change={change} />}
    </div>
  )
}

/** The sandbox, set up from here where the core's computer allows it (Windows, DECISIONS T27). */
function SetUpSandbox({ id }: { id: string }) {
  const [busy, setBusy] = useState(false)
  return <Tip label="Windows asks for an administrator once. Programs then run as a user of Demido's own, with no network."><Button size="sm" disabled={busy} onClick={() => { setBusy(true); void setUpSandbox(id).finally(() => setBusy(false)) }}>{busy ? 'Windows asks first…' : 'Set up the sandbox'}</Button></Tip>
}

function Asking({ request }: { request: PermissionRequest }) {
  const answer = (said: 'once' | 'session' | 'no') => void answerPermission(request.session_id, request.id, said)
  return (
    <section className="d-permission" data-risk={request.reach === 'runs' || request.once_only ? 'high' : request.reach === 'changes' ? 'medium' : 'low'} aria-label={request.what}>
      <header className="d-permission__head">
        <span className="d-permission__risk">{request.reach === 'runs' ? 'Runs a program' : request.reach === 'changes' ? 'Changes a file' : request.reach === 'network' ? (request.once_only ? 'An address the model wrote' : 'Reads the internet') : 'Reads'}</span>
      </header>
      <pre className="d-permission__what"><code>{request.what}</code></pre>
      <p className="d-permission__why">{request.why}</p>
      <details className="d-disclosure">
        <summary>The request</summary>
        <div className="d-disclosure__body"><pre className="d-code"><code>{JSON.stringify({ tool: request.tool, args: request.args }, null, 1)}</code></pre></div>
      </details>
      <div className="d-permission__actions">
        <Button onClick={() => answer('no')}>No</Button>
        {/* A yes to an address the model wrote is for that one address. */}
        {!request.once_only && <Tip label={`Do not ask again for ${request.tool} in this session`}><Button onClick={() => answer('session')}>Always here</Button></Tip>}
        <Button variant="primary" onClick={() => answer('once')}>Yes</Button>
      </div>
    </section>
  )
}

const TASK_SAID: Record<CoreTask['state'], [string, string]> = { pending: ['pending', 'Pending'], running: ['running', 'In progress'], done: ['done', 'Done'], failed: ['failed', 'Failed'], stopped: ['skipped', 'Stopped'] }

/** The tasks a piece of work was split into: a checklist, each task with what it gave and the tools it used. */
function TaskList({ tasks, changes, parent = null }: { tasks: CoreTask[]; changes: CoreChange[]; parent?: string | null }) {
  const mine = tasks.filter((t) => t.parent_id === parent).sort((a, b) => a.position - b.position)
  if (!mine.length) return null
  return (
    <ul className="d-tasks">
      {mine.map((t) => (
        <li key={t.id} className="d-task" data-state={TASK_SAID[t.state][0]}>
          <div className="d-task__row">
            <span className="d-glyph" role="img" aria-label={TASK_SAID[t.state][1]} />
            <span className="d-task__label">{t.title}</span>
          </div>
          {(t.result || t.steps.length > 0) && (
            <div className="chat__task-gave">
              {t.steps.map((step, i) => <ToolUse key={i} step={step} at={`${t.id}-${i}`} change={changes.find((c) => c.id === step.change)} session={t.session_id} />)}
              {t.result && <p className="d-type-small d-text-secondary" style={{ whiteSpace: 'pre-wrap' }}>{t.result}</p>}
            </div>
          )}
          <TaskList tasks={tasks} changes={changes} parent={t.id} />
        </li>
      ))}
    </ul>
  )
}

/** Reads an answer aloud with voice mode's voice (DECISIONS V11), and stops it. */
function ReadItAloud({ m }: { m: CoreMessage }) {
  const voice = useSetting<string>('voice.real', '2181')
  const [state, setState] = useState<'idle' | 'starting' | 'speaking'>('idle')
  const reading = useRef<ReadAloud | null>(null)
  useEffect(() => () => reading.current?.stop(), [])
  const start = () => {
    reading.current = new ReadAloud(m.id, voice, (s, why) => {
      if (s === 'failed') toast(`Not read aloud. ${why ?? ''}`, undefined, 'task-failed')
      setState(s === 'starting' ? 'starting' : s === 'speaking' ? 'speaking' : 'idle')
    })
    reading.current.start()
  }
  return state === 'idle'
    ? <Tip label="Read aloud"><Button variant="ghost" size="sm" iconOnly aria-label="Read aloud" onClick={start}><Icon icon={Volume2} size="sm" /></Button></Tip>
    : <Tip label="Stop"><Button variant="ghost" size="sm" iconOnly pressed aria-label="Stop reading aloud" onClick={() => reading.current?.stop()}><Icon icon={state === 'starting' ? Loader : Square} size="sm" /></Button></Tip>
}

/** Keeps an answer, or its one block of code, as a file of the session's own: an artifact, as a model gives one. */
function KeepAsFile({ m }: { m: CoreMessage }) {
  const [open, setOpen] = useState(false)
  const proposed = asAFile(m.content)
  const [name, setName] = useState(proposed.name)
  const [problem, setProblem] = useState<string | null>(null)
  const keep = async () => {
    if (!name.trim()) return
    try {
      const kept = await keepAsFile(m.session_id, name.trim(), proposed.text, m.id)
      setOpen(false)
      openArtifact(m.session_id, kept.artifact.id, kept.n)
    } catch (e) {
      setProblem((e as Error).message)
    }
  }
  return (
    <>
      <Tip label="Keep as a file"><Button variant="ghost" size="sm" iconOnly aria-label="Keep as a file" onClick={() => { setName(proposed.name); setProblem(null); setOpen(true) }}><Icon icon={FilePlus} size="sm" /></Button></Tip>
      {open && (
        <Dialog open onOpenChange={(o) => !o && setOpen(false)} title="Keep as a file"
          text={proposed.text === m.content ? 'The whole answer. The same name again keeps a new version.' : 'Its block of code. The same name again keeps a new version.'}
          actions={<><Button onClick={() => setOpen(false)}>Cancel</Button><Button variant="primary" disabled={!name.trim()} onClick={() => void keep()}>Keep</Button></>}>
          <form className="app-stack" style={{ marginBlockStart: 'var(--d-space-16)' }} onSubmit={(e) => { e.preventDefault(); void keep() }}>
            <input className="d-input d-input--mono" aria-label="Its name" autoFocus value={name} onChange={(e) => setName(e.target.value)} />
            {problem && <Signal level="caution">{problem}</Signal>}
          </form>
        </Dialog>
      )}
    </>
  )
}

/** Asks before saying a message again when later messages would go with it. */
function AgainConfirm({ count, onYes, onNo }: { count: number; onYes: () => void; onNo: () => void }) {
  return (
    <Dialog open onOpenChange={(o) => !o && onNo()} title="Send it again from here?" text={`The ${count} messages after it go. Files the model changed stay as they are.`}
      actions={<><Button onClick={onNo}>Cancel</Button><Button variant="primary" onClick={onYes}>Send again</Button></>} />
  )
}

interface MessageProps {
  m: CoreMessage
  live?: { text: string; thought: string }
  doing?: { steps?: CoreStep[]; tasks?: CoreTask[] }
  tasks: CoreTask[]
  changes: CoreChange[]
  asking?: PermissionRequest | null
  folder: boolean
  /** Whether the session is being answered: nothing is said again meanwhile. */
  answering: boolean
  /** How many messages come after this one. */
  after: number
  /** The person's message this answer answers, to ask it again. */
  request?: CoreMessage
  onPage: (shot: Shot) => void
}

function Mine({ m, answering, after }: { m: CoreMessage; answering: boolean; after: number }) {
  const [editing, setEditing] = useState(false)
  const [text, setText] = useState(m.content)
  const [asking, setAsking] = useState<string | null | undefined>(undefined)
  // Said again with its text (null: as it was), once the person agreed when later messages go with it.
  const go = (content: string | null) => {
    if (after > 1 && asking === undefined) return setAsking(content)
    setAsking(undefined)
    setEditing(false)
    void sayAgain(m.session_id, m.id, content ?? undefined)
  }
  if (editing)
    return (
      <article className="d-message" data-from="user">
        <form className="chat__edit" onSubmit={(e) => { e.preventDefault(); if (text.trim()) go(text.trim()) }}>
          <textarea className="d-composer__input" aria-label="Your message, edited" autoFocus value={text} onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); if (text.trim()) go(text.trim()) } else if (e.key === 'Escape') setEditing(false) }} />
          <div className="chat__edit-bar">
            <Button size="sm" variant="ghost" onClick={() => { setEditing(false); setText(m.content) }}>Cancel</Button>
            <Button size="sm" variant="primary" type="submit" disabled={!text.trim() || answering}>Send</Button>
          </div>
        </form>
        {asking !== undefined && <AgainConfirm count={after} onYes={() => go(asking)} onNo={() => setAsking(undefined)} />}
      </article>
    )
  return (
    <article className="d-message" data-from="user">
      <div className="d-message__body">{m.content}</div>
      <div className="d-message__actions">
        <Tip label="Copy"><Button variant="ghost" size="sm" iconOnly aria-label="Copy" onClick={() => copy(m.content)}><Icon icon={Copy} size="sm" /></Button></Tip>
        <Tip label="Edit"><Button variant="ghost" size="sm" iconOnly aria-label="Edit" disabled={answering} onClick={() => { setText(m.content); setEditing(true) }}><Icon icon={Pencil} size="sm" /></Button></Tip>
        <Tip label="Send again"><Button variant="ghost" size="sm" iconOnly aria-label="Send again" disabled={answering} onClick={() => go(null)}><Icon icon={RotateCcw} size="sm" /></Button></Tip>
      </div>
      {asking !== undefined && <AgainConfirm count={after} onYes={() => go(asking)} onNo={() => setAsking(undefined)} />}
    </article>
  )
}

function MessageView({ m, live, doing, tasks, changes, asking, folder, answering, after, request, onPage }: MessageProps) {
  const loading = useLocal((s) => s.list?.models.find((x) => x.id === m.model)?.state === 'loading')
  const at = useProviders(() => elsewhere(m.model))
  const [again, setAgain] = useState(false)
  if (m.role === 'user') return <Mine m={m} answering={answering} after={after} />
  // A message from another session in the same folder, or Demido's note of what it changed (DECISIONS S5).
  if (m.role === 'session') {
    const [from, ...rest] = m.content.split('\n')
    return (
      <article className="d-message chat__from-session" data-from="session" aria-label={`From ${from}`}>
        <div className="app-row" style={{ gap: 'var(--d-space-8)' }}><span className="d-type-caption d-text-secondary">{from.startsWith("Demido's note") ? from : from}</span><span className="d-untrusted">another session · data</span></div>
        <div className="d-message__body" style={{ whiteSpace: 'pre-wrap' }}>{rest.join('\n')}</div>
      </article>
    )
  }
  const writing = m.state === 'answering'
  const text = writing ? (live?.text ?? '') : m.content
  const thought = writing ? (live?.thought ?? '') : (m.thought ?? '')
  // While it is being written, what it is doing comes as news; afterwards, from what is stored.
  const steps = ((writing ? doing?.steps : undefined) ?? m.steps ?? []).filter((s) => s.tool !== 'plan')
  const split = (writing ? doing?.tasks : undefined) ?? tasks
  const busy = steps.length > 0 || split.length > 0
  // The last page the model saw in this answer, as a card.
  const page = shotsOf([{ ...m, steps }]).at(-1)
  const t = m.timings
  const ask = () => { if (request) void sayAgain(m.session_id, request.id) }
  return (
    <article className="d-message" data-from="assistant">
      <div className="d-message__body chat__blocks">
        {writing && !text && !thought && !busy && <p className="d-text-secondary"><Glyph name="model-loading" busy /> {at ? at : loading ? m.model : ''}</p>}
        {steps.map((step, i) => <ToolUse key={i} step={step} at={`${m.id}-${i}`} change={changes.find((c) => c.id === step.change)} session={m.session_id} program={byProgram(m.model)} />)}
        {page && <BrowserCard session={m.session_id} shot={page} onOpen={() => onPage(page)} />}
        {split.length > 0 && <TaskList tasks={split} changes={changes} />}
        {writing && asking && <Asking request={asking} />}
        {thought && (
          // Open while it is being thought, so it can be watched. Folded away once the answer has begun.
          <details className="d-thinking" open={writing && !text ? true : undefined}>
            <summary>{writing && !text ? 'Thinking…' : 'Thought'}</summary>
            <pre>{thought}</pre>
          </details>
        )}
        {/* A provider's model cannot be told from outside to end its thought. */}
        {writing && thought && !text && !at && <div><Button size="sm" onClick={() => void answerNow(m.session_id)}>Answer now</Button></div>}
        {text && <Prose text={text} />}
        {m.state === 'stopped' && <Signal level="note">{m.problem ?? 'Stopped.'}</Signal>}
        {m.state === 'failed' && <Signal level="caution">{m.problem ?? 'The answer failed.'}</Signal>}
      </div>
      {!writing && (
        <div className="d-message__actions chat__actions">
          {m.content.trim() && <Tip label="Copy"><Button variant="ghost" size="sm" iconOnly aria-label="Copy" onClick={() => copy(m.content)}><Icon icon={Copy} size="sm" /></Button></Tip>}
          {request && <Tip label="Ask again"><Button variant="ghost" size="sm" iconOnly aria-label="Ask again" disabled={answering} onClick={() => (after > 0 ? setAgain(true) : ask())}><Icon icon={RotateCcw} size="sm" /></Button></Tip>}
          {m.state === 'done' && m.content.trim() && <><ReadItAloud m={m} /><KeepAsFile m={m} /></>}
          {t && t.written > 0 && (
            <Tip label={<>{m.model}<br />{t.written} tokens written{t.per_second > 0 ? `, ${Math.round(t.per_second)} a second` : ''}<br />Prompt: {t.read} read, {t.reused} reused{t.cost_usd !== undefined ? <><br />{dollars(t.cost_usd)}{byProgram(m.model) ? ` by ${at}'s own count` : t.cost_exact ? '' : ' (estimated)'}</> : null}</>}>
              <span className="chat__meta">{t.per_second > 0 ? `${Math.round(t.per_second)} t/s` : `${t.written} t`}{t.cost_usd !== undefined ? ` · ${dollars(t.cost_usd)}` : ''}</span>
            </Tip>
          )}
          {/* In a session with a folder, an answer about files that read no file is an answer from nowhere. Small models write those. */}
          {folder && m.state === 'done' && !busy && <Tip label="No file was read or changed for this answer."><span className="chat__meta chat__meta--flag"><Icon icon={Info} size="sm" /> no file read</span></Tip>}
        </div>
      )}
      {again && <AgainConfirm count={after + 1} onYes={() => { setAgain(false); ask() }} onNo={() => setAgain(false)} />}
    </article>
  )
}

/** Where a session was made shorter: what is above is no longer given to the model word for word. */
function Cut({ account }: { account: { content: string; why: string; tokens_before: number } }) {
  return (
    <details className="chat__cut">
      <summary>Made shorter here{account.why === 'auto' ? ` at ${account.tokens_before.toLocaleString()} tokens` : ''}</summary>
      <p className="d-type-small d-text-secondary" style={{ whiteSpace: 'pre-wrap' }}>{account.content}</p>
    </details>
  )
}

/** A control of the session under the message box: an icon, a short value, and a menu. */
function Chip({ icon, label, title, tone, items, disabled }: { icon: LucideIcon; label: string; title: string; tone?: string; items: MenuEntry[]; disabled?: boolean }) {
  return (
    <Menu side="top" items={items} trigger={
      <button type="button" className="chat__chip" data-tone={tone} aria-label={`${title}: ${label}`} title={title} disabled={disabled}>
        <Icon icon={icon} size="sm" /><span className="chat__chip-label">{label}</span><Icon icon={ChevronDown} size="sm" />
      </button>
    } />
  )
}

/** The folder this session may work in, typed as a path: a folder on the computer the core runs on. */
export function FolderForm({ id, folder, sandbox, onDone }: { id: string; folder: string | null; sandbox?: { kind: string; why_not?: string | null; can_set_up?: boolean } | null; onDone?: () => void }) {
  const [path, setPath] = useState(folder ?? '')
  const shut = Boolean(sandbox && sandbox.kind !== 'none')
  return (
    <form className="chat__where" onSubmit={(e) => { e.preventDefault(); void changeSession(id, { folder: path.trim() }).then(() => onDone?.()) }}>
      <div className="app-row" style={{ gap: 'var(--d-space-6)' }}>
        <input className="d-input d-mono" style={{ flex: 1, minWidth: 0 }} aria-label="The folder, as a full path" placeholder="/home/you/project" value={path} onChange={(e) => setPath(e.target.value)} autoFocus spellCheck={false} />
        <Button variant="primary" size="sm" type="submit" disabled={!path.trim() || path.trim() === folder}>Use</Button>
        {folder && <Button size="sm" type="button" onClick={() => void changeSession(id, { folder: '' }).then(() => onDone?.())}>None</Button>}
      </div>
      <p className="chat__where-facts">
        <span><Icon icon={shut ? ShieldCheck : ShieldOff} size="sm" /> {shut ? `Programs in a sandbox (${sandbox?.kind})` : 'No sandbox: programs ask first'}</span>
        <span><Icon icon={RotateCcw} size="sm" /> Every change can be undone</span>
      </p>
      {sandbox?.can_set_up && !shut && <SetUpSandbox id={id} />}
    </form>
  )
}

function FolderChip({ id, folder, sandbox, disabled }: { id: string; folder: string | null; sandbox?: { kind: string; why_not?: string | null; can_set_up?: boolean } | null; disabled?: boolean }) {
  const [open, setOpen] = useState(false)
  return (
    <Popover.Root open={open} onOpenChange={setOpen}>
      <Popover.Trigger render={<button type="button" className="chat__chip" data-tone={folder ? 'set' : undefined} disabled={disabled} title="The session's folder" aria-label={`Folder: ${folder ?? 'none'}`} />}>
        <Icon icon={folder ? FolderOpen : Folder} size="sm" /><span className="chat__chip-label">{folder ? folderName(folder) : 'No folder'}</span><Icon icon={ChevronDown} size="sm" />
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Positioner side="top" align="start" sideOffset={8} className="app-layer">
          <Popover.Popup className="d-menu app-pop chat__folder-pop">
            <FolderForm id={id} folder={folder} sandbox={sandbox} onDone={() => setOpen(false)} />
          </Popover.Popup>
        </Popover.Positioner>
      </Popover.Portal>
    </Popover.Root>
  )
}

/** How full the session is, as a ring, and the way to make it shorter. */
function Room({ id, context, busy, canShorten }: { id: string; context: ContextUse; busy: boolean; canShorten: boolean }) {
  const shortening = useReal((s) => Boolean(s.shortening[id]))
  const full = Math.min(100, Math.round((context.used / context.limit) * 100))
  const k = (n: number) => (n >= 1000 ? `${(n / 1000).toFixed(1)}k` : String(n))
  if (shortening) return <Tip label="Making it shorter"><span className="chat__ring"><Glyph name="task-running" busy /></span></Tip>
  return (
    <Menu side="top" align="end" items={[
      { kind: 'label', label: `${k(context.used)} of ${k(context.limit)} tokens` },
      { label: 'Make shorter', hint: context.auto ? `by itself at ${k(context.compact_at)}` : undefined, disabled: busy || !canShorten, onSelect: () => void makeShorter(id) },
    ]} trigger={
      <button type="button" className="chat__ring" aria-label={`${full}% full`} title={`${full}% full`} style={{ '--ring': `${full * 3.6}deg` } as React.CSSProperties} data-full={full > 80 ? '' : undefined} />
    } />
  )
}

function Composer({ id, answering, tools, program, children }: { id: string; answering: boolean; tools: boolean; program?: boolean; children: React.ReactNode }) {
  const [text, setText] = useState('')
  const [plan, setPlan] = useState(false)
  const input = useRef<HTMLTextAreaElement>(null)
  useEffect(() => { input.current?.focus() }, [id])
  // Dictation (DECISIONS V11): each finished sentence is put at the end of what is in the box; nothing is sent by it.
  const [dictating, setDictating] = useState<'off' | 'starting' | 'listening' | 'hearing' | 'waiting'>('off')
  const dictation = useRef<Dictation | null>(null)
  useEffect(() => () => dictation.current?.stop(), [])
  const dictate = () => {
    if (dictation.current) { dictation.current.stop(); dictation.current = null; setDictating('off'); return }
    dictation.current = new Dictation(
      (heard) => setText((t) => (t.trim() ? `${t.trimEnd()} ${heard}` : heard)),
      (state, why) => {
        if (state === 'closed') { dictation.current = null; setDictating('off'); if (why) toast(`Dictation stopped. ${why}`, undefined, 'task-failed') } else setDictating(state)
      },
    )
    void dictation.current.start()
  }
  const send = async () => {
    const mine = text.trim()
    if (!mine || answering) return
    setText('')
    // "Plan first" is for the one message it was pressed for.
    if (await say(id, mine, plan && tools)) setPlan(false)
    else setText(mine)
  }
  return (
    <div className="chat__input">
      <form className="d-composer" onSubmit={(e) => { e.preventDefault(); void send() }}>
        <textarea ref={input} className="d-composer__input" rows={1} placeholder="Ask something" aria-label="Message" value={text} onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); void send() } }} />
        <div className="d-composer__bar">
          {/* A program keeps its own conversation and plans its own way. */}
          {!program && (
            <Tip label={tools ? 'Plan first: the steps, then one at a time' : 'Tools are off'}>
              <Button variant="ghost" size="sm" iconOnly aria-label="Plan first" pressed={plan && tools} disabled={!tools} onClick={() => setPlan(!plan)}><Icon icon={ListChecks} size="sm" /></Button>
            </Tip>
          )}
          <span className="d-composer__spacer" />
          {dictating === 'waiting' && <Tip label="Write it down now"><Button variant="ghost" size="sm" onClick={() => dictation.current?.done()}>Done</Button></Tip>}
          <Tip label={dictating === 'off' ? 'Dictate' : 'Stop dictating'}>
            <Button variant="ghost" iconOnly pressed={dictating !== 'off'} aria-label={dictating === 'off' ? 'Dictate' : 'Stop dictating'} onClick={dictate}><Icon icon={dictating === 'off' ? Mic : dictating === 'starting' ? Loader : MicOff} size="sm" /></Button>
          </Tip>
          {answering
            ? <Tip label="Stop"><Button variant="primary" iconOnly aria-label="Stop" onClick={() => void stopAnswer(id)}><Icon icon={Square} size="sm" /></Button></Tip>
            : <Tip label="Send"><Button variant="primary" iconOnly aria-label="Send" type="submit" disabled={!text.trim()}><Icon icon={ArrowUp} /></Button></Tip>}
        </div>
      </form>
      <div className="chat__controls">{children}</div>
    </div>
  )
}

/** At the top of a schedule's session: what it does, when it runs next, and a run now. */
function ScheduleStrip({ id }: { id: string }) {
  const [s, setS] = useState<{ schedule: Schedule; running: boolean } | null>(null)
  const [tick, setTick] = useState(0)
  useEffect(() => { scheduleRuns(id).then((r) => setS({ schedule: r.schedule, running: r.running }), () => setS(null)) }, [id, tick])
  useEffect(() => {
    const offs = ['schedule.started', 'schedule.ran', 'schedule.changed'].map((k) => onCoreEvent(k, (e) => { if (e.subject === id) setTick((t) => t + 1) }))
    return () => offs.forEach((off) => off())
  }, [id])
  if (!s) return null
  const { schedule, running } = s
  const next = running ? 'running now' : schedule.state === 'on' && schedule.next_at ? `next ${whenFromNow(schedule.next_at)}` : schedule.state === 'paused' ? 'paused' : 'done'
  return (
    <div className="chat__schedule">
      <Icon icon={CalendarClock} size="sm" />
      <span className="chat__schedule-what">{schedule.watch ? schedule.watch_said : schedule.what}</span>
      <span className="d-text-tertiary">{schedule.when_said} · {next}</span>
      <span className="app-spacer" />
      <Button size="sm" disabled={running} onClick={() => void runSchedule(id).then(() => setTick((t) => t + 1))}>{running ? 'Running…' : 'Run now'}</Button>
    </div>
  )
}

/** What the model is told, word for word. */
function Told({ detail, onClose }: { detail: NonNullable<ReturnType<typeof useReal.getState>['open'][string]>; onClose: () => void }) {
  const program = byProgram(detail.session.model)
  return (
    <Dialog wide open onOpenChange={(o) => !o && onClose()} title="What the model is told" actions={<Button onClick={onClose}>Close</Button>}>
      <div className="app-stack" style={{ marginBlockStart: 'var(--d-space-12)' }}>
        <p className="d-list__label">{program ? `Added to ${elsewhere(detail.session.model)}'s own instructions` : 'Before the conversation'}</p>
        <pre className="d-code" style={{ whiteSpace: 'pre-wrap' }}><code>{program ? detail.instructions.split('\n\n').slice(1).join('\n\n') : detail.instructions}</code></pre>
        {!program && detail.tools_told && <>
          <p className="d-list__label">Before each answer, to choose a tool</p>
          <pre className="d-code" style={{ whiteSpace: 'pre-wrap' }}><code>{detail.tools_told}</code></pre>
        </>}
      </div>
    </Dialog>
  )
}

function Chat() {
  const { id = '' } = useParams()
  const detail = useReal((s) => s.open[id])
  const known = useReal((s) => s.list?.some((x) => x.id === id))
  const listed = useReal((s) => s.list !== null)
  const writing = useReal((s) => s.writing)
  const working = useReal((s) => s.working)
  const models = useLocal((s) => s.list?.models) ?? []
  const providers = useProviders((s) => s.providers)
  const remote = chosenModels(providers)
  const navigate = useNavigate()
  const thread = useRef<HTMLDivElement>(null)
  const [told, setTold] = useState(false)
  const [sure, setSure] = useState(false)
  // What is open beside the conversation, whether it takes the whole view, and the page chosen in the browser.
  const [panel, setPanel] = useState<'browser' | 'files' | null>(null)
  const [wide, setWide] = useState(false)
  const [shot, setShot] = useState<string | null>(null)
  // An artifact opened from anywhere opens in its session's files.
  const focus = useFilesFocus((s) => s.focus)
  const [artifact, setArtifact] = useState<{ id: string; n: number } | null>(null)
  useEffect(() => {
    if (focus && focus.session === id) { setArtifact({ id: focus.artifact, n: focus.n }); setPanel('files'); useFilesFocus.getState().clear() }
  }, [focus, id])
  useEffect(() => { setTold(false); setSure(false); setPanel(null); setWide(false); setShot(null); setArtifact(null) }, [id])
  // Only a session the list knows is asked for: the address may still name one that is gone, or one of the preview's.
  useEffect(() => { if (known) void readSession(id) }, [id, known])
  // Follow the answer down as it is written.
  const last = detail?.messages.at(-1)
  const doing = last ? working[last.id] : undefined
  const height = (last ? (writing[last.id]?.text.length ?? 0) + (writing[last.id]?.thought.length ?? 0) : 0) + (detail?.messages.length ?? 0)
    + (doing?.steps?.length ?? 0) + (doing?.tasks?.reduce((n, t) => n + 1 + t.steps.length + (t.result?.length ?? 0), 0) ?? 0)
  useEffect(() => {
    const el = thread.current
    if (el) el.scrollTop = el.scrollHeight
  }, [height, id])
  if (listed && !known && !detail) return <Navigate to="/sessions" replace />
  if (!detail) return <section className="chat" aria-label="Conversation" />
  const { session, messages } = detail
  const answering = messages.some((m) => m.state === 'answering')
  const level: Level = session.permission ?? 'balanced'
  // The account in force, and the last message it stands for.
  const account = detail.summaries.at(-1)
  const covered = account?.upto_seq ?? 0
  const at = elsewhere(session.model)
  // A model that always thinks cannot be told not to: "off" asks for the least.
  const always = remote.find((r) => r.id === session.model)?.always_thinks ?? false
  const program = byProgram(session.model)
  // The accounts at its model's provider: with more than one, the session picks which it goes through.
  const accounts = (at && providers?.find((p) => p.id === session.model.split('/')[0])?.accounts) || []
  const inUse = accounts.find((a) => a.id === session.account) ?? accounts.find((a) => a.is_default)
  const offLabel = program ? 'Default' : always ? 'Least' : 'Off'
  const internet = Boolean(detail.tools_told?.includes('search_web'))
  const shots = shotsOf(messages)
  const openPage = (s: Shot) => { setShot(s.picture); setPanel('browser') }
  const toggle = (p: 'browser' | 'files') => { setPanel(panel === p ? null : p); setWide(false) }

  const modelState = (m: string) => models.find((x) => x.id === m)?.state
  const modelItems: MenuEntry[] = [
    ...(models.length ? [{ kind: 'label' as const, label: 'On this computer' }] : []),
    ...models.map((m) => ({ label: m.id, lead: <Glyph name={m.state === 'loaded' ? 'model-loaded' : m.state === 'loading' ? 'model-loading' : 'model-unloaded'} busy={m.state === 'loading'} tone={m.state === 'loaded' ? 'live' as const : undefined} />, checked: m.id === session.model, onSelect: () => void changeSession(id, { model: m.id }) })),
    ...(remote.length ? [{ kind: 'label' as const, label: 'Elsewhere' }] : []),
    ...remote.map((r) => ({ label: r.name, hint: `${elsewhere(r.id)}${r.free ? ' · free' : ''}`, checked: r.id === session.model, onSelect: () => void changeSession(id, { model: r.id }) })),
    ...(accounts.length > 1 ? [{ kind: 'sep' as const }, { kind: 'label' as const, label: 'Account' }, ...accounts.map((a) => ({ label: a.label, hint: a.is_default ? 'default' : undefined, checked: a.id === inUse?.id, onSelect: () => void changeSession(id, { account: a.id }) }))] : []),
  ]
  const modelLabel = remote.find((r) => r.id === session.model)?.name ?? session.model
  const thinking = detail.thinking_choices ?? []
  const thinkLabel = session.thinking === 'off' ? offLabel : session.thinking === 'on' ? 'On' : session.thinking

  return (
    <div className="sess" data-panel={panel ?? undefined} data-wide={panel && wide ? '' : undefined}>
      <section className="chat" aria-label="Conversation">
        <header className="chat__head">
          <h1 className="chat__title">{session.title}</h1>
          {session.schedule_id && <Tip label="A schedule's session: it runs on its own, with every tool, and asks nothing."><span className="d-badge d-badge--outline"><Icon icon={CalendarClock} size="sm" /> Runs on its own</span></Tip>}
          <span className="app-spacer" />
          <Tip label="Browser"><Button variant="ghost" size="sm" iconOnly aria-label="Browser" pressed={panel === 'browser'} onClick={() => toggle('browser')}><Icon icon={Globe} size="sm" />{shots.length > 0 && <span className="chat__dot" />}</Button></Tip>
          <Tip label={session.folder ? folderName(session.folder) : 'Files'}><Button variant="ghost" size="sm" iconOnly aria-label="Files" pressed={panel === 'files'} onClick={() => toggle('files')}><Icon icon={session.folder ? FolderOpen : Folder} size="sm" /></Button></Tip>
          <Menu align="end" trigger={<Button variant="ghost" size="sm" iconOnly aria-label="More"><Icon icon={MoreHorizontal} size="sm" /></Button>} items={[
            { label: 'What the model is told', lead: <Icon icon={Brain} size="sm" />, onSelect: () => setTold(true) },
            { kind: 'sep' },
            { label: 'Delete the session', lead: <Icon icon={Trash2} size="sm" />, danger: true, onSelect: () => setSure(true) },
          ]} />
        </header>
        <div className="chat__thread" ref={thread}>
          <div className="chat__col">
            {session.schedule_id && <ScheduleStrip id={session.schedule_id} />}
            {!messages.length && (
              <div className="chat__welcome">
                <span dangerouslySetInnerHTML={{ __html: logo }} />
                <h2 className="d-type-title-2">What do you want to know?</h2>
                <div className="chat__facts">
                  <Tip label={at ? `Runs at ${at}: what you write is sent there` : 'Runs on this computer'}><span className="chat__fact"><Glyph name={modelState(session.model) === 'loaded' ? 'model-loaded' : 'model-unloaded'} tone={modelState(session.model) === 'loaded' ? 'live' : undefined} /> {modelLabel}{at ? ` · ${at}` : ''}</span></Tip>
                  <span className="chat__fact" data-off={session.folder ? undefined : ''}><Icon icon={session.folder ? FolderOpen : Folder} size="sm" /> {session.folder ? folderName(session.folder) : 'No folder'}</span>
                  {!program && <span className="chat__fact" data-off={internet ? undefined : ''}><Icon icon={internet ? Wifi : WifiOff} size="sm" /> {internet ? 'Internet' : 'No internet'}</span>}
                  {session.folder && <span className="chat__fact" data-off={detail.sandbox && detail.sandbox.kind !== 'none' ? undefined : ''}><Icon icon={detail.sandbox && detail.sandbox.kind !== 'none' ? ShieldCheck : ShieldOff} size="sm" /> {detail.sandbox && detail.sandbox.kind !== 'none' ? 'Sandbox' : 'No sandbox'}</span>}
                  {!program && !detail.tools_told && <span className="chat__fact" data-off=""><Icon icon={ListChecks} size="sm" /> Tools off</span>}
                </div>
              </div>
            )}
            {messages.map((m, i) => (
              <div key={m.id} className="chat__blocks" style={m.seq <= covered ? { opacity: 0.55 } : undefined}>
                {account && m.seq === covered + 1 && <Cut account={account} />}
                <MessageView m={m} live={writing[m.id]} doing={working[m.id]} tasks={(detail.tasks ?? []).filter((t) => t.message_id === m.id)} changes={detail.changes ?? []}
                  asking={detail.asking && detail.asking.message_id === m.id ? detail.asking : null} folder={Boolean(session.folder)} answering={answering}
                  after={messages.length - i - 1} request={m.role === 'assistant' ? messages.slice(0, i).reverse().find((x) => x.role === 'user') : undefined} onPage={openPage} />
              </div>
            ))}
            {account && covered >= (messages.at(-1)?.seq ?? 0) && <Cut account={account} />}
          </div>
        </div>
        <Composer id={id} answering={answering} tools={Boolean(detail.tools_told)} program={program}>
          <FolderChip id={id} folder={session.folder ?? null} sandbox={detail.sandbox} disabled={answering} />
          <Chip icon={at ? Globe : Brain} title="Model" label={modelLabel} items={modelItems} tone={modelState(session.model) === 'loaded' ? 'live' : undefined} />
          {thinking.length > 0 && <Chip icon={Brain} title={always ? 'Thinking (it always thinks)' : 'Thinking'} label={`Think: ${thinkLabel}`} tone={session.thinking !== 'off' ? 'set' : undefined}
            items={[{ label: offLabel, checked: session.thinking === 'off', onSelect: () => void changeSession(id, { thinking: 'off' }) }, ...thinking.map((c) => ({ label: c === 'on' ? 'On' : c, checked: session.thinking === c, onSelect: () => void changeSession(id, { thinking: c }) }))]} />}
          {!session.schedule_id && <Chip icon={LEVELS[level].icon} title={LEVELS[level].text} label={LEVELS[level].label} tone={`level-${level}`}
            items={(Object.keys(LEVELS) as Level[]).map((l) => ({ label: LEVELS[l].label, hint: LEVELS[l].hint, lead: <Icon icon={LEVELS[l].icon} size="sm" />, checked: l === level, onSelect: () => void changeSession(id, { permission: l }) }))} />}
          <span className="app-spacer" />
          {!program && <Room id={id} context={detail.context} busy={answering} canShorten={messages.filter((m) => m.seq > covered).length >= 4} />}
        </Composer>
      </section>
      {panel === 'browser' && <BrowserPanel session={id} shots={shots} chosen={shot} onChoose={setShot} wide={wide} onWide={() => setWide(!wide)} onClose={() => setPanel(null)} />}
      {panel === 'files' && <FolderPanel session={id} folder={session.folder ?? null} focus={artifact} wide={wide} onWide={() => setWide(!wide)} onClose={() => setPanel(null)}
        empty={<><p>No folder.</p><FolderForm id={id} folder={null} sandbox={detail.sandbox} /></>} />}
      {told && <Told detail={detail} onClose={() => setTold(false)} />}
      {sure && <Dialog open onOpenChange={(o) => !o && setSure(false)} title="Delete this session?" text="Everything said in it goes. Files in its folder stay."
        actions={<><Button onClick={() => setSure(false)}>Keep</Button><Button variant="danger" onClick={async () => { setSure(false); await deleteSession(id); navigate('/sessions') }}>Delete</Button></>} />}
    </div>
  )
}

function Empty() {
  const list = useReal((s) => s.list)
  const navigate = useNavigate()
  if (list?.length) return <Navigate to={`/sessions/${list[0].id}`} replace />
  return (
    <section className="chat" aria-label="Conversation">
      <div className="chat__thread"><div className="chat__col">
        <div className="chat__welcome">
          <span dangerouslySetInnerHTML={{ __html: logo }} />
          <h2 className="d-type-title-2">No session yet</h2>
          <div className="chat__ideas"><Button variant="primary" disabled={!list} onClick={async () => { const id = await newSession(firstModel()); if (id) navigate(`/sessions/${id}`) }}><Icon icon={Plus} size="sm" />New session</Button></div>
        </div>
      </div></div>
    </section>
  )
}

export function RealSessions() {
  return (
    <div className="app-split">
      <Routes>
        <Route path="*" element={<List />} />
      </Routes>
      <Routes>
        <Route path=":id" element={<Chat />} />
        <Route path="*" element={<Empty />} />
      </Routes>
    </div>
  )
}

