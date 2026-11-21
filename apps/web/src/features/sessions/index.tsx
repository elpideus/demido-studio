import { useCore } from '@/data/core'
import { RealSessions } from './real'
import { useEffect, useMemo, useRef, useState } from 'react'
import { NavLink, Navigate, Route, Routes, useNavigate, useParams } from 'react-router'
import { ArrowUp, AudioLines, Brain, ChevronRight, Copy, Ellipsis, FileText, Folder, FolderPlus, History, MessageSquare, Mic, Paperclip, Plus, RotateCw, Search, ShieldCheck, ShieldEllipsis, ShieldHalf, ShieldOff, Square, Volume2 } from 'lucide-react'
import type { LucideIcon } from 'lucide-react'
import type { Feature } from '@/app/registry'
import { addGroup, answerPermission, loadModel, moveSession, newSession, removeGroup, renameGroup, sendMessage, setSessionField, showArtifact, stopRun, toast, toggleGroup, unloadModel, useSetting, useStore } from '@/data/store'
import type { Block, Message, Model, PermissionLevel, Session, ThinkingLevel } from '@/data/types'
import { Budget, Button, Dialog, Facts, Glyph, Icon, InspectorToggle, Menu, MODEL_STATE_LABEL, ModelChip, NotBuilt, Segmented, Signal, StepRow, Switch, Tasks, Tip, cx, useInspector } from '@/ui'
import logo from '@brand/logo/demido-symbol.svg?raw'
import { KNOWN_FOLDERS, folderName, isAssigned, listFolder } from './files'
import type { FileEntry } from './files'

export const PERMISSION: Record<PermissionLevel, { label: string; text: string }> = {
  cautious: { label: 'Cautious', text: 'Always asks before acting. Very safe, and it interrupts often.' },
  balanced: { label: 'Balanced', text: 'Asks only before things that could cause damage.' },
  autonomous: { label: 'Autonomous', text: 'Never asks. Use it only for work you can undo.' },
  smart: { label: 'Smart', text: 'The model decides when to ask. Every decision is logged.' },
}
/* One shield per level, so the level reads from the icon alone: whole, half, deciding, off. */
export const PERMISSION_ICON: Record<PermissionLevel, LucideIcon> = { cautious: ShieldCheck, balanced: ShieldHalf, smart: ShieldEllipsis, autonomous: ShieldOff }
const THINKING: Record<ThinkingLevel, string> = { off: 'Off', low: 'Low', medium: 'Medium', high: 'High' }
const TASK_GLYPH: Record<Session['state'], string> = { pending: 'task-pending', running: 'task-running', done: 'task-done', failed: 'task-failed', 'needs-input': 'task-needs-input', skipped: 'task-skipped', delegated: 'task-delegated' }

