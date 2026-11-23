/* Sessions, for real: conversations with a model on this computer, or one chosen elsewhere, kept and run by the core.

   This is what is built of the Sessions screen: talking, thinking, making a long session shorter, the first
   tools (the clock, the calculator) and work split into tasks. The preview (the same pages without a core)
   shows where it is going: tools that reach files and the web, permission levels, groups. Those are not
   here yet, and the note at the foot of the list says so. */
import { useEffect, useRef, useState } from 'react'
import { Prose } from '@/ui/prose'
import { ArrowUp, CalendarClock, Folder, ListChecks, Plus, ShieldCheck, ShieldEllipsis, ShieldHalf, ShieldOff, Square, Trash2, type LucideIcon } from 'lucide-react'
import { Navigate, Route, Routes, useNavigate, useParams } from 'react-router'
import { useLocal } from '@/data/local-models'
import { chosenModels, dollars, useProviders } from '@/data/providers'
import { answerNow, answerPermission, changeDiff, changeSession, deleteSession, makeShorter, newSession, readSession, say, setUpSandbox, stopAnswer, undoChange, useReal, type ContextUse, type CoreChange, type CoreMessage, type CoreStep, type CoreTask, type Level, type PermissionRequest } from '@/data/real-sessions'
import { Button, Dialog, Glyph, Icon, NotBuilt, Segmented, Select, Signal, StepRow, Tip } from '@/ui'
import { asAFile, keepAsFile, openArtifact } from '@/data/artifacts'
import { onCoreEvent } from '@/data/core'
import { runSchedule, scheduleRuns, whenFromNow, type Schedule } from '@/data/schedules'
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
        {!list && <p className="app-empty"><Glyph name="task-running" /> Reading the sessions…</p>}
        {list && !list.length && <p className="app-empty">No sessions yet.</p>}
        <ul className="d-list">
          {list?.map((s) => (
            <li key={s.id}>
              <button type="button" className="d-row" aria-current={s.id === current ? 'true' : undefined} onClick={() => navigate(`/sessions/${s.id}`)}>
                {s.schedule_id && <span className="d-row__lead"><Icon icon={CalendarClock} size="sm" label="A schedule's session" /></span>}
                <span className="d-row__text"><span className="d-row__title">{s.title}</span><span className="d-row__sub">{s.schedule_id ? 'Schedule · ' : ''}{s.model} · {s.messages} {s.messages === 1 ? 'message' : 'messages'}</span></span>
                <span className="d-row__meta">{ago(s.updated_at)}</span>
              </button>
            </li>
          ))}
        </ul>
      </div>
      <div className="app-side__foot">
        <span>These conversations are real: kept by the core, answered by a model on this computer, or by one you chose elsewhere (Providers).</span>
        <NotBuilt block>Pages that need scripts, and the search services that need a key; groups and search. The preview shows where these are going.</NotBuilt>
      </div>
    </aside>
  )
}


