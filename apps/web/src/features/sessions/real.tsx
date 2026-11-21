/* Sessions, for real: conversations with a model on this computer, kept and run by the core.

   This is what is built of the Sessions screen: talking, thinking, making a long session shorter, the first
   tools (the clock, the calculator) and work split into tasks. The preview (the same pages without a core)
   shows where it is going: tools that reach files and the web, permission levels, groups. Those are not
   here yet, and the note at the foot of the list says so. */
import { useEffect, useRef, useState } from 'react'
import Markdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import { ArrowUp, Folder, ListChecks, Plus, ShieldCheck, ShieldEllipsis, ShieldHalf, ShieldOff, Square, Trash2, type LucideIcon } from 'lucide-react'
import { Navigate, Route, Routes, useNavigate, useParams } from 'react-router'
import { useLocal } from '@/data/local-models'
import { answerNow, answerPermission, changeSession, deleteSession, makeShorter, newSession, readSession, say, stopAnswer, undoChange, useReal, type ContextUse, type CoreChange, type CoreMessage, type CoreStep, type CoreTask, type Level, type PermissionRequest } from '@/data/real-sessions'
import { Button, Glyph, Icon, NotBuilt, Segmented, Select, Signal, StepRow, Tip } from '@/ui'
import logo from '@brand/logo/demido-symbol-small.svg?raw'

/** How long ago, short: "now", "5 min", "3 h", "2 d". */
function ago(at: number): string {
  const min = Math.max(0, Math.round((Date.now() - at) / 60_000))
  return min < 1 ? 'now' : min < 60 ? `${min} min` : min < 60 * 48 ? `${Math.round(min / 60)} h` : `${Math.round(min / 60 / 24)} d`
}

function List() {
  const list = useReal((s) => s.list)
  const { '*': rest } = useParams()
  const current = rest?.split('/')[0]
  const navigate = useNavigate()
  const start = async () => {
    const id = await newSession()
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
                <span className="d-row__text"><span className="d-row__title">{s.title}</span><span className="d-row__sub">{s.model} · {s.messages} {s.messages === 1 ? 'message' : 'messages'}</span></span>
                <span className="d-row__meta">{ago(s.updated_at)}</span>
              </button>
            </li>
          ))}
        </ul>
      </div>
      <div className="app-side__foot">
        <span>These conversations are real: kept by the core, answered by a model on this computer.</span>
        <NotBuilt block>Tools that run programs or reach the web; the second look that Smart is to give; groups and search. The preview shows where these are going.</NotBuilt>
      </div>
    </aside>
  )
}

/** A model's words, as markdown. Never as HTML: whatever a model writes is text to be shown, not code to be run. */
function Prose({ text }: { text: string }) {
  return <div className="d-prose"><Markdown remarkPlugins={[remarkGfm]} skipHtml>{text}</Markdown></div>
}

/** What a tool is called when it is used, in plain words, and which of its arguments says what it was used on. */
const TOOL_SAID: Record<string, [string, string]> = {
  time: ['Read the clock', ''], calculate: ['Calculated', 'expression'], list_files: ['Listed the files', ''], read_file: ['Read', 'path'], search_files: ['Searched the files for', 'text'],
  add_to_file: ['Added to', 'path'], write_file: ['Wrote', 'path'], edit_file: ['Changed', 'path'],
}
/** The four levels, as they are today. One shield each, so the level reads from the icon alone: whole, half, deciding, off. */
const LEVELS: Record<Level, { label: string; text: string; icon: LucideIcon }> = {
  cautious: { label: 'Cautious', icon: ShieldCheck, text: 'Asks you before every use of a tool that reaches the folder, reading included.' },
  balanced: { label: 'Balanced', icon: ShieldHalf, text: 'Reads and changes files in the folder without asking. A change cannot leave the folder, and can be undone.' },
  smart: { label: 'Smart', icon: ShieldEllipsis, text: 'The same as Balanced today. It will differ once tools can run programs or reach the network.' },
  autonomous: { label: 'Autonomous', icon: ShieldOff, text: 'Never asks.' },
}
const flat = (text: string, most: number) => { const one = text.replace(/\s+/g, ' ').trim(); return one.length > most ? `${one.slice(0, most)}…` : one }

