/* Email, for real (DECISIONS Y): the person's own accounts, read by IMAP and sent from by SMTP.

   A message is shown as text, so none of its pictures is loaded and the sender cannot tell it was opened. Opening
   one marks nothing as read unless the person turned that on; what changes a mailbox (read, starred, moved,
   deleted) is done on the server, only when the person says so here (Y10, Y11). Deleting moves to the trash;
   from the trash it is for good, and the window asks first. A password is given once and never shown again. */
import { useEffect, useRef, useState, type ReactNode } from 'react'
import type { LucideIcon } from 'lucide-react'
import { AlertOctagon, Archive, ChevronDown, Copy, FileText, Folder as FolderIcon, FolderInput, Forward, Inbox, Mail, MailOpen, Mails, Paperclip, PenLine, Plus, RefreshCw, Reply, ReplyAll, Search, Send, Star, Trash2, X } from 'lucide-react'
import { coreGet, coreSend } from '@/data/core'
import { setSetting, toast, useSetting } from '@/data/store'
import { Button, ContextMenu, Dialog, Empty, Glyph, Icon, Menu, NotBuilt, Page, Segmented, Signal, Skeleton, Switch, Tip, type MenuEntry } from '@/ui'
import { asBase64, Compose, type Account, type Composing, type KeptDraft } from './real-write'

interface Summary { account: string; folder: string; uid: number; from: string; to: string; subject: string; date: number | null; seen: boolean; size: number; flagged: boolean }
interface Opened extends Summary { text: string; files: string[]; message_id: string | null; references: string[] }
interface Folder { name: string; role: string | null }

