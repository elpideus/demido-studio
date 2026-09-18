import { Gpu } from 'lucide-react'

import { useFit, words } from './fit'
import styles from './FitVerdict.module.css'

/**
 * Whether this card can hold this file, beside the action that downloads it
 * ([#74](https://github.com/elpideus/demido-studio/issues/74)).
 *
 * **Ink, never alarm colour.** Partial offload is information about a choice
 * the person is entitled to make, not a failure, so every verdict is drawn in
 * the same inks and differs only in its words. It never disables anything: the
 * download is the primary action's, and it does not read this.
 *
 * `weights` is the choice's weights alone (`Choice.weights`), which is what
 * loading costs; the projector is resident only while an image is read.
 */
export function FitVerdict({ weights }: { weights: number }) {
  const verdict = useFit(weights)
  if (!verdict) return null
  const { label, sentence } = words(verdict)

  return (
    <p className={styles.verdict}>
      <Gpu className={styles.icon} aria-hidden />
      <span className={styles.label}>{label}</span>
      <span className={styles.sentence}>{sentence}</span>
    </p>
  )
}
