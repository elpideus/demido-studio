import { create } from 'zustand'
import { invoke } from '@tauri-apps/api/core'

/**
 * The one call waiting on the person, if there is one.
 *
 * **The approval is a row in the transcript, not a dialog**
 * (`design/shell.md`), so this is a small piece of state the transcript draws
 * from rather than anything that opens over the desk. It is separate from
 * `chat.ts` because it has a different life: a conversation is a projection of
 * the log, and this is a question the turn loop is holding open right now. It is
 * on nothing until it is answered.
 *
 * Only one call waits at a time, because the loop dispatches calls in order. It
 * is still keyed by the call's position on the log when it is answered, since
 * "one at a time" is the loop's property rather than this store's, and a
 * decision must reach the call it was made about.
 */

/** What a call declares it will do. The Rust `Ability`, as its slug. */
export type Ability = 'read' | 'write' | 'shell' | 'network'

/** What the person answers. The Rust `Decision`. */
export type Decision = 'allow' | 'deny' | 'always'

/** One call waiting on a person. The Rust `Asking`. */
export type Asking = {
  turn: number
  /** Where the call is on the log, which is what the decision is recorded
   * against. */
  call: number
  /** What the backend called the call. */
  id: string
  tool: string
  ability: Ability
  /** The tool's one line about what this call will do. */
  summary: string
  /** Asked about in every mode, and not something *always* can cover. */
  destructive: boolean
  /** The arguments as they will run: already parsed and checked against the
   * tool's schema. */
  arguments: unknown
}

type Approval = {
  asking: Asking | null
  /** A call is waiting. Called by the `chat://asking` subscription in
   * `chat.ts`, which is the one place this window subscribes to anything. */
  waiting: (asking: Asking) => void
  /** Answer it. The row goes as soon as the answer is sent, because what
   * replaces it is the call row, which the log is about to carry. */
  decide: (decision: Decision) => void
  /** Nothing is waiting any more: the turn ended, or was stopped. */
  settled: () => void
}

export const useApprovals = create<Approval>((set, get) => ({
  asking: null,

  waiting: (asking) => set({ asking }),

  decide: (decision) => {
    const asking = get().asking
    if (!asking) return
    set({ asking: null })
    void invoke<boolean>('chat_decide', { call: asking.call, decision }).catch((error: unknown) => {
      // Nothing is put back. The turn has either taken the answer or stopped
      // waiting for one, and a row that reappeared would be asking a question
      // that no longer has anywhere to go.
      console.warn('the decision did not reach the call', error)
    })
  },

  settled: () => {
    if (get().asking) set({ asking: null })
  },
}))