// ───────────────────────── sidebar ─────────────────────────
function SessionList() {
  const sessions = useStore((s) => s.sessions)
  const navigate = useNavigate()
  const [q, setQ] = useState('')
  const [deep, setDeep] = useState(false)
  const [tag, setTag] = useState<string | null>(null)
  const groups = useStore((s) => s.groups)
  const [naming, setNaming] = useState<null | { id?: string; name: string; session?: string }>(null)
  const saveGroup = () => {
    if (!naming?.name.trim()) return
    if (naming.id) renameGroup(naming.id, naming.name)
    else {
      const gid = addGroup(naming.name)
      if (naming.session) moveSession(naming.session, gid)
    }
    setNaming(null)
  }
  const drop = (groupId: string | undefined) => ({
    onDragOver: (e: React.DragEvent) => e.preventDefault(),
    onDrop: (e: React.DragEvent) => { const sid = e.dataTransfer.getData('text/session'); if (sid) moveSession(sid, groupId) },
  })
  const row = (s: Session) => (
    <li key={s.id} className="sess__row" draggable onDragStart={(e) => e.dataTransfer.setData('text/session', s.id)}>
      <NavLink to={`/sessions/${s.id}`} className="d-row" aria-current={undefined} draggable={false}>
        <span className="d-row__lead">
          <Glyph name={TASK_GLYPH[s.state]} tone={s.state === 'needs-input' ? 'caution' : s.state === 'failed' ? 'danger' : undefined} />
        </span>
        <span className="d-row__text">
          <span className="d-row__title">{s.title}</span>
          <span className="d-row__sub">{s.tags.join(' · ') || 'no tags yet'}</span>
        </span>
        <span className="d-row__meta">{s.updated}</span>
      </NavLink>
      <Menu
        align="end"
        trigger={<button type="button" className="sess__more" aria-label={`More for ${s.title}`}><Icon icon={Ellipsis} size="sm" /></button>}
        items={[
          { kind: 'label', label: 'Move to a group' },
          ...groups.map((g) => ({ label: g.name, checked: s.groupId === g.id, onSelect: () => moveSession(s.id, g.id) })),
          { label: 'New group…', onSelect: () => setNaming({ name: '', session: s.id }) },
          ...(s.groupId ? [{ kind: 'sep' as const }, { label: 'Take out of the group', onSelect: () => moveSession(s.id, undefined) }] : []),
        ]}
      />
    </li>
  )
  const tags = useMemo(() => Array.from(new Set(sessions.flatMap((s) => s.tags))).sort(), [sessions])
  const needle = q.trim().toLowerCase()
  const shown = sessions.filter((s) => {
    if (tag && !s.tags.includes(tag)) return false
    if (!needle) return true
    if (s.title.toLowerCase().includes(needle) || s.tags.some((t) => t.includes(needle))) return true
    return deep && s.messages.some((m) => (m.text ?? '').toLowerCase().includes(needle) || m.blocks?.some((b) => b.kind === 'text' && b.plain.toLowerCase().includes(needle)))
  })
  return (
    <aside className="app-side" aria-label="Sessions">
      <div className="app-side__head">
        <span className="d-type-headline">Sessions</span>
        <span className="app-spacer" />
        <Tip label="New group">
          <Button variant="ghost" size="sm" iconOnly aria-label="New group" onClick={() => setNaming({ name: '' })}>
            <Icon icon={FolderPlus} />
          </Button>
        </Tip>
        <Tip label="New session">
          <Button variant="ghost" size="sm" iconOnly aria-label="New session" onClick={() => navigate(`/sessions/${newSession()}`)}>
            <Icon icon={Plus} />
          </Button>
        </Tip>
      </div>
      <label className="d-input-wrap">
        <Icon icon={Search} />
        <input className="d-input" type="search" placeholder={deep ? 'Search titles, tags and contents' : 'Search titles and tags'} aria-label="Search sessions" value={q} onChange={(e) => setQ(e.target.value)} />
      </label>
      {needle && (
        <label className="d-check" style={{ paddingInline: 'var(--d-space-8)' }}>
          <Switch checked={deep} onChange={setDeep} label="Search inside session contents" />
          <span className="d-check__text">
            Search inside contents<small>Slower. Keep it off unless you need it.</small>
          </span>
        </label>
      )}
      <div className="app-row" style={{ gap: 'var(--d-space-4)', paddingInline: 'var(--d-space-6)' }}>
        {tags.slice(0, 7).map((t) => (
          <button key={t} type="button" className={cx('d-badge', tag !== t && 'd-badge--outline')} style={{ border: 0, cursor: 'default' }} aria-pressed={tag === t} onClick={() => setTag(tag === t ? null : t)}>
            {t}
          </button>
        ))}
      </div>
      <div className="app-side__scroll">
        {groups.map((g) => {
          const inside = shown.filter((s) => s.groupId === g.id)
          if ((needle || tag) && !inside.length) return null
          return (
            <div key={g.id} className="sess__group" {...drop(g.id)}>
              <div className="sess__group-head">
                <button type="button" className="sess__group-toggle" aria-expanded={g.open} onClick={() => toggleGroup(g.id)}>
                  <Icon icon={ChevronRight} size="sm" /><span>{g.name}</span><span className="d-text-tertiary">{inside.length}</span>
                </button>
                <Menu
                  align="end"
                  trigger={<button type="button" className="sess__more" aria-label={`More for the group ${g.name}`}><Icon icon={Ellipsis} size="sm" /></button>}
                  items={[
                    { label: 'New session here', onSelect: () => { const sid = newSession(); moveSession(sid, g.id); navigate(`/sessions/${sid}`) } },
                    { label: 'Rename…', onSelect: () => setNaming({ id: g.id, name: g.name }) },
                    { kind: 'sep' },
                    { label: 'Delete the group', hint: 'keeps its sessions', danger: true, onSelect: () => removeGroup(g.id) },
                  ]}
                />
              </div>
              {g.open && (inside.length ? <ul className="d-list">{inside.map(row)}</ul> : <p className="sess__empty">Empty. Drag a session here, or use a session's menu.</p>)}
            </div>
          )
        })}
        <div {...drop(undefined)}>
          {(['Today', 'This week', 'Earlier'] as const).map((when) => {
            const group = shown.filter((s) => !s.groupId && s.when === when)
            if (!group.length) return null
            return (
              <div key={when}>
                <p className="d-list__label">{when}</p>
                <ul className="d-list">{group.map(row)}</ul>
              </div>
            )
          })}
        </div>
        {!shown.length && (
          <p className="app-empty">
            No session matches “{q}”.{!deep && ' Titles and tags only: switch on contents to look inside.'}
          </p>
        )}
      </div>
      <p className="app-side__foot">Sessions name and tag themselves after the first answer. You can change both.</p>
      {naming && (
        <Dialog open onOpenChange={(o) => !o && setNaming(null)} title={naming.id ? 'Rename the group' : 'New group'}
          actions={<><Button onClick={() => setNaming(null)}>Cancel</Button><Button variant="primary" disabled={!naming.name.trim()} onClick={saveGroup}>{naming.id ? 'Rename' : 'Create'}</Button></>}>
          <form onSubmit={(e) => { e.preventDefault(); saveGroup() }} style={{ marginBlockStart: 'var(--d-space-16)' }}>
            <label className="d-field"><span className="d-field__label">Name</span><input className="d-input" autoFocus placeholder="For example: Work" value={naming.name} onChange={(e) => setNaming({ ...naming, name: e.target.value })} /></label>
          </form>
        </Dialog>
      )}
    </aside>
  )
}

// ───────────────────────── message blocks ─────────────────────────
function BlockView({ block, session }: { block: Block; session: Session }) {
  const artifacts = useStore((s) => s.artifacts)
  switch (block.kind) {
    case 'thinking':
      return (
        <details className="d-thinking">
          <summary>Thought for {block.seconds} s</summary>
          <pre>{block.text}</pre>
        </details>
      )
    case 'tasks':
      return <Tasks tasks={block.tasks} />
    case 'step':
      return <StepRow step={block.step} onFull={() => toast('The full output opens here, every line. Not built in the preview.')} />
    case 'text':
      // Fixture HTML is ours. In the app this is rendered markdown, sanitised, never raw model HTML.
      return block.shown === undefined ? (
        <div className="d-prose" dangerouslySetInnerHTML={{ __html: block.html }} />
      ) : (
        <div className="d-prose">
          <p className="chat__caret">{block.plain.split(' ').slice(0, block.shown).join(' ')}</p>
        </div>
      )
    case 'signal':
      return <Signal level={block.level}>{block.text}</Signal>
    case 'permission': {
      const r = block.request
      return (
        <section className="d-permission" data-risk={r.risk} aria-label={r.title}>
          <header className="d-permission__head">
            <span className="d-permission__risk">{r.riskLabel}</span>
            <h3 className="d-permission__title">{r.title}</h3>
          </header>
          <pre className="d-permission__what"><code>{r.what}</code></pre>
          <p className="d-permission__why">{r.why}</p>
          <details className="d-disclosure">
            <summary>Full request</summary>
            <div className="d-disclosure__body"><pre className="d-code"><code>{r.raw}</code></pre></div>
          </details>
          {r.state === 'waiting' ? (
            <div className="d-permission__actions">
              <Button onClick={() => answerPermission(session.id, r.id, 'denied')}>Deny</Button>
              <Button onClick={() => answerPermission(session.id, r.id, 'allowed-session')}>Always allow in this session</Button>
              <Button variant="primary" onClick={() => answerPermission(session.id, r.id, 'allowed')}>Allow once</Button>
            </div>
          ) : (
            <p className="d-type-caption d-text-secondary">
              <Glyph name={r.state === 'denied' ? 'task-skipped' : 'task-done'} /> {r.state === 'denied' ? 'You denied this.' : r.state === 'allowed' ? 'You allowed this once.' : 'You allowed this for the session.'}
            </p>
          )}
        </section>
      )
    }
    case 'artifact': {
      const a = artifacts.find((x) => x.id === block.artifactId)
      if (!a) return null
      const last = a.versions[a.versions.length - 1]
      return (
        <article className="d-artifact">
          <Icon icon={FileText} size="lg" />
          <div className="d-artifact__text">
            <span className="d-artifact__name">{a.name}</span>
            <span className="d-artifact__meta">{a.type} · {last.size} · edited {last.at}</span>
          </div>
          <Button variant="ghost" size="sm" aria-label={`Version ${last.v}, show earlier versions`} onClick={() => showArtifact(a.id)}>
            <Icon icon={History} size="sm" />v{last.v}
          </Button>
          <Button size="sm" onClick={() => showArtifact(a.id)}>Open</Button>
        </article>
      )
    }
  }
}

function MessageView({ message, session }: { message: Message; session: Session }) {
  if (message.from === 'user')
    return (
      <article className="d-message" data-from="user">
        <div className="d-message__body">{message.text}</div>
      </article>
    )
  const empty = !message.blocks?.length
  return (
    <article className="d-message" data-from="assistant">
      <div className="d-message__body chat__blocks">
        {empty && message.streaming && (
          <p className="d-text-secondary">
            <Glyph name="model-loading" /> Waking the model…
          </p>
        )}
        {message.blocks?.map((b, i) => <BlockView key={i} block={b} session={session} />)}
      </div>
      {!message.streaming && !empty && (
        <div className="d-message__actions">
          <Tip label="Copy"><Button variant="ghost" size="sm" iconOnly aria-label="Copy" onClick={() => toast('Copied.')}><Icon icon={Copy} size="sm" /></Button></Tip>
          <Tip label="Read aloud"><Button variant="ghost" size="sm" iconOnly aria-label="Read aloud" onClick={() => toast('Reading aloud needs a voice. Not built in the preview.')}><Icon icon={Volume2} size="sm" /></Button></Tip>
          <Tip label="Retry"><Button variant="ghost" size="sm" iconOnly aria-label="Retry" onClick={() => toast('Retry runs the same request again. Not built in the preview.')}><Icon icon={RotateCw} size="sm" /></Button></Tip>
        </div>
      )}
    </article>
  )
}

// ───────────────────────── composer ─────────────────────────
function Composer({ session, model, onFolder }: { session: Session; model: Model; onFolder: () => void }) {
  const models = useStore((s) => s.models)
  const [text, setText] = useState('')
  const [recording, setRecording] = useState(false)
  const navigate = useNavigate()
  const busy = session.state === 'running' || session.state === 'needs-input'
  const send = () => {
    const t = text.trim()
    if (!t || busy) return
    setText('')
    void sendMessage(session.id, t)
  }
  // "@" points at a file or folder in the session folder. "@src/" lists what is inside src.
  const input = useRef<HTMLTextAreaElement>(null)
  const [caret, setCaret] = useState(0)
  const [pick, setPick] = useState(0)
  const [shut, setShut] = useState(false)
  const assigned = isAssigned(session.folder)
  const at = /(^|\s)@([^\s@]*)$/.exec(text.slice(0, caret))
  const token = at ? at[2] : null
  const dir = token === null ? '' : token.slice(0, token.lastIndexOf('/') + 1)
  const part = token === null ? '' : token.slice(token.lastIndexOf('/') + 1)
  const options = token !== null && assigned ? listFolder(session.folder, dir).filter((e) => e.name.toLowerCase().startsWith(part.toLowerCase())).slice(0, 8) : []
  const pointing = token !== null && !shut
  const accept = (e: FileEntry) => {
    const from = caret - (token ?? '').length
    const insert = dir + e.name + (e.dir ? '/' : ' ')
    const pos = from + insert.length
    setText(text.slice(0, from) + insert + text.slice(caret))
    setCaret(pos)
    setPick(0)
    requestAnimationFrame(() => { input.current?.focus(); input.current?.setSelectionRange(pos, pos) })
  }
  const dictate = () => {
    if (recording) return setRecording(false)
    setRecording(true)
    // What dictation does: transcribe, then clean. The raw words here would be
    // "ehm so can you can you clean up the build folder, no wait, the cache folder too".
    setTimeout(() => {
      setRecording(false)
      setText((v) => (v ? v + ' ' : '') + 'Clean up the build folder and the cache folder.')
    }, 2200)
  }
  return (
    <form className="d-composer chat__composer" onSubmit={(e) => { e.preventDefault(); send() }}>
      {pointing && (
        <div className="sess__at" role="listbox" aria-label="Files in the session folder">
          {!assigned ? (
            <p className="sess__at-note">This session has no folder of yours. <button type="button" className="app-link" onMouseDown={(e) => { e.preventDefault(); onFolder() }}>Assign a folder</button> to point at its files with @.</p>
          ) : options.length ? (
            options.map((e, i) => (
              <button key={e.name} type="button" role="option" aria-selected={i === pick} className="d-menu__item" onMouseDown={(ev) => { ev.preventDefault(); accept(e) }} onMouseEnter={() => setPick(i)}>
                <Icon icon={e.dir ? Folder : FileText} size="sm" /><span>{e.name}{e.dir && '/'}</span><span className="d-menu__hint">{e.hint}</span>
              </button>
            ))
          ) : (
            <p className="sess__at-note">Nothing in {dir || 'the session folder'} starts with “{part}”.</p>
          )}
          <p className="sess__at-foot"><span className="d-mono">{session.folder}/{dir}</span>{assigned && <span>↑↓ choose · Enter or Tab take · Esc close</span>}</p>
        </div>
      )}
      <textarea ref={input} className="d-composer__input" rows={1} placeholder={recording ? 'Listening… (scripted in the preview)' : assigned ? 'Ask, or give a task. @ points at a file.' : 'Ask, or give a task'} aria-label="Message" value={text}
        onChange={(e) => { setText(e.target.value); setCaret(e.target.selectionStart); setShut(false); setPick(0) }}
        onSelect={(e) => setCaret(e.currentTarget.selectionStart)}
        onKeyDown={(e) => {
          if (pointing && options.length) {
            if (e.key === 'ArrowDown' || e.key === 'ArrowUp') { e.preventDefault(); return setPick((pick + (e.key === 'ArrowDown' ? 1 : options.length - 1)) % options.length) }
            if (e.key === 'Enter' || e.key === 'Tab') { e.preventDefault(); return accept(options[pick]) }
          }
          if (pointing && e.key === 'Escape') { e.preventDefault(); return setShut(true) }
          if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); send() }
        }} />
      <div className="d-composer__bar">
        <Tip label="Attach a file"><Button variant="ghost" iconOnly aria-label="Attach" onClick={() => toast('Attaching files: not built in the preview.')}><Icon icon={Paperclip} /></Button></Tip>
        <Menu
          side="top"
          trigger={<ModelChip model={model} />}
          items={[
            { kind: 'label', label: 'On this computer' },
            ...models.filter((m) => m.installed && !m.caps.includes('embedding')).map((m) => ({
              label: m.name, hint: `${(m.quants.find((q) => q.id === m.installed)?.memoryGb ?? 0).toFixed(1)} GB`, checked: m.id === model.id,
              lead: <Glyph name={['loaded', 'generating', 'lent'].includes(m.state) ? 'model-loaded' : 'model-unloaded'} tone={['loaded', 'generating', 'lent'].includes(m.state) ? 'live' : undefined} />,
              onSelect: () => setSessionField(session.id, 'modelId', m.id),
            })),
            { kind: 'sep' },
            model.state === 'unloaded' ? { label: 'Load now', onSelect: () => void loadModel(model.id) } : { label: 'Unload now', onSelect: () => unloadModel(model.id), disabled: model.state === 'generating' || model.state === 'lent' },
          ]}
        />
        <Menu
          side="top"
          trigger={<Button variant="ghost" size="sm"><span className="sess__shield" data-level={session.permission}><Icon icon={PERMISSION_ICON[session.permission]} size="sm" /></span>{PERMISSION[session.permission].label}</Button>}
          items={[{ kind: 'label', label: 'Permission level' }, ...(Object.keys(PERMISSION) as PermissionLevel[]).map((p) => ({ label: PERMISSION[p].label, lead: <span className="sess__shield" data-level={p}><Icon icon={PERMISSION_ICON[p]} size="sm" /></span>, hint: p === 'autonomous' ? 'never asks' : undefined, checked: p === session.permission, onSelect: () => setSessionField(session.id, 'permission', p) }))]}
        />
        {model.caps.includes('thinking') && (
          <Menu
            side="top"
            trigger={<Button variant="ghost" size="sm"><Icon icon={Brain} size="sm" />Thinking: {THINKING[session.thinking].toLowerCase()}</Button>}
            items={[{ kind: 'label', label: 'Thinking' }, ...(Object.keys(THINKING) as ThinkingLevel[]).map((t) => ({ label: THINKING[t], checked: t === session.thinking, onSelect: () => setSessionField(session.id, 'thinking', t) }))]}
          />
        )}
        <span className="d-composer__spacer" />
        <Tip label="Voice mode"><Button variant="ghost" iconOnly aria-label="Voice mode" onClick={() => navigate('/voice/talk')}><Icon icon={AudioLines} /></Button></Tip>
        <Tip label={recording ? 'Stop dictating' : 'Dictate'}><Button variant="ghost" iconOnly aria-label="Dictate" pressed={recording} onClick={dictate}><Icon icon={Mic} /></Button></Tip>
        {busy ? (
          <Tip label="Stop"><Button variant="primary" iconOnly aria-label="Stop" onClick={() => stopRun(session.id)}><Icon icon={Square} size="sm" /></Button></Tip>
        ) : (
          <Tip label="Send"><Button variant="primary" iconOnly aria-label="Send" type="submit" disabled={!text.trim()}><Icon icon={ArrowUp} /></Button></Tip>
        )}
      </div>
    </form>
  )
}

