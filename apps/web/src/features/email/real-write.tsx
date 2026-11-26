/* Writing mail, for real (DECISIONS Y12–Y14): a new message, a reply in its thread, a reply to everyone, a
   forward with its files. The window floats over the corner, so what you answer stays readable. Closing it keeps
   the text as a draft in Demido; files are not kept with a draft. Sending cannot be undone, so the window asks
   once more, with the whole message in view. */
import { useRef, useState } from 'react'
import { ChevronDown, ChevronUp, Paperclip, SendHorizontal, Trash2, X } from 'lucide-react'
import { coreSend } from '@/data/core'
import { toast } from '@/data/store'
import { Button, Dialog, Icon, Select, Signal, Tip } from '@/ui'

interface KeyFacts { kept_in: 'keyring' | 'file'; ends_with: string }
export interface Account { id: string; address: string; label: string; preset: 'gmail' | 'other'; imap_host: string; imap_port: number; smtp_host: string; smtp_port: number; username: string; password: KeyFacts | null }
/** A file to send: read in the window, sent in base64. */
export interface Picked { name: string; content_type: string | null; base64: string; size: number }
/** A draft as the core keeps it (Y14). */
export interface KeptDraft { id: string; to: string; subject: string; text: string; in_reply_to: string | null; references: string[]; updated_at: number }
export interface Composing {
  /** The kept draft this continues, if any. */
  draft: string | null
  mode: 'new' | 'reply' | 'replyAll' | 'forward'
  account: string
  to: string
  subject: string
  text: string
  /** The message it answers, by its Message-ID, and the thread so far: a reply stays in its thread (Y12). */
  in_reply_to: string | null
  references: string[]
  files: Picked[]
}

/** The most that may be sent with one message: what Gmail takes. The core says the same (`MOST_FILES`). */
export const MOST_FILES = 25 * 1024 * 1024
export const bytes = (n: number) => (n < 1024 ? `${n} B` : n < 1024 * 1024 ? `${Math.round(n / 1024)} KB` : `${(n / 1024 / 1024).toFixed(1)} MB`)
const TITLE = { new: 'New message', reply: 'Reply', replyAll: 'Reply to everyone', forward: 'Forward' } as const
const drafts = (account: string) => `/email/accounts/${encodeURIComponent(account)}/drafts`

/** A file's bytes in base64, as a message carries them. */
export function asBase64(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onload = () => resolve(String(reader.result).replace(/^data:[^,]*,/, ''))
    reader.onerror = () => reject(reader.error ?? new Error('The file could not be read.'))
    reader.readAsDataURL(blob)
  })
}

/** Keeps what is being written as a draft; gives back its id. */
export async function keepDraft(c: Composing): Promise<string> {
  const kept = await coreSend<KeptDraft>('PUT', drafts(c.account), { id: c.draft ?? '', to: c.to, subject: c.subject, text: c.text, in_reply_to: c.in_reply_to, references: c.references })
  return kept.id
}

export function forgetDraft(account: string, id: string): Promise<void> {
  return coreSend<boolean>('DELETE', `${drafts(account)}/${encodeURIComponent(id)}`).then(() => undefined)
}

