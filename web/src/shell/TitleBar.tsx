import { useEffect, useState } from 'react'
import { Copy, Minus, Square, X } from 'lucide-react'
import { getCurrentWindow } from '@tauri-apps/api/window'

import { Indicator } from '@/downloads/Indicator'
import styles from './TitleBar.module.css'

/**
 * The application title bar.
 *
 * Drawn by the window rather than by the OS, because `design/shell.md` puts
 * the download indicator in it and an OS title bar carries nothing of ours.
 * It is still the OS window's bar in every way a person relies on: it drags
 * the window, a double click maximises, and it **keeps minimise**, which the
 * panels do not have ("The application's own title bar is an OS window and
 * keeps its minimise. Panels are not OS windows.").
 *
 * `design/system.md` gives it two states, focused and unfocused, on `chrome`:
 * the title in `ink-2` while the window has focus and `ink-3` when it does
 * not, and the controls in `ink-3` with `hover` and `active` under the pointer.
 *
 * Dragging is `data-tauri-drag-region`, which Tauri reads off the element the
 * pointer went down on and not off its ancestors, so it is on the bar and on
 * the title, and not on anything that is a control.
 */
export function TitleBar() {
  const [focused, setFocused] = useState(true)
  const [maximised, setMaximised] = useState(false)

  useEffect(() => {
    const own = getCurrentWindow()
    const settle = () =>
      void own
        .isMaximized()
        .then(setMaximised)
        .catch(() => {})
    settle()
    void own
      .isFocused()
      .then(setFocused)
      .catch(() => {})
    const focus = own.onFocusChanged(({ payload }) => setFocused(payload))
    const resized = own.onResized(settle)
    return () => {
      void focus.then((unlisten) => unlisten())
      void resized.then((unlisten) => unlisten())
    }
  }, [])

  const own = getCurrentWindow()

  return (
    <header className={styles.bar} data-focused={focused} data-tauri-drag-region>
      <span className={styles.title} data-tauri-drag-region>
        Demido Studio
      </span>
      <div className={styles.controls}>
        <Indicator />
        <button
          type="button"
          className={styles.control}
          aria-label="Minimise"
          onClick={() => void own.minimize()}
        >
          <Minus className={styles.icon} strokeWidth={1.8} aria-hidden />
        </button>
        <button
          type="button"
          className={styles.control}
          aria-label={maximised ? 'Restore' : 'Maximise'}
          onClick={() => void own.toggleMaximize()}
        >
          {maximised ? (
            <Copy className={styles.icon} strokeWidth={1.8} aria-hidden />
          ) : (
            <Square className={styles.icon} strokeWidth={1.8} aria-hidden />
          )}
        </button>
        <button
          type="button"
          className={styles.control}
          aria-label="Close"
          onClick={() => void own.close()}
        >
          <X className={styles.icon} strokeWidth={1.8} aria-hidden />
        </button>
      </div>
    </header>
  )
}
