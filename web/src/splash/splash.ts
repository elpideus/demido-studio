/**
 * The splash: the mark, the wordmark, and what is currently loading.
 *
 * Brief B41: "There should be a splash small window (similar to that of
 * Discord), showing the logo, the Demido Studio name and the loading status
 * (maybe even what it is actually loading?)".
 *
 * Vanilla, and deliberately. `design/splash.md`: "It is painted before anything
 * else exists, so it cannot wait on anything else existing." No framework, no
 * router, no store, no component library: this window's whole job is to render
 * on the first frame on a machine that has never been online, and every
 * dependency it takes is a thing that can be slow or missing when it does.
 *
 * The sequence it draws belongs to Rust (`src-tauri/src/boot.rs`). This asks
 * for the stage list, draws one tick per stage, subscribes, and only then says
 * begin: a sequence started before the window was listening would paint its
 * first stages into nothing.
 */

import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

// The canonical drawing, from the one file that owns it. Never redrawn, never
// re-typed: `design/mark.svg` says so at the top, and hard rule "icons come
// from a pack, never drawn by hand" is the same rule about somebody else's
// glyphs.
import mark from '../../../design/mark.svg?raw'
import './splash.css'

declare const __APP_VERSION__: string

/** A tick's four states, which are `design/splash.md`'s table. */
type State = 'pending' | 'running' | 'done' | 'failed'

type Stage = { id: string; name: string }

type Progress = {
  index: number
  total: number
  stage: Stage
  state: State
  detail: string | null
}

/** The event Rust reports each state change on. */
const PROGRESS = 'boot://progress'

/** An element the markup below guarantees, or a sentence naming what is gone. */
function must<T extends Element>(element: T | null, what: string): T {
  if (!element) throw new Error(`the splash has no ${what}`)
  return element
}

const root = must(document.getElementById('splash'), 'root to paint into')

root.innerHTML = `
  <div class="splash">
    <div>
      <div class="head">
        <span class="mark">${mark}</span>
        <div class="lockup">
          <span class="name">
            <b>Demido</b>
            <span class="kana">デミド</span>
          </span>
          <span class="studio">Studio</span>
        </div>
      </div>
      <p class="version">${__APP_VERSION__}</p>
    </div>
    <div class="foot">
      <div class="scale" id="scale"></div>
      <p class="stage" id="stage"></p>
    </div>
  </div>
`

const scale = must(root.querySelector<HTMLDivElement>('#scale'), 'scale')
const line = must(root.querySelector<HTMLParagraphElement>('#stage'), 'stage line')

/** Draw one tick per stage, all pending, before anything has run. */
function ticks(count: number): HTMLDivElement[] {
  const drawn: HTMLDivElement[] = []
  for (let at = 0; at < count; at += 1) {
    const tick = document.createElement('div')
    tick.className = 'tick'
    tick.dataset.state = 'pending' satisfies State
    scale.append(tick)
    drawn.push(tick)
  }
  return drawn
}

/**
 * What the stage line says.
 *
 * A failure says so on the line as well as in the tick, because a rose tick
 * three positions along is a colour nobody can attach to a name once the stage
 * after it is already running. Skipped, not stopped: the desk is still coming.
 */
function says(progress: Progress): string {
  if (progress.state === 'failed') return `${progress.stage.name}: skipped`
  return progress.stage.name
}

async function paint(): Promise<void> {
  const stages = await invoke<Stage[]>('boot_stages')
  const drawn = ticks(stages.length)

  await listen<Progress>(PROGRESS, ({ payload }) => {
    const tick = drawn[payload.index]
    if (tick) tick.dataset.state = payload.state
    // A stage that has finished leaves its name up: the line names the stage
    // that most recently had something to say, which through a whole sequence
    // is the one running.
    line.textContent = says(payload)
    line.dataset.failed = String(payload.state === 'failed')
  })

  await invoke('boot_begin')
}

/**
 * A splash that cannot talk to Rust still says something.
 *
 * There is nothing to retry: the desk is opened by the sequence, and the
 * watchdog in `src-tauri/src/lib.rs` starts that without this window when this
 * window never asks. So the failure is put on the stage line and the splash
 * waits to be replaced, rather than becoming a window with an empty scale and
 * no account of itself.
 */
paint().catch((error: unknown) => {
  line.dataset.failed = 'true'
  line.textContent = 'the startup sequence could not be reached'
  console.error('[splash] the sequence could not be reached', error)
})