/** What a tool is called when it is used, in plain words, and which of its arguments says what it was used on. */
const TOOL_SAID: Record<string, [string, string]> = {
  time: ['Read the clock', ''], calculate: ['Calculated', 'expression'], list_files: ['Listed the files', ''], read_file: ['Read', 'path'], search_files: ['Searched the files for', 'text'],
  add_to_file: ['Added to', 'path'], write_file: ['Wrote', 'path'], edit_file: ['Changed', 'path'], run_command: ['Ran', 'command'],
  search_web: ['Searched the internet for', 'query'], give_file: ['Gave you', 'file_name'], read_page: ['Read the page', 'url'], read_output: ['Read more of what it printed, from line', 'from_line'],
  // Claude Code's own tools, by their names in its stream.
  Read: ['Read', 'file_path'], Write: ['Wrote', 'file_path'], Edit: ['Changed', 'file_path'], MultiEdit: ['Changed', 'file_path'], NotebookEdit: ['Changed', 'notebook_path'],
  Bash: ['Ran', 'command'], Glob: ['Looked for files', 'pattern'], Grep: ['Searched the files for', 'pattern'], WebFetch: ['Read the page', 'url'], WebSearch: ['Searched the internet for', 'query'],
  TodoWrite: ['Wrote its to-do list', ''], Task: ['Gave part of it to a helper', 'description'], Agent: ['Gave part of it to a helper', 'description'],
  // OpenCode's own tools, by their names in its updates.
  read: ['Read', 'path'], write: ['Wrote', 'path'], edit: ['Changed', 'path'], shell: ['Ran', 'command'], bash: ['Ran', 'command'], glob: ['Looked for files', 'pattern'],
  grep: ['Searched the files for', 'pattern'], list: ['Listed the files', 'path'], webfetch: ['Read the page', 'url'], websearch: ['Searched the internet for', 'query'], todowrite: ['Wrote its to-do list', ''],
  // Everything a program changed in the folder in one turn, undone in one go.
  all_it_changed: ['Changed files in the folder', ''],
}
/** The four levels, as they are today. One shield each, so the level reads from the icon alone: whole, half, deciding, off. */
const LEVELS: Record<Level, { label: string; text: string; icon: LucideIcon }> = {
  cautious: { label: 'Cautious', icon: ShieldCheck, text: 'Asks you before every use of a tool that reaches the folder or the internet, reading included.' },
  balanced: { label: 'Balanced', icon: ShieldHalf, text: 'Reads and changes files in the folder without asking: a change cannot leave the folder, and can be undone. Searches the internet and reads pages it was given without asking. Asks before every program, before opening an address the model wrote itself, and, once the session has read from the internet, before a change to a file.' },
  smart: { label: 'Smart', icon: ShieldEllipsis, text: 'The model decides when to ask you, wherever Balanced would ask: before a program, before an address it wrote itself, and before a change once the session read the internet. It goes ahead only when it says so twice, asked both ways round. Small models do what the last thing they read tells them, so a page or a file can talk one into going ahead. The folder, the sandbox and Undo hold as at Balanced, and where there is no sandbox a program still asks you.' },
  autonomous: { label: 'Autonomous', icon: ShieldOff, text: 'Never asks. A model that obeys something it read can then send what it has read to any address.' },
}
const flat = (text: string, most: number) => { const one = text.replace(/\s+/g, ' ').trim(); return one.length > most ? `${one.slice(0, most)}…` : one }

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
      <summary>What it changed <span className="d-text-tertiary">{change.diff_stat}</span></summary>
      <div className="d-disclosure__body">
        {failed ? <p className="d-type-caption d-text-secondary">{failed}</p>
          : text === null ? <p className="d-type-caption d-text-tertiary">Loading…</p>
          : <pre className="d-code chat__diff-text"><code>{text.replace(/\n$/, '').split('\n').map((line, i) => <span key={i} data-line={kind(line)}>{line}{'\n'}</span>)}</code></pre>}
      </div>
    </details>
  )
}

/** One use of a tool: what was asked for and what came back. Opened, it shows the request as the model made it.
    A use that changed a file can be undone from here, and shows what it changed. A program's whole output is
    one press away: the model was given a short view of it. */
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
        {step.artifact && step.state === 'done' && <Tip label={`Opens ${step.artifact.name} in Files, at version ${step.artifact.n}, with its other versions.`}><Button variant="ghost" size="sm" onClick={() => { if (step.artifact) openArtifact(session, step.artifact.id, step.artifact.n) }}>Open</Button></Tip>}
        {step.output && <Tip label="Everything the command printed, as a terminal would show it. The model was given a short view of it."><a className="d-button d-button--ghost d-button--sm" href={`/api/v1/sessions/${session}/outputs/${step.output}`} target="_blank" rel="noopener">All it printed</a></Tip>}
        {step.model_chose === 'go' && <Tip label="This session is set to Smart: the model was asked whether to ask you first, both ways round, and said to go ahead both times."><span className="d-type-caption d-text-tertiary">The model chose not to ask</span></Tip>}
        {change && (change.undone_at
          ? <span className="d-type-caption d-text-tertiary">Undone</span>
          : <Tip label={change.whole_folder ? 'Puts the whole folder back as it was before this command ran. What it made is removed, what it changed or removed is as it was.' : change.created ? `Removes ${change.path}: this created it.` : `Puts ${change.path} back as it was before this.`}><Button variant="ghost" size="sm" onClick={() => void undoChange(change.session_id, change.id)}>Undo</Button></Tip>)}
      </div>
      {change?.diff_stat && <ChangeShown change={change} />}
    </div>
  )
}

