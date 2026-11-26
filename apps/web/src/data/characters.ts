/* The person's characters, kept by the core (DECISIONS E): read once, and again when one changes. */
import { create } from 'zustand'
import { coreGet, coreSend, onCoreEvent } from './core'

export interface Character { id: string; name: string; about: string; model: string | null; thinking: string | null; temperature: number | null; instructions: string; created_at: number; updated_at: number }
export type NewCharacter = Omit<Character, 'id' | 'created_at' | 'updated_at'>

export const useCharacters = create<{ list: Character[] | null; error: string | null }>(() => ({ list: null, error: null }))

export function readCharacters(): Promise<void> {
  return coreGet<Character[]>('/characters').then((list) => useCharacters.setState({ list, error: null }), (e: Error) => useCharacters.setState({ error: e.message }))
}

let watching = false
/** Read now, and again whenever a character is made, changed or deleted. */
export function watchCharacters() {
  void readCharacters()
  if (watching) return
  watching = true
  for (const kind of ['character.made', 'character.changed', 'character.deleted']) onCoreEvent(kind, () => void readCharacters())
}

export const makeCharacter = (c: NewCharacter) => coreSend<Character>('POST', '/characters', c)
export const changeCharacter = (id: string, c: NewCharacter) => coreSend<Character>('PUT', `/characters/${encodeURIComponent(id)}`, c)
export const deleteCharacter = (id: string) => coreSend<boolean>('DELETE', `/characters/${encodeURIComponent(id)}`)

/** Two letters for its picture: the first of two words, or the first two of one. */
export function initials(name: string): string {
  const words = name.trim().split(/\s+/).filter(Boolean)
  return (words.length > 1 ? words[0][0] + words[1][0] : (words[0] ?? '?').slice(0, 2)).toUpperCase()
}
