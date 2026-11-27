/* The person's own WhatsApp, Telegram and Discord accounts, for real (DECISIONS G17 to G21; research note 37).

   Linking comes after a warning with the service's facts and a tick box (G2); the person types every code and
   password. Once linked, the chats are here, a chat's messages, and a box to write in: what is written is sent as
   the person, when they press Send. Nothing is marked as read at the service; the unread counts are Demido's own.
   What other people write is shown as data. */
import { useEffect, useMemo, useRef, useState } from 'react'
import { Hash, Link2, MessagesSquare, RefreshCw, Search, SendHorizontal, Trash2 } from 'lucide-react'
import { coreGet, coreSend, onCoreEvent } from '@/data/core'
import { toast } from '@/data/store'
import { Brief, Button, Dialog, Empty, Glyph, Icon, NotBuilt, Signal, Tip } from '@/ui'
import { Avatar, Lines } from './chat'
import type { Line as ShownLine, Service } from './data'

export interface Step { kind: 'code' | 'password' | 'qr' | 'pair_code'; hint: string | null; show: string | null; problem: string | null }
export interface LinkedAccount { id: string; service: string; name: string; handle: string; state: 'linking' | 'linked' | 'gone'; step: Step | null; running: boolean; problem: string | null; created_at: number }
interface LinkedChat { account: string; id: string; name: string; kind: 'direct' | 'group' | 'channel'; place: string | null; last_at: number | null; last_text: string; unread: number }
interface LinkedLine { chat: string; id: string; from_id: string; from_name: string; mine: boolean; text: string; at: number }

const base = (id: string) => `/linked/${encodeURIComponent(id)}`
const when = (ms: number | null) => {
  if (!ms) return ''
  const d = new Date(ms)
  return d.toDateString() === new Date().toDateString() ? d.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' }) : d.toLocaleDateString(undefined, { day: 'numeric', month: 'short' })
}
const LONGEST: Record<string, number> = { discord: 2000, telegram: 4096, whatsapp: 65536 }

const HOW: Record<string, { risk: string; steps: string[] }> = {
  discord: {
    risk: 'Discord forbids programs on a person\'s account ("self-bots") and can close the account for it. Demido says to Discord what it is; it does not pretend to be Discord\'s own app.',
    steps: ['In your browser, signed in at discord.com, open the developer tools (F12) and the Network tab.', 'Click any request to discord.com/api and copy the value of its Authorization header.', 'Paste it here. It is the whole account: Demido keeps it in the vault and never shows it again. Changing your password ends it.'],
  },
  telegram: {
    risk: 'Telegram allows other apps on your account if they keep its terms. Its terms also say what is read there may not be used to "deploy" artificial intelligence: whether your own model reading your chats for you is that is not plain. Demido trains nothing and never marks messages as read.',
    steps: ['At my.telegram.org, sign in with your number, open API development tools, and make an app: it gives an api_id and an api_hash.', 'Give them here with your number. Telegram sends a code to your Telegram app; then your password, if the account has one.'],
  },
  whatsapp: {
    risk: 'WhatsApp forbids unofficial apps, and can ban the number for it, for a time or for good. Demido links as one of your devices, through a helper program beside it.',
    steps: ['Tick the box below and press Show the QR code: Demido asks WhatsApp for one, and shows it here in a moment.', 'On your phone, in WhatsApp: Settings, Linked devices, Link a device, and scan it.', 'Or give your number first, to get a code to type on the phone instead ("Link with phone number instead").'],
  },
}

