import { create } from 'zustand'
import { invoke } from '@tauri-apps/api/core'

/**
 * The one store the shell has.
 *
 * Window state lives here and nowhere else: `design/shell.md` gives the rail
 * four states that report what is open, and the seam, the pins and the float
 * layer all read the same arrangement. Two stores over one desk is two stores
 * that can disagree about what is open, and the rail is the only place in the
 * UI that answers that question.
 *
 * **It is not the source of truth.** Rust is
 * (`docs/decisions/0010-the-desk-remembers-itself.md`): this is the desk as it
 * is being drawn, hydrated from `read_layout` once and reported back through
 * `remember_layout` on every change. v2 kept the layout in `localStorage`,
 * which is scoped to an origin rather than to a person, and multi-account makes
 * that wrong.
 */

/** Which edge the rail is docked to. The Rust `Side`, and the only two values
 * it has. */
export type Side = 'left' | 'right'

/** What `read_layout` returns and `remember_layout` takes: the Rust `Shell`,
 * camel-cased. The generation number is deliberately not in it, so the rule
 * that discards an old layout has exactly one implementation and it is not
 * this one. */
export type Shell = {
  rail: Side
}

/** The desk a profile that has never arranged one gets. It matches Rust's
 * `Shell::default`, and it is what the window draws while the real one is
 * still being read, so the first frame is never blank. */
const DEFAULT: Shell = { rail: 'left' }

type Desk = Shell & {
  /** False until Rust has answered. Nothing is reported back before it is
   * true: writing during hydration would save the default over the layout
   * that is still being read. */
  hydrated: boolean
  /** Read the remembered layout once, at startup. */
  hydrate: () => Promise<void>
  /** Move the rail to an edge. */
  dock: (side: Side) => void
}

export const useDesk = create<Desk>((set, get) => ({
  ...DEFAULT,
  hydrated: false,

  hydrate: async () => {
    if (get().hydrated) return
    try {
      const remembered = await invoke<Shell>('read_layout')
      set({ ...remembered, hydrated: true })
    } catch (error) {
      // The desk opens either way. A layout that will not load is already
      // discarded in Rust and never reaches here; what does reach here is the
      // channel itself being absent, which is the frontend running without the
      // window around it. Startup never blocks (`AGENTS.md`).
      console.warn('the remembered layout could not be read; drawing the default desk', error)
      set({ ...DEFAULT, hydrated: true })
    }
  },

  dock: (side) => {
    if (get().rail === side) return
    set({ rail: side })
    remember(get())
  },
}))

/**
 * Report the desk to Rust, which decides when that becomes a file.
 *
 * Every change goes through here, including the ones in the middle of a
 * gesture. Debouncing is the store's job on the other side of the boundary
 * (`demido-shell::Debounced`), so nothing in the frontend has to know how often
 * it may speak, and a failure to save is not something the desk stops for.
 */
function remember(desk: Desk) {
  if (!desk.hydrated) return
  const shell: Shell = { rail: desk.rail }
  invoke('remember_layout', { shell }).catch((error: unknown) => {
    console.warn('the desk arrangement was not saved', error)
  })
}
