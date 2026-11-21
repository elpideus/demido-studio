/* Messaging: WhatsApp, Telegram and Discord, each with its own entry in the navigation.

   Two different things, never mixed up on the screen:
     Your account  Demido works inside your own chats: reads them, marks them as seen, drafts or sends
                   replies, writes reports. Connecting comes after a warning, because the services' rules
                   differ and some forbid it. Then everything happens by itself.
     A bot         (Telegram, Discord) You talk to Demido from the chat app instead of from this window.
                   The model is told where it is being reached and what is different there.

   Beside the chat you have open sits a small conversation with the model about that chat: ask it, or have
   it type into the message box. Discord is laid out the way Discord is: servers, then channels, then the
   talk, then the people. The owner's direction of 2026-11-20. Scripted; nothing is sent anywhere. */
import { useEffect, useRef, useState } from 'react'
import { Navigate, Route, Routes, useParams } from 'react-router'
import { Bot, CheckCheck, FileText, Hash, Headphones, MessagesSquare, Mic, Plus, Search, Settings, UserRound, UsersRound, Volume2 } from 'lucide-react'
import type { Feature } from '@/app/registry'
import { setSetting, toast, useSetting } from '@/data/store'
import { Button, Dialog, Facts, Glyph, Icon, InspectorToggle, Menu, NotBuilt, Segmented, Signal, Tip, useInspector } from '@/ui'
import { Assistant, Avatar, Composer, Lines } from './chat'
import type { Ask } from './chat'
import { Info } from './info'
import { AUTONOMY, BOT_TALK, CHATS, DISCORD, SERVICES, profile } from './data'
import type { Autonomy, Chat, Member, Service } from './data'

