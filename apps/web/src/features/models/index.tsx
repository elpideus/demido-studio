/* Models: find one, read its card, download it, load it.
   Left: the list (what is rising, what suits this computer, everything, in pages).
   Right: the model card of the one you picked, with its files and what it does on this computer. */
import { useCore } from '@/data/core'
import { useLocal } from '@/data/local-models'
import { LocalDetail, LocalList } from './local'
import { HubDetail, HubList } from './hub'
import type { Search as HubSearch } from '@/data/hub'
import { useEffect, useMemo, useState } from 'react'
import { SearchX, AudioLines, Box, Boxes, Brain, ChevronLeft, ChevronRight, Download, Eye, Heart, Search, Star, TrendingUp, Wrench } from 'lucide-react'
import type { LucideIcon } from 'lucide-react'
import type { Feature } from '@/app/registry'
import { SYSTEM } from '@/data/fixtures'
import { cancelDownload, deleteModel, downloadModel, loadModel, memoryOf, toast, unloadModel, useStore } from '@/data/store'
import type { Capability, Model } from '@/data/types'
import { Budget, Button, Facts, Glyph, Icon, MODEL_STATE_LABEL, NotBuilt, Segmented, Select, Signal, Switch, cx, Brief, Empty } from '@/ui'

const CAPS: Record<Capability, { label: string; icon: LucideIcon }> = {
  vision: { label: 'Vision', icon: Eye },
  audio: { label: 'Audio', icon: AudioLines },
  tools: { label: 'Tools', icon: Wrench },
  thinking: { label: 'Thinking', icon: Brain },
  embedding: { label: 'Embedding', icon: Boxes },
}
const FITS = { yes: 'Fits', tight: 'Fits alone', no: 'Too big' } as const
const PAGE = 10
const bestFit = (m: Model) => (m.quants.some((q) => q.fits === 'yes') ? 'yes' : m.quants.some((q) => q.fits === 'tight') ? 'tight' : 'no')
const live = (m: Model) => ['loaded', 'generating', 'lent'].includes(m.state)
const eye = (m: Model) => (m.state === 'loading' ? 'model-loading' : m.state === 'lent' ? 'model-lent' : m.state === 'queued' ? 'model-queued' : live(m) ? 'model-loaded' : 'model-unloaded')
const count = (s: string | undefined) => (s ? parseFloat(s) * (s.endsWith('M') ? 1e6 : s.endsWith('K') ? 1e3 : 1) : 0)
const billions = (m: Model) => parseFloat(m.params)
/** "3 weeks ago" as days, for sorting. */
const age = (s: string) => parseFloat(s) * (s.includes('year') ? 365 : s.includes('month') ? 30 : s.includes('week') ? 7 : 1)

/** A row in the list. */
function Row({ model, current, rank, onOpen }: { model: Model; current: boolean; rank?: number; onOpen: () => void }) {
  const fit = bestFit(model)
  return (
    <li>
      <button type="button" className="d-row" aria-current={current} onClick={onOpen}>
        <span className="d-row__lead">
          {rank ? <span className="models__rank d-mono">{rank}</span> : <Glyph name={eye(model)} busy={model.state === 'generating'} tone={live(model) ? 'live' : undefined} label={MODEL_STATE_LABEL[model.state]} />}
        </span>
        <span className="d-row__text">
          <span className="d-row__title">{model.name}{model.recommended && <Icon icon={Star} size="sm" label="Recommended for this computer" />}</span>
          <span className="d-row__sub">{model.publisher} · {model.params} · {model.downloads} downloads</span>
        </span>
        <span className="d-row__meta" style={{ display: 'grid', justifyItems: 'end', gap: 2 }}>
          {model.download ? `${model.download.progress}%` : model.installed ? <span className="d-badge d-badge--outline">On disk</span> : <span className={cx('d-badge', fit !== 'yes' && 'd-badge--outline')} data-tone={fit === 'no' ? 'caution' : undefined}>{FITS[fit]}</span>}
          {rank && model.trend ? <span className="models__up">+{model.trend}%</span> : null}
        </span>
      </button>
    </li>
  )
}

