import { create } from 'zustand'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

import { useChat, type Presence } from '@/chat/chat'
import { useToasts } from '@/shell/toasts'

/**
 * The guided set-up, as the window holds it.
 *
 * **It is not the source of truth.** `demido-setup` is, and what it says about
 * a step is derived from disk on every read (`docs/rules/setup.md` section 1),
 * so this store never edits its own copy: every gesture is a command, and what
 * the command returns replaces the whole view. That is the same rule the
 * settings ladder follows and it matters more here, because what is
 * outstanding is a fact about a folder rather than an opinion this window
 * holds.
 *
 * One thing crosses as an **event** rather than as a call: the bytes of a
 * fetch. A call is a question with one answer, and 515 MiB is not one answer.
 */

/** An accelerator, as Rust names it. */
export type Ecosystem = 'cuda' | 'rocm' | 'vulkan' | 'cpu'

/** The CUDA a driver runs. The Rust `CudaVersion`. */
export type CudaVersion = { major: number; minor: number }

/** Why an accelerator was pre-selected, as facts. The window writes the
 * sentence, which is `demido-hardware`'s rule. */
export type Reason =
  | { reason: 'cuda-driver'; adapter: string; driver: CudaVersion }
  | { reason: 'no-cuda-driver'; adapter: string }
  | { reason: 'adapter'; adapter: string; vendor: string }
  | { reason: 'no-adapter' }

/** What a row can offer, and what it says when it cannot. The Rust
 * `Availability`. */
export type Availability =
  | { availability: 'offered'; download_mib: number; on_disk_mib: number }
  | { availability: 'needs-cuda-driver'; runs: CudaVersion | null }
  | { availability: 'no-build-yet' }

export type AcceleratorRow = {
  ecosystem: Ecosystem
  availability: Availability
}

export type Accelerator = {
  rows: AcceleratorRow[]
  preselection: { ecosystem: Ecosystem; reason: Reason }
  chosen: Ecosystem
  overridden: boolean
}

/** What the runtimes ledger says a row is. The Rust `RowState`.
 *
 * Its fields are as Rust writes them, which is not this file's usual
 * camelCase: `RowState` is the shape of `runtimes.json`, a file a person can
 * open (`docs/rules/runtimes.md`), so its names are the ledger's rather than
 * the window's. The view around it is camelCase because it is a view. */
export type RowState =
  | { state: 'absent'; reason: string | null }
  | { state: 'managed'; pin: string; archives: string[]; on_disk_mib: number }
  | { state: 'linked'; path: string; detected_version: string | null }

export type ArchiveRow = {
  name: string
  pin: string
  downloadMib: number
  onDiskMib: number
  license: string
}

export type RuntimeRow = {
  id: string
  archives: ArchiveRow[]
  downloadMib: number
  onDiskMib: number
  ticked: boolean
  state: RowState
}

/** Which of the manifest's two groups a row is in. Data on the pin, which is
 * why this window has one renderer and not one per group. */
export type Group = 'required' | 'capability'

export type ManifestGroup = { group: Group; rows: RuntimeRow[] }

export type Model = { path: string; name: string; sizeMib: number; folder: string }

export type Models = {
  folders: string[]
  suggested: string[]
  models: Model[]
  chosen: string | null
}

/** One thing to settle. The Rust `Step`. */
export type Step = 'accelerator' | 'runtimes' | 'models' | 'first-answer'

export type Standing = 'done' | 'current' | 'later'

export type Planned = { step: Step; standing: Standing }

/** Everything a set-up surface draws. The Rust `View`. */
export type View = {
  plan: { steps: Planned[] }
  complete: boolean
  /** Whether the wizard has been closed, by leaving it or by finishing it. */
  closed: boolean
  accelerator: Accelerator
  manifest: ManifestGroup[]
  models: Models
}

/** What arrives on `setup://progress` while a fetch runs. */
type Fetching = { id: string; archive: string; bytes: number; total: number }

/** The shape a Rust command rejects with: `demido_core::Error`. */
type Failure = { kind: string; message: string }

