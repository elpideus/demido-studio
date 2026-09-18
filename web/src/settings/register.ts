import { create } from 'zustand'
import { invoke } from '@tauri-apps/api/core'

import { refused, sentence } from '@/shell/failure'
import { useToasts } from '@/shell/toasts'

/**
 * The paragraph register, as the editor holds it.
 *
 * **It is not the source of truth.** `demido-prompts` is, and it deliberately
 * keeps no loaded state of its own: every call reads the prompts directory
 * again, so a paragraph edited here is what the next turn sends with nothing to
 * invalidate in between (`src-tauri/crates/demido-prompts/src/register.rs`).
 * This store follows the rule `web/src/settings/ladder.ts` already follows for
 * the same reason: a change is written through and then read back, never edited
 * in place, because two live copies of a prompt are two things that can come to
 * disagree about what a model was shown.
 *
 * Both registers, as two stores over one write path: `usePrompts` for the
 * paragraphs and `useToolDocuments` for the tool documents
 * ([#77](https://github.com/elpideus/demido-studio/issues/77)). Two stores
 * because the two are keyed differently, an id and a tool name, and one list
 * holding both would make a key mean two things, which is the reason Rust keeps
 * them in two registers.
 */

/** Where the text in force came from. The Rust `Origin`. */
export type Origin = 'built-in' | 'edited'

/** What kind of promise a dependant is. The Rust `Dependency`, externally
 * tagged: a measurement names the file holding the pinned digest, a live run
 * names the suite that drove it, and a shared wording names nothing because
 * there is nothing to follow. */
export type Dependency =
  'shared' | { measured: { pinned_in: string } } | { driven: { suite: string } }

/** Something that depends on one paragraph's exact wording. The Rust
 * `Dependant`. This is what replaces a read-only flag: the sentence is rendered
 * above the field, and then the edit is allowed. */
export type Dependant = { note: string; kind: Dependency }

/** One entry's declaration. The Rust `Paragraph`. `default` is the text this
 * build ships, which is what a diff is taken against and what reverting
 * restores. */
export type Paragraph = {
  id: string
  title: string
  summary: string
  placeholders: string[]
  dependants: Dependant[]
  default: string
}

/** One paragraph as it stands right now. The Rust `Prompt`, whole: the editor
 * renders this and computes none of it. */
export type Prompt = {
  paragraph: Paragraph
  /** The text in force, placeholders still standing. */
  text: string
  origin: Origin
  hash: string
  /** The same, for the wording this build ships: the identity of what a reset
   * would restore. Equal to `hash` while nobody has edited the entry. */
  shipped: string
  /** For an edit, the hash of the built-in wording it was made from. */
  base: string | null
  /** The measured claims this text no longer supports. Empty until it is
   * edited: an edit suppresses the claim rather than being refused. */
  suppressed: Dependant[]
  /** An edited file that could not be read, or a built-in wording that has
   * changed since the edit was made. Never blocks anything. */
  note: string | null
}

/**
 * Whether this build's shipped wording has moved since the edit was made.
 *
 * The condition the diff is drawn for, and it is the same comparison Rust makes
 * to decide whether to write the note: the wording this edit was made from
 * against the wording this build ships. Not read off the note, which is a
 * shared channel that also carries an edit that could not be read, and a diff
 * drawn from a sentence is a diff that changes when somebody rewords one.
 */
export function outdated(entry: { base: string | null; shipped: string }): boolean {
  return entry.base !== null && entry.base !== entry.shipped
}

/** One host tool's declaration. The Rust `ToolEntry`. `parameters` are the
 * names the document may give prose to, in schema order; `default` is the
 * whole document this build ships. */
export type ToolEntry = {
  name: string
  title: string
  summary: string
  parameters: string[]
  dependants: Dependant[]
  default: string
}

/** A tool's schema with no prose on it: the contract with the parser, drawn
 * beside the document and never edited. Only the part the editor reads. */
export type Shape = {
  properties?: Record<string, { type?: string | string[] }>
  required?: string[]
}

/** One tool's document as it stands right now, beside its shape. The Rust
 * `Document`, whole, with the shape the command joins on. */
export type ToolDocument = {
  tool: ToolEntry
  /** The whole document: the description, then one `## <parameter>` section
   * per parameter. Edited as one text and hashed as one. */
  text: string
  origin: Origin
  hash: string
  shipped: string
  base: string | null
  suppressed: Dependant[]
  note: string | null
  /** `null` only for a document whose tool this build did not register. */
  shape: Shape | null
}