/** One use of a tool: what was asked for and what came back. Opened, it shows the request as the model made it.
    A use that changed a file can be undone from here. */
function ToolUse({ step, at, change }: { step: CoreStep; at: string; change?: CoreChange }) {
  const [said, on] = TOOL_SAID[step.tool] ?? [step.tool, '']
  const value = (name: string) => { const v = (step.args ?? {})[name]; return typeof v === 'string' ? v : v === undefined ? '' : JSON.stringify(v) }
  const shown = on ? value(on) : Object.keys(step.args ?? {}).map(value).join(', ')
  const denied = step.state === 'refused' && step.result === 'the person said no'
  const refused = step.state === 'refused' && !denied
  return (
    <div className="chat__step">
      <StepRow step={{
        id: at,
        state: step.state === 'running' ? 'running' : step.state === 'waiting' ? 'needs-input' : step.state === 'done' ? 'done' : 'failed',
        what: refused ? `Asked for ${step.tool} in a way that cannot be used` : denied ? `${said}: you said no` : said,
        code: refused ? undefined : flat(shown, 80) || undefined,
        meta: step.state === 'done' ? flat(step.result, 60) : step.state === 'running' ? '' : step.state === 'waiting' ? 'waiting for you' : step.state === 'refused' ? 'not run' : 'could not',
        request: JSON.stringify({ tool: step.tool, args: step.args }, null, 1),
        output: step.state === 'running' || step.state === 'waiting' ? undefined : step.result,
      }} />
      {change && (change.undone_at
        ? <span className="d-type-caption d-text-tertiary">Undone</span>
        : <Tip label={change.created ? `Removes ${change.path}: this created it.` : `Puts ${change.path} back as it was before this.`}><Button variant="ghost" size="sm" onClick={() => void undoChange(change.session_id, change.id)}>Undo</Button></Tip>)}
    </div>
  )
}

/** A use of a tool that waits for the person's yes. What it says is made by the core from the request; none of it is the model's words. */
function Asking({ request }: { request: PermissionRequest }) {
  const answer = (said: 'once' | 'session' | 'no') => void answerPermission(request.session_id, request.id, said)
  return (
    <section className="d-permission" data-risk={request.reach === 'changes' ? 'medium' : 'low'} aria-label={request.what}>
      <header className="d-permission__head">
        <span className="d-permission__risk">{request.reach === 'changes' ? 'Changes a file' : 'Reads'}</span>
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
        <Button onClick={() => answer('session')}>Yes, and do not ask again for {request.tool} in this session</Button>
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
              {t.steps.map((step, i) => <ToolUse key={i} step={step} at={`${t.id}-${i}`} change={changes.find((c) => c.id === step.change)} />)}
              {t.result && <p className="d-type-small d-text-secondary" style={{ whiteSpace: 'pre-wrap' }}>{t.result}</p>}
            </div>
          )}
          <TaskList tasks={tasks} changes={changes} parent={t.id} />
        </li>
      ))}
    </ul>
  )
}