type Setup = {
  /** The view, or nothing until the first read has answered. A surface draws
   * nothing rather than an empty wizard while it is in flight: an empty
   * wizard for one frame reads as a broken app. */
  view: View | null
  /** Which step the person is looking at. The plan says which is current; this
   * is which one they have opened, so a settled step can be revisited without
   * the plan being asked to lie about it. */
  step: Step | null
  /** True while a fetch or a link is in flight. */
  busy: boolean
  /** The archive being fetched and how far it has got, or nothing. */
  fetching: Fetching | null

  /** Read the view, and subscribe to the bytes. Called once, by the desk. */
  open: () => Promise<void>
  /** Read the view again. */
  read: () => Promise<void>
  show: (step: Step) => void
  choose: (ecosystem: Ecosystem) => Promise<void>
  tick: (id: string, on: boolean) => Promise<void>
  addFolder: (path: string) => Promise<void>
  removeFolder: (path: string) => Promise<void>
  chooseModel: (path: string) => Promise<void>
  fetch: () => Promise<void>
  link: (id: string, path: string) => Promise<void>
  leave: () => Promise<void>
  resume: () => Promise<void>
  /** Point the conversation at what the answers name, and start it. */
  finish: () => Promise<void>
}

export const useSetup = create<Setup>((set, get) => ({
  view: null,
  step: null,
  busy: false,
  fetching: null,

  open: async () => {
    await subscribe(set)
    await get().read()
    // The step the plan says is current, once, so opening the wizard lands on
    // the first thing outstanding rather than on the top of a list somebody
    // has already finished.
    if (!get().step) set({ step: current(get().view) })
  },

  read: async () => {
    try {
      set({ view: await invoke<View>('setup_state') })
    } catch (error) {
      // The desk stays usable. Set-up that cannot be read is a wizard nobody
      // can open, not a window that fails to draw: startup never blocks.
      console.warn('the set-up could not be read', error)
    }
  },

  show: (step) => set({ step }),

  choose: (ecosystem) =>
    gesture(set, () => invoke<View>('setup_choose_accelerator', { ecosystem })),

  tick: (id, on) => gesture(set, () => invoke<View>('setup_tick', { id, on })),

  addFolder: (path) => gesture(set, () => invoke<View>('setup_add_folder', { path })),

  removeFolder: (path) => gesture(set, () => invoke<View>('setup_remove_folder', { path })),

  chooseModel: (path) => gesture(set, () => invoke<View>('setup_choose_model', { path })),

  leave: () => gesture(set, () => invoke<View>('setup_leave')),

  resume: async () => {
    await gesture(set, () => invoke<View>('setup_resume'))
    set({ step: current(get().view) })
  },

  fetch: async () => {
    set({ busy: true })
    try {
      await gesture(set, () => invoke<View>('setup_fetch'))
    } finally {
      set({ busy: false, fetching: null })
    }
  },

  link: async (id, path) => {
    set({ busy: true })
    try {
      await gesture(set, () => invoke<View>('setup_link', { id, path }))
    } finally {
      set({ busy: false })
    }
  },

  finish: async () => {
    set({ busy: true })
    try {
      // The presence comes back because loading is minutes and the chat store
      // is what draws every state of it. It is handed over rather than kept
      // here: two stores holding one model's state is two stores that can
      // disagree about what is loaded.
      const presence = await invoke<Presence>('setup_finish')
      useChat.setState({ presence })
      await get().read()
    } catch (error) {
      useToasts.getState().show(sentence(error))
    } finally {
      set({ busy: false })
    }
  },
}))

/** One gesture: write it through, and take back whatever the whole view is
 * now. A refusal is a toast and the view is left exactly as it was, because a
 * refused gesture never reached the answers. */
async function gesture(
  set: (partial: Partial<Setup>) => void,
  write: () => Promise<View>,
): Promise<void> {
  try {
    set({ view: await write() })
  } catch (error) {
    useToasts.getState().show(sentence(error))
  }
}

/** The step the plan says is next, or nothing when set-up is finished. */
function current(view: View | null): Step | null {
  return view?.plan.steps.find((planned) => planned.standing === 'current')?.step ?? null
}

/** Subscribe to the bytes. Once per window: a second listener is a second copy
 * of every event. */
let subscribed: Promise<UnlistenFn> | null = null

async function subscribe(set: (partial: Partial<Setup>) => void): Promise<void> {
  if (subscribed) return
  subscribed = listen<Fetching>('setup://progress', (event) => set({ fetching: event.payload }))
  try {
    await subscribed
  } catch (error) {
    // Without the channel there is no progress bar and the fetch still runs.
    console.warn('the fetch will report no progress', error)
    subscribed = null
  }
}

/** The sentence a person can act on, out of whatever was thrown. */
function sentence(error: unknown): string {
  if (typeof error === 'object' && error !== null && 'message' in error) {
    return String((error as Failure).message)
  }
  return String(error)
}
