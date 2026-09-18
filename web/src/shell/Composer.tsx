import { useEffect, useRef, useState } from 'react'
import { Boxes, Send, SlidersHorizontal, Square, Wrench } from 'lucide-react'

import { loading, useChat, type Presence } from '@/chat/chat'
import { Capabilities } from '@/models/Capabilities'
import { useSettings } from '@/settings/ladder'
import { ChatSettings } from '@/settings/Settings'
import { useSetup } from '@/setup/setup'
import { ModeControl } from './ModeControl'
import { useDesk } from './desk'
import { ToolPicker } from './ToolPicker'
import styles from './Composer.module.css'

/** Which popover the composer has open. One at a time: they stand in the same
 * place over the field. */
type Open = 'settings' | 'tools' | 'mode' | null

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
 * The empty state now has one way out and it is the true one: on a profile
 * where set-up is outstanding the composer says so and offers to take it up,
 * rather than saying "pick a model" on a machine with no backend to load one
 * with. Nexus is the other way out `design/shell.md` promises and is not in
 * S1, so the desk behind the door has no Nexus rung
 * ([#48](https://github.com/elpideus/demido-studio/issues/48)); a button that
 * goes nowhere is a worse empty state than one button fewer.
 *
 * The model control and the empty state's **Browse models** both open the
 * Models window over the desk, the same browser the wizard's models step
 * renders in place ([#75](https://github.com/elpideus/demido-studio/issues/75)).
 */
export function Composer() {
  const presence = useChat((chat) => chat.presence)
  const running = useChat((chat) => chat.running)
  const send = useChat((chat) => chat.send)
  const stop = useChat((chat) => chat.stop)
  const load = useChat((chat) => chat.load)
  const outstanding = useSetup((setup) => setup.view !== null && !setup.view.complete)
  const resume = useSetup((setup) => setup.resume)
  const readSettings = useSettings((settings) => settings.read)
  const panel = useDesk((desk) => desk.panel)
  const togglePanel = useDesk((desk) => desk.toggle)
  const [message, setMessage] = useState('')
  const [open, setOpen] = useState<Open>(null)
  const bay = useRef<HTMLDivElement>(null)

  // The chat tier is read when the desk mounts rather than when a popover
  // opens, because the mode control draws its shield at rest: the strictest
  // mode's one standing mark cannot wait for somebody to open a menu.
  useEffect(() => {
    void readSettings('chat')
  }, [readSettings])

  // Dismissed by a click outside it, which is what makes it a popover rather
  // than a window. Captured, so a click that lands on a control still closes
  // the popover before that control acts on it, the way the rail's menu does.
  useEffect(() => {
    if (!open) return
    const dismiss = (event: MouseEvent) => {
      if (!bay.current?.contains(event.target as Node)) setOpen(null)
    }
    document.addEventListener('mousedown', dismiss, true)
    return () => document.removeEventListener('mousedown', dismiss, true)
  }, [open])

  const toggle = (which: Exclude<Open, null>) => setOpen(open === which ? null : which)
  const close = () => setOpen(null)

  const ready = presence.state === 'ready'
  const said = message.trim()

  function submit() {
    if (!ready || running || !said) return
    setMessage('')
    void send(said)
  }

  return (
    <div className={styles.composer} ref={bay}>
      {open === 'settings' && <ChatSettings close={close} />}
      {open === 'tools' && <ToolPicker close={close} />}
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
        <p className={styles.why}>
          {outstanding && presence.state === 'absent'
            ? 'Set-up is not finished, so there is nothing loaded to answer with.'
            : why(presence)}
        </p>
        {/* The way back into the wizard, from the place a person notices they
         * cannot send anything. It is the same gesture the desk's set-up row
         * makes, and it reaches the same plan. */}
        {outstanding && presence.state === 'absent' && (
          <button type="button" className={styles.retry} onClick={() => void resume()}>
            Finish set-up
          </button>
        )}
        {/* The way back from a crashed or failed start. Without it the composer
         * that reports the failure is also the composer that can never be used
         * again, and a subsystem that is reported and skipped has to be one the
         * desk can ask for a second time. */}
        {/* The way out `design/shell.md` promises the empty state: a model
         * nobody has yet is one the browser can find. */}
        {presence.state === 'absent' && (
          <button type="button" className={styles.retry} onClick={() => togglePanel('models')}>
            Browse models
          </button>
        )}
        {presence.state === 'failed' && (
          <button type="button" className={styles.retry} onClick={load}>
            Try again
          </button>
        )}
        {/* The two axes side by side and kept apart (`docs/rules/tools.md`):
         * what runs without asking, then what the model is shown. Both are
         * always available, including while nothing is loaded, because they
         * are decided about the message before it is sent. The chat's own
         * settings follow: a context length or a system prompt set before a
         * model starts is the ordinary order to do it in. */}
        <div className={styles.controls}>
          <ModelControl open={panel === 'models'} toggle={() => togglePanel('models')} />
          <ModeControl open={open === 'mode'} toggle={() => toggle('mode')} close={close} />
          <button
            type="button"
            className={styles.control}
            aria-label="Tools"
            aria-pressed={open === 'tools'}
            onClick={() => toggle('tools')}
          >
            <Wrench className={styles.icon} strokeWidth={1.8} aria-hidden />
          </button>
          <button
            type="button"
            className={styles.control}
            aria-label="This chat's settings"
            aria-pressed={open === 'settings'}
            onClick={() => toggle('settings')}
          >
            <SlidersHorizontal className={styles.icon} strokeWidth={1.8} aria-hidden />
          </button>
        </div>
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
 * Which model answers, with its capability tags inline (`design/system.md`,
 * "Model selector"), opening the Models window.
 *
 * The tags are the chosen file's, read from the file, because that is the
 * model the composer is talking to.
 */
function ModelControl({ open, toggle }: { open: boolean; toggle: () => void }) {
  const presence = useChat((chat) => chat.presence)
  const chosen = useSetup((setup) =>
    setup.view?.models.models.find((model) => model.path === setup.view?.models.chosen),
  )
  const name =
    presence.state === 'ready' || presence.state === 'loading'
      ? presence.model
      : (chosen?.label ?? 'No model')

  return (
    <button
      type="button"
      className={styles.model}
      aria-label={`Model: ${name}`}
      aria-pressed={open}
      onClick={toggle}
    >
      <Boxes className={styles.icon} strokeWidth={1.8} aria-hidden />
      <span className={styles.modelName}>{name}</span>
      {chosen && <Capabilities facts={chosen.capabilities} from="file" compact />}
    </button>
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
      return loading(presence.model)
    case 'failed':
      return presence.detail
    case 'ready':
      return ''
  }
}