function MessageView({ m, live, doing, tasks, changes, asking, folder }: { m: CoreMessage; live?: { text: string; thought: string }; doing?: { steps?: CoreStep[]; tasks?: CoreTask[] }; tasks: CoreTask[]; changes: CoreChange[]; asking?: PermissionRequest | null; folder: boolean }) {
  const loading = useLocal((s) => s.list?.models.find((x) => x.id === m.model)?.state === 'loading')
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
        {writing && !text && !thought && !busy && <p className="d-text-secondary"><Glyph name="model-loading" busy /> {loading ? `Loading ${m.model}…` : 'Waking the model…'}</p>}
        {steps.map((step, i) => <ToolUse key={i} step={step} at={`${m.id}-${i}`} change={changes.find((c) => c.id === step.change)} />)}
        {split.length > 0 && <TaskList tasks={split} changes={changes} />}
        {writing && asking && <Asking request={asking} />}
        {thought && (
          // Open while it is being thought, so it can be watched. Folded away once the answer has begun.
          <details className="d-thinking" open={writing && !text ? true : undefined}>
            <summary>{writing && !text ? 'Thinking…' : 'Thought first'}</summary>
            <pre>{thought}</pre>
          </details>
        )}
        {writing && thought && !text && <div><Button size="sm" onClick={() => void answerNow(m.session_id)}>Answer now</Button></div>}
        {text && <Prose text={text} />}
        {/* In a session with a folder, an answer about files that read no file is an answer from nowhere. Small models write those. */}
        {folder && m.state === 'done' && !busy && <p className="d-type-caption d-text-tertiary">No file was read or changed for this answer.</p>}
        {m.state === 'stopped' && <Signal level="note">{m.problem ?? 'Stopped. What was written is kept.'}</Signal>}
        {m.state === 'failed' && <Signal level="caution">{m.problem ?? 'The answer failed, and nothing says why.'}</Signal>}
      </div>
      {!writing && t && t.written > 0 && (
        <p className="d-type-caption d-text-tertiary" style={{ marginBlockStart: 'var(--d-space-4)' }}>
          {m.model} · {t.written} tokens at {Math.round(t.per_second)} a second · of the prompt, {t.read} tokens read and {t.reused} not read again
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

function Composer({ id, answering, context, canShorten, tools }: { id: string; answering: boolean; context: ContextUse; canShorten: boolean; tools: boolean }) {
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
        <Room id={id} context={context} busy={answering} canShorten={canShorten} />
        <Tip label={tools ? 'For work with several steps. The model first lists the steps; Demido then has it do them one at a time and shows the list as it goes.' : 'Tools are switched off (Settings › Tools and tasks), and tasks are done with them.'}>
          <Button variant="ghost" size="sm" pressed={plan && tools} disabled={!tools} onClick={() => setPlan(!plan)}><Icon icon={ListChecks} size="sm" /> Plan first</Button>
        </Tip>
        <span className="d-composer__spacer" />
        {answering
          ? <Tip label="Stop"><Button variant="primary" iconOnly aria-label="Stop" onClick={() => void stopAnswer(id)}><Icon icon={Square} size="sm" /></Button></Tip>
          : <Tip label="Send"><Button variant="primary" iconOnly aria-label="Send" type="submit" disabled={!text.trim()}><Icon icon={ArrowUp} /></Button></Tip>}
      </div>
    </form>
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
  const answering = messages.some((m) => m.state === 'answering')
  const level: Level = session.permission ?? 'balanced'
  // The account in force, and the last message it stands for.
  const account = detail.summaries.at(-1)
  const covered = account?.upto_seq ?? 0
  return (
    <section className="chat" aria-label="Conversation">
      <header className="chat__head chat__head--tools">
        <h1 className="chat__title">{session.title}</h1>
        {detail.thinking_choices && detail.thinking_choices.length > 0 && (
          <Tip label="Whether the model thinks before it answers. Slower; it can help on hard questions.">
            <span>
              {detail.thinking_choices.length <= 3
                ? <Segmented small label="Thinking" value={session.thinking} onChange={(thinking) => void changeSession(id, { thinking })} options={[{ value: 'off', label: 'No thinking' }, ...detail.thinking_choices.map((c) => ({ value: c, label: c === 'on' ? 'Think first' : `Think: ${c}` }))]} />
                : <Select label="Thinking" value={session.thinking} onChange={(thinking) => void changeSession(id, { thinking })} options={[{ value: 'off', label: 'No thinking' }, ...detail.thinking_choices.map((c) => ({ value: c, label: `Think: ${c}` }))]} />}
            </span>
          </Tip>
        )}
        <Tip label={session.folder ? `The model may read and change files in ${session.folder}, and nowhere else.` : 'Give the session a folder, and the model can read and change files in it. Without one it cannot touch a file.'}>
          <Button variant="ghost" size="sm" pressed={folding} disabled={answering} onClick={() => { setPath(session.folder ?? ''); setFolding(!folding) }}>
            <Icon icon={Folder} size="sm" /> {session.folder ? session.folder.split(/[\\/]/).filter(Boolean).at(-1) : 'No folder'}
          </Button>
        </Tip>
        <Tip label={LEVELS[level].text}>
          <span className="app-row" style={{ gap: 'var(--d-space-4)' }}>
            <span className="sess__shield" data-level={level}><Icon icon={LEVELS[level].icon} size="sm" /></span>
            <Select label="How much the model may do without asking" value={level} onChange={(permission) => void changeSession(id, { permission })}
              options={(Object.keys(LEVELS) as Level[]).map((l) => ({ value: l, label: LEVELS[l].label }))} />
          </span>
        </Tip>
        <Select label="Model for this session" value={session.model} onChange={(model) => void changeSession(id, { model })}
          options={[...new Set([session.model, ...models.map((m) => m.id)])].map((m) => ({ value: m, label: m }))} />
        <Button variant="ghost" size="sm" pressed={told} onClick={() => setTold(!told)}>What the model is told</Button>
        {sure
          ? <><Button size="sm" variant="ghost" onClick={() => setSure(false)}>Keep</Button><Button size="sm" variant="danger" onClick={async () => { await deleteSession(id); navigate('/sessions') }}>Delete it all</Button></>
          : <Tip label="Delete this session"><Button variant="ghost" size="sm" iconOnly aria-label="Delete this session" onClick={() => setSure(true)}><Icon icon={Trash2} size="sm" /></Button></Tip>}
      </header>
      <div className="chat__thread" ref={thread}>
        <div className="chat__col">
          {folding && (
            <form className="chat__folder" onSubmit={(e) => { e.preventDefault(); void changeSession(id, { folder: path.trim() }).then(() => setFolding(false)) }}>
              <label className="d-type-small" htmlFor="session-folder">The folder this session may work in, as a full path on the computer Demido runs on</label>
              <div className="app-row" style={{ gap: 'var(--d-space-8)' }}>
                <input id="session-folder" className="d-input d-mono" style={{ flex: 1, minWidth: 0 }} placeholder="/home/you/project" value={path} onChange={(e) => setPath(e.target.value)} autoFocus spellCheck={false} />
                <Button variant="primary" type="submit" disabled={!path.trim() || path.trim() === session.folder}>Use this folder</Button>
                {session.folder && <Button type="button" onClick={() => void changeSession(id, { folder: '' }).then(() => setFolding(false))}>No folder</Button>}
              </div>
              <p className="d-type-caption d-text-secondary">The model can read every file in it and change them. It cannot reach anything outside it: that is checked by Demido, not left to the model. Each change can be undone from the answer that made it.{(detail.need_not_ask ?? []).length > 0 && ` In this session you said these need not ask again: ${(detail.need_not_ask ?? []).join(', ')}.`}</p>
            </form>
          )}
          {told && (
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
              <p className="d-text-secondary">{session.model} loads when you send, and unloads when it has been idle. {!detail.tools_told ? 'It answers from what it knows: tools are switched off.' : session.folder ? `It can read and change files in ${session.folder}, read the clock, calculate, and split work into tasks. It cannot run programs or reach the web yet.` : 'It can read the clock, calculate, and split work into tasks. Give the session a folder, and it can read and change files in it. It cannot run programs or reach the web yet.'}</p>
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
      <Composer id={id} answering={answering} context={detail.context} canShorten={messages.filter((m) => m.seq > covered).length >= 4} tools={Boolean(detail.tools_told)} />
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
          <p className="d-text-secondary">A session is a conversation with a model on this computer.</p>
          <div className="chat__ideas"><Button variant="primary" disabled={!list} onClick={async () => { const id = await newSession(); if (id) navigate(`/sessions/${id}`) }}>Start a session</Button></div>
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
