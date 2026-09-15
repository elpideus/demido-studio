import {
  ArrowDownToLine,
  Brain,
  CircleAlert,
  Component,
  Puzzle,
  ScrollText,
  User,
  Wrench,
  type LucideIcon,
} from 'lucide-react'

import type { Source, Weight } from './log'

/**
 * The eight sources, each with its icon.
 *
 * `design/windows.md`: "Colour-coded by source, and every row carries its
 * source's icon as well, **because colour is never the only signal**." The
 * colour is the `--src-*` token of the same name, applied in CSS off the row's
 * `data-source`, so this table holds the half that is not a colour and there is
 * no second list of eight anywhere.
 *
 * Eight and no more: the enum, the tokens and this are the same eight by
 * construction, and a ninth source would be a ninth colour, which is a change
 * to a frozen board rather than a change here.
 *
 * Every icon is Lucide's, named by its upstream identifier, because icons come
 * from a pack and are never drawn (`design/shell.md`).
 */
export const ICONS: Record<Source, LucideIcon> = {
  /** Text Demido wrote: a host paragraph, a tool document. */
  system: ScrollText,
  /** Text a skill brought with it. */
  skill: Puzzle,
  /** What the person typed. */
  user: User,
  /** Something Demido put in the window without being asked. */
  inject: ArrowDownToLine,
  /** What the model produced, its thinking as well as its reply. */
  reasoning: Brain,
  /** A call, and what came back from it. */
  tool: Wrench,
  /** An artifact's content. */
  artifact: Component,
  /** A failure, whether or not the model was shown it. */
  error: CircleAlert,
}

/** A token count as a row shows it: exact while it is small, thousands once it
 * stops being worth reading digit by digit. */
export function tokens(count: number): string {
  return count < 1000 ? `${count}` : `${(count / 1000).toFixed(1)}k`
}

/**
 * A weight as anything in this window shows it.
 *
 * The tilde is the whole of the distinction `demido-trace` records on every
 * event: an occupancy built out of guesses and one built out of the backend's
 * own numbers look identical otherwise, and this is the window that says which
 * it is showing. One function, so a stream row and a rebuilt block cannot come
 * to mark an estimate differently.
 */
export function weighed(weight: Weight): string {
  return `${weight.basis === 'estimated' ? '~' : ''}${tokens(weight.tokens)}`
}

/** A digest at the length a person tells two of them apart by. The whole of it
 * is one tab away, in the record itself. */
export function digest(hash: string): string {
  return hash.replace(/^sha256:/, '').slice(0, 12)
}
