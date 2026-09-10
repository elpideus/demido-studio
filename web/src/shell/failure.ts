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
