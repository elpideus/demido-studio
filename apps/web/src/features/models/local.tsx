/* "On this computer", for real: the models in Demido's folder, and loading and unloading them through the core. */
import { Box, Power, PowerOff, Copy } from 'lucide-react'
import { useEffect, useState } from 'react'
import { deleteLocal, loadLocal, readModelSettings, setContext, resetModelSettings, saveModelSettings, unloadLocal, unloadsIn, useLocal, type Chosen, type LocalModel, type ModelSettings, type Sampling } from '@/data/local-models'
import { Button, Dialog, Facts, Glyph, Page, Select, Signal, ContextMenu, Icon } from '@/ui'

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
            <ContextMenu items={[
              { label: 'Open', lead: <Icon icon={Box} size="sm" />, onSelect: () => onOpen(m.id) },
              m.state === 'loaded'
                ? { label: 'Unload', lead: <Icon icon={PowerOff} size="sm" />, disabled: Boolean(waiting[m.id]), onSelect: () => void unloadLocal(m.id) }
                : { label: 'Load', lead: <Icon icon={Power} size="sm" />, disabled: Boolean(waiting[m.id]) || !list.engine.runtime || m.state === 'loading', onSelect: () => void loadLocal(m.id) },
              { kind: 'sep' },
              { label: 'Copy the name', lead: <Icon icon={Copy} size="sm" />, onSelect: () => void navigator.clipboard.writeText(m.id) },
            ]}>
            <button type="button" className="d-row" aria-current={m.id === open ? 'true' : undefined} onClick={() => onOpen(m.id)}>
              <span className="d-row__lead"><Glyph name={s.glyph} tone={s.live ? 'live' : undefined} busy={s.glyph === 'model-loading'} label={s.words} /></span>
              <span className="d-row__text"><span className="d-row__title">{m.id}</span><span className="d-row__sub">{size(m.size)} · {s.words.toLowerCase()}{m.needs_mb !== null ? ` · needs ${gb(m.needs_mb)}` : ''}</span></span>
            </button>
            </ContextMenu>
          </li>
        )
      })}
    </ul>
  )
}

/** The settings a model picks its words with, by their names, with what each means and may be. */
const KNOBS: { key: keyof Sampling; name: string; hint: string; whole?: boolean; low: number; high: number; step: number }[] = [
  { key: 'temperature', name: 'Temperature', hint: 'How freely it picks: 0 is always the likeliest word.', low: 0, high: 2, step: 0.05 },
  { key: 'top_p', name: 'Top-p', hint: 'It picks among the likeliest words that together make up this much of the chance.', low: 0, high: 1, step: 0.01 },
  { key: 'top_k', name: 'Top-k', hint: 'It picks among this many of the likeliest words. 0: no such limit.', whole: true, low: 0, high: 1000, step: 1 },
  { key: 'min_p', name: 'Min-p', hint: 'A word is let in only if it is at least this likely, against the likeliest.', low: 0, high: 1, step: 0.01 },
  { key: 'presence_penalty', name: 'Presence penalty', hint: 'How much less it picks a word it has used already: against going round.', low: -2, high: 2, step: 0.1 },
  { key: 'repeat_penalty', name: 'Repeat penalty', hint: 'How much less it picks a word for each time it used it lately. 1: not at all.', low: 0.5, high: 2, step: 0.01 },
]

/** A set of settings in a line: "temperature 0.6 · top-p 0.95 · top-k 20". */
function inWords(s: Sampling): string {
  return KNOBS.filter((k) => s[k.key] !== undefined).map((k) => `${k.name.toLowerCase()} ${s[k.key]}`).join(' · ')
}

function fromWords(c: Chosen, maker: string | null): string {
  if (c.from === 'yours') return 'your own settings, the rest as its maker says'
  if (c.from === 'maker') return `as ${maker ?? 'its maker'} recommends`
  if (c.from === 'file') return 'as its file says: Demido sends nothing'
  return "the runtime's own: Demido sends nothing"
}

