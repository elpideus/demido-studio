/* Help with writing, shared by every message box that speaks for you: the chat services and email.

   Two things:
     Draft     a model writes a reply. If you already typed something, the draft starts with your words.
     Improve   a model rewrites what you typed: a tone from the list, or an instruction of your own.

   In the preview no model runs. A few fixed rules stand in for it, and the screen says so. */
import { useState } from 'react'
import { Popover } from '@base-ui/react/popover'
import { SpellCheck, Undo2 } from 'lucide-react'
import { toast } from '@/data/store'
import { Button, Icon } from './index'

export type Tone = 'fix' | 'shorter' | 'formal' | 'friendly' | 'happy' | 'angry' | 'sorry' | 'custom'
export const TONES: { value: Exclude<Tone, 'custom'>; label: string; hint: string }[] = [
  { value: 'fix', label: 'Fix spelling and grammar', hint: 'keeps your words' },
  { value: 'shorter', label: 'Shorter', hint: 'the point only' },
  { value: 'formal', label: 'Formal', hint: 'for an office' },
  { value: 'friendly', label: 'Friendly', hint: 'warm' },
  { value: 'happy', label: 'Happy', hint: 'pleased' },
  { value: 'angry', label: 'Angry', hint: 'firm, not rude' },
  { value: 'sorry', label: 'Apologetic', hint: 'owning it' },
]

const lower = (s: string) => s.charAt(0).toLowerCase() + s.slice(1)
const upper = (s: string) => s.charAt(0).toUpperCase() + s.slice(1)
const ended = (s: string) => /[.!?…]$/.test(s)
/** Clean up without changing the words: spaces, a capital, a full stop, a lone "i". */
function tidy(text: string): string {
  const s = text.trim().replace(/[ \t]+/g, ' ').replace(/\bi\b/g, 'I').replace(/ +([,.!?;:])/g, '$1').replace(/\b(teh)\b/gi, 'the').replace(/\b(recieve)/gi, 'receive').replace(/\b(tommorow|tomorow)\b/gi, 'tomorrow')
  return s ? upper(ended(s) ? s : `${s}.`) : s
}
const PLAIN: [RegExp, string][] = [
  [/\bcan't\b/gi, 'cannot'], [/\bwon't\b/gi, 'will not'], [/\bdon't\b/gi, 'do not'], [/\bdoesn't\b/gi, 'does not'], [/\bdidn't\b/gi, 'did not'],
  [/\bI'm\b/g, 'I am'], [/\bI'll\b/g, 'I will'], [/\bI've\b/g, 'I have'], [/\bit's\b/gi, 'it is'], [/\bthat's\b/gi, 'that is'],
  [/\bthanks\b/gi, 'thank you'], [/\bok(ay)?\b/gi, 'very well'], [/\basap\b/gi, 'as soon as possible'], [/\bgonna\b/gi, 'going to'], [/\bwanna\b/gi, 'want to'],
  [/^(hi|hey|hello)\b[^,.!?\n]*[,.!]?\s*/i, ''],
]
const sentence = (s: string) => s.replace(/[.!?…]+$/, '')

/** The rules that stand in for a model. `to` is the person you are writing to. */
export function improve(text: string, tone: Tone, opts: { to?: string; medium: 'chat' | 'mail' }): string {
  const first = opts.to?.split(/[ ,@]/)[0]
  let s = tidy(text)
  if (!s) return s
  switch (tone) {
    case 'fix':
    case 'custom':
      return s
    case 'shorter': {
      const one = s.split(/(?<=[.!?…])\s+/)[0]
      const words = one.split(' ')
      return words.length > 14 ? `${sentence(words.slice(0, 14).join(' '))}.` : one
    }
    case 'formal':
      for (const [from, toWord] of PLAIN) s = s.replace(from, toWord)
      s = tidy(s)
      return opts.medium === 'mail' ? `Dear ${opts.to ?? 'Sir or Madam'},\n\n${s}\n\nKind regards` : `Good day${first ? `, ${first}` : ''}. ${s} Kind regards.`
    case 'friendly':
      s = s.replace(/\bthank you\b/gi, 'thanks').replace(/^(hi|hey|hello)\b[^,.!?\n]*[,.!]?\s*/i, '')
      return `Hi${first ? ` ${first}` : ''}! ${upper(s)} 🙂`
    case 'happy':
      return `${/^great/i.test(s) ? '' : 'Great! '}${sentence(s)}! 🎉`
    case 'angry':
      return `Honestly, ${lower(sentence(s))}. This is not acceptable.`
    case 'sorry':
      return `I am sorry. ${s} Thank you for your patience.`
  }
}

/** A draft that starts with what you already typed. */
export function continueFrom(typed: string, draft: string): string {
  const t = typed.trim()
  if (!t) return draft
  if (draft.toLowerCase().startsWith(t.toLowerCase())) return t + draft.slice(t.length)
  if (/[,;:]$/.test(t)) return `${t} ${lower(draft)}`
  if (!ended(t) && t.split(/\s+/).length <= 3) return `${t}, ${lower(draft)}`
  return `${ended(t) ? t : `${t}.`} ${draft}`
}

/** "Improve": the tones, your own instruction, and a way back to what you typed. */
export function ImproveButton({ text, to, medium, onText, disabled }: { text: string; to?: string; medium: 'chat' | 'mail'; onText: (t: string) => void; disabled?: boolean }) {
  const [open, setOpen] = useState(false)
  const [how, setHow] = useState('')
  const [mine, setMine] = useState<string | null>(null)
  const [made, setMade] = useState('')
  const apply = (tone: Tone) => {
    const next = improve(text, tone, { to, medium })
    setMine(text)
    setMade(next)
    onText(next)
    setOpen(false)
    if (tone === 'custom') toast(`A model would rewrite it the way you said: "${how.trim()}". The preview only tidied it.`)
  }
  return (
    <>
      <Popover.Root open={open} onOpenChange={setOpen}>
        <Popover.Trigger render={<Button variant="ghost" size="sm" disabled={disabled || !text.trim()} />}><Icon icon={SpellCheck} size="sm" />Improve</Popover.Trigger>
        <Popover.Portal>
          <Popover.Positioner side="top" align="start" sideOffset={8} className="app-layer">
            <Popover.Popup className="d-menu app-pop app-improve">
              <p className="d-menu__label">Rewrite what I typed</p>
              {TONES.map((t) => (
                <button key={t.value} type="button" className="d-menu__item" onClick={() => apply(t.value)}><span style={{ width: 0 }} /><span>{t.label}</span><span className="d-menu__hint">{t.hint}</span></button>
              ))}
              <hr className="d-menu__sep" />
              <form className="app-improve__own" onSubmit={(e) => { e.preventDefault(); if (how.trim()) apply('custom') }}>
                <input className="d-input" aria-label="Your own instruction" placeholder="Or say how: more direct, no emoji" value={how} onChange={(e) => setHow(e.target.value)} />
                <Button size="sm" type="submit" disabled={!how.trim()}>Rewrite</Button>
              </form>
              <p className="d-type-caption d-text-tertiary app-improve__note">Scripted: a model will do this. Here a few fixed rules stand in for it.</p>
            </Popover.Popup>
          </Popover.Positioner>
        </Popover.Portal>
      </Popover.Root>
      {mine !== null && text === made && made !== mine && <Button variant="ghost" size="sm" onClick={() => { onText(mine); setMine(null) }}><Icon icon={Undo2} size="sm" />Back to mine</Button>}
    </>
  )
}
