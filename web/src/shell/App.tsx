import { useEffect } from 'react'

import { Composer } from './Composer'
import { Rail } from './Rail'
import { useDesk } from './desk'
import styles from './App.module.css'

/**
 * The desk.
 *
 * `design/shell.md`: chat is not a window, it is the surface the application
 * *is*. It fills the frame under the rail, is never navigated away from, and
 * everything else (the session monitor, the code graph, the market charts, the
 * file explorer, the browser) will open as a window **over** it. That is why
 * this component draws a transcript and a composer directly rather than routing
 * to them: there is nothing to route to, and there never will be.
 *
 * The rail sits on either edge, and which edge is remembered by Rust in the
 * profile's `shell.json`
 * (`docs/decisions/0010-the-desk-remembers-itself.md`). It is read once, here,
 * and **nothing is drawn until the answer arrives**. That answer is a file read
 * on the other side of an IPC call, so it cannot be had before the first paint;
 * the choice is between one frame of bare rack and a rail that appears on the
 * left and jumps to the right, and a desk that visibly rearranges itself at
 * every launch is the worse of the two. The rack is the window's own background
 * either way, so the held frame is not a blank rectangle, it is the desk with
 * nothing on it yet.
 *
 * The composer sends nothing. The turn loop is the next ticket.
 */
export function App() {
  const rail = useDesk((desk) => desk.rail)
  const hydrated = useDesk((desk) => desk.hydrated)
  const hydrate = useDesk((desk) => desk.hydrate)

  useEffect(() => {
    void hydrate()
  }, [hydrate])

  if (!hydrated) return <div className={styles.shell} />

  return (
    <div className={styles.shell} data-rail={rail}>
      <Rail />
      <main className={styles.desk}>
        {/* The transcript is the rack itself rather than an island on it
         * (docs/rules/surfaces.md), which is what makes chat read as the
         * surface rather than as the largest card on one. */}
        <div className={styles.transcript}>
          <div className={styles.column}>
            <p className={styles.quiet}>Nothing has been said yet.</p>
          </div>
        </div>
        <div className={styles.bay}>
          <div className={styles.column}>
            <Composer />
          </div>
        </div>
      </main>
    </div>
  )
}