type Prompts = {
  /** Every entry, in register order, or nothing while the first read is in
   * flight. Absent rather than empty, so a page can tell "still reading" from
   * "no prompts", which are different screens. */
  entries: Prompt[] | null
  read: () => Promise<void>
  /** Take an edit, and say whether it was taken. A field told no puts the saved
   * text back, so the editor never shows a wording the register does not have. */
  set: (id: string, text: string) => Promise<boolean>
  /** Forget the edit. The built-in text comes back, and the claims it supports
   * come back with it. */
  reset: (id: string) => Promise<boolean>
}

export const usePrompts = create<Prompts>((set, get) => ({
  entries: null,

  read: async () => {
    try {
      set({ entries: await invoke<Prompt[]>('prompts_list') })
    } catch (error) {
      // The desk stays usable. A register that cannot be read has already
      // fallen back to the shipped defaults in Rust, so what reaches here is
      // the channel being absent, which is the frontend without the window
      // around it.
      console.warn('the prompt register could not be read', error)
      useToasts.getState().show(sentence(error))
    }
  },

  set: async (id, text) => write(() => invoke('prompts_set', { id, text }), get().read, refusal),

  reset: async (id) => write(() => invoke('prompts_reset', { id }), get().read, refusal),
}))

type ToolDocuments = {
  /** Every document, in register order, or nothing while the first read is in
   * flight, for the reason `Prompts.entries` gives. */
  entries: ToolDocument[] | null
  read: () => Promise<void>
  set: (name: string, text: string) => Promise<boolean>
  reset: (name: string) => Promise<boolean>
}

export const useToolDocuments = create<ToolDocuments>((set, get) => ({
  entries: null,

  read: async () => {
    try {
      set({ entries: await invoke<ToolDocument[]>('tool_documents_list') })
    } catch (error) {
      console.warn('the tool register could not be read', error)
      useToasts.getState().show(sentence(error))
    }
  },

  set: async (name, text) =>
    write(() => invoke('tool_documents_set', { name, text }), get().read, toolRefusal),

  reset: async (name) =>
    write(() => invoke('tool_documents_reset', { name }), get().read, toolRefusal),
}))

/**
 * Write a change through and read the whole register back.
 *
 * The whole register rather than the one entry that changed, and the answer the
 * command already handed back is thrown away. Two reasons, and both are about
 * this register in particular: an edit whose text is the default resets instead
 * of writing a file, so the entry that comes back is not always the one that was
 * asked for, and the note an entry carries is a fact about a directory rather
 * than about a string. One read of the directory is the same call the next turn
 * makes.
 *
 * A refusal is a toast rather than a line under the field
 * (`web/src/shell/toasts.ts`): it answers a gesture somebody just made, and the
 * sentence for it is written here because the one that crossed the boundary
 * names an id.
 */
async function write(
  call: () => Promise<unknown>,
  read: () => Promise<void>,
  refusal: (error: unknown) => string,
): Promise<boolean> {
  try {
    await call()
  } catch (error) {
    // Nothing is read back: a refused edit never reached the directory, so
    // every entry on screen is already correct except the field that was typed
    // into, and saying no is what puts that one back.
    useToasts.getState().show(refusal(error))
    return false
  }

  await read()
  return true
}

/**
 * What to tell the person, in this application's own words.
 *
 * There is exactly one refusal a person can provoke here, and it is not a
 * judgement about the wording: nothing is read-only, and an entry that is
 * load-bearing declares its dependants instead (`docs/rules/prompts.md`). What
 * Rust refuses is a placeholder the declaration does not carry, because nothing
 * would ever fill it and the model would be handed a literal pair of braces.
 *
 * Anything that is not a refusal is a disk or a channel, and there the sentence
 * Rust wrote is the only one that says what happened.
 */
function refusal(error: unknown): string {
  if (!refused(error)) return sentence(error)
  return 'That names something nothing will fill in. Only the placeholders listed under the field are replaced.'
}

/**
 * The same, for a tool document. Two refusals rather than one, and neither is a
 * judgement about the wording either: a section for a parameter the shape does
 * not have would describe nothing, and a tool document fills in no
 * placeholders.
 */
function toolRefusal(error: unknown): string {
  if (!refused(error)) return sentence(error)
  return 'A tool document can only give prose to the parameters in its shape, and fills in no placeholders.'
}
