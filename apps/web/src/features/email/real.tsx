/* Email, for real (DECISIONS Y): the person's own accounts, read by IMAP and sent from by SMTP.

   Reading marks nothing as read, moves nothing and deletes nothing: the mailbox is opened read-only and a message
   fetched without marking it. A message is shown as text, so none of its pictures is loaded and the sender cannot
   tell it was opened. Sending cannot be undone, so the window asks once more, with the whole message in view.
   A password is given once and never shown again. */
import { useEffect, useState } from 'react'
import { MailOpen, Inbox, ChevronDown, Mail, PenLine, Plus, RefreshCw, Reply, Trash2 } from 'lucide-react'
import { coreGet, coreSend } from '@/data/core'
import { toast } from '@/data/store'
import { Button, Dialog, Glyph, Icon, Menu, NotBuilt, Page, Segmented, Signal, Empty } from '@/ui'

interface KeyFacts { kept_in: 'keyring' | 'file'; ends_with: string }
interface Account { id: string; address: string; label: string; preset: 'gmail' | 'other'; imap_host: string; imap_port: number; smtp_host: string; smtp_port: number; username: string; password: KeyFacts | null }
interface Summary { account: string; folder: string; uid: number; from: string; to: string; subject: string; date: number | null; seen: boolean; size: number }
interface Opened extends Summary { text: string; files: string[] }
interface Draft { to: string; subject: string; text: string }

const when = (ms: number | null) => (ms ? new Date(ms).toLocaleString(undefined, { day: 'numeric', month: 'short', hour: '2-digit', minute: '2-digit' }) : '')

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

function Write({ account, draft, onChange, onClose }: { account: Account; draft: Draft; onChange: (d: Draft) => void; onClose: () => void }) {
  const [sure, setSure] = useState(false)
  const [busy, setBusy] = useState(false)
  const send = async () => {
    setBusy(true)
    try {
      await coreSend('POST', `/email/accounts/${encodeURIComponent(account.id)}/send`, draft)
      toast(`Sent from ${account.address} to ${draft.to}.`, undefined, 'task-done')
      onClose()
    } catch (e) {
      toast(`Not sent. ${(e as Error).message}`, undefined, 'task-failed')
      setSure(false)
    }
    setBusy(false)
  }
  return (
    <Dialog open wide onOpenChange={(o) => !o && onClose()} title={sure ? 'Send this message?' : `Write from ${account.address}`}
      text={sure ? 'Sending cannot be undone: once it has gone, it is in their mailbox.' : undefined}
      actions={sure
        ? <><Button onClick={() => setSure(false)}>Back to it</Button><Button variant="primary" disabled={busy} onClick={() => void send()}>{busy ? 'Sending…' : 'Send'}</Button></>
        : <><Button onClick={onClose}>Discard</Button><Button variant="primary" disabled={!draft.to.trim() || !draft.text.trim()} onClick={() => setSure(true)}>Send…</Button></>}>
      {sure ? (
        <div className="app-stack" style={{ marginBlockStart: 'var(--d-space-16)' }}>
          <p className="d-mono">To: {draft.to}<br />Subject: {draft.subject || '(no subject)'}</p>
          <div className="d-prose" style={{ whiteSpace: 'pre-wrap' }}>{draft.text}</div>
        </div>
      ) : (
        <div className="app-stack" style={{ marginBlockStart: 'var(--d-space-16)' }}>
          <label className="d-field"><span className="d-field__label">To</span><input className="d-input" placeholder="someone@example.com, another@example.com" value={draft.to} onChange={(e) => onChange({ ...draft, to: e.target.value })} /></label>
          <label className="d-field"><span className="d-field__label">Subject</span><input className="d-input" value={draft.subject} onChange={(e) => onChange({ ...draft, subject: e.target.value })} /></label>
          <label className="d-field"><span className="d-field__label">Message</span><textarea className="d-input" rows={10} value={draft.text} onChange={(e) => onChange({ ...draft, text: e.target.value })} /></label>
        </div>
      )}
    </Dialog>
  )
}