/** Changing a model's settings: for each way of answering, any setting may be left to its maker. */
function SettingsDialog({ s, onDone }: { s: ModelSettings; onDone: (now: ModelSettings | null) => void }) {
  const start = (which: 'thinking' | 'plain') => Object.fromEntries(KNOBS.map((k) => [k.key, s.yours[which][k.key]?.toString() ?? ''])) as Record<keyof Sampling, string>
  const [thinking, setThinking] = useState(() => start('thinking'))
  const [plain, setPlain] = useState(() => start('plain'))
  const [saving, setSaving] = useState(false)
  const read = (typed: Record<keyof Sampling, string>): Sampling => Object.fromEntries(KNOBS.filter((k) => typed[k.key].trim() !== '').map((k) => [k.key, Number(typed[k.key])])) as Sampling
  const bad = KNOBS.some((k) => [thinking, plain].some((typed) => {
    const v = typed[k.key].trim()
    if (v === '') return false
    const n = Number(v)
    return Number.isNaN(n) || n < k.low || n > k.high || (k.whole && !Number.isInteger(n))
  }))
  const save = async () => {
    if (bad || saving) return
    setSaving(true)
    const now = await saveModelSettings(s.model, read(thinking), read(plain))
    setSaving(false)
    if (now) onDone(now)
  }
  // What a setting left empty is: its maker's, or else the file's or the runtime's.
  const under = (which: 'thinking' | 'plain', k: keyof Sampling) => (s.maker?.[which][k] ?? s.in_file[k] ?? s.runtime[k])
  return (
    <Dialog open wide onOpenChange={(o) => !o && onDone(null)} title={`How ${s.model} picks its words`}
      text={<>Each setting you leave empty is {s.maker ? `${s.maker.who}'s` : "the runtime's"}, shown in grey. These are for the answers you read; Demido's own short questions to the model (which tool, which steps) keep theirs.</>}
      actions={<>
        {(Object.keys(s.yours.thinking).length > 0 || Object.keys(s.yours.plain).length > 0) && <Button variant="ghost" onClick={async () => onDone(await resetModelSettings(s.model))}>{s.maker ? `Use ${s.maker.who}'s again` : 'Use the runtime\'s again'}</Button>}
        <Button onClick={() => onDone(null)}>Cancel</Button>
        <Button variant="primary" disabled={bad || saving} onClick={() => void save()}>{saving ? 'Keeping them…' : 'Keep these'}</Button>
      </>}>
      <form className="mdl__sampling" onSubmit={(e) => { e.preventDefault(); void save() }} onKeyDown={(e) => { if (e.key === 'Enter') { e.preventDefault(); void save() } }}>
        <span /><span className="d-type-caption d-text-secondary">When it thinks</span><span className="d-type-caption d-text-secondary">When it does not</span>
        {KNOBS.map((k) => (
          <div key={k.key} style={{ display: 'contents' }}>
            <span className="app-stack" style={{ gap: 'var(--d-space-2)' }}><span>{k.name}</span><span className="d-type-caption d-text-secondary">{k.hint}</span></span>
            {(['thinking', 'plain'] as const).map((which) => {
              const [typed, setTyped] = which === 'thinking' ? [thinking, setThinking] : [plain, setPlain]
              return <input key={which} className="d-input d-input--mono" inputMode="decimal" aria-label={`${k.name}, ${which === 'thinking' ? 'when it thinks' : 'when it does not'}`} placeholder={under(which, k.key)?.toString() ?? ''} value={typed[k.key]} onChange={(e) => setTyped({ ...typed, [k.key]: e.target.value })} />
            })}
          </div>
        ))}
      </form>
      {bad && <Signal level="caution">A setting is outside what it can be: temperature 0 to 2, top-p and min-p 0 to 1, top-k a whole number to 1000, the presence penalty −2 to 2, the repeat penalty 0.5 to 2.</Signal>}
    </Dialog>
  )
}

/** How a model picks its words, thinking and not, and who says so. */
function SamplingPart({ id }: { id: string }) {
  const [s, setS] = useState<ModelSettings | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [editing, setEditing] = useState(false)
  useEffect(() => {
    let alive = true
    setS(null)
    setError(null)
    readModelSettings(id).then((r) => { if (alive) setS(r) }, (e: Error) => { if (alive) setError(e.message) })
    return () => { alive = false }
  }, [id])
  if (error) return <Signal level="caution">The core did not say how this model is asked: {error}</Signal>
  if (!s) return null
  const maker = s.maker
  const said = maker
    ? <>{maker.how === 'card' ? `${maker.who} says so on its model card` : `${maker.who} put them in the model's generation_config.json`}{maker.read_on ? `, read on ${maker.read_on}` : ''}: <a href={maker.source} target="_blank" rel="noreferrer noopener">{maker.source.replace('https://', '')}</a>.</>
    : <>Demido knows nothing its maker recommends{Object.keys(s.in_file).length ? '; the file carries settings of its own, which the runtime takes' : ", so the runtime's own settings apply"}.</>
  return (
    <div className="d-card"><div className="d-card__body app-stack">
      <div className="app-row">
        <span className="app-stack" style={{ gap: 'var(--d-space-2)', flex: 1 }}>
          <h3 className="d-card__title">How it picks its words</h3>
          <span className="d-type-caption d-text-secondary">{said}</span>
        </span>
        <Button size="sm" onClick={() => setEditing(true)}>Change…</Button>
      </div>
      <Facts rows={[
        ['When it thinks', <>{inWords(s.thinking.in_effect)} <span className="d-text-tertiary">({fromWords(s.thinking, maker?.who ?? null)})</span></>],
        ['When it does not', <>{inWords(s.plain.in_effect)} <span className="d-text-tertiary">({fromWords(s.plain, maker?.who ?? null)})</span></>],
      ]} />
      {editing && <SettingsDialog s={s} onDone={(now) => { setEditing(false); if (now) setS(now) }} />}
    </div></div>
  )
}

export function LocalDetail({ id }: { id: string | null }) {
  const list = useLocal((s) => s.list)
  const readAt = useLocal((s) => s.readAt)
  const error = useLocal((s) => s.error)
  const waiting = useLocal((s) => s.waiting)
  const progress = useLocal((s) => s.progress)
  const now = useNow(5000)
  const [sure, setSure] = useState<string | null>(null)
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
              <Button variant="ghost" disabled={busy} onClick={() => setSure(m.id)}>Delete…</Button>
            </div>
            {sure === m.id && (
              <Signal level="warning" actions={<><Button size="sm" onClick={() => setSure(null)}>Keep it</Button><Button size="sm" variant="danger" onClick={() => { setSure(null); void deleteLocal(m.id) }}>Delete {size(m.size)}</Button></>}>
                {m.file} is deleted from the models folder{m.state === 'loaded' ? ', after it is unloaded' : ''}. It cannot be undone; a model from Hugging Face can be downloaded again. Sessions that use it keep their messages, and need another model to go on.
              </Signal>
            )}
            <Facts rows={[
              ['File', m.file],
              ['On disk', size(m.size)],
              ['On the graphics card', m.needs_mb !== null ? `${gb(m.needs_mb)} at a context of ${m.context.toLocaleString()} tokens, as the runtime measured it` : 'Measured the first time the runtime looks at it'],
              ['Loads in', m.loads_in_ms !== null ? `${(m.loads_in_ms / 1000).toFixed(1)} s, the last time` : 'Known once it has been loaded'],
              ['Writes', m.tokens_per_second !== null ? `about ${Math.round(m.tokens_per_second)} tokens a second on this computer, from its real answers` : 'Known once it has answered something'],
              ['Thinking', m.thinking === null ? 'Known once it has been loaded' : !m.thinking.length ? 'It does not think before it answers' : m.thinking.length === 1 && m.thinking[0] === 'on' ? 'Can think first: on or off, set in each session' : `Can think first, at these levels: ${m.thinking.join(', ')}`],
              ['Takes in', m.takes === null ? 'Known once it has been loaded' : ['text', ...m.takes].join(', ')],
              ['Can hold in mind', m.context_length !== null ? `up to ${m.context_length.toLocaleString()} tokens, as it says itself` : 'Known once the runtime has looked at it'],
              ['Loaded with room for', <span className="app-row" style={{ gap: 'var(--d-space-8)', flexWrap: 'wrap' }}>
                <Select label={`What ${m.id} is loaded with room for`} value={m.own_context ? String(m.context) : 'default'} onChange={(v) => void setContext(m.id, v === 'default' ? null : Number(v))}
                  options={[{ value: 'default', label: `${e.context.toLocaleString()} tokens, as every model (Settings)` },
                    ...[2048, 4096, 8192, 16384, 32768, 65536, 131072].filter((c) => m.context_length === null || c <= m.context_length).filter((c) => c !== e.context || m.own_context).map((c) => ({ value: String(c), label: `${c.toLocaleString()} tokens, its own` }))]} />
                <span className="d-type-caption d-text-tertiary">More holds a longer conversation and needs more memory. Changing it starts the runtime again, unless an answer is being written.</span>
              </span>],
            ]} />
          </div></div>
        )
      })()}
      {m && <SamplingPart id={m.id} />}
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
    </Page>
  )
}