function LinkDialog({ svc, onClose }: { svc: Service; onClose: () => void }) {
  const [understood, setUnderstood] = useState(false)
  const [token, setToken] = useState('')
  const [apiId, setApiId] = useState('')
  const [apiHash, setApiHash] = useState('')
  const [phone, setPhone] = useState('')
  const [busy, setBusy] = useState(false)
  const [problem, setProblem] = useState<string | null>(null)
  const how = HOW[svc.id]
  const ready = understood && (svc.id === 'discord' ? !!token.trim() : svc.id === 'telegram' ? !!apiId.trim() && !!apiHash.trim() && !!phone.trim() : true)
  const link = async () => {
    setBusy(true); setProblem(null)
    try {
      const body = svc.id === 'discord' ? { service: 'discord', understood, token } : svc.id === 'telegram' ? { service: 'telegram', understood, api_id: Number(apiId), api_hash: apiHash, phone } : { service: 'whatsapp', understood, phone: phone.trim() || null }
      const a = await coreSend<LinkedAccount>('POST', '/linked', body)
      toast(a.state === 'linked' ? `${a.name} is linked.` : svc.id === 'telegram' ? 'Telegram sent a code to your Telegram app.' : 'Waiting for your phone.', undefined, 'task-done')
      onClose()
    } catch (e) { setProblem((e as Error).message) }
    setBusy(false)
  }
  return (
    <Dialog open wide onOpenChange={(o) => !o && onClose()} title={`Link your ${svc.name} account`}
      actions={<><Button onClick={onClose}>Cancel</Button><Button variant="primary" disabled={!ready || busy} onClick={() => void link()}>{busy ? `Asking ${svc.name}…` : svc.id === 'whatsapp' ? (phone.trim() ? 'Show the code' : 'Show the QR code') : 'Link it'}</Button></>}>
      <div className="app-stack" style={{ marginBlockStart: 'var(--d-space-16)' }}>
        <Signal level={svc.risk}>{how.risk}</Signal>
        <ol className="d-prose" style={{ paddingInlineStart: '1.25rem' }}>{how.steps.map((s) => <li key={s}>{s}</li>)}</ol>
        {svc.id === 'discord' && <label className="d-field"><span className="d-field__label">Your account's token</span><input className="d-input d-input--mono" type="password" autoComplete="off" autoFocus value={token} onChange={(e) => setToken(e.target.value)} /></label>}
        {svc.id === 'telegram' && (
          <div className="chr__pair">
            <label className="d-field"><span className="d-field__label">api_id</span><input className="d-input d-input--mono" inputMode="numeric" autoFocus value={apiId} onChange={(e) => setApiId(e.target.value.replace(/\D/g, ''))} /></label>
            <label className="d-field"><span className="d-field__label">api_hash</span><input className="d-input d-input--mono" type="password" autoComplete="off" value={apiHash} onChange={(e) => setApiHash(e.target.value)} /></label>
          </div>
        )}
        {svc.id !== 'discord' && <label className="d-field"><span className="d-field__label">{svc.id === 'telegram' ? 'Phone number, with its country code' : 'Phone number (only for a code instead of a QR code)'}</span><input className="d-input d-input--mono" type="tel" placeholder="+39 333 123 4567" value={phone} onChange={(e) => setPhone(e.target.value)} /></label>}
        <label className="d-check"><input className="d-checkbox" type="checkbox" checked={understood} onChange={(e) => setUnderstood(e.target.checked)} /><span className="d-check__text">I read this, and understand what it risks for my {svc.name} account</span></label>
        {!understood && <p className="d-type-caption d-text-tertiary">Tick the box to go on.</p>}
        {problem && <Signal level="caution">{problem}</Signal>}
      </div>
    </Dialog>
  )
}

function Linking({ account, svc }: { account: LinkedAccount; svc: Service }) {
  const [text, setText] = useState('')
  const [busy, setBusy] = useState(false)
  const step = account.step
  const answer = async () => {
    if (!step) return
    setBusy(true)
    try {
      await coreSend('POST', `${base(account.id)}/answer`, step.kind === 'password' ? { password: text } : { code: text.trim() })
      setText('')
    } catch (e) { toast((e as Error).message, undefined, 'task-failed') }
    setBusy(false)
  }
  return (
    <article className="d-card" style={{ maxWidth: '34rem' }}>
      <header className="d-card__head">
        <Glyph name={account.problem ? 'task-failed' : 'task-needs-input'} size="1.25rem" />
        <div className="app-stack" style={{ gap: 'var(--d-space-2)', flex: 1 }}><h3 className="d-card__title">Linking {account.handle || svc.name}</h3>
          <span className="d-card__sub">{account.problem ?? (step?.kind === 'qr' ? 'Scan the code with WhatsApp on your phone: Settings, Linked devices, Link a device.' : step?.kind === 'pair_code' ? 'On your phone: Linked devices, Link a device, Link with phone number instead, and type this code.' : step?.kind === 'password' ? 'The account has a password of its own (two-step verification).' : 'Telegram sent a code to your Telegram app.')}</span></div>
      </header>
      <div className="d-card__body app-stack">
        {step?.kind === 'qr' && (step.show ? <div style={{ background: '#fff', padding: 8, borderRadius: 8, width: 'fit-content' }} dangerouslySetInnerHTML={{ __html: step.show }} /> : <p className="app-empty"><Glyph name="task-running" busy /> Waiting for WhatsApp's code…</p>)}
        {step?.kind === 'pair_code' && (step.show ? <p className="d-mono" style={{ fontSize: '2rem', letterSpacing: '0.2em' }}>{step.show}</p> : <p className="app-empty"><Glyph name="task-running" busy /> Waiting for WhatsApp's code…</p>)}
        {(step?.kind === 'code' || step?.kind === 'password') && (
          <form className="app-row" onSubmit={(e) => { e.preventDefault(); void answer() }}>
            <input className="d-input d-input--mono" autoFocus type={step.kind === 'password' ? 'password' : 'text'} inputMode={step.kind === 'code' ? 'numeric' : undefined} autoComplete="off" placeholder={step.kind === 'password' ? (step.hint ? `Hint: ${step.hint}` : 'Password') : 'The code'} value={text} onChange={(e) => setText(e.target.value)} />
            <Button variant="primary" type="submit" disabled={busy || !text}>{busy ? 'Checking…' : 'Go on'}</Button>
          </form>
        )}
        {step?.problem && <Signal level="caution">{step.problem}</Signal>}
      </div>
    </article>
  )
}

/** The chats and the open one, as the service lays them out: a list and a chat, or Discord's servers and channels. */
function Chats({ account, svc, onRemove }: { account: LinkedAccount; svc: Service; onRemove: () => void }) {
  const [chats, setChats] = useState<LinkedChat[]>([])
  const [open, setOpen] = useState<string | null>(null)
  const [lines, setLines] = useState<LinkedLine[]>([])
  const [q, setQ] = useState('')
  const [text, setText] = useState('')
  const [busy, setBusy] = useState(false)
  const [server, setServer] = useState<string>('home')
  const end = useRef<HTMLDivElement>(null)
  const discord = svc.id === 'discord'
  const readChats = () => coreGet<LinkedChat[]>(`${base(account.id)}/chats`).then(setChats, () => {})
  const readLines = (chat: string, fresh: boolean) => coreGet<LinkedLine[]>(`${base(account.id)}/chats/${encodeURIComponent(chat)}/messages${fresh ? '?fresh=true' : ''}`).then(setLines, (e: Error) => toast(`Not read. ${e.message}`, undefined, 'task-failed'))
  useEffect(() => {
    void readChats()
    const offs = [onCoreEvent('linked.chats', (e) => { if (e.subject === account.id) void readChats() }), onCoreEvent('linked.message', (e) => { if (e.subject === account.id) void readChats() })]
    return () => offs.forEach((o) => o())
  }, [account.id])
  useEffect(() => {
    if (!open) return
    void readLines(open, false).then(() => readLines(open, true)).then(() => readChats())
    const off = onCoreEvent('linked.message', (e) => { if (e.subject === account.id && e.data.chat === open) void readLines(open, false) })
    return off
  }, [open])
  useEffect(() => { end.current?.scrollIntoView({ block: 'end' }) }, [lines.length, open])
  const places = useMemo(() => [...new Set(chats.filter((c) => c.place).map((c) => c.place!))], [chats])
  const shown = chats.filter((c) => (!discord || (server === 'home' ? !c.place : c.place === server)) && (!q || c.name.toLowerCase().includes(q.toLowerCase()) || c.last_text.toLowerCase().includes(q.toLowerCase())))
  const chat = chats.find((c) => c.id === open) ?? null
  const send = async () => {
    if (!open || !text.trim()) return
    setBusy(true)
    try {
      await coreSend('POST', `${base(account.id)}/chats/${encodeURIComponent(open)}/send`, { text })
      setText('')
      void readLines(open, false); void readChats()
    } catch (e) { toast(`Not sent. ${(e as Error).message}`, undefined, 'task-failed') }
    setBusy(false)
  }
  const shownLines: ShownLine[] = lines.map((l) => ({ who: l.mine ? 'me' : 'them', name: chat?.kind !== 'direct' || discord ? l.from_name : undefined, text: l.text, at: when(l.at) }))
  const list = (
    <ul className="d-list">
      {shown.map((c) => (
        <li key={c.id}><button type="button" className="d-row" aria-current={c.id === open} onClick={() => setOpen(c.id)}>
          <span className="d-row__lead">{c.kind === 'channel' && discord ? <Icon icon={Hash} /> : <Avatar name={c.name} group={c.kind === 'group'} />}</span>
          <span className="d-row__text"><span className="d-row__title" style={{ fontWeight: c.unread ? 'var(--d-font-weight-semibold)' : undefined }}>{c.name}</span><span className="d-row__sub">{c.last_text}</span></span>
          <span className="d-row__meta" style={{ display: 'grid', justifyItems: 'end', gap: 2 }}>{when(c.last_at)}{c.unread ? <span className="d-badge">{c.unread}</span> : null}</span>
        </button></li>
      ))}
      {!shown.length && <li className="app-empty">{chats.length ? 'No chat matches.' : account.running ? 'Reading your chats…' : `Not connected${account.problem ? `: ${account.problem}` : ''}.`}</li>}
    </ul>
  )
  return (
    <div className="msgs__body" data-layout={discord ? 'discord' : 'chat'}>
      {discord && (
        <nav className="dc__servers" aria-label="Servers">
          <Tip side="right" label="Direct messages"><button type="button" className="dc__server" aria-current={server === 'home'} onClick={() => setServer('home')}><Icon icon={MessagesSquare} /></button></Tip>
          {places.map((p) => <Tip key={p} side="right" label={p}><button type="button" className="dc__server" aria-current={server === p} data-unread={chats.some((c) => c.place === p && c.unread) ? '' : undefined} onClick={() => setServer(p)}>{p.slice(0, 2).toUpperCase()}</button></Tip>)}
        </nav>
      )}
      <aside className="app-side" aria-label={`${svc.name} chats`}>
        <div className="app-row"><span className="d-type-headline" style={{ flex: 1, minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis' }}>{discord && server !== 'home' ? server : account.name}</span>
          <Tip label="Remove the account from Demido"><Button size="sm" variant="ghost" iconOnly aria-label="Remove the account" onClick={onRemove}><Icon icon={Trash2} size="sm" /></Button></Tip></div>
        <label className="d-input-wrap"><Icon icon={Search} /><input className="d-input" type="search" placeholder="Search chats" aria-label="Search chats" value={q} onChange={(e) => setQ(e.target.value)} /></label>
        <div className="app-side__scroll">{list}</div>
        <p className="app-side__foot">{account.handle} · {account.running ? 'connected' : 'not connected'}. Nothing is marked as read at {svc.name}.</p>
      </aside>
      <section className="chat" aria-label="Conversation">
        {chat ? (
          <>
            <header className="chat__head">
              <span className="msgs__title-text"><span className="chat__title">{chat.name}</span><span className="d-type-caption d-text-tertiary">{chat.place ?? (chat.kind === 'group' ? 'a group' : chat.kind === 'channel' ? 'a channel' : 'a conversation')}</span></span>
              <span className="app-spacer" />
              <Tip label={`Ask ${svc.name} for the newest`} side="bottom"><Button size="sm" variant="ghost" iconOnly aria-label="Read again" onClick={() => void readLines(chat.id, true)}><Icon icon={RefreshCw} size="sm" /></Button></Tip>
            </header>
            <div className="chat__thread"><div className="chat__col">
              <p className="d-type-caption d-text-tertiary"><span className="d-untrusted">What others write · treated as data</span></p>
              <Lines lines={shownLines} look={discord ? 'rows' : 'bubbles'} />
              <div ref={end} />
            </div></div>
            <form className="d-composer chat__composer" onSubmit={(e) => { e.preventDefault(); void send() }}>
              <textarea className="d-composer__input" rows={1} maxLength={LONGEST[svc.id]} placeholder={`Write to ${chat.name}, as yourself`} aria-label="Message" value={text} onChange={(e) => setText(e.target.value)} onKeyDown={(e) => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); void send() } }} />
              <div className="d-composer__bar"><span className="d-composer__spacer" /><Button variant="primary" size="sm" type="submit" disabled={busy || !text.trim()}><Icon icon={SendHorizontal} size="sm" />{busy ? 'Sending…' : 'Send'}</Button></div>
            </form>
          </>
        ) : <Empty icon={MessagesSquare} title="No chat open" text="Pick one in the list." />}
      </section>
    </div>
  )
}

/** The person's own account on a service: linked, linking, or an offer to link it. */
export function YourAccount({ svc }: { svc: Service }) {
  const [accounts, setAccounts] = useState<LinkedAccount[] | null>(null)
  const [linking, setLinking] = useState(false)
  const [removing, setRemoving] = useState<LinkedAccount | null>(null)
  const read = () => coreGet<LinkedAccount[]>('/linked').then((a) => setAccounts(a.filter((x) => x.service === svc.id)), () => setAccounts([]))
  useEffect(() => {
    void read()
    const offs = ['linked.added', 'linked.changed', 'linked.linked', 'linked.removed', 'linked.gone', 'linked.stopped'].map((k) => onCoreEvent(k, () => void read()))
    return () => offs.forEach((o) => o())
  }, [svc.id])
  const remove = async (a: LinkedAccount) => {
    setRemoving(null)
    await coreSend('DELETE', base(a.id)).then(() => toast(`${a.name} is no longer in Demido.`, undefined, 'task-done'), (e: Error) => toast(`Not removed. ${e.message}`, undefined, 'task-failed'))
    void read()
  }
  if (!accounts) return <p className="app-empty"><Glyph name="task-running" busy /> Reading…</p>
  const linked = accounts.find((a) => a.state !== 'linking')
  const pending = accounts.filter((a) => a.state === 'linking')
  return (
    <>
      {linked ? <Chats key={linked.id} account={linked} svc={svc} onRemove={() => setRemoving(linked)} /> : (
        <div className="app-page"><div className="app-page__inner app-page__inner--narrow" style={{ gap: 'var(--d-space-16)' }}>
          {pending.map((a) => <div key={a.id} className="app-stack"><Linking account={a} svc={svc} /><div><Button size="sm" variant="ghost" onClick={() => setRemoving(a)}><Icon icon={Trash2} size="sm" />Stop linking</Button></div></div>)}
          {!pending.length && <Empty icon={Link2} title={`Your own ${svc.name} account`} text={`Your chats in Demido, and messages written here sent as you. ${svc.id === 'telegram' ? 'Telegram allows it.' : `${svc.name} forbids it: read the warning first.`}`} action={<Button variant="primary" onClick={() => setLinking(true)}><Icon icon={Link2} />Link your account</Button>} />}
          <Signal level={svc.risk}><Brief className="" text={HOW[svc.id].risk} /></Signal>
          <NotBuilt block>A model reading these chats or drafting replies (G3), marking as seen, pictures and files, voice messages, several accounts of one service shown at once{svc.id === 'whatsapp' ? ', older messages than the phone sent at linking' : ''}.</NotBuilt>
        </div></div>
      )}
      {linking && <LinkDialog svc={svc} onClose={() => { setLinking(false); void read() }} />}
      {removing && <Dialog open onOpenChange={(o) => !o && setRemoving(null)} title={`Remove ${removing.name} from Demido?`} text={svc.id === 'discord' ? 'Demido forgets its token, its chats and what was kept of them. Discord cannot be told to end a token from here: change your password to end it.' : `Demido signs out at ${svc.name}, then forgets the sign-in, the chats and what was kept of them. Nothing is deleted at ${svc.name}.`}
        actions={<><Button onClick={() => setRemoving(null)}>Cancel</Button><Button variant="danger" onClick={() => void remove(removing)}>Remove</Button></>} />}
    </>
  )
}
