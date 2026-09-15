/**
 * What a refused command looks like from the window, in one place.
 *
 * Every Rust command rejects with the same shape, so every store that calls
 * one needs the same two lines to get a sentence out of it. They were written
 * three times (the chat, the settings ladder and the set-up) before this file
 * existed, which is three chances for one of them to start reading a different
 * field.
 */

/** The shape a Rust command rejects with: `demido_core::Error`. */
export type Failure = { kind: string; message: string }

/** The sentence a person can act on, out of whatever was thrown. */
export function sentence(error: unknown): string {
  if (typeof error === 'object' && error !== null && 'message' in error) {
    return String((error as Failure).message)
  }
  return String(error)
}

/**
 * Whether the request itself was wrong rather than the doing of it.
 *
 * `demido_core::Error::kind` is `invalid` for exactly that, and it exists so a
 * frontend branches on a tag instead of on the wording of a sentence. What
 * follows from it is the same everywhere: the value never reached Rust, so the
 * control that was typed into puts the saved one back and the toast says why.
 *
 * Here for the same reason [`sentence`] is: the settings ladder and the prompt
 * register both ask it, and two copies of one field name is two chances for one
 * of them to start reading a different one.
 */
export function refused(error: unknown): boolean {
  return typeof error === 'object' && error !== null && (error as Failure).kind === 'invalid'
}
