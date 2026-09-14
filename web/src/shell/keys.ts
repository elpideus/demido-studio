import { useEffect, useRef } from 'react'

/**
 * Keyboard shortcuts, and the one property that makes them safe to have.
 *
 * `design/windows.md` gives every binding a **scope**, and says why in one
 * sentence: "Scoping is what lets `Enter` approve a tool call without stealing
 * `Enter` from the composer." That is exactly the case this module exists for,
 * and it is [#55](https://github.com/elpideus/demido-studio/issues/55)'s: an
 * approval waiting somewhere on the desk must not take the key the composer
 * uses most.
 *
 * Two rules do it, and neither is a special case for the approval prompt.
 *
 * **Global beats panel beats desk.** One chord can be bound in more than one
 * scope, and the narrowest live binding wins rather than all of them running.
 * `design/windows.md` again: two commands may share a chord in different scopes
 * and that is not a conflict.
 *
 * **A field keeps the chords it actually uses.** This is the half that answers
 * the composer. Its Enter is the field's own behaviour rather than a binding,
 * so a scope order alone would not have saved it: both would fire, the message
 * would send *and* the call would be approved, from one keystroke.
 *
 * Which chords those are is [`typed`], and it is a rule rather than a list of
 * exceptions: **a bare key with no Ctrl, Alt or Meta is text, and everything
 * else is a shortcut.** Escape is the one bare key carved out of it, because no
 * text field in this application does anything with Escape and every surface
 * that can be dismissed already listens for it. Without that carve-out the
 * approval row's Escape would be unreachable in practice: focus sits in the
 * composer, so denying would mean clicking somewhere else first, which is the
 * opposite of the point of binding a key to it.
 *
 * A chord somebody else has already acted on is left alone, so a popover that
 * closes itself on Escape is not also a denial.
 *
 * The bindings are not yet user-editable. The keymap editor
 * (`design/windows.md`'s Keys section) is its own work, and what it will edit is
 * which chord reaches which command rather than this. What lands here now is the
 * mechanism that editor needs to exist first.
 */

/** Where a binding listens. Narrowest last, which is the order they are beaten
 * in: global beats panel beats desk. */
export type Scope = 'global' | 'panel' | 'desk'

const ORDER: Scope[] = ['global', 'panel', 'desk']

type Binding = {
  scope: Scope
  chord: string
  run: (event: KeyboardEvent) => void
}

const bound = new Set<Binding>()
let listening = false

/**
 * The chord an event is, written the way a binding names it.
 *
 * Modifiers in a fixed order, so `Ctrl+Shift+K` is one string however it was
 * typed. The key itself is the browser's own name for it, which is what a
 * keymap editor will capture and store.
 */
function chordOf(event: KeyboardEvent): string {
  const parts: string[] = []
  if (event.ctrlKey) parts.push('Ctrl')
  if (event.altKey) parts.push('Alt')
  if (event.shiftKey) parts.push('Shift')
  if (event.metaKey) parts.push('Meta')
  parts.push(event.key)
  return parts.join('+')
}

/** Whether the keystroke is going into something a person is typing in. A
 * `contenteditable` counts, because the artifact editor and the prompt editor
 * will both be one. */
function editable(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false
  if (target.isContentEditable) return true
  return ['INPUT', 'TEXTAREA', 'SELECT'].includes(target.tagName)
}

/**
 * Whether this chord is the field's rather than the application's.
 *
 * A bare key is text, or caret movement, or the Enter that submits it, and a
 * field owns all of those. A key with Ctrl, Alt or Meta on it is not text in
 * any field, so it stays a shortcut wherever it is pressed.
 *
 * Escape is the carve-out, and there is exactly one. No field here does
 * anything with it, every dismissable surface already listens for it, and the
 * approval row's Deny would otherwise be a key nobody can reach without
 * clicking somewhere else first.
 */
function typed(event: KeyboardEvent): boolean {
  if (event.key === 'Escape') return false
  return !event.ctrlKey && !event.altKey && !event.metaKey
}

function handle(event: KeyboardEvent) {
  // Somebody already acted on it. A popover that closes on Escape has taken
  // that key; a second handler treating the same press as a decision would be
  // one keystroke doing two things.
  if (event.defaultPrevented) return

  const chord = chordOf(event)
  const scoped = editable(event.target) && typed(event) ? (['global'] as Scope[]) : ORDER
  for (const scope of scoped) {
    const binding = [...bound].find((it) => it.scope === scope && it.chord === chord)
    if (!binding) continue
    // Claimed before it runs, so anything downstream sees it as taken.
    event.preventDefault()
    binding.run(event)
    return
  }
}

/** Listen, and bind a chord until the returned function is called. */
function bind(binding: Binding): () => void {
  if (!listening) {
    // On the document rather than on any element: a binding at `desk` or
    // `panel` scope is about what is open, not about what has focus, and a
    // handler that waited for focus to be somewhere would be a shortcut that
    // works only after you have clicked something.
    document.addEventListener('keydown', handle)
    listening = true
  }
  bound.add(binding)
  return () => {
    bound.delete(binding)
  }
}

/**
 * Bind a chord for as long as the component is mounted.
 *
 * The caller's own presence is the condition, which is the right one here: a
 * row that is no longer taking a decision is a row that is no longer drawn.
 * `run` is kept in a ref rather than in the dependency list, so an inline
 * closure at the call site does not rebind on every render.
 */
export function useKey(scope: Scope, chord: string, run: (event: KeyboardEvent) => void) {
  const latest = useRef(run)
  latest.current = run

  useEffect(() => bind({ scope, chord, run: (event) => latest.current(event) }), [scope, chord])
}
