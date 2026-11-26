/* Email. Several accounts behind one switch at the top; folders and labels with icons; a search that
   filters and sorts; and what a mail program is expected to do: write, reply, reply to everyone, forward,
   file, label, star, mark, delete, one message or many at once. All mail here is made up, and nothing is
   sent anywhere. */
import { useMemo, useState } from 'react'
import { AlertOctagon, Archive, ArrowDownUp, CalendarDays, ChevronDown, Ellipsis, FolderInput, Forward, Mail, MailOpen, Paperclip, PenLine, Plus, Reply, ReplyAll, Search, Star, Tag, Trash2, UserRoundCheck, X } from 'lucide-react'
import type { LucideIcon } from 'lucide-react'
import type { Feature } from '@/app/registry'
import { useCore } from '@/data/core'
import { RealEmail } from './real'
import { setSetting, toast, useSetting } from '@/data/store'
import { Button, Dialog, Icon, Menu, NotBuilt, Segmented, Signal, Switch, Tip, cx } from '@/ui'
import { AskPanel } from '@/ui/ask'
import type { Answer, Ask } from '@/ui/ask'
import { Compose, REPLIES } from './compose'
import type { Draft } from './compose'
import { ACCOUNTS, FOLDERS, LABELS, MAIL, NOW, RANGES, SORTS, when } from './data'
import type { Account, Folder, Msg, Range, Sort } from './data'

const MOVE_TO: { box: Msg['box']; label: string }[] = [{ box: 'inbox', label: 'Inbox' }, { box: 'archive', label: 'Done' }, { box: 'spam', label: 'Spam' }, { box: 'trash', label: 'Bin' }]
const stamp = () => `2026-11-20T12:${String(NOW.getMinutes() + (Date.now() % 50)).padStart(2, '0')}`

function AddAccount({ onClose }: { onClose: () => void }) {
  const [kind, setKind] = useState<'google' | 'imap'>('google')
  return (
    <Dialog open wide onOpenChange={(o) => !o && onClose()} title="Add a mail account"
      actions={<><Button onClick={onClose}>Cancel</Button><Button variant="primary" onClick={() => { toast('Adding accounts: not built in the preview.'); onClose() }}>Check and add</Button></>}>
      <div className="app-stack" style={{ marginBlockStart: 'var(--d-space-16)' }}>
        <Segmented label="Account type" value={kind} onChange={setKind} options={[{ value: 'google', label: 'Google' }, { value: 'imap', label: 'Other (IMAP and SMTP)' }]} />
        <label className="d-field"><span className="d-field__label">Address</span><input className="d-input" type="email" placeholder="you@example.com" /></label>
        {kind === 'google' ? (
          <>
            <label className="d-field"><span className="d-field__label">App password</span><input className="d-input d-input--mono" type="password" placeholder="16 letters" /><span className="d-field__help">Made in your Google account under Security. Stored in your system keyring.</span></label>
            <Signal level="note">Demido uses an app password for Google, not "Sign in with Google". Full mail access through Google sign-in needs Google to verify the app first, which an open project may not be able to carry.</Signal>
          </>
        ) : (
          <>
            <label className="d-field"><span className="d-field__label">Incoming server (IMAP)</span><input className="d-input d-input--mono" placeholder="imap.example.com:993" /></label>
            <label className="d-field"><span className="d-field__label">Outgoing server (SMTP)</span><input className="d-input d-input--mono" placeholder="smtp.example.com:465" /></label>
            <label className="d-field"><span className="d-field__label">Password</span><input className="d-input d-input--mono" type="password" /></label>
            <p className="d-type-caption d-text-secondary">POP3 <NotBuilt>not planned for the first version</NotBuilt></p>
          </>
        )}
      </div>
    </Dialog>
  )
}