/** The sandbox, set up from here where the core's computer allows it (Windows, DECISIONS T27): once, with an
    administrator's rights, which Windows asks for on that computer's screen. */
function SetUpSandbox({ id }: { id: string }) {
  const [busy, setBusy] = useState(false)
  return (
    <div className="app-row" style={{ gap: 'var(--d-space-8)', alignItems: 'center' }}>
      <Button type="button" disabled={busy} onClick={() => { setBusy(true); void setUpSandbox(id).finally(() => setBusy(false)) }}>{busy ? 'Setting up… (Windows asks first)' : 'Set up the sandbox'}</Button>
      <span className="d-type-caption d-text-secondary">Windows asks for an administrator once. Programs then run as a user of Demido's own: it cannot read your profile or change anything outside the session's folder, and has no network. It can still read what Windows lets every user read, such as folders at C:\ and on other disks.</span>
    </div>
  )
}

function Asking({ request }: { request: PermissionRequest }) {
  const answer = (said: 'once' | 'session' | 'no') => void answerPermission(request.session_id, request.id, said)
  return (
    <section className="d-permission" data-risk={request.reach === 'runs' || request.once_only ? 'high' : request.reach === 'changes' ? 'medium' : 'low'} aria-label={request.what}>
      <header className="d-permission__head">
        <span className="d-permission__risk">{request.reach === 'runs' ? 'Runs a program' : request.reach === 'changes' ? 'Changes a file' : request.reach === 'network' ? (request.once_only ? 'An address the model wrote' : 'Reads the internet') : 'Reads'}</span>
        <h3 className="d-permission__title">The model wants to use a tool</h3>
      </header>
      <pre className="d-permission__what"><code>{request.what}</code></pre>
      <p className="d-permission__why">{request.why}</p>
      <details className="d-disclosure">
        <summary>The request, as the model made it</summary>
        <div className="d-disclosure__body"><pre className="d-code"><code>{JSON.stringify({ tool: request.tool, args: request.args }, null, 1)}</code></pre></div>
      </details>
      <div className="d-permission__actions">
        <Button onClick={() => answer('no')}>No</Button>
        {/* A yes to an address the model wrote is for that one address. */}
        {!request.once_only && <Button onClick={() => answer('session')}>Yes, and do not ask again for {request.tool} in this session</Button>}
        <Button variant="primary" onClick={() => answer('once')}>Yes, this once</Button>
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
      <Tip label="Keeps this answer as a file of the session's own, shown in Files with every version. Nothing is put in the session's folder."><Button variant="ghost" size="sm" onClick={() => { setName(proposed.name); setProblem(null); setOpen(true) }}>Keep as a file</Button></Tip>
      {open && (
        <Dialog open onOpenChange={(o) => !o && setOpen(false)} title="Keep as a file"
          text={proposed.text === m.content ? 'The whole answer, as a file of the session’s own. The same name again keeps a new version of it.' : 'The answer’s block of code, as a file of the session’s own. The same name again keeps a new version of it.'}
          actions={<><Button onClick={() => setOpen(false)}>Cancel</Button><Button variant="primary" disabled={!name.trim()} onClick={() => void keep()}>Keep it</Button></>}>
          <form className="app-stack" style={{ marginBlockStart: 'var(--d-space-16)' }} onSubmit={(e) => { e.preventDefault(); void keep() }}>
            <label className="d-field"><span className="d-field__label">Name, with the ending that says what it is</span><input className="d-input d-input--mono" autoFocus value={name} onChange={(e) => setName(e.target.value)} /></label>
            {problem && <Signal level="caution">{problem}</Signal>}
          </form>
        </Dialog>
      )}
    </>
  )
}

