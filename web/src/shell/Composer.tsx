import { Send } from 'lucide-react'

import styles from './Composer.module.css'

/**
 * The composer, drawn and disabled.
 *
 * It sends nothing: the turn loop is the next ticket. What it does do is state
 * why, which is the rule `design/shell.md` sets for every disabled state on
 * this desk. Nothing is loaded, so the sentence it shows is the one that file
 * already wrote for exactly this moment, and it is `ink-3` rather than `ink-4`
 * because it is the only thing on screen at the moment somebody reads it
 * (`docs/rules/surfaces.md`).
 *
 * The two ways out that sentence promises, Browse models and Start on Nexus,
 * are not here. They land with the set-up wizard and the model browser that can
 * honour them; a button that goes nowhere is a worse empty state than one
 * button fewer.
 */
export function Composer() {
  return (
    <div className={styles.composer}>
      <textarea
        className={styles.field}
        rows={2}
        placeholder="Ask Demido"
        aria-label="Message"
        disabled
      />
      <div className={styles.foot}>
        <p className={styles.why}>Pick a model to start. Nothing is loaded yet.</p>
        <button type="button" className={styles.send} aria-label="Send" disabled>
          <Send className={styles.icon} strokeWidth={1.8} aria-hidden />
        </button>
      </div>
    </div>
  )
}
