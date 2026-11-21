/* "On this computer", for real: the models in Demido's folder, and loading and unloading them through the core. */
import { useEffect, useState } from 'react'
import { loadLocal, unloadLocal, unloadsIn, useLocal, type LocalModel } from '@/data/local-models'
import { Button, Facts, Glyph, NotBuilt, Page, Signal } from '@/ui'

const MB = 1024 * 1024
export const size = (bytes: number) => (bytes >= 1024 * MB ? `${(bytes / 1024 / MB).toFixed(1)} GB` : `${Math.round(bytes / MB)} MB`)
const gb = (mib: number) => `${(mib / 1024).toFixed(1)} GB`

/** A wait in words, rounded up: "in 5 min", "in 40 s". */
function wait(seconds: number): string {
  return seconds > 90 ? `${Math.ceil(seconds / 60)} min` : `${seconds} s`
}

/** Re-draws every few seconds so that countdowns move. Asks nothing of anyone. */
function useNow(everyMs: number): number {
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), everyMs)
    return () => clearInterval(t)
  }, [everyMs])
  return now
}

export function stateOf(m: LocalModel, waiting?: 'loading' | 'unloading'): { glyph: string; words: string; live: boolean } {
  if (waiting === 'unloading') return { glyph: 'model-loading', words: 'Unloading', live: false }
  if (waiting === 'loading' || m.state === 'loading') return { glyph: 'model-loading', words: 'Loading', live: false }
  return m.state === 'loaded' ? { glyph: 'model-loaded', words: 'Loaded', live: true } : { glyph: 'model-unloaded', words: 'Not loaded', live: false }
}

export function LocalList({ open, onOpen }: { open: string | null; onOpen: (id: string) => void }) {
  const list = useLocal((s) => s.list)
  const waiting = useLocal((s) => s.waiting)
  if (!list) return <p className="app-empty"><Glyph name="task-running" /> Looking in the models folder…</p>
  if (!list.models.length) return <p className="app-empty">No model is in the folder yet.</p>
  return (
    <ul className="d-list">
      {list.models.map((m) => {
        const s = stateOf(m, waiting[m.id])
        return (
          <li key={m.id}>
            <button type="button" className="d-row" aria-current={m.id === open ? 'true' : undefined} onClick={() => onOpen(m.id)}>
              <span className="d-row__lead"><Glyph name={s.glyph} tone={s.live ? 'live' : undefined} busy={s.glyph === 'model-loading'} label={s.words} /></span>
              <span className="d-row__text"><span className="d-row__title">{m.id}</span><span className="d-row__sub">{size(m.size)} · {s.words.toLowerCase()}{m.needs_mb !== null ? ` · needs ${gb(m.needs_mb)}` : ''}</span></span>
            </button>
          </li>
        )
      })}
    </ul>
  )
}

export function LocalDetail({ id }: { id: string | null }) {
  const list = useLocal((s) => s.list)
  const readAt = useLocal((s) => s.readAt)
  const error = useLocal((s) => s.error)
  const waiting = useLocal((s) => s.waiting)
  const progress = useLocal((s) => s.progress)
  const now = useNow(5000)
  if (error && !list) return <Page narrow title="On this computer"><Signal level="caution">The core could not list the models: {error}</Signal></Page>
  if (!list) return <Page narrow title="On this computer"><p className="d-text-secondary"><Glyph name="task-running" /> Looking in the models folder…</p></Page>
  const e = list.engine
  const m = list.models.find((x) => x.id === id) ?? list.models[0]
  const idleRule = e.idle_unload_min === null ? 'A loaded model stays loaded until you unload it.' : `A loaded model that is not used for ${e.idle_unload_min} min is unloaded.`
  return (
    <Page narrow title={m ? m.id : 'On this computer'} lead="A model in Demido's folder on this computer. Loading it puts it on the graphics card; unloading ends its process and frees the memory.">
      {!e.runtime && <Signal level="caution">No model runtime is installed, so nothing can be loaded. Install one on the Setup screen.</Signal>}
      {!m && <p className="d-text-secondary">No model is in <span className="d-mono">{list.folder}</span> yet. Put a GGUF file there and it appears here.</p>}
      {m && (() => {
        const s = stateOf(m, waiting[m.id])
        const left = unloadsIn(m, readAt, now)
        const busy = Boolean(waiting[m.id]) || m.state === 'loading'
        const far = progress[m.id]
        return (
          <div className="d-card"><div className="d-card__body app-stack">
            <div className="app-row">
              <span className="d-badge" data-tone={s.live ? 'success' : undefined}><Glyph name={s.glyph} tone={s.live ? 'live' : undefined} busy={s.glyph === 'model-loading'} />{s.words}{waiting[m.id] === 'loading' && far > 0 && far < 1 ? ` · ${Math.round(far * 100)}%` : ''}</span>
              {m.state === 'loaded' && left !== null && <span className="d-type-caption d-text-tertiary">Unused, it is unloaded in {wait(left)}.</span>}
              <span className="app-spacer" />
              {m.state === 'loaded'
                ? <Button disabled={busy} onClick={() => unloadLocal(m.id)}>Unload</Button>
                : <Button variant="primary" disabled={busy || !e.runtime} onClick={() => loadLocal(m.id)}>{busy ? 'Loading…' : 'Load'}</Button>}
            </div>
            <Facts rows={[
              ['File', m.file],
              ['On disk', size(m.size)],
              ['On the graphics card', m.needs_mb !== null ? `${gb(m.needs_mb)} at a context of ${e.context.toLocaleString()} tokens, as the runtime measured it` : 'Measured the first time the runtime looks at it'],
              ['Loads in', m.loads_in_ms !== null ? `${(m.loads_in_ms / 1000).toFixed(1)} s, the last time` : 'Known once it has been loaded'],
              ['Writes', m.tokens_per_second !== null ? `about ${Math.round(m.tokens_per_second)} tokens a second on this computer, from its real answers` : 'Known once it has answered something'],
              ['Thinking', m.thinking === null ? 'Known once it has been loaded' : !m.thinking.length ? 'It does not think before it answers' : m.thinking.length === 1 && m.thinking[0] === 'on' ? 'Can think first: on or off, set in each session' : `Can think first, at these levels: ${m.thinking.join(', ')}`],
              ['Takes in', m.takes === null ? 'Known once it has been loaded' : ['text', ...m.takes].join(', ')],
              ['Can hold in mind', m.context_length !== null ? `up to ${m.context_length.toLocaleString()} tokens; Demido loads it with ${e.context.toLocaleString()}` : 'Known once the runtime has looked at it'],
            ]} />
          </div></div>
        )
      })()}
      <div>
        <p className="d-list__label" style={{ paddingInline: 0 }}>How models are run here</p>
        <Facts rows={[
          ['Runtime', e.runtime ? `${e.runtime} · ${e.running ? 'running' : 'not running; it starts when a model is loaded'}` : 'none installed'],
          ['Context', `${e.context.toLocaleString()} tokens for every model (Settings › Models and memory)`],
          ['Memory kept free', `${gb(e.margin_mb)} of graphics memory. When a model would not fit beside what is loaded, the one used longest ago is unloaded first.`],
          ['When idle', idleRule],
          ['Folder', list.folder],
        ]} />
      </div>
      <NotBuilt block>Asking a loaded model something (the chat loop). Downloading models from within Demido: for now, put GGUF files in the folder. Choosing the context for each model by itself.</NotBuilt>
    </Page>
  )
}
