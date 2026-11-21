/* Writing mail: a new message, a reply, a reply to everyone, a forward.
   The window floats over the corner, so you can still read what you are answering. Closing it keeps the
   text as a draft. A model can write the message or finish what you started; Send is always yours. */
import { useState } from 'react'
import { ChevronDown, ChevronUp, Paperclip, PenLine, SendHorizontal, Trash2, X } from 'lucide-react'
import { Button, Icon, NotBuilt, Select, Tip } from '@/ui'
import { ImproveButton, continueFrom } from '@/ui/write'
import { ACCOUNTS } from './data'
import type { Account } from './data'

export interface Draft {
  /** The saved draft this continues, if any. */
  id?: string
  mode: 'new' | 'reply' | 'replyAll' | 'forward'
  account: Account
  to: string
  cc: string
  bcc: string
  subject: string
  body: string
  files: string[]
  /** The message it answers or forwards. */
  about?: string
  /** Who it answers, by name: for the greeting. */
  name?: string
}
const TITLE = { new: 'New message', reply: 'Reply', replyAll: 'Reply to everyone', forward: 'Forward' } as const

/** What a model would write, by the message being answered. Scripted. */
export const REPLIES: Record<string, string> = {
  '1': 'Hi Anna,\n\n15:00 on Thursday works for me. Please book the room.\n\nThanks',
  '2': 'Dear Sir or Madam,\n\nthe meter reading is 48,213.\n\nKind regards',
  '3': 'Hi Jonas,\n\nI checked page 2. The hours are right. You can send it.\n\nBest',
  '8': 'Hi Mara,\n\nthank you for the notes. I will take point 3, and come back to you on the other two by Friday.\n\nBest regards',
}
/** The text you write sits above the quoted message; only that part is drafted or improved. */
const split = (body: string) => {
  const at = body.search(/\n\n(On [^\n]* wrote:|---------- Forwarded message ----------)/)
  return at < 0 ? { mine: body, quoted: '' } : { mine: body.slice(0, at), quoted: body.slice(at) }
}

export function Compose({ draft, onChange, onSend, onClose, onDiscard }: { draft: Draft; onChange: (d: Draft) => void; onSend: () => void; onClose: () => void; onDiscard: () => void }) {
  const [small, setSmall] = useState(false)
  const [copies, setCopies] = useState(draft.cc !== '' || draft.bcc !== '')
  const [busy, setBusy] = useState(false)
  const [modelWrote, setModelWrote] = useState(false)
  const set = (patch: Partial<Draft>) => onChange({ ...draft, ...patch })
  const { mine, quoted } = split(draft.body)
  const ready = /\S+@\S+/.test(draft.to) && (draft.subject.trim() !== '' || mine.trim() !== '')
  const write = () => {
    setBusy(true)
    setTimeout(() => {
      const model = draft.mode === 'forward' ? 'Hello,\n\nplease see the message below.\n\nKind regards' : (draft.about && REPLIES[draft.about]) || `Hello${draft.name ? ` ${draft.name.split(' ')[0]}` : ''},\n\n${draft.mode === 'new' ? `I am writing about ${draft.subject.trim() || 'the matter we spoke about'}.` : 'thank you for your message. I will come back to you shortly.'}\n\nKind regards`
      set({ body: continueFrom(mine, model) + quoted })
      setModelWrote(true)
      setBusy(false)
    }, 900)
  }
  return (
    <section className="email__compose" data-small={small ? '' : undefined} aria-label={TITLE[draft.mode]}>
      <header className="email__compose-head">
        <span className="d-type-headline">{TITLE[draft.mode]}{small && draft.subject ? `: ${draft.subject}` : ''}</span>
        <span className="app-spacer" />
        <Tip label={small ? 'Open' : 'Make small'}><Button variant="ghost" size="sm" iconOnly aria-label={small ? 'Open' : 'Make small'} onClick={() => setSmall(!small)}><Icon icon={small ? ChevronUp : ChevronDown} /></Button></Tip>
        <Tip label="Close and keep as a draft"><Button variant="ghost" size="sm" iconOnly aria-label="Close and keep as a draft" onClick={onClose}><Icon icon={X} /></Button></Tip>
      </header>
      {!small && (
        <>
          <div className="email__compose-fields">
            <label><span>From</span><Select label="From" value={draft.account} onChange={(v) => set({ account: v })} options={ACCOUNTS.map((a) => ({ value: a.id, label: `${a.name} · ${a.address}` }))} /></label>
            <label><span>To</span><input className="d-input" type="text" autoFocus={draft.mode === 'new' || draft.mode === 'forward'} placeholder="name@example.com" value={draft.to} onChange={(e) => set({ to: e.target.value })} />{!copies && <button type="button" className="app-link" onClick={() => setCopies(true)}>Cc, Bcc</button>}</label>
            {copies && <label><span>Cc</span><input className="d-input" type="text" value={draft.cc} onChange={(e) => set({ cc: e.target.value })} /></label>}
            {copies && <label><span>Bcc</span><input className="d-input" type="text" value={draft.bcc} onChange={(e) => set({ bcc: e.target.value })} /></label>}
            <label><span>Subject</span><input className="d-input" type="text" value={draft.subject} onChange={(e) => set({ subject: e.target.value })} /></label>
          </div>
          <textarea className="email__compose-body" aria-label="Message" autoFocus={draft.mode === 'reply' || draft.mode === 'replyAll'} placeholder="Write, or let a model start" value={draft.body} onChange={(e) => { set({ body: e.target.value }); setModelWrote(false) }} />
          {draft.files.length > 0 && (
            <div className="app-row email__compose-files">
              {draft.files.map((f) => <span key={f} className="d-chip"><Icon icon={Paperclip} size="sm" />{f}<button type="button" className="mk__mini" style={{ opacity: 1 }} aria-label={`Remove ${f}`} onClick={() => set({ files: draft.files.filter((x) => x !== f) })}><Icon icon={X} size="sm" /></button></span>)}
              <NotBuilt>Files are not real</NotBuilt>
            </div>
          )}
          <footer className="email__compose-foot">
            <Button variant="primary" size="sm" disabled={!ready} onClick={onSend}><Icon icon={SendHorizontal} size="sm" />Send</Button>
            <Tip label={mine.trim() && !modelWrote ? 'A model finishes it, starting with your words' : 'A model writes it for you to send'}>
              <Button variant="ghost" size="sm" disabled={busy} onClick={write}><Icon icon={PenLine} size="sm" />{busy ? 'Writing…' : mine.trim() && !modelWrote ? 'Finish my text' : 'Draft with a model'}</Button>
            </Tip>
            <ImproveButton text={mine} to={draft.name} medium="mail" onText={(t) => { set({ body: t + quoted }); setModelWrote(false) }} />
            <Tip label="Attach a file"><Button variant="ghost" size="sm" iconOnly aria-label="Attach a file" onClick={() => set({ files: [...draft.files, `file-${draft.files.length + 1}.pdf`] })}><Icon icon={Paperclip} /></Button></Tip>
            <span className="app-spacer" />
            {modelWrote && <span className="d-type-caption d-text-secondary">Written by a model. Not sent yet.</span>}
            <Tip label="Throw this message away"><Button variant="ghost" size="sm" iconOnly aria-label="Throw this message away" onClick={onDiscard}><Icon icon={Trash2} /></Button></Tip>
          </footer>
        </>
      )}
    </section>
  )
}