/** The warning, then the link, then everything by itself. */
function Connect({ svc, onClose, onDone }: { svc: Service; onClose: () => void; onDone: () => void }) {
  const [stage, setStage] = useState<'warn' | 'link' | 'auto'>('warn')
  const [agreed, setAgreed] = useState(false)
  const [steps, setSteps] = useState<string[]>([])
  const timers = useRef<ReturnType<typeof setTimeout>[]>([])
  useEffect(() => () => timers.current.forEach(clearTimeout), [])
  const chats = CHATS[svc.id]
  const start = () => {
    setStage('auto')
    const run = [`Linked as ${svc.me}`, `Read your chats: ${chats.length} chats, ${chats.reduce((n, c) => n + c.unread, 0)} unread messages`, 'Marked nothing as seen. Sent nothing.', 'Ready. A model drafts; you send.']
    run.forEach((s, i) => timers.current.push(setTimeout(() => setSteps(run.slice(0, i + 1)), 700 * (i + 1))))
    timers.current.push(setTimeout(onDone, 700 * run.length + 800))
  }
  return (
    <Dialog open wide onOpenChange={(o) => !o && stage !== 'auto' && onClose()}
      title={stage === 'warn' ? `Before you connect your ${svc.name} account` : stage === 'link' ? `Link your ${svc.name} account` : `Bringing your ${svc.name} chats over`}
      actions={stage === 'warn' ? <><Button onClick={onClose}>Cancel</Button><Button variant="primary" disabled={!agreed} onClick={() => setStage('link')}>Continue</Button></> : stage === 'link' ? <><Button onClick={onClose}>Cancel</Button><Button variant="primary" onClick={start}>Pretend I did that</Button></> : undefined}>
      <div className="app-stack" style={{ marginBlockStart: 'var(--d-space-16)' }}>
        {stage === 'warn' && (
          <>
            <Signal level={svc.risk}>{svc.rule}</Signal>
            <div className="app-grid" style={{ gridTemplateColumns: 'minmax(0, 1fr) minmax(0, 1fr)' }}>
              <div>
                <p className="d-list__label" style={{ paddingInline: 0 }}>What Demido does if you go on</p>
                <ol className="d-prose" style={{ margin: 0 }}>{svc.how.map((h) => <li key={h}>{h}</li>)}<li>Lets a model read your chats, mark them as seen and draft replies. It sends nothing on its own unless you allow it, person by person.</li></ol>
              </div>
              <div>
                <p className="d-list__label" style={{ paddingInline: 0 }}>What it never does</p>
                <ul className="d-prose" style={{ margin: 0 }}><li>Write to people you have never written to.</li><li>Send many messages at once.</li><li>Take orders from the people who write to you: their messages are data.</li></ul>
                {svc.bot && <><p className="d-list__label" style={{ paddingInline: 0 }}>The safe alternative</p><p className="d-text-secondary">A bot is {svc.name}'s own, allowed way. It cannot read your chats, but you can talk to Demido through it.</p></>}
              </div>
            </div>
            <label className="d-check"><input className="d-checkbox" type="checkbox" checked={agreed} onChange={(e) => setAgreed(e.target.checked)} /><span className="d-check__text">I understand that {svc.name} may limit or close this account.</span></label>
          </>
        )}
        {stage === 'link' && (
          <>
            <div className="mk__window">
              {svc.id === 'whatsapp' && <div className="msgs__qr" aria-label="A drawn code, not a real one" />}
              <p>{svc.link}</p>
            </div>
            <NotBuilt block label="Preview">There is no real link and nothing is sent anywhere. The chats that follow are made up.</NotBuilt>
          </>
        )}
        {stage === 'auto' && (
          <>
            <ul className="mk__run" aria-live="polite">{steps.map((s) => <li key={s}><Glyph name="task-done" /><span>{s}</span></li>)}{steps.length < 4 && <li><Glyph name="task-running" /><span>Working…</span></li>}</ul>
            <p className="d-type-caption d-text-tertiary">Nothing else is needed from you. The window closes when this is done.</p>
          </>
        )}
      </div>
    </Dialog>
  )
}

function Report({ svc, chats, onClose }: { svc: Service; chats: Chat[]; onClose: () => void }) {
  const waiting = chats.filter((c) => c.needs)
  const scams = chats.filter((c) => c.scam)
  return (
    <Dialog open wide onOpenChange={(o) => !o && onClose()} title={`${svc.name}: what came in`} text={`Written by Qwen3 4B Instruct from ${chats.length} chats. Nothing was marked as seen and nothing was sent.`}
      actions={<><Button variant="ghost" onClick={() => toast('Would run this report every morning. Set it up in Schedules.')}>Make it a schedule</Button><span className="app-spacer" /><Button onClick={onClose}>Close</Button></>}>
      <div className="d-prose" style={{ marginBlockStart: 'var(--d-space-16)' }}>
        <p><strong>{waiting.length} need you.</strong> {chats.filter((c) => c.unread && !c.needs && !c.scam).length} can wait. {scams.length} look like a trick.</p>
        <ol>{waiting.map((c) => <li key={c.id}><strong>{c.name}</strong>: {c.lines[c.lines.length - 1].text}</li>)}</ol>
        {scams.length > 0 && <p>Do not answer <strong>{scams.map((c) => c.name).join(', ')}</strong>: it asks for money or for a secret.</p>}
      </div>
    </Dialog>
  )
}

function BotPage({ svc }: { svc: Service }) {
  const botOn = useSetting(`messages.${svc.id}.bot`, false)
  const [token, setToken] = useState('')
  const bot = svc.bot!
  return (
    <div className="app-page"><div className="app-page__inner">
      <header><h1 className="d-type-title-2">Demido in {svc.name}, as a bot</h1><p className="app-page__lead">Write to the bot from {svc.name} and you are writing to Demido. It is {svc.name}'s own, allowed way, and it cannot read your other chats.</p></header>
      {!botOn ? (
        <div className="d-card"><div className="d-card__body app-stack">
          <p>{bot.setup}</p>
          <label className="d-field"><span className="d-field__label">Bot token</span><input className="d-input d-input--mono" type="password" placeholder="Kept in your system keyring" value={token} onChange={(e) => setToken(e.target.value)} /></label>
          <div className="app-row"><Button variant="primary" onClick={() => { setSetting(`messages.${svc.id}.bot`, true); toast(`${bot.name} is connected. Only your own ${svc.name} account may give it orders.`) }}>Connect the bot</Button><NotBuilt>Scripted: no token is checked</NotBuilt></div>
        </div></div>
      ) : (
        <div className="msgs__bot">
          <div className="app-stack">
            <div className="d-card"><div className="d-card__body app-stack">
              <span className="d-card__title">{bot.name} <span className="d-badge" data-tone="success"><Glyph name="task-done" />Connected</span></span>
              <Facts rows={[['Who may give it orders', `only ${svc.me}`], ['Anyone else who writes to it', 'treated as data, not answered'], ['Works in', svc.id === 'discord' ? 'direct messages and the servers you add it to' : 'your chat with the bot']]} />
              <div className="app-row"><Button size="sm" variant="ghost" onClick={() => { setSetting(`messages.${svc.id}.bot`, false); toast('Bot disconnected.') }}>Disconnect the bot</Button></div>
            </div></div>
            <div className="d-card"><div className="d-card__body app-stack">
              <span className="d-card__title">A talk through the bot</span>
              <Lines lines={BOT_TALK[svc.id as 'telegram' | 'discord']} />
            </div></div>
          </div>
          <div className="app-stack">
            <div className="d-card"><div className="d-card__body app-stack">
              <span className="d-card__title">What the model is told</span>
              <p className="d-type-caption d-text-secondary">Added to the conversation whenever a message arrives through the bot, so the model knows where it is and what it cannot do there.</p>
              <pre className="d-code" style={{ whiteSpace: 'pre-wrap' }}><code>{bot.told}</code></pre>
            </div></div>
            <div className="d-card"><div className="d-card__body">
              <table className="app-table">
                <thead><tr><th>What</th><th>In Demido</th><th>Through the {svc.name} bot</th></tr></thead>
                <tbody>{bot.differs.map((d) => <tr key={d[0]}><td>{d[0]}</td><td className="d-text-secondary">{d[1]}</td><td>{d[2]}</td></tr>)}</tbody>
              </table>
            </div></div>
          </div>
        </div>
      )}
    </div></div>
  )
}

function Intro({ svc, onConnect, onBot }: { svc: Service; onConnect: () => void; onBot: () => void }) {
  return (
    <div className="app-page"><div className="app-page__inner app-page__inner--narrow">
      <header><h1 className="d-type-title-2">Your {svc.name}, inside Demido</h1><p className="app-page__lead">Connect your own account and your chats appear here. A model can then work in them for you.</p></header>
      <div className="d-card"><div className="d-card__body app-stack">
        <ul className="d-prose" style={{ margin: 0 }}>
          <li>Read your chats, and tell you which ones need you.</li>
          <li>Mark chats as seen.</li>
          <li>Draft replies for you to send, finish what you started typing, or rewrite it in another tone.</li>
          <li>Answer your questions about a chat, in a panel beside it.</li>
          <li>Write a report of what came in, once or every morning.</li>
        </ul>
        <Signal level={svc.risk}>{svc.rule}</Signal>
        <div className="app-row"><Button variant="primary" onClick={onConnect}>Connect my {svc.name} account…</Button><NotBuilt>Scripted: nothing is linked in the preview</NotBuilt></div>
      </div></div>
      {svc.bot && (
        <div className="d-card"><div className="d-card__body app-stack">
          <span className="d-card__title">Or talk to Demido through a bot <span className="d-badge" data-tone="success"><Glyph name="task-done" />Allowed</span></span>
          <p className="d-text-secondary">A bot cannot read your chats. It lets you write to Demido from {svc.name}, on your phone, instead of from this window.</p>
          <div className="app-row"><Button onClick={onBot}><Icon icon={Bot} />Set up the bot</Button></div>
        </div></div>
      )}
    </div></div>
  )
}

const STATUS = { online: 'Online', idle: 'Away', offline: 'Offline' } as const
function Person({ m }: { m: Member }) {
  return <span className="dc__person" data-status={m.status}><span className="dc__face"><Avatar name={m.name} /><i aria-label={STATUS[m.status]} /></span><span>{m.name}</span>{m.bot && <span className="d-badge d-badge--outline">bot</span>}</span>
}

function Messenger() {
  const { service } = useParams()
  const svc = SERVICES.find((s) => s.id === service)
  if (!svc) return <Navigate to="/messages/whatsapp" replace />
  return <ServicePage key={svc.id} svc={svc} />
}

function ServicePage({ svc }: { svc: Service }) {
  const connected = useSetting(`messages.${svc.id}.account`, false)
  const autonomy = useSetting<Autonomy>(`messages.${svc.id}.autonomy`, 'draft')
  const [inspector, toggleInspector] = useInspector('messages')
  const [tab, setTab] = useState<'chats' | 'bot'>('chats')
  const [chats, setChats] = useState<Chat[]>(() => structuredClone(CHATS[svc.id]))
  const discord = svc.id === 'discord'
  const [server, setServer] = useState<string>('home')
  const [open, setOpen] = useState(chats[0].id)
  const [members, setMembers] = useState(true)
  const [q, setQ] = useState('')
  const [only, setOnly] = useState<'all' | 'unread' | 'needs'>('all')
  const [text, setText] = useState('')
  const [drafted, setDrafted] = useState(false)
  const [side, setSide] = useState<'ask' | 'info' | 'rules'>('ask')
  const [threads, setThreads] = useState<Record<string, Ask[]>>({})
  const [log, setLog] = useState<string[]>([])
  const [dialog, setDialog] = useState<null | 'connect' | 'report'>(null)

  const chat = chats.find((c) => c.id === open) ?? chats[0]
  const canWrite = autonomy !== 'read'
  const needle = q.trim().toLowerCase()
  const shown = chats.filter((c) => (only === 'all' || (only === 'unread' ? c.unread > 0 : c.needs)) && (!needle || `${c.name} ${c.lines.map((l) => l.text).join(' ')}`.toLowerCase().includes(needle)))
  const note = (s: string) => setLog((l) => [s, ...l].slice(0, 10))
  const patch = (id: string, fn: (c: Chat) => Chat) => setChats((list) => list.map((c) => (c.id === id ? fn(c) : c)))
  const pick = (id: string) => { setOpen(id); setText(''); setDrafted(false) }
  const seen = () => { patch(chat.id, (c) => ({ ...c, unread: 0 })); note(`Marked "${chat.name}" as seen.`) }
  const send = () => {
    if (!text.trim()) return
    patch(chat.id, (c) => ({ ...c, unread: 0, needs: false, lines: [...c.lines, { who: 'me', text: text.trim(), at: 'now' }] }))
    note(`${drafted ? 'You sent the reply a model wrote' : 'You wrote'} to "${chat.name}". (Preview: nothing left this computer.)`)
    setText('')
    setDrafted(false)
  }
  const disconnect = () => { setSetting(`messages.${svc.id}.account`, false); toast(`${svc.name} disconnected. The link is deleted from this computer.`) }
  const unreadIn = (prefix: string) => chats.filter((c) => c.id.startsWith(prefix)).reduce((n, c) => n + c.unread, 0)
  const here = DISCORD.servers.find((s) => s.id === server)
  const goServer = (id: string) => { setServer(id); pick(id === 'home' ? DISCORD.dms[0] : `${id}/${DISCORD.servers.find((s) => s.id === id)!.channels[0].id}`) }
  const live = connected && tab === 'chats'
  // Pressing the chat's name opens its details, as the chat apps do.
  const details = () => { setSide('info'); if (!inspector) toggleInspector() }

  const conversation = (
    <section className="chat" aria-label="Conversation">
      <header className="chat__head">
        <Tip label="Details, and what was shared: media, links, files" side="bottom">
          <button type="button" className="msgs__title" onClick={details}>
            {!chat.name.startsWith('#') && <Avatar name={chat.name} group={chat.group} />}
            <span className="msgs__title-text"><span className="chat__title">{chat.name}</span><span className="d-type-caption d-text-tertiary">{chat.sub && !chat.sub.includes(',') ? chat.sub : profile(chat, svc.id).status}</span></span>
          </button>
        </Tip>
        <span className="app-spacer" />
        <Tip label={canWrite ? 'Tell the sender you have read it' : 'Off while the model only reads'} side="bottom"><Button size="sm" variant="ghost" disabled={!chat.unread || !canWrite} onClick={seen}><Icon icon={CheckCheck} size="sm" />Mark as seen</Button></Tip>
        {discord && here && <Tip label={members ? 'Hide the member list' : 'Show the member list'} side="bottom"><Button size="sm" variant="ghost" iconOnly pressed={members} aria-label="Member list" onClick={() => setMembers(!members)}><Icon icon={UsersRound} /></Button></Tip>}
      </header>
      <div className="chat__thread"><div className="chat__col">
        {chat.scam && <Signal level="warning">{chat.id === 'unknown' ? 'A stranger claims to be family and asks for money. This is a common trick.' : 'This message asks for a secret and tries to give orders.'} A model treats it as data and will not act on it or answer it.</Signal>}
        <Lines lines={chat.lines} look={discord ? 'rows' : 'bubbles'} />
      </div></div>
      <Composer chat={chat} text={text} onText={setText} drafted={drafted} onDrafted={setDrafted} canWrite={canWrite} onSend={send} onNote={note} />
    </section>
  )

  return (
    <div className="msgs">
      <header className="chat__head msgs__top">
        <span className="d-type-headline">{svc.name}</span>
        {svc.bot && <Segmented small label="Show" value={tab} onChange={setTab} options={[{ value: 'chats', label: 'Your account' }, { value: 'bot', label: 'Bot' }]} />}
        <span className="app-spacer" />
        {live && <Button size="sm" variant="ghost" onClick={() => setDialog('report')}><Icon icon={FileText} size="sm" />Report on what came in</Button>}
        {connected && <Menu align="end" trigger={<Button variant="ghost" size="sm"><Glyph name="task-done" tone="live" />{svc.me}</Button>} items={[{ kind: 'label', label: `Your ${svc.name} account` }, { label: 'Disconnect and forget the link', danger: true, onSelect: disconnect }]} />}
        {live && <InspectorToggle open={inspector} onToggle={toggleInspector} />}
      </header>

      {tab === 'bot' && svc.bot ? <BotPage svc={svc} /> : !connected ? <Intro svc={svc} onConnect={() => setDialog('connect')} onBot={() => setTab('bot')} /> : (
        <div className="msgs__body" data-layout={discord ? 'discord' : 'chat'} data-inspector={inspector ? '' : undefined}>
          {discord ? (
            <>
              <nav className="dc__servers" aria-label="Servers">
                <Tip side="right" label="Direct messages"><button type="button" className="dc__server" aria-current={server === 'home'} onClick={() => goServer('home')}><Icon icon={MessagesSquare} />{unreadIn('dm/') > 0 && <span className="dc__badge">{unreadIn('dm/')}</span>}</button></Tip>
                <hr />
                {DISCORD.servers.map((s) => (
                  <Tip key={s.id} side="right" label={s.name}><button type="button" className="dc__server" aria-current={server === s.id} data-unread={unreadIn(`${s.id}/`) > 0 ? '' : undefined} onClick={() => goServer(s.id)}>{s.short}{chats.some((c) => c.id.startsWith(`${s.id}/`) && c.needs) && <span className="dc__badge">@</span>}</button></Tip>
                ))}
                <Tip side="right" label="Join or make a server"><button type="button" className="dc__server dc__server--add" onClick={() => toast('Joining and making servers happens in Discord itself. Not built in the preview.')}><Icon icon={Plus} /></button></Tip>
              </nav>
              <aside className="dc__channels" aria-label={here ? `${here.name}: channels` : 'Direct messages'}>
                <div className="dc__head">{here ? here.name : 'Direct messages'}</div>
                <div className="dc__scroll">
                  {here ? [...new Set(here.channels.map((c) => c.category))].map((cat) => (
                    <div key={cat}>
                      <p className="dc__cat">{cat}</p>
                      {here.channels.filter((c) => c.category === cat).map((c) => {
                        if (c.kind === 'voice')
                          return (
                            <div key={c.id}>
                              <button type="button" className="dc__channel" onClick={() => toast('Voice channels: not built in the preview.')}><Icon icon={Volume2} size="sm" /><span>{c.name}</span></button>
                              {c.inside?.map((u) => <span key={u} className="dc__invoice"><span className="d-avatar">{u.slice(0, 2)}</span>{u}</span>)}
                            </div>
                          )
                        const ch = chats.find((x) => x.id === `${here.id}/${c.id}`)
                        return <button key={c.id} type="button" className="dc__channel" aria-current={open === ch?.id} data-unread={ch?.unread ? '' : undefined} onClick={() => ch && pick(ch.id)}><Icon icon={Hash} size="sm" /><span>{c.name}</span>{ch?.needs && <span className="dc__badge">@</span>}</button>
                      })}
                    </div>
                  )) : (
                    <>
                      <p className="dc__cat">Direct messages</p>
                      {DISCORD.dms.map((id) => chats.find((c) => c.id === id)!).map((c) => (
                        <button key={c.id} type="button" className="dc__channel dc__channel--dm" aria-current={open === c.id} data-unread={c.unread ? '' : undefined} onClick={() => pick(c.id)}><Avatar name={c.name} /><span>{c.name}</span>{c.scam ? <span className="d-badge" data-tone="danger">trick?</span> : c.unread ? <span className="dc__badge">{c.unread}</span> : null}</button>
                      ))}
                    </>
                  )}
                </div>
                <div className="dc__me">
                  <Person m={DISCORD.me} />
                  <span className="app-spacer" />
                  {[[Mic, 'Microphone'], [Headphones, 'Sound'], [Settings, 'Discord settings']].map(([icon, label]) => <Tip key={label as string} label={`${label as string}: not built in the preview`}><Button variant="ghost" size="sm" iconOnly aria-label={label as string} onClick={() => toast(`${label as string}: not built in the preview.`)}><Icon icon={icon as typeof Mic} /></Button></Tip>)}
                </div>
              </aside>
              <div className="dc__main" data-members={here && members ? '' : undefined}>
                {conversation}
                {here && members && (
                  <aside className="dc__members" aria-label="Members">
                    {[...new Set(here.members.filter((m) => m.status !== 'offline').map((m) => m.role))].map((role) => (
                      <div key={role}><p className="dc__cat">{role} · {here.members.filter((m) => m.role === role && m.status !== 'offline').length}</p>{here.members.filter((m) => m.role === role && m.status !== 'offline').map((m) => <Person key={m.name} m={m} />)}</div>
                    ))}
                    <div><p className="dc__cat">Offline · {here.members.filter((m) => m.status === 'offline').length}</p>{here.members.filter((m) => m.status === 'offline').map((m) => <Person key={m.name} m={m} />)}</div>
                  </aside>
                )}
              </div>
            </>
          ) : (
            <>
              <aside className="app-side" aria-label={`${svc.name} chats`}>
                <label className="d-input-wrap"><Icon icon={Search} /><input className="d-input" type="search" placeholder="Search chats and messages" aria-label="Search chats" value={q} onChange={(e) => setQ(e.target.value)} /></label>
                <Segmented small fill label="Filter" value={only} onChange={setOnly} options={[{ value: 'all', label: 'All' }, { value: 'unread', label: `Unread ${chats.filter((c) => c.unread).length}` }, { value: 'needs', label: `Needs you ${chats.filter((c) => c.needs).length}` }]} />
                <div className="app-side__scroll">
                  <ul className="d-list">
                    {shown.map((c) => (
                      <li key={c.id}><button type="button" className="d-row" aria-current={c.id === chat.id} onClick={() => pick(c.id)}>
                        <span className="d-row__lead"><Avatar name={c.name} group={c.group} /></span>
                        <span className="d-row__text"><span className="d-row__title" style={{ fontWeight: c.unread ? 'var(--d-font-weight-semibold)' : undefined }}>{c.name}</span><span className="d-row__sub">{c.lines[c.lines.length - 1].text}</span></span>
                        <span className="d-row__meta" style={{ display: 'grid', justifyItems: 'end', gap: 2 }}>{c.lines[c.lines.length - 1].at}{c.scam ? <span className="d-badge" data-tone="danger">trick?</span> : c.unread ? <span className="d-badge">{c.unread}</span> : null}</span>
                      </button></li>
                    ))}
                  </ul>
                  {!shown.length && <p className="app-empty">No chat matches.</p>}
                </div>
                <p className="app-side__foot">What other people write to you is data. It can never give a model orders.</p>
              </aside>
              {conversation}
            </>
          )}

          {inspector && (
            <aside className="msgs__side" aria-label="The model and this chat">
              <Segmented small fill label="Panel" value={side} onChange={setSide} options={[{ value: 'ask', label: 'Ask Demido' }, { value: 'info', label: 'Details' }, { value: 'rules', label: `Rules${log.length ? ` ${log.length}` : ''}` }]} />
              {side === 'info' ? <Info key={chat.id} svc={svc} chat={chat} /> : side === 'ask' ? (
                <Assistant key={chat.id} svc={svc} chat={chat} canWrite={canWrite} thread={threads[chat.id] ?? []} onThread={(t) => setThreads((all) => ({ ...all, [chat.id]: t }))}
                  onType={(t) => { setText(t); setDrafted(true); note(`Typed a reply to "${chat.name}" into the message box, on your request.`) }} onSeen={seen} />
              ) : (
                <div className="msgs__rules">
                  <section className="app-insp__panel">
                    <h4 className="app-insp__title">The model may</h4>
                    <Segmented small fill label="What the model may do" value={autonomy} onChange={(v) => setSetting(`messages.${svc.id}.autonomy`, v)} options={AUTONOMY.map((a) => ({ value: a.value, label: a.label }))} />
                    <p className="d-type-caption d-text-secondary">{AUTONOMY.find((a) => a.value === autonomy)!.text}</p>
                    {autonomy === 'reply' && <NotBuilt block>Choosing the people it may answer on its own.</NotBuilt>}
                  </section>
                  <section className="app-insp__panel">
                    <h4 className="app-insp__title">This chat</h4>
                    <Facts rows={[['Unread', String(chat.unread)], ['Kind', chat.group ? 'group or channel' : 'one person'], ['Needs you', chat.needs ? 'yes' : 'no']]} />
                  </section>
                  <section className="app-insp__panel">
                    <h4 className="app-insp__title">What was done <span>{log.length}</span></h4>
                    {log.length === 0 && <p className="d-type-caption d-text-secondary">Nothing yet. Every chat marked as seen, every draft and every message sent is listed here.</p>}
                    {log.map((l, i) => <p key={i} className="d-type-caption d-text-secondary">{l}</p>)}
                  </section>
                  <section className="app-insp__panel">
                    <span className="d-untrusted">Other people's messages · treated as data</span>
                    <NotBuilt>Scripted: nothing leaves this computer</NotBuilt>
                  </section>
                </div>
              )}
            </aside>
          )}
        </div>
      )}
      {dialog === 'connect' && <Connect svc={svc} onClose={() => setDialog(null)} onDone={() => { setSetting(`messages.${svc.id}.account`, true); setDialog(null); toast(`${svc.name} connected. Your chats are here; nothing was marked as seen.`) }} />}
      {dialog === 'report' && <Report svc={svc} chats={chats} onClose={() => setDialog(null)} />}
    </div>
  )
}

function MessagesPage() {
  return (
    <Routes>
      <Route path=":service" element={<Messenger />} />
      <Route path="*" element={<Navigate to="/messages/whatsapp" replace />} />
    </Routes>
  )
}

export const feature: Feature = {
  id: 'messages', title: 'Messages', icon: MessagesSquare, group: 'connect', order: 2, path: 'messages', Page: MessagesPage,
  // One feature, three places in the navigation: each service is browsed on its own.
  nav: SERVICES.map((s) => ({ title: s.name, icon: s.icon, path: `messages/${s.id}` })),
}