function MessageView({ m, live, doing, tasks, changes, asking, folder }: { m: CoreMessage; live?: { text: string; thought: string }; doing?: { steps?: CoreStep[]; tasks?: CoreTask[] }; tasks: CoreTask[]; changes: CoreChange[]; asking?: PermissionRequest | null; folder: boolean }) {
  const loading = useLocal((s) => s.list?.models.find((x) => x.id === m.model)?.state === 'loading')
  const at = useProviders(() => elsewhere(m.model))
  if (m.role === 'user')
    return <article className="d-message" data-from="user"><div className="d-message__body" style={{ whiteSpace: 'pre-wrap' }}>{m.content}</div></article>
  const writing = m.state === 'answering'
  const text = writing ? (live?.text ?? '') : m.content
  const thought = writing ? (live?.thought ?? '') : (m.thought ?? '')
  // While it is being written, what it is doing comes as news; afterwards, from what is stored.
  const steps = ((writing ? doing?.steps : undefined) ?? m.steps ?? []).filter((s) => s.tool !== 'plan')
  const split = (writing ? doing?.tasks : undefined) ?? tasks
  const busy = steps.length > 0 || split.length > 0
  const t = m.timings
  return (
    <article className="d-message" data-from="assistant">
      <div className="d-message__body chat__blocks">
        {writing && !text && !thought && !busy && <p className="d-text-secondary"><Glyph name="model-loading" busy /> {at ? `Asking ${at}…` : loading ? `Loading ${m.model}…` : 'Waking the model…'}</p>}
        {steps.map((step, i) => <ToolUse key={i} step={step} at={`${m.id}-${i}`} change={changes.find((c) => c.id === step.change)} session={m.session_id} program={byProgram(m.model)} />)}
        {split.length > 0 && <TaskList tasks={split} changes={changes} />}
        {writing && asking && <Asking request={asking} />}
        {thought && (
          // Open while it is being thought, so it can be watched. Folded away once the answer has begun.
          <details className="d-thinking" open={writing && !text ? true : undefined}>
            <summary>{writing && !text ? 'Thinking…' : 'Thought first'}</summary>
            <pre>{thought}</pre>
          </details>
        )}
        {/* A provider's model cannot be told from outside to end its thought. */}
        {writing && thought && !text && !at && <div><Button size="sm" onClick={() => void answerNow(m.session_id)}>Answer now</Button></div>}
        {text && <Prose text={text} />}
        {/* In a session with a folder, an answer about files that read no file is an answer from nowhere. Small models write those. */}
        {folder && m.state === 'done' && !busy && <p className="d-type-caption d-text-tertiary">No file was read or changed for this answer.</p>}
        {m.state === 'stopped' && <Signal level="note">{m.problem ?? 'Stopped. What was written is kept.'}</Signal>}
        {m.state === 'failed' && <Signal level="caution">{m.problem ?? 'The answer failed, and nothing says why.'}</Signal>}
        {m.state === 'done' && m.content.trim() && <div><KeepAsFile m={m} /></div>}
      </div>
      {!writing && t && t.written > 0 && (
        <p className="d-type-caption d-text-tertiary" style={{ marginBlockStart: 'var(--d-space-4)' }}>
          {m.model} · {t.written} tokens{t.per_second > 0 ? ` at ${Math.round(t.per_second)} a second` : ''} · of the prompt, {t.read} tokens read and {t.reused} {at ? `from ${at}'s cache` : 'not read again'}
          {t.cost_usd !== undefined && (byProgram(m.model)
            ? <> · <Tip label={m.model?.startsWith('claude-code/') ? 'What Claude Code said this turn would cost at API prices. Under a Claude plan nothing is billed per token: the turn counts against your plan’s limits.' : 'What OpenCode said this turn cost. Free models cost nothing; others bill through the provider you set up in OpenCode.'}><span>{dollars(t.cost_usd)} by {at}'s own count</span></Tip></>
            : <> · <Tip label={t.cost_exact ? `What ${at ?? 'the provider'} said this answer cost, with every request made for it (the tools it chose, its tasks).` : `What this answer cost, with every request made for it, as Demido counted it from the tokens ${at ?? 'the provider'} reported and the prices it read.`}><span>{dollars(t.cost_usd)}{t.cost_exact ? '' : ' (estimated)'}</span></Tip></>)}
        </p>
      )}
    </article>
  )
}

