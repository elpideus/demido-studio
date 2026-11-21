/* The pieces every chat service shares: the messages, the message box with its writing help, and the small
   conversation with the model about the chat you have open. */
import { useState } from 'react'
import { PenLine, SendHorizontal, UserRound, UsersRound } from 'lucide-react'
import { Button, Icon, Tip, cx } from '@/ui'
import { AskPanel } from '@/ui/ask'
import type { Ask } from '@/ui/ask'
import { ImproveButton, continueFrom } from '@/ui/write'
import type { Chat, Line, Service } from './data'

/** What a model would draft, chat by chat. Scripted. */
const DRAFTS: Record<string, string> = {
  anna: 'Yes, 15:00 works. Please book the room. Thank you!',
  family: 'I will bring the big table cloth.',
  landlord: 'Good morning, the meter reading is 48,213. Kind regards',
  mara: 'I asked for access yesterday. I will chase it today and write as soon as I have it.',
  'dm/lina': 'In from about 21:00. Count me in.',
  'lab/builds': 'No, the last merge only touched the invoice code. I am looking at the token tests now.',
  'models/help': 'One more thing that helps: keep each tool description to a single line.',
}
const draftFor = (chat: Chat) => DRAFTS[chat.id] ?? 'Thanks. I will get back to you today.'
const author = (l: Line) => (l.who === 'me' ? 'you' : (l.name ?? 'them'))

/** A profile picture. Drawn for the preview: a colour from the name, with its initials. */
export function Avatar({ name, group, size }: { name: string; group?: boolean; size?: 'xl' }) {
  let hue = 0
  for (const ch of name) hue = (hue * 31 + ch.charCodeAt(0)) % 360
  const letters = name.replace(/^[^A-Za-z]+/, '').split(/[\s-]+/).slice(0, 2).map((w) => w.charAt(0)).join('').toUpperCase()
  return (
    <span className={cx('d-avatar', 'msgs__avatar', size && `msgs__avatar--${size}`)} style={{ background: `oklch(0.56 0.1 ${hue})` }} aria-hidden="true">
      {group && !name.startsWith('#') ? <Icon icon={UsersRound} size={size === 'xl' ? 'xl' : 'sm'} /> : name.startsWith('#') ? '#' : letters || <Icon icon={UserRound} size={size === 'xl' ? 'xl' : 'sm'} />}
    </span>
  )
}

/** Bubbles, as WhatsApp and Telegram show a chat; rows with a name, as Discord does. */
export function Lines({ lines, look = 'bubbles' }: { lines: Line[]; look?: 'bubbles' | 'rows' }) {
  if (look === 'rows')
    return (
      <div className="msgs__rows">
        {lines.map((l, i) => {
          const first = i === 0 || author(lines[i - 1]) !== author(l)
          return (
            <div key={i} className="msgs__row" data-first={first ? '' : undefined}>
              {first ? <Avatar name={author(l)} /> : <span />}
              <div>
                {first && <p className="msgs__who"><strong>{author(l)}</strong><span className="d-type-caption d-text-tertiary">{l.at}</span></p>}
                <p>{l.text}</p>
              </div>
            </div>
          )
        })}
      </div>
    )
  return (
    <div className="msgs__lines">
      {lines.map((l, i) => (
        <div key={i} className="msgs__line" data-who={l.who}>
          {l.name && <span className="d-type-micro d-text-tertiary">{l.name}</span>}
          <p>{l.text}</p>
          <span className="d-type-caption d-text-tertiary">{l.at}</span>
        </div>
      ))}
    </div>
  )
}

/** The message box. "Draft" continues from what you typed; "Improve" rewrites it. */
export function Composer({ chat, text, onText, drafted, onDrafted, canWrite, onSend, onNote }: { chat: Chat; text: string; onText: (t: string) => void; drafted: boolean; onDrafted: (d: boolean) => void; canWrite: boolean; onSend: () => void; onNote: (s: string) => void }) {
  const [busy, setBusy] = useState(false)
  const typed = text.trim() !== '' && !drafted
  const draft = () => {
    setBusy(true)
    setTimeout(() => {
      onText(continueFrom(text, draftFor(chat)))
      onDrafted(true)
      setBusy(false)
      onNote(typed ? `Finished your reply to "${chat.name}", starting from what you typed.` : `Drafted a reply to "${chat.name}". Waiting for you.`)
    }, 900)
  }
  return (
    <form className="d-composer chat__composer" onSubmit={(e) => { e.preventDefault(); onSend() }}>
      <textarea className="d-composer__input" rows={1} placeholder={`Write to ${chat.name}`} aria-label="Reply" value={text} onChange={(e) => { onText(e.target.value); onDrafted(false) }} onKeyDown={(e) => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); onSend() } }} />
      <div className="d-composer__bar">
        <Tip label={typed ? 'A model finishes the reply, starting with your words' : 'A model writes a reply for you to send'}>
          <Button variant="ghost" size="sm" disabled={busy || !canWrite || chat.scam} onClick={draft}><Icon icon={PenLine} size="sm" />{busy ? 'Drafting…' : typed ? 'Finish my reply' : 'Draft a reply'}</Button>
        </Tip>
        <ImproveButton text={text} to={chat.group ? undefined : chat.name} medium="chat" onText={(t) => { onText(t); onDrafted(false) }} disabled={!canWrite} />
        {drafted && <span className="d-type-caption d-text-secondary">Written by a model. Not sent yet.</span>}
        <span className="d-composer__spacer" />
        <Tip label="Send as you"><Button variant="primary" iconOnly aria-label="Send as you" type="submit" disabled={!text.trim()}><Icon icon={SendHorizontal} /></Button></Tip>
      </div>
    </form>
  )
}