const when = (ms: number | null) => (ms ? new Date(ms).toLocaleString(undefined, { day: 'numeric', month: 'short', hour: '2-digit', minute: '2-digit' }) : '')
const ROLE: Record<string, { label: string; icon: LucideIcon }> = {
  inbox: { label: 'Inbox', icon: Inbox }, drafts: { label: 'Drafts', icon: FileText }, sent: { label: 'Sent', icon: Send }, archive: { label: 'Archive', icon: Archive },
  all: { label: 'All mail', icon: Mails }, flagged: { label: 'Starred', icon: Star }, junk: { label: 'Junk', icon: AlertOctagon }, trash: { label: 'Trash', icon: Trash2 },
}
const INBOX: Folder = { name: 'INBOX', role: 'inbox' }
/** Demido's own drafts, shown where a folder would be. */
const KEPT = '\u0000kept'
const labelOf = (f: Folder) => (f.role && ROLE[f.role] ? ROLE[f.role].label : f.name.replace(/^\[(Gmail|Google Mail)\]\//, ''))
const addressOf = (who: string) => (who.match(/<([^>]+)>/)?.[1] ?? who).trim()
const base = (account: string) => `/email/accounts/${encodeURIComponent(account)}`
const inFolder = (folder: string) => `?folder=${encodeURIComponent(folder)}`
const fileUrl = (m: Opened, n: number) => `/api/v1${base(m.account)}/messages/${m.uid}/files/${n}${inFolder(m.folder)}`
const quote = (m: Opened) => `\n\nOn ${when(m.date) || 'an earlier day'}, ${m.from} wrote:\n${m.text.split('\n').map((l) => `> ${l}`).join('\n')}`
const re = (subject: string, mark: 'Re' | 'Fwd') => (new RegExp(`^${mark}:`, 'i').test(subject.trim()) ? subject : `${mark}: ${subject}`)

function AddAccount({ onClose, onAdded }: { onClose: () => void; onAdded: (a: Account) => void }) {
  const [kind, setKind] = useState<'gmail' | 'other'>('gmail')
  const [address, setAddress] = useState('')
  const [password, setPassword] = useState('')
  const [imap, setImap] = useState('')
  const [smtp, setSmtp] = useState('')
  const [busy, setBusy] = useState(false)
  const [problem, setProblem] = useState<string | null>(null)
  const hostPort = (s: string, port: number): [string, number] => { const [h, p] = s.trim().split(':'); return [h ?? '', Number(p) || port] }
  const add = async () => {
    setBusy(true); setProblem(null)
    const [imapHost, imapPort] = hostPort(imap, 993)
    const [smtpHost, smtpPort] = hostPort(smtp, 465)
    try {
      const added = await coreSend<Account>('POST', '/email/accounts', kind === 'gmail'
        ? { address, preset: 'gmail', password }
        : { address, preset: 'other', password, imap_host: imapHost, imap_port: imapPort, smtp_host: smtpHost, smtp_port: smtpPort })
      toast(`Added: ${added.address}. Its password is kept in ${added.password?.kept_in === 'keyring' ? 'the system keyring' : 'a file only your user can read'}.`, undefined, 'task-done')
      onAdded(added)
    } catch (e) {
      setProblem((e as Error).message)
    }
    setBusy(false)
  }
  return (
    <Dialog open wide onOpenChange={(o) => !o && onClose()} title="Add a mail account" text="Demido signs in first, and keeps the account only if that works. The password is kept in the vault and never shown again."
      actions={<><Button onClick={onClose}>Cancel</Button><Button variant="primary" disabled={busy || !address.trim() || !password} onClick={() => void add()}>{busy ? 'Signing in…' : 'Check and add'}</Button></>}>
      <div className="app-stack" style={{ marginBlockStart: 'var(--d-space-16)' }}>
        <Segmented label="Account type" value={kind} onChange={setKind} options={[{ value: 'gmail', label: 'Gmail' }, { value: 'other', label: 'Other (IMAP and SMTP)' }]} />
        <label className="d-field"><span className="d-field__label">Address</span><input className="d-input" type="email" autoFocus placeholder="you@example.com" value={address} onChange={(e) => setAddress(e.target.value)} /></label>
        {kind === 'gmail' ? (
          <>
            <label className="d-field"><span className="d-field__label">App password</span><input className="d-input d-input--mono" type="password" autoComplete="off" placeholder="16 letters, as Google shows them" value={password} onChange={(e) => setPassword(e.target.value)} />
              <span className="d-field__help">Made in your Google account under Security, with 2-Step Verification on (<a href="https://myaccount.google.com/apppasswords" target="_blank" rel="noreferrer noopener">myaccount.google.com/apppasswords</a>). Not your Google password.</span></label>
            <Signal level="note">An app password does not work for a work or school account, an account with only security keys, or Advanced Protection; Google discourages app passwords. Demido does not offer "Sign in with Google": that needs Google to verify the app, which costs money every year.</Signal>
          </>
        ) : (
          <>
            <label className="d-field"><span className="d-field__label">Incoming server (IMAP, over TLS)</span><input className="d-input d-input--mono" placeholder="imap.example.com:993" value={imap} onChange={(e) => setImap(e.target.value)} /></label>
            <label className="d-field"><span className="d-field__label">Outgoing server (SMTP, over TLS)</span><input className="d-input d-input--mono" placeholder="smtp.example.com:465 (or :587)" value={smtp} onChange={(e) => setSmtp(e.target.value)} /></label>
            <label className="d-field"><span className="d-field__label">Password</span><input className="d-input d-input--mono" type="password" autoComplete="off" value={password} onChange={(e) => setPassword(e.target.value)} /></label>
          </>
        )}
        {problem && <Signal level="caution">{problem}</Signal>}
      </div>
    </Dialog>
  )
}

export function RealEmail() {
  const [accounts, setAccounts] = useState<Account[] | null>(null)
  const [current, setCurrent] = useState<string | null>(null)
  const [folders, setFolders] = useState<Folder[]>([INBOX])
  const [foldersProblem, setFoldersProblem] = useState<string | null>(null)
  const [place, setPlace] = useState<string>(INBOX.name)
  const [list, setList] = useState<Summary[]>([])
  const [found, setFound] = useState<Summary[] | null>(null)
  const [q, setQ] = useState('')
  const [searching, setSearching] = useState(false)
  const [unread, setUnread] = useState(false)
  const [starred, setStarred] = useState(false)
  const [open, setOpen] = useState<Opened | null>(null)
  const [reading, setReading] = useState(false)
  const [kept, setKept] = useState<KeptDraft[]>([])
  const [composing, setComposing] = useState<Composing | null>(null)
  const [forGood, setForGood] = useState<Summary | null>(null)
  const [forwarding, setForwarding] = useState(false)
  const [adding, setAdding] = useState(false)
  const [removing, setRemoving] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const readOnOpen = useSetting('email.read_on_open', false)
  const account = accounts?.find((a) => a.id === current) ?? accounts?.[0] ?? null
  /** Where the list is: an answer for another place, come late, is not shown. */
  const where = useRef('')
  const folder = folders.find((f) => f.name === place) ?? (place === KEPT ? null : { name: place, role: null })
  const trash = folders.find((f) => f.role === 'trash') ?? null
  const archive = folders.find((f) => f.role === 'archive') ?? folders.find((f) => f.role === 'all') ?? null

  const readAccounts = () => coreGet<Account[]>('/email/accounts').then((a) => { setAccounts(a); setError(null) }, (e: Error) => setError(e.message))
  const readKept = (id: string) => coreGet<KeptDraft[]>(`${base(id)}/drafts`).then(setKept, () => setKept([]))
  const readList = async (fresh: boolean) => {
    if (!account || place === KEPT) return
    const here = `${account.id}|${place}`
    setReading(true)
    try {
      const got = await coreGet<Summary[]>(`${base(account.id)}/messages${inFolder(place)}${fresh ? '&fresh=true' : ''}`)
      if (where.current === here) { setList(got); setError(null) }
    } catch (e) {
      if (where.current === here) setError((e as Error).message)
    }
    if (where.current === here) setReading(false)
  }
  useEffect(() => { void readAccounts() }, [])
  useEffect(() => {
    setFolders([INBOX]); setPlace(INBOX.name); setFoldersProblem(null); setKept([])
    if (!account) return
    const id = account.id
    void coreGet<Folder[]>(`${base(id)}/folders`).then((f) => setFolders(f.length ? f : [INBOX]), (e: Error) => setFoldersProblem(e.message))
    void readKept(id)
  }, [account?.id])
  useEffect(() => {
    where.current = `${account?.id}|${place}`
    setOpen(null); setList([]); setFound(null); setQ(''); setReading(false)
    // What was kept first, at once; then what is new on the server.
    if (account && place !== KEPT) void readList(false).then(() => readList(true))
  }, [account?.id, place])

  /** One message as it is now, in the list, the search and the open message. */
  const change = (m: Summary, patch: Partial<Summary> | null) => {
    const same = (x: Summary) => x.uid === m.uid && x.folder === m.folder && x.account === m.account
    const apply = (all: Summary[]) => (patch ? all.map((x) => (same(x) ? { ...x, ...patch } : x)) : all.filter((x) => !same(x)))
    setList(apply)
    setFound((f) => (f ? apply(f) : f))
    setOpen((o) => (o && same(o) ? (patch ? { ...o, ...patch } : null) : o))
  }
  const mark = async (m: Summary, flags: { seen?: boolean; flagged?: boolean }) => {
    change(m, flags)
    try {
      await coreSend('POST', `${base(m.account)}/messages/${m.uid}/flags${inFolder(m.folder)}`, flags)
    } catch (e) {
      change(m, { seen: m.seen, flagged: m.flagged })
      toast(`Not changed on the server. ${(e as Error).message}`, undefined, 'task-failed')
    }
  }
  const pick = async (s: Summary) => {
    try {
      const opened = await coreGet<Opened>(`${base(s.account)}/messages/${s.uid}${inFolder(s.folder)}`)
      setOpen(opened)
      if (readOnOpen && !s.seen) void mark(s, { seen: true })
    } catch (e) {
      toast(`Not opened. ${(e as Error).message}`, undefined, 'task-failed')
    }
  }
  const moveTo = async (m: Summary, to: Folder) => {
    try {
      await coreSend('POST', `${base(m.account)}/messages/${m.uid}/move${inFolder(m.folder)}`, { to: to.name })
      change(m, null)
      toast(`Moved to ${labelOf(to)}.`, undefined, 'task-done')
    } catch (e) {
      toast(`Not moved. ${(e as Error).message}`, undefined, 'task-failed')
    }
  }
  /** Into the trash; from the trash (or where there is none), for good, once the person says so. */
  const remove = async (m: Summary, sure = false) => {
    const final = !trash || trash.name === m.folder
    if (final && !sure) { setForGood(m); return }
    try {
      const { moved_to } = await coreSend<{ moved_to: string | null }>('DELETE', `${base(m.account)}/messages/${m.uid}${inFolder(m.folder)}`)
      change(m, null)
      toast(moved_to ? `Moved to ${labelOf(folders.find((f) => f.name === moved_to) ?? { name: moved_to, role: 'trash' })}.` : 'Deleted for good.', undefined, 'task-done')
    } catch (e) {
      toast(`Not deleted. ${(e as Error).message}`, undefined, 'task-failed')
    }
  }
  const search = async () => {
    if (!account || place === KEPT) return
    const words = q.trim()
    if (!words) { setFound(null); return }
    const here = where.current
    setSearching(true)
    try {
      const got = await coreGet<Summary[]>(`${base(account.id)}/search${inFolder(place)}&q=${encodeURIComponent(words)}`)
      if (where.current === here) setFound(got)
    } catch (e) {
      toast(`Not searched. ${(e as Error).message}`, undefined, 'task-failed')
    }
    setSearching(false)
  }
  const write = (patch: Partial<Composing>) => {
    if (!account) return
    setComposing({ draft: null, mode: 'new', account: account.id, to: '', subject: '', text: '', in_reply_to: null, references: [], files: [], ...patch })
  }
  const thread = (m: Opened) => ({ in_reply_to: m.message_id, references: m.message_id ? [...m.references.filter((r) => r !== m.message_id), m.message_id] : m.references })
  const reply = (m: Opened, everyone: boolean) => {
    const mine = account?.address.toLowerCase()
    const people = everyone ? [m.from, ...m.to.split(',')] : [m.from]
    const to = [...new Map(people.map((p) => addressOf(p)).filter((a) => a && a.toLowerCase() !== mine).map((a) => [a.toLowerCase(), a])).values()]
    write({ mode: everyone ? 'replyAll' : 'reply', to: to.join(', '), subject: re(m.subject, 'Re'), text: quote(m), ...thread(m) })
  }
  const forward = async (m: Opened) => {
    const text = `\n\n---------- Forwarded message ----------\nFrom: ${m.from}\nDate: ${when(m.date)}\nSubject: ${m.subject}\nTo: ${m.to}\n\n${m.text}`
    setForwarding(true)
    try {
      const files = await Promise.all(m.files.map(async (name, n) => {
        const res = await fetch(fileUrl(m, n))
        if (!res.ok) throw new Error(`${name} could not be fetched (${res.status}).`)
        const blob = await res.blob()
        return { name, content_type: blob.type || null, base64: await asBase64(blob), size: blob.size }
      }))
      write({ mode: 'forward', subject: re(m.subject, 'Fwd'), text, files })
    } catch (e) {
      toast(`Forwarded without its files. ${(e as Error).message}`, undefined, 'task-failed')
      write({ mode: 'forward', subject: re(m.subject, 'Fwd'), text })
    }
    setForwarding(false)
  }
  const resume = (d: KeptDraft) => account && setComposing({ draft: d.id, mode: d.in_reply_to ? 'reply' : 'new', account: account.id, to: d.to, subject: d.subject, text: d.text, in_reply_to: d.in_reply_to, references: d.references, files: [] })
  const removeAccount = async () => {
    setRemoving(false)
    if (!account) return
    await coreSend('DELETE', base(account.id)).then(() => toast(`${account.address} is no longer in Demido. Its mailbox is as it was.`, undefined, 'task-done'), (e: Error) => toast(`Not removed. ${e.message}`, undefined, 'task-failed'))
    setCurrent(null)
    void readAccounts()
  }
  const added = (a: Account) => { setAdding(false); setCurrent(a.id); void readAccounts() }

  if (error && !accounts) return <Page title="Email"><Signal level="caution">The core did not say. {error}</Signal></Page>
  if (!accounts) return <Page title="Email"><p className="app-empty"><Glyph name="task-running" busy /> Reading your accounts…</p></Page>
  if (accounts.length === 0 || !account) {
    return (
      <Page title="Email" lead="Your own mail accounts, read and sent from by Demido."
        about={<><p>A message is shown as text, so its pictures are never loaded. Opening one marks nothing as read unless you turn that on. Sending asks you first and cannot be undone.</p><p>Models can list your inbox, read a message and write one when Settings › Email allows it.</p></>}>
        <Empty icon={Mail} title="No mail account yet" text="Your mail, read as text: folders, search, stars, files and replies in their thread. Sending always asks you first." action={<Button variant="primary" onClick={() => setAdding(true)}><Icon icon={Plus} />Add a mail account</Button>} />
        <NotBuilt block>Cc and Bcc, labels, choosing several messages at once, drafts kept in the server's Drafts folder, a model drafting the message, and "Ask Demido about this" here (ask in a session instead).</NotBuilt>
        {adding && <AddAccount onClose={() => setAdding(false)} onAdded={added} />}
      </Page>
    )
  }

  const shown = (found ?? list).filter((m) => (!unread || !m.seen) && (!starred || m.flagged))
  const title = place === KEPT ? 'Drafts kept here' : folder ? labelOf(folder) : place
  const others = (m: Summary) => folders.filter((f) => f.name !== m.folder)
  const moveItems = (m: Summary): MenuEntry[] => [{ kind: 'label', label: 'Move to' }, ...others(m).map((f) => ({ label: labelOf(f), lead: <Icon icon={f.role ? ROLE[f.role]?.icon ?? FolderIcon : FolderIcon} size="sm" />, onSelect: () => void moveTo(m, f) }))]
  const finalHere = (m: Summary) => !trash || trash.name === m.folder
  const rowMenu = (m: Summary): MenuEntry[] => [
    { label: 'Open', lead: <Icon icon={MailOpen} size="sm" />, onSelect: () => void pick(m) },
    { label: m.seen ? 'Mark as unread' : 'Mark as read', lead: <Icon icon={m.seen ? Mail : MailOpen} size="sm" />, onSelect: () => void mark(m, { seen: !m.seen }) },
    { label: m.flagged ? 'Remove the star' : 'Star', lead: <Icon icon={Star} size="sm" />, onSelect: () => void mark(m, { flagged: !m.flagged }) },
    ...(archive && archive.name !== m.folder ? [{ label: `Move to ${labelOf(archive)}`, lead: <Icon icon={Archive} size="sm" />, onSelect: () => void moveTo(m, archive) }] : []),
    { kind: 'sep' },
    { label: 'Copy the sender', lead: <Icon icon={Copy} size="sm" />, onSelect: () => void navigator.clipboard.writeText(addressOf(m.from)).then(() => toast('Copied.')) },
    { label: 'Copy the subject', lead: <Icon icon={Copy} size="sm" />, onSelect: () => void navigator.clipboard.writeText(m.subject).then(() => toast('Copied.')) },
    { kind: 'sep' },
    { label: finalHere(m) ? 'Delete for good…' : 'Move to the trash', lead: <Icon icon={Trash2} size="sm" />, danger: true, onSelect: () => void remove(m) },
  ]
  const act = (label: string, icon: LucideIcon, run: () => void, on?: boolean): ReactNode => (
    <Tip label={label} side="bottom"><Button variant="ghost" size="sm" iconOnly pressed={on} aria-label={label} onClick={run}><Icon icon={icon} /></Button></Tip>
  )
  const chip = (on: boolean, set: (v: boolean) => void, text: string, icon: LucideIcon) => (
    <button type="button" className="d-chip email__chip" aria-pressed={on} onClick={() => set(!on)}><Icon icon={icon} size="sm" />{text}</button>
  )
  const sent = folder?.role === 'sent' || folder?.role === 'drafts'

  return (
    <div className="app-split email">
      <aside className="app-side" aria-label="Accounts and folders">
        <Menu
          trigger={<button type="button" className="email__account"><span className="d-avatar">{account.label.slice(0, 2)}</span><span className="d-row__text"><span className="d-row__title">{account.label}</span><span className="d-row__sub">{account.address} · {account.preset === 'gmail' ? 'Gmail' : account.imap_host}</span></span><Icon icon={ChevronDown} size="sm" /></button>}
          items={[
            { kind: 'label', label: 'Show mail from' },
            ...accounts.map((a, i) => ({ label: a.label, hint: i === 0 ? 'models use this one' : a.address, checked: a.id === account.id, onSelect: () => setCurrent(a.id) })),
            { kind: 'sep' },
            { label: 'Add an account…', lead: <Icon icon={Plus} size="sm" />, onSelect: () => setAdding(true) },
            { label: `Remove ${account.label}…`, lead: <Icon icon={Trash2} size="sm" />, onSelect: () => setRemoving(true) },
          ]}
        />
        <Button variant="primary" onClick={() => write({})}><Icon icon={PenLine} />Write</Button>
        <div className="app-side__scroll">
          <ul className="d-list">
            {folders.map((f) => (
              <li key={f.name}><button type="button" className="d-row d-row--compact" aria-current={place === f.name} onClick={() => setPlace(f.name)} title={f.name}>
                <span className="d-row__lead"><Icon icon={f.role ? ROLE[f.role]?.icon ?? FolderIcon : FolderIcon} /></span><span className="d-row__text"><span className="d-row__title">{labelOf(f)}</span></span>
                <span className="d-row__meta">{place === f.name && f.role === 'inbox' ? list.filter((m) => !m.seen).length || '' : ''}</span>
              </button></li>
            ))}
          </ul>
          <p className="d-list__label">In Demido</p>
          <ul className="d-list">
            <li><button type="button" className="d-row d-row--compact" aria-current={place === KEPT} onClick={() => { setPlace(KEPT); void readKept(account.id) }}>
              <span className="d-row__lead"><Icon icon={PenLine} /></span><span className="d-row__text"><span className="d-row__title">Drafts kept here</span></span><span className="d-row__meta">{kept.length || ''}</span>
            </button></li>
          </ul>
          {foldersProblem && <p className="d-type-caption d-text-tertiary" style={{ paddingInline: 'var(--d-space-8)' }}>Only the inbox: the folders were not read. {foldersProblem}</p>}
        </div>
        <label className="d-check email__images"><Switch label="Mark a message read when I open it" checked={readOnOpen} onChange={(v) => setSetting('email.read_on_open', v)} /><span className="d-check__text">Mark read when I open one<small>{readOnOpen ? 'Opening a message marks it read on the server.' : 'Off: opening changes nothing on the server.'}</small></span></label>
        <p className="app-side__foot">Pictures in mail are never loaded. What you change here is changed on the server.</p>
      </aside>

      <div className="app-side email__list" aria-label={place === KEPT ? 'Drafts' : 'Messages'}>
        {place === KEPT ? (
          <>
            <div className="app-side__head"><span className="d-type-headline">{title}</span><span className="d-type-caption d-text-tertiary">{kept.length} {kept.length === 1 ? 'draft' : 'drafts'}</span></div>
            <div className="app-side__scroll">
              <ul className="d-list">
                {kept.map((d) => (
                  <li key={d.id}>
                    <ContextMenu items={[
                      { label: 'Go on writing', lead: <Icon icon={PenLine} size="sm" />, onSelect: () => resume(d) },
                      { kind: 'sep' },
                      { label: 'Throw it away', lead: <Icon icon={Trash2} size="sm" />, danger: true, onSelect: () => void coreSend('DELETE', `${base(account.id)}/drafts/${encodeURIComponent(d.id)}`).then(() => readKept(account.id)) },
                    ]}>
                      <button type="button" className="d-row" onClick={() => resume(d)}>
                        <span className="d-row__text"><span className="d-row__title">To: {d.to || 'nobody yet'}</span><span className="d-row__sub" style={{ color: 'var(--d-color-text-secondary)' }}>{d.subject || '(no subject)'}</span></span>
                        <span className="d-row__meta" style={{ display: 'grid', justifyItems: 'end', gap: 2 }}>{when(d.updated_at)}<span className="d-badge d-badge--outline">draft</span></span>
                      </button>
                    </ContextMenu>
                  </li>
                ))}
              </ul>
              {!kept.length && <Empty icon={PenLine} title="No drafts" text="Close a message you are writing and it is kept here, without its files." />}
            </div>
          </>
        ) : (
          <>
            <form className="d-input-wrap" role="search" onSubmit={(e) => { e.preventDefault(); void search() }}>
              <Icon icon={Search} />
              <input className="d-input" type="search" placeholder={`Search ${title.toLowerCase()} on the server`} aria-label="Search mail" value={q}
                onChange={(e) => { setQ(e.target.value); if (!e.target.value) setFound(null) }} onKeyDown={(e) => { if (e.key === 'Escape' && q) { e.preventDefault(); setQ(''); setFound(null) } }} />
            </form>
            <div className="email__filters">
              {chip(unread, setUnread, 'Unread', Mail)}
              {chip(starred, setStarred, 'Starred', Star)}
              <span className="app-spacer" />
              <Tip label="Ask the server for what is new"><Button size="sm" variant="ghost" disabled={reading} onClick={() => void readList(true)}><Icon icon={RefreshCw} size="sm" />{reading ? 'Reading…' : 'Read again'}</Button></Tip>
            </div>
            <div className="app-side__head">
              <span className="d-type-headline">{found ? `Found in ${title}` : title}</span>
              <span className="d-type-caption d-text-tertiary">{searching ? 'Searching…' : found ? <>{shown.length} found · <button type="button" className="app-link" onClick={() => { setQ(''); setFound(null) }}>show all</button></> : `${shown.length} newest`}</span>
            </div>
            {error && <Signal level="caution">{error}</Signal>}
            <div className="app-side__scroll">
              <ul className="d-list">
                {shown.map((m) => (
                  <li key={`${m.account}-${m.folder}-${m.uid}`}>
                    <ContextMenu items={rowMenu(m)}>
                      <button type="button" className="d-row" aria-current={open?.uid === m.uid && open.folder === m.folder} onClick={() => void pick(m)}>
                        <span className="d-row__text">
                          <span className="d-row__title" style={{ fontWeight: m.seen ? undefined : 'var(--d-font-weight-semibold)' }}>{sent ? `To: ${m.to || 'nobody'}` : m.from || '(no sender)'}{m.flagged && <Icon icon={Star} size="sm" label="Starred" />}</span>
                          <span className="d-row__sub" style={{ color: 'var(--d-color-text-secondary)' }}>{m.subject || '(no subject)'}</span>
                        </span>
                        <span className="d-row__meta">{when(m.date)}</span>
                      </button>
                    </ContextMenu>
                  </li>
                ))}
              </ul>
              {!shown.length && !reading && !searching && (found || unread || starred
                ? <p className="app-empty">Nothing here matches.</p>
                : <Empty icon={Inbox} title="Nothing read yet" action={<Button size="sm" onClick={() => void readList(true)}>Read {title.toLowerCase()}</Button>} />)}
              {(reading || searching) && !shown.length && <Skeleton rows={6} />}
            </div>
          </>
        )}
      </div>

      <section className="chat" style={{ gridTemplateRows: open ? 'auto auto minmax(0, 1fr)' : 'minmax(0, 1fr)' }} aria-label="Message">
        {open ? (
          <>
            <header className="chat__head"><span className="chat__title">{open.subject || '(no subject)'}</span><span className="app-spacer" /><Tip label="Close the message"><Button size="sm" variant="ghost" iconOnly aria-label="Close the message" onClick={() => setOpen(null)}><Icon icon={X} /></Button></Tip></header>
            <div className="email__actions" role="toolbar" aria-label="What to do with this message">
              <Button size="sm" onClick={() => reply(open, false)}><Icon icon={Reply} size="sm" />Reply</Button>
              <Button size="sm" variant="ghost" onClick={() => reply(open, true)}><Icon icon={ReplyAll} size="sm" />Reply to everyone</Button>
              <Button size="sm" variant="ghost" disabled={forwarding} onClick={() => void forward(open)}><Icon icon={Forward} size="sm" />{forwarding ? 'Fetching its files…' : 'Forward'}</Button>
              <span className="browser__sep" />
              {archive && archive.name !== open.folder && act(`Move to ${labelOf(archive)}`, Archive, () => void moveTo(open, archive))}
              {act(finalHere(open) ? 'Delete for good…' : 'Move to the trash', Trash2, () => void remove(open))}
              {act(open.seen ? 'Mark as unread' : 'Mark as read', open.seen ? Mail : MailOpen, () => void mark(open, { seen: !open.seen }))}
              {act(open.flagged ? 'Remove the star' : 'Star', Star, () => void mark(open, { flagged: !open.flagged }), open.flagged)}
              <Menu trigger={<Button variant="ghost" size="sm" iconOnly aria-label="Move to"><Icon icon={FolderInput} /></Button>} items={moveItems(open)} />
            </div>
            <div className="app-page"><div className="app-page__inner app-page__inner--narrow" style={{ gap: 'var(--d-space-16)' }}>
              <div className="app-row"><span className="d-avatar">{open.from.slice(0, 2)}</span><div className="app-stack" style={{ gap: 0, minWidth: 0 }}><span style={{ fontWeight: 'var(--d-font-weight-medium)' }}>{open.from}</span><span className="d-type-caption d-text-tertiary d-mono">to {open.to} · {when(open.date)}</span></div><span className="app-spacer" /><span className="d-untrusted">From an email · treated as data</span></div>
              <div className="d-prose" style={{ whiteSpace: 'pre-wrap', overflowWrap: 'anywhere' }}>{open.text || '(The message has no text.)'}</div>
              {open.files.length > 0 && (
                <div className="app-stack" style={{ gap: 'var(--d-space-6)' }}>
                  <span className="d-type-caption d-text-secondary">{open.files.length === 1 ? 'A file with it' : `${open.files.length} files with it`}: saved, never opened here.</span>
                  <div className="app-row" style={{ flexWrap: 'wrap' }}>{open.files.map((f, n) => <a key={`${f}-${n}`} className="d-chip" href={fileUrl(open, n)} download={f}><Icon icon={Paperclip} size="sm" />{f}</a>)}</div>
                </div>
              )}
              <div className="app-row"><Button onClick={() => reply(open, false)}><Icon icon={Reply} />Reply</Button><Button variant="ghost" disabled={forwarding} onClick={() => void forward(open)}><Icon icon={Forward} />{forwarding ? 'Fetching its files…' : 'Forward'}</Button></div>
            </div></div>
          </>
        ) : (
          <div className="app-stack" style={{ placeContent: 'center', justifyItems: 'center', padding: 'var(--d-space-24)' }}>
            <Empty icon={MailOpen} title="No message open" text="Pick one in the list." />
            <div style={{ maxWidth: '26rem' }}><NotBuilt block>Cc and Bcc, labels, choosing several messages at once, a model drafting the message, and "Ask Demido about this" here (ask in a session instead).</NotBuilt></div>
          </div>
        )}
      </section>
      {composing && <Compose accounts={accounts} composing={composing} onChange={setComposing} onDone={() => { setComposing(null); void readKept(account.id) }} />}
      {adding && <AddAccount onClose={() => setAdding(false)} onAdded={added} />}
      {forGood && <Dialog open onOpenChange={(o) => !o && setForGood(null)} title="Delete it for good?" text={`"${forGood.subject || '(no subject)'}" is taken off the server${trash ? ` from ${labelOf(trash)}` : ''}. It cannot be had back.`}
        actions={<><Button onClick={() => setForGood(null)}>Keep it</Button><Button variant="danger" onClick={() => { const m = forGood; setForGood(null); void remove(m, true) }}>Delete for good</Button></>} />}
      {removing && <Dialog open onOpenChange={(o) => !o && setRemoving(false)} title={`Remove ${account.address} from Demido?`} text="Its password, its drafts and what Demido kept of its mail are deleted. The mailbox itself is not touched." actions={<><Button onClick={() => setRemoving(false)}>Cancel</Button><Button variant="danger" onClick={() => void removeAccount()}>Remove</Button></>} />}
    </div>
  )
}