// ───────────────────────── inspector ─────────────────────────
function Inspector({ session, model }: { session: Session; model: Model }) {
  const agents = useStore((s) => s.agents).filter((a) => a.sessionId === session.id)
  const compactAt = useSetting('context.compactAt', 24)
  const used = session.contextUsed
  const stateText = model.state === 'lent' ? `Lent to ${model.lentTo}. Back when it finishes.` : model.state === 'loaded' ? 'Loaded. Unloads after 5 min idle.' : model.state === 'loading' ? `Loading: ${model.loadProgress ?? 0}%` : model.state === 'generating' ? 'Generating.' : model.state === 'queued' ? 'Waiting for memory.' : 'Asleep on disk. Loads when you send.'
  return (
    <aside className="app-insp" aria-label="Session details">
      <section className="app-insp__panel">
        <h4 className="app-insp__title">Model</h4>
        <div className="app-card-head">
          <Glyph name={model.state === 'loading' ? 'model-loading' : model.state === 'lent' ? 'model-lent' : model.state === 'unloaded' ? 'model-unloaded' : model.state === 'queued' ? 'model-queued' : 'model-loaded'} busy={model.state === 'generating'} tone={['loaded', 'generating', 'lent'].includes(model.state) ? 'live' : undefined} size="1.5rem" label={MODEL_STATE_LABEL[model.state]} />
          <div>
            <span style={{ fontWeight: 'var(--d-font-weight-medium)' }}>{model.name}</span>
            <span className="d-type-caption d-text-secondary">{stateText}</span>
          </div>
        </div>
        <Facts rows={[['Quantization', model.installed ?? 'none'], ['Speed', model.speed ? `${model.speed} tokens/s` : 'not measured'], ['Thinking', model.caps.includes('thinking') ? THINKING[session.thinking] : 'not supported']]} />
      </section>
      <section className="app-insp__panel">
        <h4 className="app-insp__title">Memory <span>RTX 3060 · 12.0 GB</span></h4>
        <Budget />
      </section>
      <section className="app-insp__panel">
        <h4 className="app-insp__title">Context <span>{used.toLocaleString()} / {model.context.toLocaleString()}</span></h4>
        <div className="d-meter" role="meter" aria-valuenow={used} aria-valuemin={0} aria-valuemax={model.context} aria-label="Context used">
          <span style={{ '--d-meter': `${Math.min(100, (used / model.context) * 100)}%` } as React.CSSProperties} />
        </div>
        <p className="d-type-caption d-text-secondary">Compacts on its own at {compactAt}k tokens. <button type="button" className="app-reset" style={{ color: 'var(--d-color-text-primary)', textDecoration: 'underline' }} onClick={() => toast('Compaction summarises older messages and shows you the summary. Not built in the preview.')}>Compact now</button></p>
      </section>
      <section className="app-insp__panel">
        <h4 className="app-insp__title">Agents</h4>
        {agents.length ? agents.map((a) => (
          <p key={a.id} className="app-now__row"><Glyph name="task-delegated" /> <span>Agent 1: {a.label.toLowerCase()}. Using the session’s model.</span></p>
        )) : <p className="d-type-caption d-text-secondary">None running. Mechanical sub-tasks go to agents with a fresh context.</p>}
      </section>
      <section className="app-insp__panel">
        <h4 className="app-insp__title">Permission level</h4>
        <Segmented small fill label="Permission level" value={session.permission} onChange={(p) => setSessionField(session.id, 'permission', p)}
          options={[{ value: 'cautious', label: 'Cautious' }, { value: 'balanced', label: 'Balanced' }, { value: 'autonomous', label: 'Auto' }, { value: 'smart', label: 'Smart' }]} />
        <p className="app-now__row"><span className="sess__shield" data-level={session.permission}><Icon icon={PERMISSION_ICON[session.permission]} /></span><span className="d-type-caption d-text-secondary">{PERMISSION[session.permission].text}</span></p>
      </section>
      <section className="app-insp__panel">
        <h4 className="app-insp__title">This session</h4>
        <Facts rows={[['Folder', session.folder.replace('~/Demido/sessions/', '…/')], ['Outside content', session.taint.length ? session.taint.join(', ') : 'none']]} />
        <p className="d-type-caption d-text-secondary">{session.taint.length ? 'This session has read outside content. It is treated as data: it cannot give orders.' : 'Nothing from outside has entered this session.'}</p>
      </section>
    </aside>
  )
}