/** The model card. Written from what Demido knows about the model; the publisher's own page is not loaded in the preview. */
function Card({ model }: { model: Model }) {
  const b = billions(model)
  const good = [
    model.caps.includes('tools') && 'Calling tools with well-formed arguments.',
    model.caps.includes('thinking') && 'Working a problem through before it answers. Thinking can be turned down or off.',
    model.caps.includes('vision') && 'Reading screenshots, photos and scanned pages.',
    model.caps.includes('audio') && 'Listening to speech and other audio.',
    model.caps.includes('embedding') && 'Turning text into vectors, for search and for picking the right tool.',
    !model.caps.includes('embedding') && b < 3 && 'Quick, mechanical work for agents: renaming, sorting, extracting.',
    !model.caps.includes('embedding') && b >= 3 && b < 10 && 'Everyday questions, summaries and short tasks, at a good speed.',
    b >= 10 && 'Longer reasoning and longer pieces of writing.',
  ].filter(Boolean) as string[]
  const limits = [
    model.caps.includes('embedding') && 'It does not chat. It only produces vectors.',
    !model.caps.includes('vision') && !model.caps.includes('embedding') && 'It cannot see images. Demido can lend it eyes: a small vision model describes the image for it.',
    !model.caps.includes('tools') && !model.caps.includes('embedding') && 'It was not trained to call tools. Demido can still make it, with a stricter format, but it will be less reliable.',
    b < 3 && !model.caps.includes('embedding') && 'It loses the thread in long tasks. Give it a task list and short steps.',
    model.context <= 8192 && `A short context: ${model.context.toLocaleString()} tokens. Long files must be read in parts.`,
    bestFit(model) === 'no' && `Too big for this GPU (${SYSTEM.vramGb.toFixed(0)} GB). Part of it runs on the CPU, which is slow.`,
  ].filter(Boolean) as string[]
  return (
    <div className="d-prose models__card">
      <p>{model.summary}</p>
      <h3>Good at</h3>
      <ul>{good.map((g) => <li key={g}>{g}</li>)}</ul>
      <h3>Limits</h3>
      <ul>{limits.length ? limits.map((g) => <li key={g}>{g}</li>) : <li>Nothing that matters on this computer.</li>}</ul>
      <h3>Prompt format</h3>
      <p>Demido applies the chat template that ships with the model. You never write it by hand.</p>
      <h3>Licence</h3>
      <p>{model.license}. {model.gated ? 'The publisher asks you to accept its terms before the first download.' : 'No extra step before downloading.'} {/non-commercial/i.test(model.license) && 'It may not be used commercially.'}</p>
      <NotBuilt block label="Preview">The publisher's own model card is not loaded here. This page is written from the model's listed facts; its numbers and licence label are illustrative.</NotBuilt>
    </div>
  )
}

function Files({ model, quant, onQuant }: { model: Model; quant: string; onQuant: (q: string) => void }) {
  const q = model.quants.find((x) => x.id === quant)!
  return (
    <div className="app-stack">
      <p className="d-text-secondary">One model, several files. A smaller file needs less memory and loses a little quality.</p>
      <div className="app-stack" style={{ gap: 0 }}>
        {model.quants.map((x) => (
          <label key={x.id} className="d-check" style={{ display: 'flex', width: '100%' }}>
            <input className="d-radio" type="radio" name="quant" checked={quant === x.id} onChange={() => onQuant(x.id)} />
            <span className="d-check__text" style={{ flex: 1 }}>
              <span><span className="d-mono">{x.id}</span> {x.recommended && <span className="d-badge">Recommended for this computer</span>} {model.installed === x.id && <span className="d-badge d-badge--outline">On disk</span>}</span>
              <small>{x.note}</small>
            </span>
            <span className="d-mono d-text-secondary">{x.fileGb.toFixed(1)} GB file · {x.memoryGb.toFixed(1)} GB memory</span>
          </label>
        ))}
      </div>
      {q.fits === 'no' && <Signal level="caution">{quant} needs {q.memoryGb.toFixed(1)} GB. This GPU has {SYSTEM.vramGb.toFixed(1)} GB. {q.note}.</Signal>}
      <Facts rows={[['Format', 'GGUF'], ['Runs with', SYSTEM.runtime], ['Comes from', `huggingface.co/${model.publisher.toLowerCase().replace(/\s+/g, '-')}`]]} />
    </div>
  )
}

