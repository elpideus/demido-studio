import { useEffect } from 'react'

import { Transcript } from '@/chat/Transcript'
import { useChat } from '@/chat/chat'
import { Settings } from '@/settings/Settings'
import { SetupRow, Wizard, useSetupOnce } from '@/setup/Wizard'
import { Composer } from './Composer'
import { Toasts } from './Toast'
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
 * A panel opens **over** the desk rather than beside it, which is
 * `design/shell.md`'s one structural rule: chat is the surface the application
 * is, and everything else covers it. Settings is the first panel there is, and
 * it is drawn inside the desk rather than over the rail so that the rail stays
 * reachable while it is open.
 *
 * The set-up wizard is drawn over all of it, and it is the first thing on
 * screen on a profile that has not finished set-up
 * (`docs/rules/setup.md` section 1). It is read beside the layout and the
 * conversation rather than before them, because a person who has already set
 * up must not wait on a folder scan to see their desk, and one who has not
 * gets the wizard a frame later on a desk that is already drawn.
 *
 * The conversation is opened beside the layout rather than after it, and the
 * desk does not wait for it. A layout decides what the first frame looks like;
 * a transcript decides what is on it, and the model behind that transcript
 * takes minutes to load. Holding the window for either would be a boot screen,
 * and startup never blocks (`AGENTS.md`).
 */
export function App() {
  const rail = useDesk((desk) => desk.rail)
  const hydrated = useDesk((desk) => desk.hydrated)
  const hydrate = useDesk((desk) => desk.hydrate)
  const panel = useDesk((desk) => desk.panel)
  const open = useChat((chat) => chat.open)
  useSetupOnce()

  useEffect(() => {
    void hydrate()
    void open()
  }, [hydrate, open])

  if (!hydrated) return <div className={styles.shell} />

  return (
    <div className={styles.shell} data-rail={rail}>
      <Rail />
      <main className={styles.desk}>
        <Transcript />
        <div className={styles.bay}>
          <div className={styles.column}>
            {/* Above the composer rather than in a corner: what it offers is
             * the rest of the set-up, and the composer beside it is the thing
             * that cannot answer until that is done. */}
            <SetupRow />
            <Composer />
          </div>
        </div>
        {panel === 'settings' && <Settings />}
        <Wizard />
        {/* Over everything on the desk, including the settings window, because
         * what it reports is usually a value that window just refused. */}
        <Toasts />
      </main>
    </div>
  )
}