// ───────────────────────── chat ─────────────────────────
const IDEAS = ['Compare two laptops for local models', 'Clean up the build and cache folders', 'Reply to Anna about Thursday']

function Chat() {
  const { id } = useParams()
  const session = useStore((s) => s.sessions.find((x) => x.id === id))
  const models = useStore((s) => s.models)
  const thread = useRef<HTMLDivElement>(null)
  const [inspector, toggleInspector] = useInspector('sessions')
  const [folders, setFolders] = useState(false)
  const count = session?.messages.reduce((n, m) => n + 1 + (m.blocks?.length ?? 0) + (m.blocks?.reduce((k, b) => k + (b.kind === 'text' ? (b.shown ?? 0) : 0), 0) ?? 0), 0)
  useEffect(() => {
    const el = thread.current
    if (el) el.scrollTop = el.scrollHeight
  }, [count, id])
  if (!session) return <Navigate to="/sessions" replace />
  const model = models.find((m) => m.id === session.modelId) ?? models[0]
  return (
    <>
      <section className="chat" aria-label="Conversation">
        <header className="chat__head">
          <h1 className="chat__title">{session.title}</h1>
          {session.tags.map((t) => <span key={t} className="d-badge">{t}</span>)}
          <span className="app-spacer" />
          <Tip label={session.taint.length ? `Has read outside content: ${session.taint.join(', ')}` : 'Nothing from outside has entered this session'}>
            <span className={cx('d-badge', 'd-badge--outline')}>{session.taint.length ? `Outside content: ${session.taint.join(', ')}` : 'Clean'}</span>
          </Tip>
          <Tip label={isAssigned(session.folder) ? `The model works in ${session.folder}` : 'Assign a folder for the model to work in'}>
            <Button variant="ghost" size="sm" aria-label="Session folder" onClick={() => setFolders(true)}><Icon icon={Folder} size="sm" />{isAssigned(session.folder) ? folderName(session.folder) : 'No folder'}</Button>
          </Tip>
          <InspectorToggle open={inspector} onToggle={toggleInspector} />
        </header>
        <div className="chat__thread" ref={thread}>
          <div className="chat__col">
            {!session.messages.length && (
              <div className="chat__welcome">
                <span dangerouslySetInnerHTML={{ __html: logo }} />
                <h2 className="d-type-title-2">What should we do?</h2>
                <p className="d-text-secondary">The model loads when you send, and unloads when it has been idle. Try one of these:</p>
                <div className="chat__ideas">
                  {IDEAS.map((t) => <Button key={t} onClick={() => void sendMessage(session.id, t)}>{t}</Button>)}
                </div>
                <NotBuilt>Preview: answers are scripted, no model runs</NotBuilt>
              </div>
            )}
            {session.messages.map((m) => <MessageView key={m.id} message={m} session={session} />)}
          </div>
        </div>
        <Composer session={session} model={model} onFolder={() => setFolders(true)} />
      </section>
      {inspector && <Inspector session={session} model={model} />}
      {folders && (
        <Dialog open onOpenChange={(o) => !o && setFolders(false)} title="Session folder" text="The model works in this folder: it reads, writes and runs commands there. In the message box, @ points at its files and folders."
          actions={<><Button variant="ghost" onClick={() => toast('Opens the folder in Files. Not built in the preview.')}>Open in Files</Button><span className="app-spacer" /><Button onClick={() => setFolders(false)}>Done</Button></>}>
          <div className="app-stack" style={{ marginBlockStart: 'var(--d-space-16)', gap: 'var(--d-space-4)' }}>
            {[...new Set([...(isAssigned(session.folder) ? [session.folder] : []), ...KNOWN_FOLDERS])].map((f) => (
              <label key={f} className="d-check"><input className="d-radio" type="radio" name="folder" checked={session.folder === f} onChange={() => setSessionField(session.id, 'folder', f)} /><span className="d-check__text"><span className="d-mono">{f}</span><small>{listFolder(f, '').length ? `${listFolder(f, '').length} items at the top` : 'Not read in the preview'}</small></span></label>
            ))}
            <label className="d-check"><input className="d-radio" type="radio" name="folder" checked={!isAssigned(session.folder)} onChange={() => setSessionField(session.id, 'folder', `~/Demido/sessions/${session.id}`)} /><span className="d-check__text">No folder of mine<small>Demido keeps a private folder for this session. @ is off.</small></span></label>
            <div className="app-row" style={{ marginBlockStart: 'var(--d-space-8)' }}><Button onClick={() => toast('Opens your system\'s folder chooser. Not built in the preview.')}>Choose another folder…</Button><NotBuilt>System folder chooser</NotBuilt></div>
          </div>
        </Dialog>
      )}
    </>
  )
}