/** How full the session is, and the way to make it shorter. */
function Room({ id, context, busy, canShorten }: { id: string; context: ContextUse; busy: boolean; canShorten: boolean }) {
  const shortening = useReal((s) => Boolean(s.shortening[id]))
  const full = Math.min(100, Math.round((context.used / context.limit) * 100))
  const k = (n: number) => (n >= 1000 ? `${(n / 1000).toFixed(1)}k` : String(n))
  if (shortening) return <span className="d-type-caption d-text-secondary"><Glyph name="task-running" busy /> Writing an account of the older part…</span>
  return (
    <>
      <Tip label={`About ${context.used.toLocaleString()} of the ${context.limit.toLocaleString()} tokens the model is loaded with room for. ${context.auto ? `At ${context.compact_at.toLocaleString()} the older part is replaced by a short account, by itself.` : 'It is not made shorter by itself (Settings › Context and compaction).'}`}>
        <span className="app-row" style={{ gap: 'var(--d-space-6)' }}>
          <span className="d-meter" style={{ width: '3.5rem' }} role="meter" aria-valuenow={full} aria-valuemin={0} aria-valuemax={100} aria-label="How full the session is"><span style={{ '--d-meter': `${full}%` } as React.CSSProperties} /></span>
          <span className="d-type-caption d-text-tertiary d-mono">{k(context.used)} of {k(context.limit)}</span>
        </span>
      </Tip>
      <Tip label="The model writes a short account of the older messages and works from that. The messages stay here for you.">
        <Button variant="ghost" size="sm" disabled={busy || !canShorten} onClick={() => void makeShorter(id)}>Make shorter</Button>
      </Tip>
    </>
  )
}

/** Where a session was made shorter: what is above is no longer given to the model word for word. */
function Cut({ account }: { account: { content: string; why: string; tokens_before: number } }) {
  return (
    <Signal level="note">
      Made shorter here{account.why === 'auto' ? ', by itself: the conversation had reached about ' + account.tokens_before.toLocaleString() + ' tokens' : ''}. The model no longer sees the messages above word for word. They stay here for you; the model is given this account of them:
      <span style={{ display: 'block', marginBlockStart: 'var(--d-space-6)', whiteSpace: 'pre-wrap' }}>{account.content}</span>
    </Signal>
  )
}

function Composer({ id, answering, context, canShorten, tools, program }: { id: string; answering: boolean; context: ContextUse; canShorten: boolean; tools: boolean; program?: boolean }) {
  const [text, setText] = useState('')
  const [plan, setPlan] = useState(false)
  const input = useRef<HTMLTextAreaElement>(null)
  useEffect(() => { input.current?.focus() }, [id])
  const send = async () => {
    const mine = text.trim()
    if (!mine || answering) return
    setText('')
    // "Plan first" is for the one message it was pressed for.
    if (await say(id, mine, plan && tools)) setPlan(false)
    else setText(mine)
  }
  return (
    <form className="d-composer chat__composer" onSubmit={(e) => { e.preventDefault(); void send() }}>
      <textarea ref={input} className="d-composer__input" rows={1} placeholder="Ask something" aria-label="Message" value={text} onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); void send() } }} />
      <div className="d-composer__bar">
        {/* A program keeps its own conversation and plans its own way: Demido's room and tasks are not its. */}
        {!program && <Room id={id} context={context} busy={answering} canShorten={canShorten} />}
        {!program && (
          <Tip label={tools ? 'For work with several steps. The model first lists the steps; Demido then has it do them one at a time and shows the list as it goes.' : 'Tools are switched off (Settings › Tools and tasks), and tasks are done with them.'}>
            <Button variant="ghost" size="sm" pressed={plan && tools} disabled={!tools} onClick={() => setPlan(!plan)}><Icon icon={ListChecks} size="sm" /> Plan first</Button>
          </Tip>
        )}
        <span className="d-composer__spacer" />
        {answering
          ? <Tip label="Stop"><Button variant="primary" iconOnly aria-label="Stop" onClick={() => void stopAnswer(id)}><Icon icon={Square} size="sm" /></Button></Tip>
          : <Tip label="Send"><Button variant="primary" iconOnly aria-label="Send" type="submit" disabled={!text.trim()}><Icon icon={ArrowUp} /></Button></Tip>}
      </div>
    </form>
  )
}

