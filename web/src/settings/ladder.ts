import { create } from 'zustand'
import { invoke } from '@tauri-apps/api/core'

import { useChat } from '@/chat/chat'
import { sentence, type Failure } from '@/shell/failure'
import { useToasts } from '@/shell/toasts'

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

type Settings = {
  /** The rows of each tier that has been asked for. A tier nobody has opened
   * is absent rather than empty, so a page can tell "still reading" from "no
   * settings", which are different screens. */
  rows: Partial<Record<Tier, Row[]>>
  /** Read a tier's rows. Called when a surface for it opens. */
  read: (tier: Tier) => Promise<void>
  /** Take a value, and say whether it was taken. A control that is told no puts
   * the saved value back, so the field never shows a number the ladder does not
   * have. */
  set: (tier: Tier, setting: Setting, value: unknown) => Promise<boolean>
  /** Forget this tier's opinion, so the value below it applies again. */
  clear: (tier: Tier, setting: Setting) => Promise<boolean>
}

export const useSettings = create<Settings>((set, get) => ({
  rows: {},

  read: async (tier) => {
    try {
      const rows = await invoke<Row[]>('settings_rows', { tier })
      set({ rows: { ...get().rows, [tier]: rows } })
    } catch (error) {
      // The desk stays usable. A ladder that cannot be read has already fallen
      // back to its defaults in Rust, so what reaches here is the channel being
      // absent, which is the frontend running without the window around it.
      console.warn('the settings could not be read', error)
      useToasts.getState().show(sentence(error))
    }
  },

  set: async (tier, setting, value) =>
    change(setting, () => invoke('settings_set', { tier, id: setting.id, value }), get),

  clear: async (tier, setting) =>
    change(setting, () => invoke('settings_clear', { tier, id: setting.id }), get),
}))

/**
 * Write a change through, read every open tier back, and start the model again
 * when the change was one the server is started with.
 *
 * A refusal is a toast rather than a line under the row (`web/src/shell/toasts.ts`):
 * it answers a gesture somebody just made, it is about that gesture alone, and
 * the two settings surfaces can both be open, so a sentence printed under one
 * page's rows would appear under the other's as well.
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
  setting: Setting,
  write: () => Promise<unknown>,
  get: () => Settings,
): Promise<boolean> {
  try {
    await write()
  } catch (error) {
    // Nothing is read back and nothing is redrawn: a refused value never
    // reached the ladder, so every row on screen is already correct except the
    // field that was typed into, and saying no is what puts that one back.
    useToasts.getState().show(refusal(setting, error))
    return false
  }

  await Promise.all((Object.keys(get().rows) as Tier[]).map((open) => get().read(open)))

  if (setting.reloads && useChat.getState().presence.state !== 'absent') {
    useChat.getState().load()
  }

  return true
}

/**
 * What to tell the person, in this application's own words.
 *
 * Rust carries the fact and the window writes the sentence, which is the rule
 * `Presence` already follows: what crossed the boundary here is "invalid a
 * setting: conversation.context_length: outside 512 to 262144", and nobody
 * should have to read an id or the word invalid. Everything needed to say it
 * properly is in the declaration the row was drawn from.
 *
 * Anything that is not a refusal is the other kind of failure, a disk or a
 * channel, and there the sentence Rust wrote is the only one that says what
 * happened.
 */
function refusal(setting: Setting, error: unknown): string {
  if (!isRefusal(error)) return sentence(error)

  switch (setting.kind.control) {
    case 'count':
      return `${setting.title} is a whole number from ${setting.kind.min} to ${setting.kind.max}.`
    case 'amount':
      return `${setting.title} is a number from ${setting.kind.min} to ${setting.kind.max}, or off.`
    case 'text':
      return `${setting.title} was not saved.`
  }
}

/** Whether the failure is the value being wrong rather than the saving of it.
 * The tag is `demido_core::Error::kind`, which exists so a frontend branches on
 * a tag instead of on the wording of a sentence. */
function isRefusal(error: unknown): boolean {
  return typeof error === 'object' && error !== null && (error as Failure).kind === 'invalid'
}
