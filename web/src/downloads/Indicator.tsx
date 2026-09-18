import { useEffect, useRef, useState } from 'react'
import { ArrowDownToLine, CircleAlert } from 'lucide-react'

import { size } from '@/shell/bytes'
import { moving, open, useRows, type Row } from './downloads'
import { Queue } from './Queue'
import styles from './Indicator.module.css'

/**
 * The download indicator, in the application title bar.
 *
 * `design/shell.md` puts it there so a multi-gigabyte fetch is visible without
 * keeping a window open: the title bar is the one piece of chrome that is on
 * screen whatever is open over the desk. `design/system.md` gives it three
 * states, and each is carried by the icon and the words rather than by a dot:
 *
 * - **idle**, nothing moving: the icon alone, in `ink-3`, so the queue is
 *   still one click away for a paused or finished row.
 * - **active**: the icon in `signal`, and the bytes of everything still to
 *   arrive against their total.
 * - **failed**: a `rose` alert and how many failed, which wins over active,
 *   because a failure waits for a person and a running download does not.
 *
 * The queue opens beneath it as a popover, dismissed by a click outside or by
 * Escape from inside it, the way the composer's popovers are. Escape is the
 * popover's own key rather than a scoped binding, because the approval row
 * binds Escape to deny, and a person closing the queue must not also refuse
 * a tool call waiting on the desk.
 */
export function Indicator() {
  const rows = useRows()
  const [shown, setShown] = useState(false)
  const bay = useRef<HTMLDivElement>(null)

  useEffect(() => {
    void open()
  }, [])

  useEffect(() => {
    if (!shown) return
    const dismiss = (event: MouseEvent) => {
      if (!bay.current?.contains(event.target as Node)) setShown(false)
    }
    document.addEventListener('mousedown', dismiss, true)
    return () => document.removeEventListener('mousedown', dismiss, true)
  }, [shown])

  const state = summary(rows)

  return (
    <div
      className={styles.bay}
      ref={bay}
      onKeyDown={(event) => {
        if (event.key === 'Escape' && shown) {
          event.preventDefault()
          setShown(false)
        }
      }}
    >
      <button
        type="button"
        className={styles.indicator}
        data-state={state.state}
        aria-label="Downloads"
        aria-expanded={shown}
        onClick={() => setShown(!shown)}
      >
        {state.state === 'failed' ? (
          <CircleAlert className={styles.icon} strokeWidth={1.8} aria-hidden />
        ) : (
          <ArrowDownToLine className={styles.icon} strokeWidth={1.8} aria-hidden />
        )}
        {state.words && <span className={styles.words}>{state.words}</span>}
      </button>
      {shown && (
        <div className={styles.popover}>
          <Queue />
        </div>
      )}
    </div>
  )
}

type Summary = { state: 'idle' | 'active' | 'failed'; words: string | null }

/** What the indicator says about the whole queue. */
function summary(rows: Row[]): Summary {
  const failed = rows.filter((row) => row.state === 'failed').length
  if (failed > 0) return { state: 'failed', words: `${failed} failed` }
  const arriving = rows.filter(moving)
  if (arriving.length === 0) return { state: 'idle', words: null }
  const received = arriving.reduce((sum, row) => sum + row.received, 0)
  const total = arriving.reduce((sum, row) => sum + row.total, 0)
  return { state: 'active', words: `${size(received)} of ${size(total)}` }
}