function PreviewEmail() {
  const [mail, setMail] = useState<Msg[]>(MAIL)
  const [account, setAccount] = useState<Account | 'all'>('all')
  const [folder, setFolder] = useState<Folder>('inbox')
  const [label, setLabel] = useState<string | null>(null)
  const [q, setQ] = useState('')
  const [unread, setUnread] = useState(false)
  const [files, setFiles] = useState(false)
  const [needs, setNeeds] = useState(false)
  const [range, setRange] = useState<Range>('any')
  const [from, setFrom] = useState('2026-11-16T00:00')
  const [to, setTo] = useState('2026-11-20T12:00')
  const [sort, setSort] = useState<Sort>('newest')
  const [open, setOpen] = useState('1')
  const [picked, setPicked] = useState<string[]>([])
  const [images, setImages] = useState<string[]>([])
  const [adding, setAdding] = useState(false)
  const [draft, setDraft] = useState<Draft | null>(null)
  // Asking the model about the open message happens beside it, not in another screen.
  const [asking, setAsking] = useState(false)
  const [threads, setThreads] = useState<Record<string, Ask[]>>({})
  const always = useSetting('email.images', false)

  const mine = useMemo(() => mail.filter((m) => account === 'all' || m.account === account), [mail, account])
  const inFolder = (m: Msg, f: Folder) => (f === 'needs' ? !!m.needs && m.box === 'inbox' : f === 'starred' ? !!m.star && m.box !== 'trash' : m.box === f)
  const needle = q.trim().toLowerCase()
  const span = range === 'custom' ? ([new Date(from), new Date(to)] as [Date, Date]) : RANGES.find((r) => r.id === range)!.span()
  const shown = mine
    .filter((m) => (label ? m.labels?.includes(label) && m.box !== 'trash' : inFolder(m, folder)))
    .filter((m) => !needle || `${m.from} ${m.addr} ${m.subject} ${m.body}`.toLowerCase().includes(needle))
    .filter((m) => (!unread || m.unread) && (!files || m.files) && (!needs || m.needs))
    .filter((m) => !span || (new Date(m.at) >= span[0] && new Date(m.at) < span[1]))
    .sort((a, b) => (sort === 'oldest' ? a.at.localeCompare(b.at) : sort === 'sender' ? a.from.localeCompare(b.from) || b.at.localeCompare(a.at) : sort === 'unread' ? Number(!!b.unread) - Number(!!a.unread) || b.at.localeCompare(a.at) : b.at.localeCompare(a.at)))
  const msg = shown.find((m) => m.id === open) ?? shown[0]
  const filtering = needle !== '' || unread || files || needs || range !== 'any'
  const clear = () => { setQ(''); setUnread(false); setFiles(false); setNeeds(false); setRange('any') }
  const current = account === 'all' ? null : ACCOUNTS.find((a) => a.id === account)!
  const title = label ?? FOLDERS.find((f) => f.id === folder)!.label
  const chosen = picked.filter((id) => shown.some((m) => m.id === id))

  // ───────── what you can do with mail ─────────
  const change = (ids: string[], fn: (m: Msg) => Msg) => setMail((all) => all.map((m) => (ids.includes(m.id) ? fn(m) : m)))
  const move = (ids: string[], box: Msg['box']) => {
    change(ids, (m) => ({ ...m, box, needs: box === 'inbox' ? m.needs : false }))
    setPicked([])
    toast(`${ids.length === 1 ? 'Message' : `${ids.length} messages`} moved to ${MOVE_TO.find((x) => x.box === box)?.label ?? box}.`, { label: 'Undo', run: () => setMail(mail) })
  }
  const bin = (ids: string[]) => {
    if (ids.every((id) => mail.find((m) => m.id === id)?.box === 'trash')) {
      setMail((all) => all.filter((m) => !ids.includes(m.id)))
      setPicked([])
      return toast(`${ids.length === 1 ? 'Message' : `${ids.length} messages`} deleted for good.`, { label: 'Undo', run: () => setMail(mail) })
    }
    move(ids, 'trash')
  }
  const read = (ids: string[], isRead: boolean) => { change(ids, (m) => ({ ...m, unread: !isRead })); setPicked([]) }
  const pick = (id: string) => {
    const m = mail.find((x) => x.id === id)
    if (m?.box === 'drafts') return setDraft({ id: m.id, mode: 'new', account: m.account, to: m.to ?? '', cc: m.cc ?? '', bcc: '', subject: m.subject, body: m.body, files: [] })
    setOpen(id)
    if (m?.unread) change([id], (x) => ({ ...x, unread: false }))
  }
  const quote = (m: Msg) => `\n\nOn ${when(m.at, true)}, ${m.from} wrote:\n${m.body.split('\n').map((l) => `> ${l}`).join('\n')}`
  const replyTo = (m: Msg, all: boolean): Draft => ({ mode: all ? 'replyAll' : 'reply', account: m.account, to: m.box === 'sent' ? (m.to ?? '') : m.addr, cc: all ? (m.cc ?? (m.account === 'work' ? 'team@studio.example' : '')) : '', bcc: '', subject: /^re:/i.test(m.subject) ? m.subject : `Re: ${m.subject}`, body: quote(m), files: [], about: m.id, name: m.from })
  const reply = (m: Msg, all: boolean) => setDraft(replyTo(m, all))
  const forward = (m: Msg) => setDraft({ mode: 'forward', account: m.account, to: '', cc: '', bcc: '', subject: /^fwd?:/i.test(m.subject) ? m.subject : `Fwd: ${m.subject}`, body: `\n\n---------- Forwarded message ----------\nFrom: ${m.from} <${m.addr}>\nDate: ${when(m.at, true)}\nSubject: ${m.subject}\n\n${m.body}`, files: Array.from({ length: m.files ?? 0 }, (_, i) => (m.files === 1 ? 'attachment.pdf' : `attachment-${i + 1}.pdf`)), about: m.id })
  const write = () => setDraft({ mode: 'new', account: account === 'all' ? 'personal' : account, to: '', cc: '', bcc: '', subject: '', body: '', files: [] })
  const asMsg = (d: Draft, box: 'sent' | 'drafts'): Msg => ({ id: d.id ?? `n${Date.now()}`, account: d.account, box, from: 'You', addr: ACCOUNTS.find((a) => a.id === d.account)!.address, to: d.to, cc: d.cc, subject: d.subject || '(no subject)', at: stamp(), body: d.body, files: d.files.length || undefined })
  const send = () => {
    if (!draft) return
    const before = mail
    const sent = asMsg({ ...draft, id: undefined }, 'sent')
    setMail((all) => [sent, ...all.filter((m) => m.id !== draft.id).map((m) => (m.id === draft.about ? { ...m, unread: false, needs: false } : m))])
    setDraft(null)
    toast(`Sent to ${draft.to}. (Preview: nothing left this computer.)`, { label: 'Undo', run: () => { setMail(before); setDraft(draft) } })
  }
  const close = () => {
    if (!draft) return
    const worth = draft.to.trim() || draft.subject.trim() || draft.body.trim()
    if (worth) {
      const saved = asMsg(draft, 'drafts')
      setMail((all) => (all.some((m) => m.id === saved.id) ? all.map((m) => (m.id === saved.id ? saved : m)) : [saved, ...all]))
      toast('Kept in Drafts.')
    }
    setDraft(null)
  }
  const discard = () => { if (draft?.id) setMail((all) => all.filter((m) => m.id !== draft.id)); setDraft(null); toast('Message thrown away.') }

  /** The scripted stand-in for a model that has this one message in front of it. */
  const answer = (q: string, m: Msg): Answer => {
    const s = q.toLowerCase()
    const first = m.body.replace(/\[IGNORE[^\]]*\]/g, '').split(/\n+/).map((l) => l.trim()).filter((l) => l && !/^(hi|hello|dear|kind regards|thanks|best)\b/i.test(l) && l.length > 12).slice(0, 2).join(' ')
    if (/summar|tl;?dr|in short|\bwhat\b.*\b(want|ask|say|said|need|about)|needs? an answer/.test(s))
      return { text: `${m.from} writes: “${first || m.subject}” ${m.needs ? 'It needs an answer from you.' : 'Nothing is asked of you.'}${m.files ? ` ${m.files === 1 ? 'One file is' : `${m.files} files are`} attached.` : ''}` }
    if (/\b(when|date|deadline|due|time|day)\b/.test(s)) {
      const found = [...new Set(m.body.match(/\b(monday|tuesday|wednesday|thursday|friday|saturday|sunday|today|tomorrow|\d{1,2}:\d{2}|\d{1,2} (january|february|march|april|may|june|july|august|september|october|november|december))\b/gi) ?? [])]
      return { text: found.length ? `It mentions: ${found.join(', ')}.` : 'It mentions no date and no time.' }
    }
    if (/\b(safe|scam|trust|phish|spam|real|fake)\b/.test(s))
      return { text: m.inject ? 'It carries a line written to give orders to a model. I read it as text and did nothing it says. Apart from that it is an ordinary newsletter.' : m.box === 'spam' ? 'I would not trust it. It promises a prize and wants you to act fast.' : 'Nothing looks wrong. The address fits the sender, and it asks for nothing unusual.' }
    if (/\b(reply|respond|write|draft|tell|say)\b/.test(s) || /^(please\s+)?answer\b/.test(s)) {
      const said = /\bthat\s+(.+)$/i.exec(q)
      const own = said ? said[1].trim().replace(/^./, (c) => c.toUpperCase()).replace(/([^.!?])$/, '$1.') : null
      const body = own ? `Hello ${m.from.split(' ')[0]},\n\n${own}\n\nKind regards` : (REPLIES[m.id] ?? `Hello ${m.from.split(' ')[0]},\n\nthank you for your message. I will come back to you shortly.\n\nKind regards`)
      return { text: 'I opened a reply and wrote it. Read it, change it, then press Send. I have sent nothing.', run: () => setDraft({ ...replyTo(m, false), body: body + quote(m) }) }
    }
    if (/\b(done|archive|file it|file this)\b/.test(s)) return { text: 'Filed as done. It is out of the inbox; you find it under Done.', run: () => move([m.id], 'archive') }
    return { text: 'About this message I can: say what it wants from you, list the dates in it, tell you whether it looks safe, write a reply for you to send, or file it as done.' }
  }

  const chip = (on: boolean, set: (v: boolean) => void, text: string, icon: LucideIcon) => (
    <button type="button" className="d-chip email__chip" aria-pressed={on} onClick={() => set(!on)}><Icon icon={icon} size="sm" />{text}</button>
  )
  const act = (label: string, icon: LucideIcon, run: () => void, on?: boolean) => (
    <Tip label={label} side="bottom"><Button variant="ghost" size="sm" iconOnly pressed={on} aria-label={label} onClick={run}><Icon icon={icon} /></Button></Tip>
  )

  return (
    <div className="app-split email" data-ask={asking && msg ? '' : undefined}>
      <aside className="app-side" aria-label="Accounts and folders">
        <Menu
          trigger={<button type="button" className="email__account"><span className="d-avatar">{current ? current.name.slice(0, 2) : 'All'}</span><span className="d-row__text"><span className="d-row__title">{current ? current.name : 'All accounts'}</span><span className="d-row__sub">{current ? `${current.address} · ${current.kind}` : `${ACCOUNTS.length} accounts together`}</span></span><Icon icon={ChevronDown} size="sm" /></button>}
          items={[
            { kind: 'label', label: 'Show mail from' },
            { label: 'All accounts', hint: String(mail.filter((m) => m.box === 'inbox' && m.unread).length), checked: account === 'all', onSelect: () => setAccount('all') },
            ...ACCOUNTS.map((a) => ({ label: a.name, hint: a.address, checked: account === a.id, onSelect: () => setAccount(a.id) })),
            { kind: 'sep' },
            { label: 'Add an account…', lead: <Icon icon={Plus} size="sm" />, onSelect: () => setAdding(true) },
          ]}
        />
        <Button variant="primary" onClick={write}><Icon icon={PenLine} />Write</Button>
        <div className="app-side__scroll">
          <ul className="d-list">
            {FOLDERS.map((f) => {
              const n = mine.filter((m) => inFolder(m, f.id) && (f.id === 'inbox' ? m.unread : f.id === 'needs' || f.id === 'drafts')).length
              return (
                <li key={f.id}><button type="button" className="d-row d-row--compact" aria-current={!label && folder === f.id} onClick={() => { setFolder(f.id); setLabel(null); setPicked([]) }}>
                  <span className="d-row__lead"><Icon icon={f.icon} /></span><span className="d-row__text"><span className="d-row__title">{f.label}</span></span><span className="d-row__meta">{n || ''}</span>
                </button></li>
              )
            })}
          </ul>
          <p className="d-list__label">Labels</p>
          <ul className="d-list">
            {LABELS.map((l) => (
              <li key={l}><button type="button" className="d-row d-row--compact" aria-current={label === l} onClick={() => { setLabel(l); setPicked([]) }}>
                <span className="d-row__lead"><Icon icon={Tag} /></span><span className="d-row__text"><span className="d-row__title">{l}</span></span><span className="d-row__meta">{mine.filter((m) => m.labels?.includes(l) && m.box !== 'trash').length || ''}</span>
              </button></li>
            ))}
          </ul>
        </div>
        <label className="d-check email__images"><Switch label="Always load images and styles" checked={always} onChange={(v) => setSetting('email.images', v)} /><span className="d-check__text">Always load images and styles<small>{always ? 'Senders can see when you open their mail.' : 'Off: Demido asks for each message.'}</small></span></label>
        <p className="app-side__foot">Models may read subjects and senders. Opening a body, and sending, follow the permission level.</p>
      </aside>

      <div className="app-side email__list" aria-label="Messages">
        <label className="d-input-wrap"><Icon icon={Search} /><input className="d-input" type="search" placeholder={`Search ${title.toLowerCase()}: sender, subject or text`} aria-label="Search mail" value={q} onChange={(e) => setQ(e.target.value)} /></label>
        <div className="email__filters">
          <Menu trigger={<button type="button" className="d-chip email__chip" aria-pressed={range !== 'any'}><Icon icon={CalendarDays} size="sm" />{range === 'custom' ? 'Between…' : RANGES.find((r) => r.id === range)!.label}</button>}
            items={[{ kind: 'label', label: 'Received' }, ...RANGES.map((r) => ({ label: r.label, checked: r.id === range, onSelect: () => setRange(r.id) }))]} />
          {chip(unread, setUnread, 'Unread', Mail)}
          {chip(files, setFiles, 'Has files', Paperclip)}
          {chip(needs, setNeeds, 'Needs you', UserRoundCheck)}
          <span className="app-spacer" />
          <Menu align="end" trigger={<button type="button" className="d-chip email__chip"><Icon icon={ArrowDownUp} size="sm" />{SORTS[sort]}</button>}
            items={[{ kind: 'label', label: 'Sort' }, ...(Object.keys(SORTS) as Sort[]).map((s) => ({ label: SORTS[s], checked: s === sort, onSelect: () => setSort(s) }))]} />
        </div>
        {range === 'custom' && (
          <div className="email__range">
            <label className="d-field"><span className="d-field__label">From</span><input className="d-input" type="datetime-local" value={from} max={to} onChange={(e) => setFrom(e.target.value)} /></label>
            <label className="d-field"><span className="d-field__label">Until</span><input className="d-input" type="datetime-local" value={to} min={from} onChange={(e) => setTo(e.target.value)} /></label>
          </div>
        )}
        {chosen.length > 0 ? (
          <div className="email__bulk">
            <input className="d-checkbox" type="checkbox" aria-label="Choose none" checked onChange={() => setPicked([])} />
            <span>{chosen.length} chosen</span>
            <span className="app-spacer" />
            {act('Done: out of the inbox', Archive, () => move(chosen, 'archive'))}
            {act('Move to the bin', Trash2, () => bin(chosen))}
            {act('Mark as read', MailOpen, () => read(chosen, true))}
            {act('Mark as unread', Mail, () => read(chosen, false))}
            <Menu align="end" trigger={<Button variant="ghost" size="sm" iconOnly aria-label="Move to"><Icon icon={FolderInput} /></Button>} items={[{ kind: 'label', label: 'Move to' }, ...MOVE_TO.map((x) => ({ label: x.label, onSelect: () => move(chosen, x.box) }))]} />
          </div>
        ) : (
          <div className="app-side__head">
            <label className="email__all"><input className="d-checkbox" type="checkbox" aria-label="Choose every message shown" checked={false} disabled={!shown.length} onChange={() => setPicked(shown.map((m) => m.id))} /><span className="d-type-headline">{title}</span></label>
            <span className="d-type-caption d-text-tertiary">{shown.length} {shown.length === 1 ? 'message' : 'messages'}{filtering && <> · <button type="button" className="app-link" onClick={clear}>clear filters</button></>}</span>
          </div>
        )}
        <div className="app-side__scroll">
          <ul className="d-list">
            {shown.map((m) => (
              <li key={m.id} className="email__row">
                <input className="d-checkbox" type="checkbox" aria-label={`Choose the message from ${m.from}`} checked={picked.includes(m.id)} onChange={() => setPicked(picked.includes(m.id) ? picked.filter((x) => x !== m.id) : [...picked, m.id])} />
                <button type="button" className="d-row" aria-current={m.id === msg?.id} onClick={() => pick(m.id)}>
                  <span className="d-row__text">
                    <span className="d-row__title" style={{ fontWeight: m.unread ? 'var(--d-font-weight-semibold)' : undefined }}>{m.box === 'sent' || m.box === 'drafts' ? `To: ${m.to || 'nobody yet'}` : m.from}{m.star && <Icon icon={Star} size="sm" label="Starred" />}{m.files ? <Icon icon={Paperclip} size="sm" label={`${m.files} files`} /> : null}</span>
                    <span className="d-row__sub" style={{ color: 'var(--d-color-text-secondary)' }}>{m.subject}</span>
                    {account === 'all' && <span className="d-row__sub">{ACCOUNTS.find((a) => a.id === m.account)!.name}</span>}
                  </span>
                  <span className="d-row__meta" style={{ display: 'grid', justifyItems: 'end', gap: 2 }}>{when(m.at)}{m.needs ? <span className="d-badge" data-tone="caution">needs you</span> : m.box === 'drafts' ? <span className="d-badge d-badge--outline">draft</span> : null}</span>
                </button>
              </li>
            ))}
          </ul>
          {!shown.length && <p className="app-empty">Nothing here matches.{filtering && <Button size="sm" onClick={clear}><Icon icon={X} size="sm" />Clear filters</Button>}</p>}
        </div>
      </div>

      <section className={cx('chat')} style={{ gridTemplateRows: 'auto auto minmax(0, 1fr)' }} aria-label="Message">
        {msg ? (
          <>
            <header className="chat__head">
              <span className="chat__title">{msg.subject}</span>
              {msg.labels?.map((l) => <span key={l} className="d-badge d-badge--outline"><Icon icon={Tag} size="sm" />{l}</span>)}
              <span className="app-spacer" />
              <Button size="sm" pressed={asking} className={asking ? 'email__asking' : undefined} onClick={() => setAsking(!asking)}>Ask Demido about this</Button>
            </header>
            <div className="email__actions" role="toolbar" aria-label="What to do with this message">
              <Button size="sm" onClick={() => reply(msg, false)}><Icon icon={Reply} size="sm" />Reply</Button>
              <Button size="sm" variant="ghost" onClick={() => reply(msg, true)}><Icon icon={ReplyAll} size="sm" />Reply to everyone</Button>
              <Button size="sm" variant="ghost" onClick={() => forward(msg)}><Icon icon={Forward} size="sm" />Forward</Button>
              <span className="browser__sep" />
              {msg.box !== 'archive' && act('Done: out of the inbox', Archive, () => move([msg.id], 'archive'))}
              {act(msg.box === 'trash' ? 'Delete for good' : 'Move to the bin', Trash2, () => bin([msg.id]))}
              {act(msg.unread ? 'Mark as read' : 'Mark as unread', msg.unread ? MailOpen : Mail, () => read([msg.id], !!msg.unread))}
              {act(msg.star ? 'Remove the star' : 'Star', Star, () => change([msg.id], (m) => ({ ...m, star: !m.star })), !!msg.star)}
              <Menu trigger={<Button variant="ghost" size="sm" iconOnly aria-label="Move to"><Icon icon={FolderInput} /></Button>} items={[{ kind: 'label', label: 'Move to' }, ...MOVE_TO.map((x) => ({ label: x.label, checked: msg.box === x.box, onSelect: () => move([msg.id], x.box) }))]} />
              <Menu trigger={<Button variant="ghost" size="sm" iconOnly aria-label="Labels"><Icon icon={Tag} /></Button>} items={[{ kind: 'label', label: 'Labels' }, ...LABELS.map((l) => ({ label: l, checked: !!msg.labels?.includes(l), onSelect: () => change([msg.id], (m) => ({ ...m, labels: m.labels?.includes(l) ? m.labels.filter((x) => x !== l) : [...(m.labels ?? []), l] })) }))]} />
              <Menu align="end" trigger={<Button variant="ghost" size="sm" iconOnly aria-label="More"><Icon icon={Ellipsis} /></Button>}
                items={[
                  { label: msg.needs ? 'It does not need me' : 'It needs me', onSelect: () => change([msg.id], (m) => ({ ...m, needs: !m.needs })) },
                  { label: 'Report as spam', lead: <Icon icon={AlertOctagon} size="sm" />, onSelect: () => move([msg.id], 'spam') },
                  { kind: 'sep' },
                  { label: 'Block the sender', onSelect: () => toast('Blocking senders: not built in the preview.') },
                  { label: 'Remind me later', onSelect: () => toast('Reminders: not built in the preview.') },
                  { label: 'Print', onSelect: () => toast('Printing: not built in the preview.') },
                  { label: 'Show the original', onSelect: () => toast('The raw message with its headers: not built in the preview.') },
                ]} />
            </div>
            <div className="app-page"><div className="app-page__inner app-page__inner--narrow" style={{ gap: 'var(--d-space-16)' }}>
              <div className="app-row"><span className="d-avatar">{msg.from.slice(0, 2)}</span><div className="app-stack" style={{ gap: 0 }}><span style={{ fontWeight: 'var(--d-font-weight-medium)' }}>{msg.from}</span><span className="d-type-caption d-text-tertiary d-mono">{msg.box === 'sent' ? `to ${msg.to}` : msg.addr} · {when(msg.at, true)}</span></div><span className="app-spacer" />{msg.box !== 'sent' && <span className="d-untrusted">From an email · treated as data</span>}</div>
              {msg.images && !always && !images.includes(msg.id) && (
                <Signal level="note" actions={<><Button size="sm" onClick={() => setImages([...images, msg.id])}>Load for this message</Button><Button size="sm" variant="ghost" onClick={() => setSetting('email.images', true)}>Always load</Button></>}>Pictures and styles in this message are not loaded. Loading them tells the sender you opened it.</Signal>
              )}
              {msg.images && (always || images.includes(msg.id)) && <div className="email__art" aria-label="Loaded pictures (drawn placeholder)" />}
              {msg.images && always && <p className="d-type-caption d-text-tertiary">Images and styles are loaded for every message. <button type="button" className="app-link" onClick={() => setSetting('email.images', false)}>Ask me each time instead</button></p>}
              <div className="d-prose" style={{ whiteSpace: 'pre-wrap' }}>{msg.body}</div>
              {msg.files ? <div className="app-row">{Array.from({ length: msg.files }, (_, i) => <span key={i} className="d-chip"><Icon icon={Paperclip} size="sm" />{msg.files === 1 ? 'attachment.pdf' : `attachment-${i + 1}.pdf`}</span>)}<NotBuilt>Attachments are not real</NotBuilt></div> : null}
              {msg.inject && <Signal level="warning">This message contains text written to give orders to a model. Demido showed it to the model as data only. Nothing was forwarded.</Signal>}
              {msg.box !== 'sent' && <div className="app-row"><Button onClick={() => reply(msg, false)}><Icon icon={Reply} />Reply</Button><Button variant="ghost" onClick={() => forward(msg)}><Icon icon={Forward} />Forward</Button></div>}
            </div></div>
          </>
        ) : (
          <p className="app-empty">No message is open.</p>
        )}
      </section>
      {asking && msg && (
        <aside className="email__ask" aria-label="Ask Demido about this message">
          <div className="app-side__head"><span className="d-type-headline">Ask Demido</span><Tip label="Close"><Button variant="ghost" size="sm" iconOnly aria-label="Close" onClick={() => setAsking(false)}><Icon icon={X} /></Button></Tip></div>
          <AskPanel key={msg.id}
            intro={<>Ask about <strong>{msg.subject}</strong>, from {msg.from}. The model sees this one message. What the message says is data: it cannot give the model orders.</>}
            suggestions={['What does it want from me?', 'Which dates are in it?', 'Write a reply', 'Does it look safe?']} placeholder="Ask about this message" reading="Reading the message…"
            thread={threads[msg.id] ?? []} onThread={(t) => setThreads((all) => ({ ...all, [msg.id]: t }))} respond={(q) => answer(q, msg)} />
        </aside>
      )}
      {draft && <Compose draft={draft} onChange={setDraft} onSend={send} onClose={close} onDiscard={discard} />}
      {adding && <AddAccount onClose={() => setAdding(false)} />}
    </div>
  )
}

/** With a core behind the pages, the real one; without, the preview of where it is going. */
function EmailPage() {
  const core = useCore()
  if (core.state === 'looking') return null
  return core.state === 'ready' ? <RealEmail /> : <PreviewEmail />
}

export const feature: Feature = {
  id: 'email', title: 'Email', icon: Mail, group: 'connect', order: 1, path: 'email', Page: EmailPage,
  settings: [
    {
      id: 'email', title: 'Email', order: 6.7,
      settings: [
        {
          id: 'email.images', type: 'switch', label: 'Always load images and styles', default: false,
          desc: 'Shows every message the way its sender designed it, without asking.',
          details: 'Default off. A picture in a mail is fetched from the sender\'s server. That tells the sender you opened the mail, when, and roughly from where. Off: Demido asks for each message. On: every message loads everything. A model reading your mail never loads them either way.',
        },
        {
          id: 'email.read_on_open', type: 'switch', label: 'Mark a message read when I open it', default: false,
          desc: 'Off: opening a message on the Email screen changes nothing on the server.',
          details: 'Default off. On: opening a message marks it read on the server, as other mail programs do, so it shows as read on your phone too. A model reading your mail never marks anything, either way.',
        },
      ],
    },
  ],
}