function Detail({ model }: { model: Model }) {
  const [tab, setTab] = useState<'card' | 'files' | 'here'>('card')
  const [quant, setQuant] = useState(model.installed ?? (model.quants.find((q) => q.recommended) ?? model.quants[0]).id)
  const q = model.quants.find((x) => x.id === quant)!
  const busy = model.state === 'generating' || model.state === 'lent'
  return (
    <div className="app-page">
      <div className="app-page__inner models__detail">
        <header className="models__head">
          <Glyph name={eye(model)} busy={model.state === 'generating'} tone={live(model) ? 'live' : undefined} size="2rem" label={MODEL_STATE_LABEL[model.state]} />
          <div className="app-stack" style={{ gap: 'var(--d-space-4)', flex: 1 }}>
            <h1 className="d-type-title-2">{model.name}</h1>
            <p className="d-text-secondary">{model.publisher} · {model.license} · updated {model.updated}</p>
          </div>
          <div className="app-row">
            {model.installed && <Button variant="ghost" disabled={busy} onClick={() => { deleteModel(model.id); toast(`${model.name} deleted from this computer.`) }}>Delete from disk</Button>}
            {model.download ? (
              <Button onClick={() => cancelDownload(model.id)}>Cancel · {model.download.progress}%</Button>
            ) : model.gated && !model.installed ? (
              <Button variant="primary" onClick={() => toast('Opens the publisher page to accept the licence. Not built in the preview.')}>Accept the licence first</Button>
            ) : model.installed !== quant ? (
              <Button variant="primary" onClick={() => void downloadModel(model.id, quant)}><Icon icon={Download} />Download {quant} · {q.fileGb.toFixed(1)} GB</Button>
            ) : live(model) || model.state === 'loading' ? (
              <Button variant="primary" disabled={busy} onClick={() => unloadModel(model.id)}>{busy ? 'In use' : 'Unload'}</Button>
            ) : (
              <Button variant="primary" onClick={() => void loadModel(model.id)}>Load model</Button>
            )}
          </div>
        </header>
        {(model.download || model.state === 'loading') && (
          <div className="d-meter d-meter--lg" role="meter" aria-valuenow={model.download?.progress ?? model.loadProgress ?? 0} aria-valuemin={0} aria-valuemax={100} aria-label={model.download ? 'Download' : 'Loading'}>
            <span style={{ '--d-meter': `${model.download?.progress ?? model.loadProgress ?? 0}%` } as React.CSSProperties} />
          </div>
        )}
        <div className="models__stats">
          <span><Icon icon={Download} size="sm" />{model.downloads} downloads</span>
          {model.likes && <span><Icon icon={Heart} size="sm" />{model.likes} likes</span>}
          {model.trend && <span className="models__up"><Icon icon={TrendingUp} size="sm" />+{model.trend}% this week</span>}
          <span>{model.params} parameters</span>
          <span>{model.context.toLocaleString()} tokens of context</span>
        </div>
        <div className="app-row" style={{ gap: 'var(--d-space-6)' }}>
          {(Object.keys(CAPS) as Capability[]).filter((c) => c !== 'embedding' || model.caps.includes(c)).map((c) => (
            <span key={c} className="d-chip" data-off={model.caps.includes(c) ? undefined : ''}><Icon icon={CAPS[c].icon} size="sm" />{CAPS[c].label}</span>
          ))}
          {model.recommended && <span className="d-badge"><Icon icon={Star} size="sm" />Recommended for this computer</span>}
        </div>
        <Segmented label="Show" value={tab} onChange={setTab} options={[{ value: 'card', label: 'Model card' }, { value: 'files', label: `Files · ${model.quants.length}` }, { value: 'here', label: 'On this computer' }]} />
        {tab === 'card' && <Card model={model} />}
        {tab === 'files' && <Files model={model} quant={quant} onQuant={setQuant} />}
        {tab === 'here' && (
          <div className="app-stack">
            <Facts rows={[
              ['On disk', model.installed ? `${model.installed} · ${(model.quants.find((x) => x.id === model.installed)?.fileGb ?? 0).toFixed(1)} GB` : 'not downloaded'],
              ['State', MODEL_STATE_LABEL[model.state]],
              ['Memory when loaded', `${memoryOf(model).toFixed(1)} GB of ${SYSTEM.vramGb.toFixed(1)} GB`],
              ['Speed here', model.speed ? `${model.speed} tokens/s` : 'not measured yet'],
              ['Best file for this computer', `${(model.quants.find((x) => x.recommended) ?? model.quants[0]).id} · ${FITS[bestFit(model)].toLowerCase()}`],
            ]} />
            <p className="d-type-caption d-text-secondary">Memory now, on {SYSTEM.gpu}:</p>
            <Budget />
            <NotBuilt block>Load settings for this model alone: context size, layers on the GPU, how long it may sit idle.</NotBuilt>
          </div>
        )}
      </div>
    </div>
  )
}

