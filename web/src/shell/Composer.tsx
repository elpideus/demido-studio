import { useEffect, useRef, useState } from 'react'
import { Send, SlidersHorizontal, Square } from 'lucide-react'

import { useChat, type Presence } from '@/chat/chat'
import { ChatSettings } from '@/settings/Settings'
import styles from './Composer.module.css'

/**
 * The composer.
 *
 * `design/system.md`: a field is a recess, disabled states the reason in `ink-3`
 * and never `ink-4`, and **send becomes stop while the model runs**. The last
 * one is why there is one button rather than two: a stop that appears beside
 * send is a second control in the place a person's hand already is, and the two
 * are never both available.
 *
 * The reasons it can be disabled are the states `Presence` distinguishes, and
 * the sentences are here rather than in Rust because they are UI copy. What
 * crosses the boundary is the fact.
 *
 * The settings button beside it opens this conversation's own tier of the
 * ladder, as a popover, which is the shape `design/shell.md` gives the tool
 * picker and for the same reason: what a chat overrides belongs to the chat,
 * and `design/windows.md` keeps the main Settings window to global values and
 * nothing else. You always know what you are editing because it is the thing
 * you are standing in.
 *
 * The two ways out of the empty state that `design/shell.md` promises, Browse
 * models and Start on Nexus, are still not here. They land with the set-up
 * wizard and the model browser that can honour them; a button that goes nowhere
 * is a worse empty state than one button fewer.
 */
export function Composer() {
  const presence = useChat((chat) => chat.presence)
  const running = useChat((chat) => chat.running)
  const send = useChat((chat) => chat.send)
  const stop = useChat((chat) => chat.stop)
  const load = useChat((chat) => chat.load)
  const [message, setMessage] = useState('')
  const [settings, setSettings] = useState(false)
  const bay = useRef<HTMLDivElement>(null)

  // Dismissed by a click outside it, which is what makes it a popover rather
  // than a window. Captured, so a click that lands on a control still closes
  // the popover before that control acts on it, the way the rail's menu does.
  useEffect(() => {
    if (!settings) return
    const dismiss = (event: MouseEvent) => {
      if (!bay.current?.contains(event.target as Node)) setSettings(false)
    }
    document.addEventListener('mousedown', dismiss, true)
    return () => document.removeEventListener('mousedown', dismiss, true)
  }, [settings])

  const ready = presence.state === 'ready'
  const said = message.trim()

  function submit() {
    if (!ready || running || !said) return
    setMessage('')
    void send(said)
  }

  return (
    <div className={styles.composer} ref={bay}>
      {settings && <ChatSettings close={() => setSettings(false)} />}
      <textarea
        className={styles.field}
        rows={2}
        placeholder="Ask Demido"
        aria-label="Message"
        // Typing during a generation is allowed: the next question is often
        // written while the current answer is still arriving. Sending is not,
        // and the button says so by being a stop.
        disabled={!ready}
        value={message}
        onChange={(event) => setMessage(event.target.value)}
        onKeyDown={(event) => {
          // Enter sends and Shift+Enter is a newline, which is the arrangement
          // every chat in this shape has. A composer that needed a click for
          // the common case would be the one thing on this desk that does.
          if (event.key === 'Enter' && !event.shiftKey && !event.nativeEvent.isComposing) {
            event.preventDefault()
            submit()
          }
        }}
      />
      <div className={styles.foot}>
        <p className={styles.why}>{why(presence)}</p>
        {/* The way back from a crashed or failed start. Without it the composer
         * that reports the failure is also the composer that can never be used
         * again, and a subsystem that is reported and skipped has to be one the
         * desk can ask for a second time. */}
        {presence.state === 'failed' && (
          <button type="button" className={styles.retry} onClick={load}>
            Try again
          </button>
        )}
        {/* Always available, including while nothing is loaded: a context
         * length or a system prompt set before a model starts is the ordinary
         * order to do it in, and it is the one the set-up wizard will use. */}
        <button
          type="button"
          className={styles.settings}
          aria-label="This chat's settings"
          aria-pressed={settings}
          onClick={() => setSettings(!settings)}
        >
          <SlidersHorizontal className={styles.icon} strokeWidth={1.8} aria-hidden />
        </button>
        {running ? (
          <button type="button" className={styles.stop} aria-label="Stop" onClick={stop}>
            <Square className={styles.icon} strokeWidth={1.8} aria-hidden />
          </button>
        ) : (
          <button
            type="button"
            className={styles.send}
            aria-label="Send"
            disabled={!ready || !said}
            onClick={submit}
          >
            <Send className={styles.icon} strokeWidth={1.8} aria-hidden />
          </button>
        )}
      </div>
    </div>
  )
}

/**
 * Why the composer will not send, in one sentence, or nothing when it will.
 *
 * A model that is still loading says so rather than presenting an idle
 * composer: a slow load and a broken app look identical from the outside, and
 * this is the only thing that tells them apart.
 */
function why(presence: Presence): string {
  switch (presence.state) {
    case 'absent':
      // The sentence `design/shell.md` wrote for exactly this moment.
      return 'Pick a model to start. Nothing is loaded yet.'
    case 'loading':
      return `Loading ${presence.model}. The first launch reads several gigabytes off disk.`
    case 'failed':
      return presence.detail
    case 'ready':
      return ''
  }
}
