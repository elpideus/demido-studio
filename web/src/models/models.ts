import { useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'

import type { Capabilities, Damage, Model } from '@/setup/setup'
import { size } from '@/shell/bytes'
import { sentence } from '@/shell/failure'
import { useToasts } from '@/shell/toasts'

/**
 * The Models window's data, as the window holds it
 * ([#75](https://github.com/elpideus/demido-studio/issues/75)).
 *
 * Two sources, and they fail apart. **What is on disk** is the set-up view's
 * library (`useSetup`), read from the folders with no network at all. **What
 * the index publishes** is asked here, and it answers with a state rather than
 * an error (`demido_models::index::Answer`), so a dead connection is a
 * sentence beside the installed models rather than a pane that failed to
 * draw.
 */

/** Why the index could not be read. The Rust `Cause`. */
export type Cause = 'offline' | 'rate-limited' | 'missing' | 'refused' | 'malformed'

/** What asking the index got. The Rust `Answer`. */
export type Answer<T> =
  { state: 'read'; found: T } | { state: 'unreadable'; cause: Cause; reason: string }

/** A repository in the index. The Rust `demido_models::index::Repo`: the
 * publisher's own fields, and the capabilities it states, which are `yes` or
 * `unknown` and never `no`. */
export type Repo = {
  id: string
  author?: string
  downloads: number
  likes: number
  gated: boolean
  pipeline?: string
  library?: string
  tags: string[]
  params?: number
  architecture?: string
  context?: number
  stated: Capabilities
}

/** One `.gguf` in a repository. The Rust `File`. */
export type File = { path: string; bytes: number; sha256?: string }

/** A quantisation. `bits` is absent for a label nobody published a number
 * for, which is shown with no number rather than an approximate one. */
export type Quant = { label: string; bits?: number }

/** One thing a person can download from a repository. The Rust `Choice`: a
 * projector travels with the weights that need it and a draft is never here,
 * so nothing in this list is anything but a model. */
export type Choice = {
  name: string
  quant?: Quant
  pieces: File[]
  projector?: File
  /** What the download costs. */
  bytes: number
  /** What loading it costs, before any context: what the fit verdict prices. */
  weights: number
}

/** A call that never reached the command: the channel itself. Said the way
 * a dead connection is, because to a person it is one. */
function unanswered(error: unknown): Answer<never> {
  return { state: 'unreadable', cause: 'offline', reason: sentence(error) }
}

/** How long typing has to pause before the index is asked. */
const SETTLE_MS = 300

/**
 * The index's answer for `query`, asked once typing settles. `undefined`
 * while the first answer is in flight; the previous answer stays on screen
 * while a later one is, so a list does not blink empty between keystrokes.
 */
export function useSearch(query: string): { answer?: Answer<Repo[]>; asking: boolean } {
  const [answer, setAnswer] = useState<Answer<Repo[]>>()
  const [asking, setAsking] = useState(true)

  useEffect(() => {
    let live = true
    setAsking(true)
    const timer = window.setTimeout(() => {
      invoke<Answer<Repo[]>>('models_search', { query }).then(
        (found) => {
          if (!live) return
          setAnswer(found)
          setAsking(false)
        },
        (error: unknown) => {
          if (!live) return
          // The command answers a dead connection as a state. What reaches
          // here is the channel itself.
          setAnswer(unanswered(error))
          setAsking(false)
        },
      )
    }, SETTLE_MS)
    return () => {
      live = false
      window.clearTimeout(timer)
    }
  }, [query])

  return { answer, asking }
}

/** What has been read of each repository this window has opened. A choice
 * list does not move while the window is open, and reading it again every
 * time a row is revisited would be a request per click. */
const read = new Map<string, Answer<Choice[]>>()

/** What a person can download from `repo`, most faithful first. */
export function useChoices(repo: string): Answer<Choice[]> | undefined {
  const [answer, setAnswer] = useState(() => read.get(repo))

  useEffect(() => {
    let live = true
    const known = read.get(repo)
    setAnswer(known)
    if (known) return
    invoke<Answer<Choice[]>>('models_choices', { repo }).then(
      (found) => {
        // An unreadable answer is not kept, so opening the row again asks.
        if (found.state === 'read') read.set(repo, found)
        if (live) setAnswer(found)
      },
      (error: unknown) => {
        if (live) setAnswer(unanswered(error))
      },
    )
    return () => {
      live = false
    }
  }, [repo])

  return answer
}

/** Queue a choice. The queue owns it from here, and the title bar's indicator
 * is where it is watched; a refusal is a toast, because somebody pressed
 * something a moment ago and it was not taken. */
export async function download(repo: string, choice: Choice): Promise<boolean> {
  try {
    await invoke('downloads_add', { repo, name: choice.name })
    return true
  } catch (error) {
    useToasts.getState().show(sentence(error))
    return false
  }
}

/** Whether a model on disk is `query`'s, by the words a person would type. */
export function matches(model: Model, query: string): boolean {
  const words = query.trim().toLowerCase()
  if (!words) return true
  return [model.label, model.repo ?? '', model.library].some((said) =>
    said.toLowerCase().includes(words),
  )
}

/** Why the index could not be read, as the window says it. */
export function unreadable(cause: Cause): string {
  switch (cause) {
    case 'offline':
      return 'The index could not be reached.'
    case 'rate-limited':
      return 'Hugging Face is limiting requests from this address for a few minutes.'
    case 'missing':
      return 'Hugging Face has no public repository there.'
    case 'refused':
      return 'Hugging Face refused the request.'
    case 'malformed':
      return 'Something answered, but not with the index.'
  }
}

/** A parameter count the way a model is spoken of: 7.5B, 596M. */
export function params(count: number): string {
  if (count >= 1e9) return `${(count / 1e9).toFixed(1)}B`
  if (count >= 1e6) return `${Math.round(count / 1e6)}M`
  return `${Math.round(count / 1e3)}K`
}

/** A context length in tokens, the way a model card writes it. */
export function tokens(count: number): string {
  return count >= 1024 && count % 1024 === 0 ? `${count / 1024}K` : count.toLocaleString('en')
}

/** A quantisation as a download option says it. */
export function quant(choice: Choice): string {
  const label = choice.quant?.label ?? choice.name
  return choice.quant?.bits === undefined ? label : `${label}, ${choice.quant.bits.toFixed(2)} bpw`
}

/** The primary action's words: the size is stated before it is spent. */
export function spend(choice: Choice): string {
  return `Download ${size(choice.bytes)}`
}

/** Why a file on disk is not offered, in this application's own words. */
export function damage(damage: Damage): string {
  switch (damage.kind) {
    case 'not-gguf':
      return 'which is not a GGUF file'
    case 'unsupported':
      return `which is GGUF version ${damage.version}, a version this build does not read`
    case 'malformed':
      return `whose header is malformed: ${damage.reason}`
    case 'truncated':
      return damage.needs === null
        ? `which is cut short inside its header at ${size(damage.has)}`
        : `which is cut short: ${size(damage.has)} of ${size(damage.needs)}`
    case 'missing-piece':
      return `whose piece ${damage.index} of ${damage.total} is missing`
    case 'unreadable':
      return `which could not be read: ${damage.reason}`
  }
}
