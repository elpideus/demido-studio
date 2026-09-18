import { useEffect, useRef, useState, type CSSProperties } from 'react'
import { X } from 'lucide-react'

import { useChat } from '@/chat/chat'
import { useDesk } from '@/shell/desk'
import { Assembled, Detail } from './Inspector'
import { Scope } from './Scope'
import { Stream } from './Stream'
import { useMonitor } from './log'
import styles from './Monitor.module.css'

/**
 * The session monitor, pinned to the bottom of the desk.
 *
 * Brief B07: "Everything the model sees is recorded in an append-only session
 * log", and the half of that sentence this window answers is the one the brief
 * asks for next: "In the Trajectory view, you can inspect these records by
 * source."
 *
 * It opens **pinned** rather than floating, which `design/shell.md` gives as
 * one of a panel's two modes: the chat island gives up exactly its height and
 * nothing is covered. That is the mode this panel wants, because what it is
 * read against is the conversation that produced it, and a float that covered
 * the transcript would hide the thing being explained.
 *
 * Six of the nine components `design/system.md` lists under Session monitor
 * are here: the stream, the assembly and the detail pane from
 * [#57](https://github.com/elpideus/demido-studio/issues/57), and the agent
 * scope, the slot strip and the source ledger from
 * [#68](https://github.com/elpideus/demido-studio/issues/68). The rest are
 * deferred rather than forgotten: the lanes, the token weight bars and the
 * occupancy header are each a projection of data this log already carries, so
 * the screens can arrive without the record changing shape.
 *
 * **Escape does not close it**, unlike the settings window. A pinned panel is
 * part of the layout rather than something over it, and Escape at panel scope
 * is the approval row's Deny (`web/src/shell/keys.ts`): binding the two to one
 * key would make dismissing a window and refusing a call the same keystroke.
 * The rail icon and the close button are how it goes away.
 */

export function Monitor() {
  const height = useDesk((desk) => desk.monitorHeight)
  const resize = useDesk((desk) => desk.resizeMonitor)
  const close = useDesk((desk) => desk.toggleMonitor)
  const read = useMonitor((monitor) => monitor.read)
  const assembly = useMonitor((monitor) => monitor.assembly)
  const events = useMonitor((monitor) => monitor.events)
  // The log is re-read whenever the conversation beside it changes, which is
  // every time a turn records something: `chat_transcript` is replaced on a
  // `recorded` update and again when the turn ends. Following the desk's own
  // projection rather than subscribing to `chat://update` a second time keeps
  // one listener on that channel, and keeps this panel a reader of the log
  // rather than a second thing assembling a session from events.
  const transcript = useChat((chat) => chat.transcript)
  // And when a turn starts or ends, which is what the slot strip counts
  // against: a request out on the log is a filled slot only while a turn runs.
  const running = useChat((chat) => chat.running)

  useEffect(() => {
    void read()
  }, [read, transcript, running])

  const panel = useRef<HTMLElement>(null)
  const reduced = useReduced(panel)

  return (
    <div className={styles.dock} style={{ '--height': `${height}px` } as CSSProperties}>
      <Seam resize={resize} />
      <section
        ref={panel}
        className={styles.panel}
        aria-label="Session monitor"
        data-reduced={reduced}
      >
        <header className={styles.bar}>
          <h2 className={styles.name}>Session monitor</h2>
          {/* What the window is reading, stated rather than implied: an empty
           * log and a log that failed to read look identical otherwise. */}
          <p className={styles.count}>{`${events.length} events`}</p>
          {/* Maximise is the other half of what a panel title bar carries
           * (`design/shell.md`) and it arrives with the gesture that can
           * honour it: a maximise that could not snap would be a control
           * lying about what it does. */}
          <button type="button" className={styles.close} aria-label="Close" onClick={close}>
            <X className={styles.icon} strokeWidth={1.8} aria-hidden />
          </button>
        </header>

        <div className={styles.columns}>
          {/* The agent scope ([#68](https://github.com/elpideus/demido-studio/issues/68)),
           * which replaced a Sub-agents window: at the floor it drops to icons
           * rather than going, because scoping is how a sub-agent is read at
           * all and a reduced form without it would hide delegation exactly
           * where the window is smallest. */}
          <Scope reduced={reduced} />
          <Stream />
          {/* The reduced form, stated as `design/shell.md` requires a panel
           * with a floor to state one: at 210px the scope column is icons, the
           * stream keeps the rest of the panel and the inspector is not drawn,
           * because there is no float layer yet to detach it into. Three
           * columns squeezed into the height of two rows is the failure that
           * rule exists to prevent, and it keeps every element by making all
           * of them unreadable. */}
          {!reduced && (
            <>
              <Assembled assembly={assembly} />
              <Detail />
            </>
          )}
        </div>
      </section>
    </div>
  )
}

