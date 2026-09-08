import { create } from 'zustand'
import { invoke } from '@tauri-apps/api/core'

import { useChat } from '@/chat/chat'

/**
 * The settings ladder, as the window holds it.
 *
 * **It is not the source of truth.** `demido-settings` is: every row here came
 * from `settings_rows`, and a change is written through `settings_set` and then
 * read back rather than edited in place. That is the same rule the transcript
 * follows (`docs/decisions/0011-the-window-draws-the-log-not-its-draft.md`),
 * and it is what keeps the value on screen the value a turn would actually
 * send.
 *
 * Two tiers are live, global and chat, and the chat is the last word
 * (`docs/decisions/0007-a-chat-outranks-its-character.md`). Rust knows which
 * conversation the chat tier belongs to; this store names a tier and never a
 * subject.
 */

/** The tiers a surface here can edit. Rust has four and stores four. */
export type Tier = 'global' | 'chat'

/** Where a value came from. The Rust `Origin`, as its slug. */
export type Origin = 'default' | 'global' | 'model' | 'character' | 'chat'

/** What a setting accepts, and what it is when nobody has said otherwise. The
 * Rust `Kind`, tagged by its control. */
export type Kind =
  | { control: 'amount'; default: number; min: number; max: number; on: boolean }
  | { control: 'count'; default: number; min: number; max: number }
  | { control: 'text'; default: string; multiline: boolean }

/** One setting's declaration. The Rust `Setting`. */
export type Setting = {
  id: string
  section: string
  title: string
  summary: string
  kind: Kind
  /** True when changing it means the model has to be started again. */
  reloads: boolean
}

/** One line of a settings page. The Rust `Row`. */
export type Row = {
  setting: Setting
  /** What is in force, after every tier. */
  value: unknown
  from: Origin
  /** Whether the tier being edited is one of the ones with an opinion. */
  setHere: boolean
  /** What reverting this tier would leave behind. */
  inherited: unknown
  inheritedFrom: Origin
}

/** The shape a Rust command rejects with: `demido_core::Error`. */
type Failure = { kind: string; message: string }

type Settings = {
  /** The rows of each tier that has been asked for. A tier nobody has opened
   * is absent rather than empty, so a page can tell "still reading" from "no
   * settings", which are different screens. */
  rows: Partial<Record<Tier, Row[]>>
  /** Why the last change on a tier was refused, or nothing. A settings page
   * that quietly reverted would be indistinguishable from one that never
   * saved.
   *
   * Per tier rather than one field, because the two surfaces are open at the
   * same time: a refusal in the chat popover printed under the global window's
   * rows would name a row that surface does not have. */
  failures: Partial<Record<Tier, string>>

  /** Read a tier's rows. Called when a surface for it opens. */
  read: (tier: Tier) => Promise<void>
  set: (tier: Tier, setting: Setting, value: unknown) => Promise<void>
  /** Forget this tier's opinion, so the value below it applies again. */
  clear: (tier: Tier, setting: Setting) => Promise<void>
}

export const useSettings = create<Settings>((set, get) => ({
  rows: {},
  failures: {},

  read: async (tier) => {
    try {
      const rows = await invoke<Row[]>('settings_rows', { tier })
      set({ rows: { ...get().rows, [tier]: rows } })
    } catch (error) {
      // The desk stays usable. A ladder that cannot be read has already fallen
      // back to its defaults in Rust, so what reaches here is the channel being
      // absent, which is the frontend running without the window around it.
      console.warn('the settings could not be read', error)
      set({ failures: { ...get().failures, [tier]: sentence(error) } })
    }
  },

  set: async (tier, setting, value) => {
    await change(
      tier,
      setting,
      () => invoke('settings_set', { tier, id: setting.id, value }),
      set,
      get,
    )
  },

  clear: async (tier, setting) => {
    await change(tier, setting, () => invoke('settings_clear', { tier, id: setting.id }), set, get)
  },
}))

/**
 * Write a change through, read every open tier back, and start the model again
 * when the change was one the server is started with.
 *
 * Both tiers are re-read rather than only the one that changed, because they
 * are two views of one ladder: a global value that a chat is not overriding is
 * what that chat's page draws, so a change to either can move a row on both.
 *
 * The reload is the honest half. A context length is a flag on the process
 * (`Setting::reloads`), so a number that was accepted but never reached a
 * server would be a settings page that lies about what the model is doing. It
 * is the same call the desk makes at startup, and the composer already draws
 * every state it passes through.
 */
async function change(
  tier: Tier,
  setting: Setting,
  write: () => Promise<unknown>,
  set: (partial: Partial<Settings>) => void,
  get: () => Settings,
) {
  try {
    await write()
    set({ failures: { ...get().failures, [tier]: undefined } })
  } catch (error) {
    set({ failures: { ...get().failures, [tier]: sentence(error) } })
    return
  }

  await Promise.all((Object.keys(get().rows) as Tier[]).map((open) => get().read(open)))

  if (setting.reloads && useChat.getState().presence.state !== 'absent') {
    useChat.getState().load()
  }
}

/** The sentence a person can act on, out of whatever was thrown. */
function sentence(error: unknown): string {
  if (typeof error === 'object' && error !== null && 'message' in error) {
    return String((error as Failure).message)
  }
  return String(error)
}
