/* Sessions, for real: conversations with a model on this computer, kept and run by the core.

   This is what is built of the Sessions screen. The preview (the same pages without a core) shows where it
   is going: tools, the task list, files, permission levels, groups. None of those are here yet, and the
   note at the foot of the list says so. */
import { useEffect, useRef, useState } from 'react'
import Markdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import { ArrowUp, Plus, Square, Trash2 } from 'lucide-react'
import { Navigate, Route, Routes, useNavigate, useParams } from 'react-router'
import { useLocal } from '@/data/local-models'
import { answerNow, changeSession, deleteSession, makeShorter, newSession, readSession, say, stopAnswer, useReal, type ContextUse, type CoreMessage } from '@/data/real-sessions'
import { Button, Glyph, Icon, NotBuilt, Segmented, Select, Signal, Tip } from '@/ui'
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
        <NotBuilt block>Tools, the task list, files, permission levels, groups and search. The preview shows where these are going.</NotBuilt>
      </div>
    </aside>
  )
}

/** A model's words, as markdown. Never as HTML: whatever a model writes is text to be shown, not code to be run. */
function Prose({ text }: { text: string }) {
  return <div className="d-prose"><Markdown remarkPlugins={[remarkGfm]} skipHtml>{text}</Markdown></div>
}

function MessageView({ m, live }: { m: CoreMessage; live?: { text: string; thought: string } }) {
  const loading = useLocal((s) => s.list?.models.find((x) => x.id === m.model)?.state === 'loading')
  if (m.role === 'user')
    return <article className="d-message" data-from="user"><div className="d-message__body" style={{ whiteSpace: 'pre-wrap' }}>{m.content}</div></article>
  const writing = m.state === 'answering'
  const text = writing ? (live?.text ?? '') : m.content
  const thought = writing ? (live?.thought ?? '') : (m.thought ?? '')
  const t = m.timings
  return (
    <article className="d-message" data-from="assistant">
      <div className="d-message__body chat__blocks">
        {writing && !text && !thought && <p className="d-text-secondary"><Glyph name="model-loading" busy /> {loading ? `Loading ${m.model}…` : 'Waking the model…'}</p>}
        {thought && (
          // Open while it is being thought, so it can be watched. Folded away once the answer has begun.
          <details className="d-thinking" open={writing && !text ? true : undefined}>
            <summary>{writing && !text ? 'Thinking…' : 'Thought first'}</summary>
            <pre>{thought}</pre>
          </details>
        )}
        {writing && thought && !text && <div><Button size="sm" onClick={() => void answerNow(m.session_id)}>Answer now</Button></div>}
        {text && <Prose text={text} />}
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

function Composer({ id, answering, context, canShorten }: { id: string; answering: boolean; context: ContextUse; canShorten: boolean }) {
  const [text, setText] = useState('')
  const input = useRef<HTMLTextAreaElement>(null)
  useEffect(() => { input.current?.focus() }, [id])
  const send = async () => {
    const mine = text.trim()
    if (!mine || answering) return
    setText('')
    if (!(await say(id, mine))) setText(mine)
  }
  return (
    <form className="d-composer chat__composer" onSubmit={(e) => { e.preventDefault(); void send() }}>
      <textarea ref={input} className="d-composer__input" rows={1} placeholder="Ask something" aria-label="Message" value={text} onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); void send() } }} />
      <div className="d-composer__bar">
        <Room id={id} context={context} busy={answering} canShorten={canShorten} />
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
  const models = useLocal((s) => s.list?.models) ?? []
  const navigate = useNavigate()
  const thread = useRef<HTMLDivElement>(null)
  const [told, setTold] = useState(false)
  const [sure, setSure] = useState(false)
  useEffect(() => { setTold(false); setSure(false) }, [id])
  // Only a session the list knows is asked for: the address may still name one that is gone, or one of the preview's.
  useEffect(() => { if (known) void readSession(id) }, [id, known])
  // Follow the answer down as it is written.
  const last = detail?.messages.at(-1)
  const height = (last ? (writing[last.id]?.text.length ?? 0) + (writing[last.id]?.thought.length ?? 0) : 0) + (detail?.messages.length ?? 0)
  useEffect(() => {
    const el = thread.current
    if (el) el.scrollTop = el.scrollHeight
  }, [height, id])
  if (listed && !known && !detail) return <Navigate to="/sessions" replace />
  if (!detail) return <section className="chat" aria-label="Conversation" />
  const { session, messages } = detail
  const answering = messages.some((m) => m.state === 'answering')
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
        <Select label="Model for this session" value={session.model} onChange={(model) => void changeSession(id, { model })}
          options={[...new Set([session.model, ...models.map((m) => m.id)])].map((m) => ({ value: m, label: m }))} />
        <Button variant="ghost" size="sm" pressed={told} onClick={() => setTold(!told)}>What the model is told</Button>
        {sure
          ? <><Button size="sm" variant="ghost" onClick={() => setSure(false)}>Keep</Button><Button size="sm" variant="danger" onClick={async () => { await deleteSession(id); navigate('/sessions') }}>Delete it all</Button></>
          : <Tip label="Delete this session"><Button variant="ghost" size="sm" iconOnly aria-label="Delete this session" onClick={() => setSure(true)}><Icon icon={Trash2} size="sm" /></Button></Tip>}
      </header>
      <div className="chat__thread" ref={thread}>
        <div className="chat__col">
          {told && (
            <Signal level="note">
              Before the conversation, the model is told this, word for word: <span className="d-mono">{detail.instructions}</span> Then everything said so far, oldest first. Thinking is {session.thinking === 'off' ? 'switched off' : `set to "${session.thinking}"`}.
            </Signal>
          )}
          {!messages.length && (
            <div className="chat__welcome">
              <span dangerouslySetInnerHTML={{ __html: logo }} />
              <h2 className="d-type-title-2">What do you want to know?</h2>
              <p className="d-text-secondary">{session.model} loads when you send, and unloads when it has been idle. It answers from what it knows: it cannot look anything up or do anything yet.</p>
            </div>
          )}
          {messages.map((m) => (
            <div key={m.id} className="chat__blocks" style={m.seq <= covered ? { opacity: 0.55 } : undefined}>
              {account && m.seq === covered + 1 && <Cut account={account} />}
              <MessageView m={m} live={writing[m.id]} />
            </div>
          ))}
          {account && covered >= (messages.at(-1)?.seq ?? 0) && <Cut account={account} />}
        </div>
      </div>
      <Composer id={id} answering={answering} context={detail.context} canShorten={messages.filter((m) => m.seq > covered).length >= 4} />
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