export function Compose({ accounts, composing, onChange, onDone }: { accounts: Account[]; composing: Composing; onChange: (c: Composing) => void; onDone: () => void }) {
  const [small, setSmall] = useState(false)
  const [sure, setSure] = useState(false)
  const [busy, setBusy] = useState(false)
  const [reading, setReading] = useState(false)
  const files = useRef<HTMLInputElement>(null)
  const c = composing
  const set = (patch: Partial<Composing>) => onChange({ ...c, ...patch })
  const from = accounts.find((a) => a.id === c.account) ?? accounts[0]
  const total = c.files.reduce((n, f) => n + f.size, 0)
  const tooBig = total > MOST_FILES
  const ready = /\S+@\S+/.test(c.to) && (c.subject.trim() !== '' || c.text.trim() !== '') && !tooBig
  const nothing = !c.to.trim() && !c.subject.trim() && !c.text.trim() && !c.files.length

  const attach = async (list: FileList | null) => {
    if (!list?.length) return
    setReading(true)
    try {
      const read = await Promise.all(Array.from(list).map(async (f) => ({ name: f.name, content_type: f.type || null, base64: await asBase64(f), size: f.size })))
      onChange({ ...c, files: [...c.files, ...read] })
    } catch (e) {
      toast(`Not attached. ${(e as Error).message}`, undefined, 'task-failed')
    }
    setReading(false)
    if (files.current) files.current.value = ''
  }
  /** A draft belongs to its account: written from another, the old one is thrown away and a new one kept. */
  const fromAccount = (id: string) => {
    if (c.draft) void forgetDraft(c.account, c.draft).catch(() => undefined)
    set({ account: id, draft: null })
  }
  const close = async () => {
    if (nothing) {
      if (c.draft) await forgetDraft(c.account, c.draft).catch(() => undefined)
      onDone()
      return
    }
    try {
      await keepDraft(c)
      toast(c.files.length ? 'Kept as a draft. Its files are not kept with it: attach them again when you send it.' : 'Kept as a draft.')
      onDone()
    } catch (e) {
      toast(`Not kept. ${(e as Error).message}`, undefined, 'task-failed')
    }
  }
  const discard = async () => {
    if (c.draft) await forgetDraft(c.account, c.draft).catch(() => undefined)
    onDone()
  }
  const send = async () => {
    setBusy(true)
    try {
      await coreSend('POST', `/email/accounts/${encodeURIComponent(from.id)}/send`, {
        to: c.to, subject: c.subject, text: c.text, in_reply_to: c.in_reply_to, references: c.references,
        files: c.files.map((f) => ({ name: f.name, content_type: f.content_type, base64: f.base64 })),
      })
      if (c.draft) await forgetDraft(c.account, c.draft).catch(() => undefined)
      toast(`Sent from ${from.address} to ${c.to}.`, undefined, 'task-done')
      onDone()
    } catch (e) {
      toast(`Not sent. ${(e as Error).message}`, undefined, 'task-failed')
      setSure(false)
    }
    setBusy(false)
  }

  return (
    <section className="email__compose" data-small={small ? '' : undefined} aria-label={TITLE[c.mode]}>
      <header className="email__compose-head">
        <span className="d-type-headline">{TITLE[c.mode]}{small && c.subject ? `: ${c.subject}` : ''}</span>
        <span className="app-spacer" />
        <Tip label={small ? 'Open' : 'Make small'}><Button variant="ghost" size="sm" iconOnly aria-label={small ? 'Open' : 'Make small'} onClick={() => setSmall(!small)}><Icon icon={small ? ChevronUp : ChevronDown} /></Button></Tip>
        <Tip label="Close and keep as a draft"><Button variant="ghost" size="sm" iconOnly aria-label="Close and keep as a draft" onClick={() => void close()}><Icon icon={X} /></Button></Tip>
      </header>
      {!small && (
        <>
          <div className="email__compose-fields">
            <label><span>From</span><Select label="From" value={from.id} onChange={fromAccount} options={accounts.map((a) => ({ value: a.id, label: `${a.label} · ${a.address}` }))} /></label>
            <label><span>To</span><input className="d-input" type="text" autoFocus={c.mode === 'new' || c.mode === 'forward'} placeholder="name@example.com, another@example.com" value={c.to} onChange={(e) => set({ to: e.target.value })} /></label>
            <label><span>Subject</span><input className="d-input" type="text" value={c.subject} onChange={(e) => set({ subject: e.target.value })} /></label>
          </div>
          <textarea className="email__compose-body" aria-label="Message" autoFocus={c.mode === 'reply' || c.mode === 'replyAll'} placeholder="Write your message" value={c.text}
            onChange={(e) => set({ text: e.target.value })} />
          {(c.files.length > 0 || reading) && (
            <div className="app-row email__compose-files">
              {c.files.map((f, i) => (
                <span key={`${f.name}-${i}`} className="d-chip"><Icon icon={Paperclip} size="sm" />{f.name} <span className="d-text-tertiary">{bytes(f.size)}</span>
                  <button type="button" className="mk__mini" style={{ opacity: 1 }} aria-label={`Remove ${f.name}`} onClick={() => set({ files: c.files.filter((_, j) => j !== i) })}><Icon icon={X} size="sm" /></button></span>
              ))}
              {reading && <span className="d-type-caption d-text-tertiary">Reading the files…</span>}
              {tooBig && <Signal level="caution">{bytes(total)} of files: a message carries {bytes(MOST_FILES)} at most.</Signal>}
            </div>
          )}
          <footer className="email__compose-foot">
            <Button variant="primary" size="sm" disabled={!ready || reading} onClick={() => setSure(true)}><Icon icon={SendHorizontal} size="sm" />Send…</Button>
            <input ref={files} type="file" multiple hidden onChange={(e) => void attach(e.target.files)} />
            <Tip label="Attach files"><Button variant="ghost" size="sm" iconOnly aria-label="Attach files" onClick={() => files.current?.click()}><Icon icon={Paperclip} /></Button></Tip>
            <span className="app-spacer" />
            {c.in_reply_to && <span className="d-type-caption d-text-tertiary">In the same thread</span>}
            <Tip label="Throw this message away"><Button variant="ghost" size="sm" iconOnly aria-label="Throw this message away" onClick={() => void discard()}><Icon icon={Trash2} /></Button></Tip>
          </footer>
        </>
      )}
      {sure && (
        <Dialog open wide onOpenChange={(o) => !o && setSure(false)} title="Send this message?" text="Sending cannot be undone: once it has gone, it is in their mailbox."
          actions={<><Button onClick={() => setSure(false)}>Back to it</Button><Button variant="primary" disabled={busy} onClick={() => void send()}>{busy ? 'Sending…' : 'Send'}</Button></>}>
          <div className="app-stack" style={{ marginBlockStart: 'var(--d-space-16)' }}>
            <p className="d-mono">From: {from.address}<br />To: {c.to}<br />Subject: {c.subject || '(no subject)'}{c.files.length > 0 && <><br />Files: {c.files.map((f) => `${f.name} (${bytes(f.size)})`).join(', ')}</>}</p>
            <div className="d-prose" style={{ whiteSpace: 'pre-wrap', maxHeight: '18rem', overflow: 'auto' }}>{c.text}</div>
          </div>
        </Dialog>
      )}
    </section>
  )
}