// ───────── asking the model about this chat ─────────
export type { Ask } from '@/ui/ask'
const SUGGEST = ['What needs an answer here?', 'Write that I will be ten minutes late', 'Mark it as seen']
const asSentence = (s: string) => { const t = s.trim().replace(/^["“”']+|["“”']+$/g, ''); const u = t.charAt(0).toUpperCase() + t.slice(1); return /[.!?…]$/.test(u) ? u : `${u}.` }

/** The scripted stand-in for a model that has this chat in front of it. Returns what it says, and what it does. */
function answer(q: string, chat: Chat, canWrite: boolean): { text: string; type?: string; seen?: boolean } {
  const s = q.toLowerCase()
  const them = chat.lines.filter((l) => l.who === 'them')
  const last = them[them.length - 1]
  const readOnly = 'I may only read here. Set "The model may" to Drafts, then ask again.'
  if (/\b(seen|read)\b/.test(s) && /\b(mark|set|make)\b/.test(s)) return canWrite ? { text: `Done. "${chat.name}" is marked as seen.`, seen: true } : { text: readOnly }
  // A question about the chat comes before a request to write: "what needs an answer?" is not "answer".
  if (/summar|catch me up|tl;?dr|\bwhat\b.*\b(want|ask|say|said|need|about|happen)|needs? an answer/.test(s))
    return { text: `${them.length} ${them.length === 1 ? 'message' : 'messages'} from ${chat.group ? 'the group' : chat.name}. ${chat.needs && last ? `Waiting on you: “${last.text}”` : last ? `Last: “${last.text}” Nothing waits on you.` : 'Nothing from them yet.'}` }
  if (/\b(who|trust|safe|scam|trick|spam)\b/.test(s))
    return { text: chat.scam ? 'I would not trust it. It asks for money or a secret, and it is not someone you have written to before. Do not answer.' : `${chat.name} ${chat.group ? 'is a group you are in' : 'is someone you have written to before'}. Nothing here looks like a trick.` }
  if (/\b(write|type|reply|respond|tell|say|draft)\b/.test(s) || /^(please\s+)?answer\b/.test(s)) {
    if (!canWrite) return { text: readOnly }
    if (chat.scam) return { text: 'I will not write to this one. It looks like a trick, and an answer tells them the number is alive.' }
    const said = /\bthat\s+(.+)$/i.exec(q) ?? /\b(?:write|type|say|reply|answer|respond)\b(?:\s+(?:back|to\s+\w+|her|him|them))?[:,]?\s+(.+)$/i.exec(q)
    const own = said && !/^(something|a reply|an answer|it|back|for me|nicely|politely)\b/i.test(said[1].trim()) ? asSentence(said[1]) : null
    const body = own ?? draftFor(chat)
    return { text: `I typed this into the message box:\n“${body}”\nRead it, change it, then press Send. I have sent nothing.`, type: body }
  }
  return { text: 'About this chat I can: say what needs an answer, sum it up, mark it as seen, or type a reply into the message box for you ("write that I am on my way"). I send nothing on my own here.' }
}

export function Assistant({ svc, chat, canWrite, thread, onThread, onType, onSeen }: { svc: Service; chat: Chat; canWrite: boolean; thread: Ask[]; onThread: (t: Ask[]) => void; onType: (text: string) => void; onSeen: () => void }) {
  return (
    <AskPanel
      intro={<>Ask about <strong>{chat.name}</strong>, or have something typed for you. The model sees this {svc.name} chat and nothing else from here.</>}
      suggestions={SUGGEST} placeholder={`Ask about ${chat.name}`} reading="Reading the chat…" thread={thread} onThread={onThread}
      respond={(q) => {
        const a = answer(q, chat, canWrite)
        return { text: a.text, run: () => { if (a.type) onType(a.type); if (a.seen) onSeen() } }
      }}
    />
  )
}
