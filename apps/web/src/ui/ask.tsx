/* A small conversation with the model about the thing you are looking at: a chat, an email.
   It sits beside that thing, so you never leave it to ask. The model may also act on it (type a reply,
   file a message); what it did is said in its answer.

   In the preview the model is a function you pass in: a few fixed answers. */
import { useEffect, useRef, useState } from 'react'
import type { ReactNode } from 'react'
import { SendHorizontal } from 'lucide-react'
import { Button, Glyph, Icon, NotBuilt } from './index'

export interface Ask { who: 'me' | 'demido'; text: string }
/** What the stand-in model says, and what it does when the answer appears. */
export interface Answer { text: string; run?: () => void }

export function AskPanel({ intro, suggestions, placeholder, thread, onThread, respond, reading = 'Reading…' }: { intro: ReactNode; suggestions: string[]; placeholder: string; thread: Ask[]; onThread: (t: Ask[]) => void; respond: (q: string) => Answer; reading?: string }) {
  const [q, setQ] = useState('')
  const [thinking, setThinking] = useState(false)
  const box = useRef<HTMLDivElement>(null)
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)
  useEffect(() => () => clearTimeout(timer.current), [])
  useEffect(() => { if (box.current) box.current.scrollTop = box.current.scrollHeight }, [thread.length, thinking])
  const ask = (text: string) => {
    const mine = text.trim()
    if (!mine || thinking) return
    const next: Ask[] = [...thread, { who: 'me', text: mine }]
    onThread(next)
    setQ('')
    setThinking(true)
    timer.current = setTimeout(() => {
      const a = respond(mine)
      a.run?.()
      onThread([...next, { who: 'demido', text: a.text }])
      setThinking(false)
    }, 800)
  }
  return (
    <div className="app-ask">
      <div className="app-ask__thread" ref={box}>
        {thread.length === 0 && (
          <div className="app-stack">
            <p className="d-text-secondary">{intro}</p>
            {suggestions.map((s) => <Button key={s} size="sm" onClick={() => ask(s)}>{s}</Button>)}
            <NotBuilt>Scripted: a few fixed answers</NotBuilt>
          </div>
        )}
        {thread.map((a, i) => (
          <div key={i} className="app-ask__line" data-who={a.who}>
            {a.who === 'demido' && <Glyph name="model-loaded" tone="live" />}
            <p>{a.text}</p>
          </div>
        ))}
        {thinking && <div className="app-ask__line" data-who="demido"><Glyph name="model-loaded" busy tone="live" /><p className="d-text-tertiary">{reading}</p></div>}
      </div>
      <form className="app-ask__box" onSubmit={(e) => { e.preventDefault(); ask(q) }}>
        <input className="d-input" aria-label={placeholder} placeholder={placeholder} value={q} onChange={(e) => setQ(e.target.value)} />
        <Button variant="primary" iconOnly aria-label="Ask" type="submit" disabled={!q.trim() || thinking}><Icon icon={SendHorizontal} /></Button>
      </form>
    </div>
  )
}