/**
 * The seam between the chat island and the panel.
 *
 * `design/shell.md`: "The `--space-gutter` of rack between chat and a pinned
 * panel is a **draggable seam**. Dragging it rebalances the two." It is one of
 * the two cases `docs/rules/gaps-and-hairlines.md` allows a hairline for, and
 * it is the only chrome that says the panel is pinned.
 *
 * The drag sets a number and nothing else. What that number is allowed to be is
 * the floor and the ceiling in CSS, so the pointer maths cannot be the thing
 * that gets a floor wrong, and there is no second copy of 210 anywhere.
 *
 * **A drag starts from the height the panel has, not from the one it last asked
 * for.** Those are different numbers the moment a drag runs into the floor, and
 * taking the asked-for one makes the seam stick: drag well past the floor and
 * the panel stops at 210 while the stored number keeps falling, so the way back
 * is as far as the overshoot went before anything moves. Measuring makes every
 * drag start from what is on screen, which is what the person is aiming at.
 */
function Seam({ resize }: { resize: (height: number) => void }) {
  return (
    <div
      className={styles.seam}
      role="separator"
      aria-orientation="horizontal"
      aria-label="Resize the session monitor"
      onPointerDown={(event) => {
        // The moves are listened for on the document rather than the seam
        // captured, which is what a drag needs: the pointer leaves an 8px strip
        // immediately, and capture would buy nothing over a listener that is
        // already above everything it could cross.
        const from = event.clientY
        const was = event.currentTarget.parentElement?.getBoundingClientRect().height ?? 0
        const move = (at: PointerEvent) => resize(was + (from - at.clientY))
        const done = () => {
          document.removeEventListener('pointermove', move)
          document.removeEventListener('pointerup', done)
        }
        document.addEventListener('pointermove', move)
        document.addEventListener('pointerup', done)
      }}
    />
  )
}

/**
 * Whether the panel is at its floor, and so drawing its reduced form.
 *
 * Measured rather than derived from the height somebody dragged to, because
 * the two are not the same number: the desk can be shorter than the panel asked
 * for, and what decides the form is what the panel actually got. It is the
 * **panel** that is measured and not the dock around it, because 210px is the
 * panel's floor and the dock is that plus a seam and a gutter. The floor is
 * read from `--floor-monitor` so that the CSS that clamps it and the JavaScript
 * that reports it are looking at one value (`design/tokens.css`).
 */
function useReduced(panel: React.RefObject<HTMLElement | null>): boolean {
  const [reduced, setReduced] = useState(false)

  useEffect(() => {
    const element = panel.current
    if (!element) return
    const floor = Number.parseFloat(
      getComputedStyle(document.documentElement).getPropertyValue('--floor-monitor'),
    )
    const watch = new ResizeObserver(() => {
      // Half a pixel of slack, because a clamped height on a fractional device
      // pixel ratio lands just under the floor rather than on it.
      setReduced(element.getBoundingClientRect().height <= floor + 0.5)
    })
    watch.observe(element)
    return () => watch.disconnect()
  }, [panel])

  return reduced
}
