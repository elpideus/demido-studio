import { create } from 'zustand'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

import { append, clear } from './stream'

/**
 * The conversation, as the window holds it.
 *
 * **It is not the source of truth.** The session log is
 * (`src-tauri/crates/demido-trace`), and everything here is a projection of it:
 * `transcript` is what `chat_transcript` answered, and it is replaced by
 * another read of the log at the end of every turn rather than edited. See
 * `docs/decisions/0011-the-window-draws-the-log-not-its-draft.md`.
 *
 * Two things cross the boundary and they cross it differently. The transcript,
 * the presence and the stop are **calls**, because each is a question with one
 * answer. The tokens are **events**, because they keep happening; they do not
 * come through here at all, but through `stream.ts`, which is a ref rather than
 * state (#34: never `setState` per token).
 */

/** One thing said, as the log recorded it. The Rust `Said`. */
export type Said = {
  seq: number
  turn: number
  role: 'system' | 'user' | 'assistant'
  text: string
}

/** Whether there is anything to talk to. The Rust `Presence`, tagged. */
export type Presence =
  | { state: 'absent' }
  | { state: 'loading'; model: string }
  | { state: 'ready'; model: string }
  | { state: 'failed'; detail: string }

/** What arrives on `chat://update` while a turn runs. The Rust `Update`.
 *
 * The window acts on the first two. `done` and `failed` are on the channel
 * because a turn's whole life belongs on one channel and the Session Monitor
 * will read it, and this window learns a turn is over from `chat_send`
 * resolving instead: one completion signal, which is also the one that carries
 * the failure. */
type Update =
  | { update: 'text'; text: string }
  | { update: 'thinking'; text: string }
  | { update: 'done' }
  | { update: 'failed' }

/** The shape a Rust command rejects with: `demido_core::Error`. */
type Failure = { kind: string; message: string }

type Chat = {
  presence: Presence
  transcript: Said[]
  /** The message that was just sent, drawn while its turn runs. It has no
   * sequence number yet, because it is not on the log until the turn begins,
   * and it is replaced by the log's own copy when the turn ends. */
  pending: string | null
  /** True from the moment a message is sent until the turn is over, however it
   * ended. The send button is a stop button for exactly this long. */
  running: boolean
  /** What the last turn failed with, cleared when the next one starts. */
  failure: string | null
  /** False until the log and the presence have been read once. */
  hydrated: boolean

  /** Subscribe, read the log, and start the model. Called once, by the desk. */
  open: () => Promise<void>
  /** Start the model again after it failed. */
  load: () => void
  send: (message: string) => Promise<void>
  stop: () => void
}

const START: Pick<
  Chat,
  'presence' | 'transcript' | 'pending' | 'running' | 'failure' | 'hydrated'
> = {
  presence: { state: 'absent' },
  transcript: [],
  pending: null,
  running: false,
  failure: null,
  hydrated: false,
}

/**
 * Whether `open` has already been entered, checked before anything is awaited.
 *
 * `hydrated` cannot do this job. It is set after the first `await`, and React's
 * StrictMode runs an effect twice on mount, so the second call arrives while
 * the first is still in flight and sees `hydrated` false. It then subscribes a
 * second listener to `chat://update` and every token is appended twice, which
 * is what a doubled answer in the bubble looked like.
 */
let opening = false

export const useChat = create<Chat>((set, get) => ({
  ...START,

  open: async () => {
    if (opening) return
    opening = true

    try {
      // Subscribed before anything is read, so a token generated between the
      // read and the subscription cannot be the one that is missed.
      await listen<Update>('chat://update', ({ payload }) => {
        if (payload.update === 'text') append({ text: payload.text })
        else if (payload.update === 'thinking') append({ thinking: payload.text })
      })
      await listen<Presence>('chat://presence', ({ payload }) => set({ presence: payload }))

      const [transcript, presence] = await Promise.all([
        invoke<Said[]>('chat_transcript'),
        invoke<Presence>('chat_presence'),
      ])
      set({ transcript, presence, hydrated: true })
    } catch (error) {
      // The desk opens either way: startup never blocks (`AGENTS.md`). What
      // reaches here is the channel being absent, which is the frontend running
      // without the window around it, or a log this build cannot read, which is
      // a conversation that cannot be shown and not a reason to show nothing at
      // all.
      console.warn('the conversation could not be read', error)
      set({ ...START, hydrated: true })
    }

    // Starting the model is minutes and the desk is drawn now. Every state it
    // passes through arrives on `chat://presence`, which is why this is not
    // awaited.
    get().load()
  },

  load: () => {
    // The same call the desk makes at startup, so trying again after a failure
    // is the first attempt repeated rather than a recovery path of its own.
    // Without it a backend that crashed leaves a composer that can never be
    // enabled again, and "the desk stays usable" would mean "until you restart
    // the app".
    if (get().presence.state === 'loading') return
    void invoke<Presence>('chat_load')
      .then((presence) => set({ presence }))
      .catch((error: unknown) => console.warn('the model could not be started', error))
  },

  send: async (message) => {
    const said = message.trim()
    if (!said || get().running || get().presence.state !== 'ready') return

    clear()
    set({ pending: said, running: true, failure: null })

    try {
      // Resolves when the turn is over, however it ended. One completion
      // signal, which is also the one that carries the failure.
      await invoke('chat_send', { message: said })
    } catch (error) {
      set({ failure: sentence(error) })
    } finally {
      // The draft is discarded and the transcript is read back from the log,
      // which is where the answer actually is. A stopped turn comes back with
      // its partial answer in it, because that is what was recorded.
      clear()
      const transcript = await invoke<Said[]>('chat_transcript').catch(() => get().transcript)
      set({ transcript, pending: null, running: false })
    }
  },

  stop: () => {
    if (!get().running) return
    // Nothing is set here. The stop reaches the generation, the stream ends
    // with what it had, and the turn finishes down the same path a completed
    // one takes.
    void invoke<boolean>('chat_stop').catch((error: unknown) => {
      console.warn('the generation was not stopped', error)
    })
  },
}))

/** The sentence a person can act on, out of whatever was thrown. */
function sentence(error: unknown): string {
  if (typeof error === 'object' && error !== null && 'message' in error) {
    return String((error as Failure).message)
  }
  return String(error)
}
