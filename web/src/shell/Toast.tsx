import { CircleAlert, X } from 'lucide-react'

import { useToasts } from './toasts'
import styles from './Toast.module.css'

/**
 * Refusals, over the desk.
 *
 * Transient chrome rather than a panel, so it takes the shape `design/system.md`
 * gives the snap menu and the rail's own menu: a `panel` face with the one
 * shadow in the system, at `--radius-control`. It is not a surface anything
 * lives on, and nothing is ever drawn inside it but a sentence.
 *
 * There is no coloured circle beside the words (`design/shell.md` bans it). The
 * state is carried by a real icon in `rose`, which is the colour this system
 * gives an error, and by the words themselves.
 *
 * Stacked oldest at the bottom, because a second refusal arriving must not move
 * the sentence somebody is already reading.
 */
export function Toasts() {
  const toasts = useToasts((state) => state.toasts)
  const dismiss = useToasts((state) => state.dismiss)

  if (toasts.length === 0) return null

  return (
    // Polite rather than assertive: the person caused this a moment ago and is
    // still looking at the control that refused, so it is an answer rather than
    // an interruption.
    <div className={styles.rail} role="status" aria-live="polite">
      {toasts.map((toast) => (
        <div key={toast.id} className={styles.toast}>
          <CircleAlert className={styles.icon} strokeWidth={1.8} aria-hidden />
          <p className={styles.message}>{toast.message}</p>
          {/* It goes on its own after a few seconds. This is for the person who
           * has read it and wants the desk back now. */}
          <button
            type="button"
            className={styles.close}
            aria-label="Dismiss"
            onClick={() => dismiss(toast.id)}
          >
            <X className={styles.close_icon} strokeWidth={1.8} aria-hidden />
          </button>
        </div>
      ))}
    </div>
  )
}
