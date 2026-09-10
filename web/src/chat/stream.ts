import { useSyncExternalStore } from 'react'

/**
 * The answer being generated, held outside React.
 *
 * A token arriving is not a state change. It is one or two characters, sixty or
 * more times a second, and putting each one through `setState` re-renders the
 * whole transcript per token: v2 did exactly that and a long answer made the
 * window unusable while it was arriving. #34 fixes the shape rather than the
 * symptom, in one line: "Streaming tokens go into a ref with a subscription,
 * never `setState` per token."
 *
 * So this module is the ref. `append` mutates a buffer and tells nobody.
 * Subscribers are notified once per animation frame, which is the most often a
 * screen can show anything, and the snapshot they read is replaced only at that
 * moment. The result is one render per frame no matter how fast the model is.
 *
 * ## Why it is not in the store
 *
 * The desk's store is zustand, and a zustand `set` is a `setState`. Putting the
 * live answer there would be the defect above with a nicer API. The store holds
 * the transcript, which changes once a turn; this holds the answer, which
 * changes constantly and is drawn by exactly one component.
 *
 * ## It is a draft, and the log is the record
 *
 * What accumulates here is what the window has been told so far. When the turn
 * ends it is cleared and the transcript is read back from the session log,
 * which is the only place a conversation is kept. See
 * `docs/decisions/0011-the-window-draws-the-log-not-its-draft.md`.
 */

/** What a subscriber reads. Replaced rather than mutated, so React's
 * `useSyncExternalStore` can compare it by identity and skip a render when a
 * frame produced nothing. */
export type Streaming = {
  /** The answer so far. */
  text: string
  /** The reasoning so far, where the model separates it. */
  thinking: string
}

const NOTHING: Streaming = { text: '', thinking: '' }

/** The buffer. Mutated on every token, read by nobody until a flush. */
const buffer = { text: '', thinking: '' }

let snapshot: Streaming = NOTHING
let queued = false
const listeners = new Set<() => void>()

function flush() {
  queued = false
  snapshot = { text: buffer.text, thinking: buffer.thinking }
  for (const listener of listeners) listener()
}

/** At most one flush per frame, however many tokens arrived in it. */
function schedule() {
  if (queued) return
  queued = true
  requestAnimationFrame(flush)
}

/** A token, appended and not announced. */
export function append(part: Partial<Streaming>) {
  if (part.text) buffer.text += part.text
  if (part.thinking) buffer.thinking += part.thinking
  schedule()
}

/**
 * Throw the draft away, now rather than at the next frame.
 *
 * Called when a turn starts and when it ends. Immediate on purpose: at the end
 * of a turn the transcript is about to be redrawn from the log, and a frame in
 * which both the draft and the recorded answer are on screen is a duplicated
 * bubble somebody will see.
 */
export function clear() {
  buffer.text = ''
  buffer.thinking = ''
  queued = false
  snapshot = NOTHING
  for (const listener of listeners) listener()
}

export function subscribe(listener: () => void) {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

export function read() {
  return snapshot
}

/** The answer as it is being generated. One component reads this. */
export function useStreaming(): Streaming {
  return useSyncExternalStore(subscribe, read, read)
}
