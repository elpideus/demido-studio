import { useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'

import { size } from '@/shell/bytes'

/**
 * The fit verdict, as the window holds it
 * ([#74](https://github.com/elpideus/demido-studio/issues/74)).
 *
 * `demido-vram` decides; the window writes the sentence. The verdict is read
 * **again while the pane is open** rather than once when it was drawn, because
 * the card's free memory moves by more than the rig's tightest row has to
 * spare whenever a browser window opens. And nothing that starts a download
 * reads it: the verdict informs, and the machine is the person's.
 */

/** The Rust `demido_vram::Verdict`. Every figure is in bytes. */
export type Verdict =
  | { fit: 'fits'; needs: number; room: number; spare: number; contextPriced: boolean }
  | { fit: 'partial'; needs: number; room: number; over: number; contextPriced: boolean }
  | { fit: 'unread'; needs: number }
  | { fit: 'unpriced' }

/** How often an open pane reads the card again. */
const AGAIN_EVERY_MS = 5000

/** Whether this card can hold weights of `weights` bytes, read now. */
export function readFit(weights: number): Promise<Verdict> {
  return invoke<Verdict>('models_fit', { weights })
}

/**
 * The verdict for weights of `weights` bytes, read now, again every few
 * seconds while mounted, and again when the window comes back into focus.
 * `undefined` until the first reading answers, or when it could not be asked.
 */
export function useFit(weights: number): Verdict | undefined {
  const [verdict, setVerdict] = useState<Verdict>()

  useEffect(() => {
    let live = true
    const read = () => {
      readFit(weights).then(
        (read) => live && setVerdict(read),
        () => live && setVerdict(undefined),
      )
    }
    setVerdict(undefined)
    read()
    const every = window.setInterval(read, AGAIN_EVERY_MS)
    window.addEventListener('focus', read)
    return () => {
      live = false
      window.clearInterval(every)
      window.removeEventListener('focus', read)
    }
  }, [weights])

  return verdict
}

/** The verdict in two parts: a label a person scans, and the sentence behind
 * it. Each of the four is something different to do. */
export function words(verdict: Verdict): { label: string; sentence: string } {
  switch (verdict.fit) {
    case 'fits':
      return {
        label: 'Fits on this card',
        sentence: verdict.contextPriced
          ? `${size(verdict.needs)} leaves ${size(verdict.spare)} of the ${size(verdict.room)} this card has free.`
          : `The weights take ${size(verdict.needs)} of the ${size(verdict.room)} this card has free, before the context.`,
      }
    case 'partial':
      return {
        label: 'Partial offload',
        sentence: `${size(verdict.over)} over the ${size(verdict.room)} this card has free, so part of it runs from system memory, more slowly.`,
      }
    case 'unread':
      return {
        label: 'Card not read',
        sentence: `The weights take ${size(verdict.needs)}. This card's free memory could not be read.`,
      }
    case 'unpriced':
      return {
        label: 'Size not published',
        sentence: 'The server states no size for these weights, so there is nothing to price.',
      }
  }
}