function SessionsPage() {
  const first = useStore((s) => s.sessions[0]?.id)
  const [inspector] = useInspector('sessions')
  // With a core behind the pages, sessions are real conversations with a model on this computer.
  const core = useCore().state
  if (core === 'ready') return <RealSessions />
  // Not known yet which it is (a few milliseconds): show neither, so the preview's sessions do not flash by.
  if (core === 'looking') return <div className="app-split" />
  return (
    <div className="app-split" data-inspector={inspector ? '' : undefined}>
      <SessionList />
      <Routes>
        <Route path=":id" element={<Chat />} />
        <Route path="*" element={<Navigate to={`/sessions/${first}`} replace />} />
      </Routes>
      <ArtifactPanel />
    </div>
  )
}

/** Artifacts open beside the conversation, with every earlier version one press away. */
function ArtifactPanel() {
  const open = useStore((s) => s.openArtifact)
  const artifact = useStore((s) => s.artifacts.find((a) => a.id === s.openArtifact?.id))
  if (!open || !artifact) return null
  const v = artifact.versions.find((x) => x.v === open.version) ?? artifact.versions[artifact.versions.length - 1]
  return (
    <div className="app-artifact" role="dialog" aria-label={artifact.name}>
      <header className="chat__head">
        <Icon icon={FileText} />
        <span className="chat__title">{artifact.name}</span>
        <span className="d-badge">{artifact.type}</span>
        <span className="app-spacer" />
        <Button size="sm" onClick={() => toast(`${artifact.name} v${v.v} saved to your Downloads folder. (Scripted.)`)}>Download</Button>
        <Button variant="ghost" size="sm" onClick={() => showArtifact(null)}>Close</Button>
      </header>
      <div className="app-artifact__body">
        <ul className="d-list app-artifact__versions" aria-label="Versions">
          <li className="d-list__label">Versions</li>
          {[...artifact.versions].reverse().map((x) => (
            <li key={x.v}>
              <button type="button" className="d-row d-row--compact" aria-current={x.v === v.v} onClick={() => showArtifact(artifact.id, x.v)}>
                <span className="d-row__text"><span className="d-row__title">v{x.v} · {x.note}</span><span className="d-row__sub">{x.at} · {x.size}</span></span>
              </button>
            </li>
          ))}
        </ul>
        <pre className="d-code app-artifact__content">{v.content}</pre>
      </div>
    </div>
  )
}

export const feature: Feature = {
  id: 'sessions',
  title: 'Sessions',
  icon: MessageSquare,
  group: 'work',
  order: 0,
  path: 'sessions',
  home: '/sessions/s-sweater',
  Page: SessionsPage,
}
