import { create } from 'zustand'

/**
 * What went wrong with something the person just did.
 *
 * **This is not the update vocabulary, and the distinction is the whole reason
 * it is allowed to exist.** `docs/rules/runtimes.md` section 7 and
 * `docs/rules/releases.md` section 2 refuse a toast for an available update,
 * and `docs/rules/lessons.md` refuses one for a lesson being written: those are
 * things Demido noticed on its own, nobody asked for them, and a notice that
 * dismisses into nothing is worse than a dot that survives being ignored.
 *
 * A refusal is the opposite case. Somebody typed a value a moment ago and it
 * was not taken, so there is a person waiting for an answer, the answer is only
 * about that gesture, and it stops being true as soon as they type something
 * else. That is exactly what a toast is for, and `design/system.md` carries the
 * row.
 *
 * It follows that nothing ambient may be routed here. If a thing has no gesture
 * behind it, it is a dot on the settings icon or a row on a page.
 */

/** How long a toast stays before it goes. Long enough to read a sentence twice,
 * because the person is reading it while still looking at the control that
 * refused, and short enough that it is gone before the next attempt. */
export const LINGERS_FOR = 6_000

export type Toast = {
  /** Increasing, and the React key. Two identical refusals are two toasts, so
   * a second attempt that fails the same way visibly happened. */
  id: number
  message: string
}

type Toasts = {
  toasts: Toast[]
  /** Say that something did not happen, and why. */
  show: (message: string) => void
  dismiss: (id: number) => void
}

let next = 0

export const useToasts = create<Toasts>((set, get) => ({
  toasts: [],

  show: (message) => {
    next += 1
    const id = next
    set({ toasts: [...get().toasts, { id, message }] })
    // The timer is here rather than in the component, so a toast's life does
    // not depend on which surface happened to be mounted when it was raised:
    // the settings window can be closed while its refusal is still on screen.
    setTimeout(() => get().dismiss(id), LINGERS_FOR)
  },

  dismiss: (id) => set({ toasts: get().toasts.filter((toast) => toast.id !== id) }),
}))
