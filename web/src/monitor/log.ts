import { create } from 'zustand'
import { invoke } from '@tauri-apps/api/core'

/**
 * The session log, as the monitor reads it.
 *
 * **It is not the source of truth, and it holds no copy of one.** The log is
 * (`src-tauri/crates/demido-trace`), and this is one more projection of it
 * beside the transcript: `events` is what `monitor_log` answered and it is
 * replaced by another read rather than appended to, exactly as
 * `web/src/chat/chat.ts` replaces the transcript. Two live copies of one
 * session is two things that can come to disagree about the run, and not
 * disagreeing is the whole product claim (`design/windows.md`).
 *
 * Both questions are **calls**. The monitor subscribes to nothing: the desk
 * already learns from `chat://update` that the log gained something, and the
 * panel re-reads when the transcript it is drawn beside changes.
 */

/** Who put this in front of the model: the eight `--src-*` tokens, and the
 * Rust `Source` with the same names. A ninth would be a ninth colour. */
export type Source =
  'system' | 'skill' | 'user' | 'inject' | 'reasoning' | 'tool' | 'artifact' | 'error'

/** What one event cost, and whether anybody counted it. The Rust `Weight`. */
export type Weight = { tokens: number; basis: 'counted' | 'estimated' }

/** Which tier of the settings ladder decided the offered set, or the registry
 * when nobody did. The Rust `Layer`. */
export type Layer = 'registry' | 'global' | 'model' | 'character' | 'chat'

/**
 * One line of the log, flat, exactly as it is on disk.
 *
 * The body's fields sit beside `seq` and `source` rather than nested under
 * them, because the raw JSON tab is read by a person and a line that says what
 * it is on the line is a line they can scan (`demido-trace`'s own
 * `a_line_names_its_event_kind_flat`). So this type is deliberately open: the
 * fields a kind carries are read where that kind is drawn, and a kind this
 * build has never heard of still renders as a row with a source, a weight and
 * its own JSON.
 */
export type Event = {
  seq: number
  at: number
  session: string
  /** Who produced it: `main` for the conversation, or one sub-agent of it. On
   * every line from the first commit that wrote one, which is what makes the
   * monitor's agent scope a filter over this stream rather than a retrofit
   * (the Rust `AgentId`). */
  agent: string
  turn: number
  source: Source
  weight: Weight
  event: string
} & Record<string, unknown>

/** How one block of an assembly stands against the assembly before it. The
 * Rust `Change`. */
export type Change = 'unchanged' | 'inserted' | 'evicted'

/** One block of a rebuilt assembly. The Rust `Placed`. */
export type Placed = {
  seq: number
  turn: number
  role: 'system' | 'user' | 'assistant' | 'tool'
  source: Source
  weight: Weight
  /** The text as it was sent: a paragraph refilled from the wording the log
   * stored, never a copy of what it produced. */
  text: string
  change: Change
}

/** One tool of an offered set, in the wording it was offered in. The Rust
 * `OfferedTool`. */
export type OfferedTool = { name: string; hash: string; text: string }

/** Why a group of the registry is or is not in an assembly. The Rust
 * `Standing`. `switched-off` and `dropped` are the pair that must never look
 * alike, `withheld` is Demido's own doing after a step that ran nothing,
 * `past-the-depth` is a sub-agent at the bottom of its chain, and `nothing` is
 * where the log cannot tell the first two apart. */
export type Standing =
  'offered' | 'partial' | 'switched-off' | 'dropped' | 'withheld' | 'past-the-depth' | 'nothing'

/** What one group came to in one assembly. The Rust `Grouped`. */
export type Grouped = {
  group: string
  offered: string[]
  absent: string[]
  standing: Standing
}

/** The assembly as it stood at one event. The Rust `Assembly`, which is the
 * log's `Rebuild` flattened with the registry's groups beside it. */
export type Assembly = {
  at: number
  seq: number
  turn: number
  previous: number | null
  model: string
  options: { temperature: number | null; maxTokens?: number; seed?: number }
  blocks: Placed[]
  tools: { seq: number; layer: Layer; tools: OfferedTool[] } | null
  groups: Grouped[]
}

type Monitor = {
  events: Event[]
  /** The event the inspector is showing, or nothing before the first read. */
  selected: number | null
  /** The assembly at `selected`, or null where there is none: a moment before
   * the first assembly was composed is a real state rather than an error. */
  assembly: Assembly | null
  /** Read the log again. Called when the panel opens and whenever the
   * conversation it is drawn beside records something. */
  read: () => Promise<void>
  /** Show one event. */
  select: (seq: number) => Promise<void>
}

export const useMonitor = create<Monitor>((set, get) => ({
  events: [],
  selected: null,
  assembly: null,

  read: async () => {
    const events = await invoke<Event[]>('monitor_log').catch((error: unknown) => {
      // The panel opens either way and says it has nothing, which is also what
      // an empty log looks like. What reaches here is the channel being absent,
      // which is the frontend running without the window around it.
      console.warn('the session log could not be read', error)
      return [] as Event[]
    })
    set({ events })

    // The monitor opens on the present moment rather than on nothing, and
    // follows it while a turn runs. A selection somebody made is left alone:
    // reading the log again must not move the thing they are reading.
    const selected = get().selected
    const last = events.at(-1)
    if (!last) return
    if (selected === null || !events.some((event) => event.seq === selected)) {
      await get().select(last.seq)
    }
  },

  select: async (seq) => {
    set({ selected: seq })
    const assembly = await invoke<Assembly | null>('monitor_assembly', { at: seq }).catch(
      (error: unknown) => {
        console.warn('the assembly at that moment could not be rebuilt', error)
        return null
      },
    )
    // Dropped if the selection moved while this was in flight, so a slow
    // rebuild cannot land on top of a newer one.
    if (get().selected === seq) set({ assembly })
  },
}))