function ModelsPage() {
  const models = useStore((s) => s.models)
  const [view, setView] = useState<'discover' | 'all' | 'mine'>('discover')
  const [q, setQ] = useState('')
  const [caps, setCaps] = useState<Capability[]>([])
  const [fitsOnly, setFitsOnly] = useState(false)
  const [sort, setSort] = useState<'recommended' | 'trending' | 'downloads' | 'likes' | 'updated' | 'size'>('downloads')
  const [page, setPage] = useState(0)
  const [open, setOpen] = useState(models[0].id)
  // With a core behind the pages, "On this computer" is real: the models in Demido's folder, loaded and unloaded for real.
  const real = useCore().state === 'ready'
  const local = useLocal((s) => s.list)
  const [openLocal, setOpenLocal] = useState<string | null>(null)
  const mineForReal = real && view === 'mine'
  // Discover and All, for real: Hugging Face, searched through the core.
  const [openHub, setOpenHub] = useState<string | null>(null)
  const [hubSort, setHubSort] = useState<HubSearch['sort']>('downloads')
  const hubSearch: HubSearch = {
    q,
    sort: view === 'discover' && !q.trim() ? 'trending' : hubSort,
    takes: caps.flatMap((c) => (c === 'vision' ? ['images'] : c === 'audio' ? ['sound'] : [])),
    does: caps.filter((c) => c === 'tools' || c === 'thinking' || c === 'embedding'),
    fits: fitsOnly,
  }
  useEffect(() => { if (real) setView('mine') }, [real])
  const needle = q.trim().toLowerCase()
  const searching = needle !== '' || caps.length > 0 || fitsOnly

  const matches = useMemo(() => {
    const by = {
      recommended: (a: Model, b: Model) => Number(!!b.recommended) - Number(!!a.recommended) || count(b.downloads) - count(a.downloads),
      trending: (a: Model, b: Model) => (b.trend ?? 0) - (a.trend ?? 0),
      downloads: (a: Model, b: Model) => count(b.downloads) - count(a.downloads),
      likes: (a: Model, b: Model) => count(b.likes) - count(a.likes),
      updated: (a: Model, b: Model) => age(a.updated) - age(b.updated),
      size: (a: Model, b: Model) => billions(a) - billions(b),
    }[sort]
    return models
      .filter((m) => (!needle || `${m.name} ${m.publisher} ${m.license} ${m.caps.join(' ')}`.toLowerCase().includes(needle)) && caps.every((c) => m.caps.includes(c)) && (!fitsOnly || bestFit(m) !== 'no') && (view !== 'mine' || m.installed || m.download))
      .sort(by)
  }, [models, needle, caps, fitsOnly, sort, view])
  const pages = Math.max(1, Math.ceil(matches.length / PAGE))
  const at = Math.min(page, pages - 1)
  const current = models.find((m) => m.id === open) ?? models[0]
  const trending = [...models].filter((m) => m.trend).sort((a, b) => (b.trend ?? 0) - (a.trend ?? 0)).slice(0, 6)
  const picks = models.filter((m) => m.recommended)
  const fresh = [...models].sort((a, b) => age(a.updated) - age(b.updated)).slice(0, 5)
  const list = (items: Model[], ranked = false) => <ul className="d-list">{items.map((m, i) => <Row key={m.id} model={m} current={m.id === open} rank={ranked ? i + 1 : undefined} onOpen={() => setOpen(m.id)} />)}</ul>
  const reset = () => setPage(0)

  return (
    <div className="app-split models">
      <aside className="app-side" aria-label="Models">
        <div className="app-side__head"><span className="d-type-headline">Models</span><span className="d-type-caption d-text-tertiary">{models.length} listed</span></div>
        <label className="d-input-wrap">
          <Icon icon={Search} />
          <input className="d-input" type="search" placeholder="Name, publisher, licence or capability" aria-label="Search models" value={q} onChange={(e) => { setQ(e.target.value); reset() }} />
        </label>
        <Segmented small fill label="Show" value={view} onChange={(v) => { setView(v); reset() }} options={[{ value: 'discover', label: 'Discover' }, { value: 'all', label: 'All' }, { value: 'mine', label: 'On this computer' }]} />
        <div className="app-row" style={{ gap: 'var(--d-space-4)', paddingInline: 'var(--d-space-2)' }}>
          {(Object.keys(CAPS) as Capability[]).map((c) => {
            const on = caps.includes(c)
            return (
              <button key={c} type="button" className="d-chip" aria-pressed={on} style={{ border: 0, cursor: 'default', background: on ? 'var(--d-color-fill-selected)' : 'transparent', color: on ? 'var(--d-color-text-primary)' : undefined }} onClick={() => { setCaps(on ? caps.filter((x) => x !== c) : [...caps, c]); reset() }}>
                <Icon icon={CAPS[c].icon} size="sm" />{CAPS[c].label}
              </button>
            )
          })}
        </div>
        <label className="d-check" style={{ paddingInline: 'var(--d-space-6)' }}><Switch label="Only what fits this computer" checked={fitsOnly} onChange={(v) => { setFitsOnly(v); reset() }} /><span className="d-check__text">Only what fits this computer</span></label>

        <div className="app-side__scroll">
          {mineForReal ? (
            <>
              <p className="d-list__label">In Demido's models folder{local ? ` · ${local.models.length}` : ''}</p>
              <LocalList open={openLocal ?? local?.models[0]?.id ?? null} onOpen={setOpenLocal} />
            </>
          ) : real ? (
            <>
              {view === 'discover' && !q.trim() ? <p className="d-list__label">Rising this week, on Hugging Face</p> : (
                <div className="models__sort">
                  <span className="d-type-caption d-text-tertiary">On Hugging Face</span>
                  <Select label="Sort by" value={hubSort} onChange={setHubSort} options={[{ value: 'downloads', label: 'Most downloaded' }, { value: 'trending', label: 'Rising fastest' }, { value: 'likes', label: 'Most liked' }, { value: 'updated', label: 'Updated lately' }]} />
                </div>
              )}
              <HubList search={hubSearch} open={openHub} onOpen={setOpenHub} />
            </>
          ) : view === 'discover' && !searching ? (
            <>
              <p className="d-list__label">Rising this week</p>
              {list(trending, true)}
              <p className="d-list__label">Recommended for this computer</p>
              {list(picks)}
              <p className="d-list__label">New and updated</p>
              {list(fresh)}
              <Button variant="ghost" size="sm" onClick={() => setView('all')}>Browse all {models.length} models</Button>
            </>
          ) : (
            <>
              <div className="models__sort">
                <span className="d-type-caption d-text-tertiary">{matches.length} {matches.length === 1 ? 'model' : 'models'}</span>
                <Select label="Sort by" value={sort} onChange={(v) => { setSort(v); reset() }} options={[{ value: 'downloads', label: 'Most downloaded' }, { value: 'trending', label: 'Rising fastest' }, { value: 'likes', label: 'Most liked' }, { value: 'updated', label: 'Recently updated' }, { value: 'recommended', label: 'Recommended first' }, { value: 'size', label: 'Smallest first' }]} />
              </div>
              {list(matches.slice(at * PAGE, at * PAGE + PAGE))}
              {!matches.length && (view === 'mine' && !searching ? <Empty icon={Box} title="Nothing downloaded yet" /> : <Empty icon={SearchX} title="No model matches" text="Loosen a filter." />)}
            </>
          )}
        </div>
        {!real && (view !== 'discover' || searching) && pages > 1 && (
          <div className="models__pages">
            <Button variant="ghost" size="sm" iconOnly aria-label="Previous page" disabled={at === 0} onClick={() => setPage(at - 1)}><Icon icon={ChevronLeft} /></Button>
            <span className="d-type-caption d-text-secondary">{at * PAGE + 1}–{Math.min(matches.length, at * PAGE + PAGE)} of {matches.length} · page {at + 1} of {pages}</span>
            <Button variant="ghost" size="sm" iconOnly aria-label="Next page" disabled={at >= pages - 1} onClick={() => setPage(at + 1)}><Icon icon={ChevronRight} /></Button>
          </div>
        )}
        <div className="app-side__foot">
          {!real && <span>{SYSTEM.gpu} · {SYSTEM.vramGb.toFixed(1)} GB · {SYSTEM.ramGb} GB RAM</span>}
          {!real && <Budget compact />}
          {real ? <Brief className="" text="Models this runtime runs (GGUF), from Hugging Face. Whether one fits is worked out from its size; the runtime measures it once it is loaded." /> : <span>Model families are real. Sizes, counts and licence labels here are illustrative.</span>}
        </div>
      </aside>
      {mineForReal ? <LocalDetail id={openLocal} /> : real ? <HubDetail repo={openHub} /> : <Detail key={current.id} model={current} />}
    </div>
  )
}

export const feature: Feature = { id: 'models', title: 'Models', icon: Box, group: 'library', order: 0, path: 'models', Page: ModelsPage }