/** At the top of a schedule's session: what the schedule does, when it runs next, and a run now. */
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
  const next = running ? 'It is running now.' : schedule.state === 'on' && schedule.next_at ? `Next ${whenFromNow(schedule.next_at)}.` : schedule.state === 'paused' ? `Paused${schedule.paused_why ? `: ${schedule.paused_why}` : '.'}` : 'It does not run again.'
  return (
    <Signal level="note" actions={<>
      <Button size="sm" disabled={running} onClick={() => void runSchedule(id).then(() => setTick((t) => t + 1))}>{running ? 'Running…' : 'Run now'}</Button>
      <Button size="sm" variant="ghost" onClick={() => { window.location.hash = '#/schedules' }}>Schedules</Button>
    </>}>
      This is the session of a schedule: {schedule.watch ? `${(schedule.watch_said ?? '').charAt(0).toLowerCase()}${(schedule.watch_said ?? '').slice(1)}` : `"${schedule.what}"`},{schedule.when_said.charAt(0).toLowerCase() + schedule.when_said.slice(1)}. {next} Its runs are said and answered here, on their own.
    </Signal>
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
  // The folder is typed as a path: it is a folder on the computer the core runs on, which may not be this one.
  const [folding, setFolding] = useState(false)
  const [path, setPath] = useState('')
  useEffect(() => { setTold(false); setSure(false); setFolding(false) }, [id])
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
  // Whether models may read from the internet is the person's setting; the list of tools a model is shown says so.
  const internet = detail.tools_told?.includes('search_web') ? 'It can search the internet and read a page from its address; it can only read there, and it asks before opening an address it wrote itself.' : 'Reading from the internet is switched off in the settings.'
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
  const offLabel = program ? 'Its own default' : always ? 'Least thinking' : 'No thinking'
  // Demido's way of answering: what the core tells every model, after the date.
  const INSTRUCTIONS = detail.instructions.split('\n\n').slice(1).join('\n\n')
  return (
    <section className="chat" aria-label="Conversation">
      <header className="chat__head chat__head--tools">
        <h1 className="chat__title">{session.title}</h1>
        {detail.thinking_choices && detail.thinking_choices.length > 0 && (
          <Tip label={always ? 'This model always thinks before it answers. "Least thinking" asks it for as little as it does.' : 'Whether the model thinks before it answers. Slower; it can help on hard questions.'}>
            <span>
              {detail.thinking_choices.length <= 3
                ? <Segmented small label="Thinking" value={session.thinking} onChange={(thinking) => void changeSession(id, { thinking })} options={[{ value: 'off', label: offLabel }, ...detail.thinking_choices.map((c) => ({ value: c, label: c === 'on' ? 'Think first' : `Think: ${c}` }))]} />
                : <Select label="Thinking" value={session.thinking} onChange={(thinking) => void changeSession(id, { thinking })} options={[{ value: 'off', label: offLabel }, ...detail.thinking_choices.map((c) => ({ value: c, label: `Think: ${c}` }))]} />}
            </span>
          </Tip>
        )}
        <Tip label={session.folder ? `The model may read and change files in ${session.folder}, and nowhere else.` : 'Give the session a folder, and the model can read and change files in it. Without one it cannot touch a file.'}>
          <Button variant="ghost" size="sm" pressed={folding} disabled={answering} onClick={() => { setPath(session.folder ?? ''); setFolding(!folding) }}>
            <Icon icon={Folder} size="sm" /> {session.folder ? session.folder.split(/[\\/]/).filter(Boolean).at(-1) : 'No folder'}
          </Button>
        </Tip>
        {session.schedule_id ? (
          <Tip label="This is a schedule's session. A schedule runs on its own, with every tool, and asks you nothing: it has no permission level. What you say here yourself is answered the same way."><span className="d-badge d-badge--outline">Runs on its own</span></Tip>
        ) : <Tip label={LEVELS[level].text}>
          <span className="app-row" style={{ gap: 'var(--d-space-4)' }}>
            <span className="sess__shield" data-level={level}><Icon icon={LEVELS[level].icon} size="sm" /></span>
            <Select label="How much the model may do without asking" value={level} onChange={(permission) => void changeSession(id, { permission })}
              options={(Object.keys(LEVELS) as Level[]).map((l) => ({ value: l, label: LEVELS[l].label }))} />
          </span>
        </Tip>}
        {at && (program
          ? <Tip label={`${at} runs this session on this computer, with its own tools and its own loop, and sends what it needs to its models. Demido shows what it does and asks you what it asks. Before each turn the folder is saved, so all a turn changed can be undone.`}><span className="d-badge d-badge--outline">Runs in {at}</span></Tip>
          : <Tip label={`What is said in this session, and what its tools read, is sent to ${at}. Each answer costs money: Providers shows the month's count and limit.`}><span className="d-badge d-badge--outline">Sent to {at}</span></Tip>)}
        {accounts.length > 1 && inUse && (
          <Tip label={program ? `${at} runs this session signed in as ${inUse.label}, on its plan. With another account it starts again in that account's own profile, and is given the conversation so far.` : `Each answer is paid with ${inUse.label}'s key at ${at}, and counted against it.`}>
            <span><Select label="Account" value={inUse.id} onChange={(account) => void changeSession(id, { account })} options={accounts.map((a) => ({ value: a.id, label: `${a.label}${a.is_default ? ' (default)' : ''}` }))} /></span>
          </Tip>
        )}
        <Select label="Model for this session" value={session.model} onChange={(model) => void changeSession(id, { model })}
          options={[...new Set([session.model, ...models.map((m) => m.id), ...remote.map((m) => m.id)])].map((m) => {
            const there = remote.find((r) => r.id === m)
            return { value: m, label: there ? `${there.name} · ${elsewhere(m)}${there.free ? ', free' : ''}` : m }
          })} />
        <Button variant="ghost" size="sm" pressed={told} onClick={() => setTold(!told)}>What the model is told</Button>
        {sure
          ? <><Button size="sm" variant="ghost" onClick={() => setSure(false)}>Keep</Button><Button size="sm" variant="danger" onClick={async () => { await deleteSession(id); navigate('/sessions') }}>Delete it all</Button></>
          : <Tip label="Delete this session"><Button variant="ghost" size="sm" iconOnly aria-label="Delete this session" onClick={() => setSure(true)}><Icon icon={Trash2} size="sm" /></Button></Tip>}
      </header>
      <div className="chat__thread" ref={thread}>
        <div className="chat__col">
          {session.schedule_id && <ScheduleStrip id={session.schedule_id} />}
          {folding && (
            <form className="chat__folder" onSubmit={(e) => { e.preventDefault(); void changeSession(id, { folder: path.trim() }).then(() => setFolding(false)) }}>
              <label className="d-type-small" htmlFor="session-folder">The folder this session may work in, as a full path on the computer Demido runs on</label>
              <div className="app-row" style={{ gap: 'var(--d-space-8)' }}>
                <input id="session-folder" className="d-input d-mono" style={{ flex: 1, minWidth: 0 }} placeholder="/home/you/project" value={path} onChange={(e) => setPath(e.target.value)} autoFocus spellCheck={false} />
                <Button variant="primary" type="submit" disabled={!path.trim() || path.trim() === session.folder}>Use this folder</Button>
                {session.folder && <Button type="button" onClick={() => void changeSession(id, { folder: '' }).then(() => setFolding(false))}>No folder</Button>}
              </div>
              <p className="d-type-caption d-text-secondary">The model can read every file in it, change them, and run programs in it. It cannot reach anything outside it: that is checked by Demido, not left to the model. Each change can be undone from the answer that made it. {detail.sandbox && detail.sandbox.kind !== 'none' ? `Programs are shut in a sandbox (${detail.sandbox.kind}).` : `There is no sandbox here, so every program asks first. ${detail.sandbox?.why_not ?? ''}`}{(detail.need_not_ask ?? []).length > 0 && ` In this session you said these need not ask again: ${(detail.need_not_ask ?? []).join(', ')}.`}</p>
              {detail.sandbox?.can_set_up && detail.sandbox.kind === 'none' && <SetUpSandbox id={id} />}
            </form>
          )}
          {told && program && (
            <Signal level="note">
              {at} is given your messages, and Demido's way of answering is added to its own instructions, word for word: <span className="d-mono" style={{ whiteSpace: 'pre-wrap' }}>{INSTRUCTIONS}</span> What else it is told, which tools it has and how it goes about the work are its own. When an earlier part of this conversation was answered by another model, {at} is given that part with your next message.
            </Signal>
          )}
          {told && !program && (
            <Signal level="note">
              Before the conversation, the model is told this, word for word: <span className="d-mono" style={{ whiteSpace: 'pre-wrap' }}>{detail.instructions}</span> Then everything said so far, oldest first. Thinking is {session.thinking === 'off' ? 'switched off' : `set to "${session.thinking}"`}.
              {detail.tools_told
                ? <> Before each answer it is first asked, apart from the conversation, whether it needs a tool. It is shown this, with your request in the middle: <span className="d-mono" style={{ whiteSpace: 'pre-wrap', display: 'block', marginBlockStart: 'var(--d-space-6)' }}>{detail.tools_told}</span></>
                : ' Tools are switched off (Settings › Tools and tasks).'}
            </Signal>
          )}
          {!messages.length && (
            <div className="chat__welcome">
              <span dangerouslySetInnerHTML={{ __html: logo }} />
              <h2 className="d-type-title-2">What do you want to know?</h2>
              <p className="d-text-secondary">{program ? `${at} runs this session, with its own tools and its own way of working: it reads, changes and runs what its own rules allow, and asks you through Demido what they say to ask. ${session.folder ? `It works in ${session.folder}, which is saved before each turn so that a turn can be undone.` : 'It works in an empty folder of its own: give the session a folder to let it work on yours.'} Demido's own tools are not used here.` : at ? `${remote.find((r) => r.id === session.model)?.name ?? session.model} runs at ${at}: what you write here, and what its tools read, is sent there, and each answer costs money. ` : `${session.model} loads when you send, and unloads when it has been idle. `}{program ? '' : !detail.tools_told ? 'It answers from what it knows: tools are switched off.' : session.folder ? `It can read and change files in ${session.folder} and run programs there, read the clock, calculate, and split work into tasks. ${detail.sandbox && detail.sandbox.kind !== 'none' ? 'Programs run shut in a sandbox: they see the folder and the system, not the rest of your home, and no network.' : `There is no sandbox on this computer, so it asks before each program. ${detail.sandbox?.why_not ?? ''}`} ${internet}` : `It can read the clock, calculate, and split work into tasks. Give the session a folder, and it can read and change files in it and run programs there. ${internet}`}</p>
            </div>
          )}
          {messages.map((m) => (
            <div key={m.id} className="chat__blocks" style={m.seq <= covered ? { opacity: 0.55 } : undefined}>
              {account && m.seq === covered + 1 && <Cut account={account} />}
              <MessageView m={m} live={writing[m.id]} doing={working[m.id]} tasks={(detail.tasks ?? []).filter((t) => t.message_id === m.id)} changes={detail.changes ?? []} asking={detail.asking && detail.asking.message_id === m.id ? detail.asking : null} folder={Boolean(session.folder)} />
            </div>
          ))}
          {account && covered >= (messages.at(-1)?.seq ?? 0) && <Cut account={account} />}
        </div>
      </div>
      <Composer id={id} answering={answering} context={detail.context} canShorten={messages.filter((m) => m.seq > covered).length >= 4} tools={Boolean(detail.tools_told)} program={program} />
    </section>
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
          <p className="d-text-secondary">A session is a conversation with a model on this computer, or with one you chose elsewhere (Providers).</p>
          <div className="chat__ideas"><Button variant="primary" disabled={!list} onClick={async () => { const id = await newSession(firstModel()); if (id) navigate(`/sessions/${id}`) }}>Start a session</Button></div>
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