export function RealEmail() {
  const [accounts, setAccounts] = useState<Account[] | null>(null)
  const [current, setCurrent] = useState<string | null>(null)
  const [list, setList] = useState<Summary[]>([])
  const [open, setOpen] = useState<Opened | null>(null)
  const [reading, setReading] = useState(false)
  const [adding, setAdding] = useState(false)
  const [removing, setRemoving] = useState(false)
  const [draft, setDraft] = useState<Draft | null>(null)
  const [error, setError] = useState<string | null>(null)
  const account = accounts?.find((a) => a.id === current) ?? accounts?.[0] ?? null
  const readAccounts = () => coreGet<Account[]>('/email/accounts').then((a) => { setAccounts(a); setError(null) }, (e: Error) => setError(e.message))
  const readList = async (fresh: boolean) => {
    if (!account) return
    setReading(true)
    try {
      setList(await coreGet<Summary[]>(`/email/accounts/${encodeURIComponent(account.id)}/messages${fresh ? '?fresh=true' : ''}`))
      setError(null)
    } catch (e) {
      setError((e as Error).message)
    }
    setReading(false)
  }
  useEffect(() => { void readAccounts() }, [])
  useEffect(() => {
    setOpen(null); setList([])
    // What was kept first, at once; then what is new on the server.
    if (account) void readList(false).then(() => readList(true))
  }, [account?.id])
  const pick = async (s: Summary) => {
    try {
      setOpen(await coreGet<Opened>(`/email/accounts/${encodeURIComponent(s.account)}/messages/${s.uid}`))
    } catch (e) {
      toast(`Not opened. ${(e as Error).message}`, undefined, 'task-failed')
    }
  }
  const remove = async () => {
    setRemoving(false)
    if (!account) return
    await coreSend('DELETE', `/email/accounts/${encodeURIComponent(account.id)}`).then(() => toast(`${account.address} is no longer in Demido. Its mailbox is as it was.`, undefined, 'task-done'), (e: Error) => toast(`Not removed. ${e.message}`, undefined, 'task-failed'))
    setCurrent(null)
    void readAccounts()
  }
  const reply = (m: Opened) => {
    const address = m.from.match(/<([^>]+)>/)?.[1] ?? m.from
    setDraft({ to: address, subject: m.subject.startsWith('Re:') ? m.subject : `Re: ${m.subject}`, text: `\n\n${m.from} wrote:\n${m.text.split('\n').map((l) => `> ${l}`).join('\n')}` })
  }

  if (error && !accounts) return <Page title="Email"><Signal level="caution">The core did not say. {error}</Signal></Page>
  if (!accounts) return <Page title="Email"><p className="app-empty"><Glyph name="task-running" busy /> Reading your accounts…</p></Page>
  if (accounts.length === 0) {
    return (
      <Page title="Email" lead="Your own mail accounts, read and sent from by Demido."
        about={<><p>Reading marks nothing as read. A message is shown as text, so its pictures are never loaded. Sending asks you first and cannot be undone.</p><p>Models can list your inbox, read a message and write one when Settings › Email allows it.</p></>}>
        <Empty icon={Mail} title="No mail account yet" text="Your inbox, read as text. Sending always asks you first." action={<Button variant="primary" onClick={() => setAdding(true)}><Icon icon={Plus} />Add a mail account</Button>} />
        <NotBuilt block>Folders other than the inbox, replying in the same thread, files, search, labels, stars, moving, deleting, drafts kept, and "Ask Demido about this" here (ask in a session instead).</NotBuilt>
        {adding && <AddAccount onClose={() => setAdding(false)} onAdded={(a) => { setAdding(false); setCurrent(a.id); void readAccounts() }} />}
      </Page>
    )
  }
  return (
    <div className="app-split email">
      <aside className="app-side" aria-label="Accounts">
        {account && (
          <Menu
            trigger={<button type="button" className="email__account"><span className="d-avatar">{account.label.slice(0, 2)}</span><span className="d-row__text"><span className="d-row__title">{account.label}</span><span className="d-row__sub">{account.address} · {account.preset === 'gmail' ? 'Gmail' : account.imap_host}</span></span><Icon icon={ChevronDown} size="sm" /></button>}
            items={[
              { kind: 'label', label: 'Show mail from' },
              ...(accounts ?? []).map((a, i) => ({ label: a.label, hint: i === 0 ? 'models use this one' : a.address, checked: a.id === account.id, onSelect: () => setCurrent(a.id) })),
              { kind: 'sep' },
              { label: 'Add an account…', lead: <Icon icon={Plus} size="sm" />, onSelect: () => setAdding(true) },
              { label: `Remove ${account.label}…`, lead: <Icon icon={Trash2} size="sm" />, onSelect: () => setRemoving(true) },
            ]}
          />
        )}
        {account && <Button variant="primary" onClick={() => setDraft({ to: '', subject: '', text: '' })}><Icon icon={PenLine} />Write</Button>}
        <div className="app-side__scroll">
          <ul className="d-list"><li><button type="button" className="d-row d-row--compact" aria-current><span className="d-row__lead"><Icon icon={Mail} /></span><span className="d-row__text"><span className="d-row__title">Inbox</span></span><span className="d-row__meta">{list.filter((m) => !m.seen).length || ''}</span></button></li></ul>
        </div>
        <p className="app-side__foot">Read-only: Demido marks nothing as read and moves nothing. Pictures in mail are never loaded.</p>
      </aside>
      <div className="app-side email__list" aria-label="Messages">
        <div className="app-side__head">
          <span className="d-type-headline">Inbox</span>
          <span className="d-type-caption d-text-tertiary">{list.length} newest</span>
          <Button size="sm" variant="ghost" disabled={reading} onClick={() => void readList(true)}><Icon icon={RefreshCw} size="sm" />{reading ? 'Reading…' : 'Read again'}</Button>
        </div>
        {error && <Signal level="caution">{error}</Signal>}
        <div className="app-side__scroll">
          <ul className="d-list">
            {list.map((m) => (
              <li key={`${m.account}-${m.uid}`} className="email__row">
                <button type="button" className="d-row" aria-current={open?.uid === m.uid} onClick={() => void pick(m)}>
                  <span className="d-row__text">
                    <span className="d-row__title" style={{ fontWeight: m.seen ? undefined : 'var(--d-font-weight-semibold)' }}>{m.from || '(no sender)'}</span>
                    <span className="d-row__sub" style={{ color: 'var(--d-color-text-secondary)' }}>{m.subject}</span>
                  </span>
                  <span className="d-row__meta">{when(m.date)}</span>
                </button>
              </li>
            ))}
          </ul>
          {!list.length && !reading && <Empty icon={Inbox} title="Nothing read yet" action={<Button size="sm" onClick={() => void readList(true)}>Read the inbox</Button>} />}
          {reading && !list.length && <p className="app-empty"><Glyph name="task-running" busy /> Reading the inbox…</p>}
        </div>
      </div>
      <section className="chat" style={{ gridTemplateRows: 'auto minmax(0, 1fr)' }} aria-label="Message">
        {open ? (
          <>
            <header className="chat__head"><span className="chat__title">{open.subject}</span><span className="app-spacer" /><Button size="sm" onClick={() => reply(open)}><Icon icon={Reply} size="sm" />Reply</Button></header>
            <div className="app-page"><div className="app-page__inner app-page__inner--narrow" style={{ gap: 'var(--d-space-16)' }}>
              <div className="app-row"><span className="d-avatar">{open.from.slice(0, 2)}</span><div className="app-stack" style={{ gap: 0 }}><span style={{ fontWeight: 'var(--d-font-weight-medium)' }}>{open.from}</span><span className="d-type-caption d-text-tertiary d-mono">to {open.to} · {when(open.date)}</span></div><span className="app-spacer" /><span className="d-untrusted">From an email · treated as data</span></div>
              <div className="d-prose" style={{ whiteSpace: 'pre-wrap' }}>{open.text || '(The message has no text.)'}</div>
              {open.files.length > 0 && <p className="d-type-caption d-text-secondary">Files with it, not opened here: {open.files.join(', ')}</p>}
            </div></div>
          </>
        ) : <Empty icon={MailOpen} title="No message open" text="Pick one in the list." />}
      </section>
      {adding && <AddAccount onClose={() => setAdding(false)} onAdded={(a) => { setAdding(false); setCurrent(a.id); void readAccounts() }} />}
      {draft && account && <Write account={account} draft={draft} onChange={setDraft} onClose={() => setDraft(null)} />}
      {removing && account && <Dialog open onOpenChange={(o) => !o && setRemoving(false)} title={`Remove ${account.address} from Demido?`} text="Its password and what Demido kept of its mail are deleted. The mailbox itself is not touched." actions={<><Button onClick={() => setRemoving(false)}>Cancel</Button><Button variant="danger" onClick={() => void remove()}>Remove</Button></>} />}
    </div>
  )
}
