/* Discover and All, for real: models on Hugging Face that the runtime runs (GGUF), searched through the core,
   with whether each fits this computer, and downloading one into the models folder. */
import { useEffect, useMemo, useState } from 'react'
import { AudioLines, Boxes, Brain, Download as DownloadIcon, ExternalLink, Eye, Heart, Wrench } from 'lucide-react'
import type { LucideIcon } from 'lucide-react'
import { hubModel, readToken, removeToken, saveToken, searchHub, startDownload, stopDownload, useDownloads, type HubDetail as Detail, type HubModel, type HubQuant, type Machine, type Search, type TokenFacts } from '@/data/hub'
import { Button, Dialog, Facts, Glyph, Icon, NotBuilt, Page, Segmented, Signal, cx, Brief } from '@/ui'
import { Prose } from '@/ui/prose'
import { size } from './local'

const ABLE: Record<string, { label: string; icon: LucideIcon }> = {
  images: { label: 'Sees images', icon: Eye },
  sound: { label: 'Hears', icon: AudioLines },
  tools: { label: 'Calls tools', icon: Wrench },
  thinking: { label: 'Thinks first', icon: Brain },
  embedding: { label: 'Embedding', icon: Boxes },
}
const FIT_SAID = { fits: 'Fits', slow: 'Slow here', no: 'Too big' } as const
const gb = (mib: number) => `${(mib / 1024).toFixed(1)} GB`
const billions = (p: number | null) => (p === null ? null : p >= 1e9 ? `${(p / 1e9).toFixed(p >= 1e10 ? 0 : 1)}B` : `${Math.round(p / 1e6)}M`)
const many = (n: number) => (n >= 1e6 ? `${(n / 1e6).toFixed(1)}M` : n >= 1e3 ? `${Math.round(n / 1e3)}K` : `${n}`)
const day = (iso: string | null) => (iso ? new Date(iso).toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric' }) : 'not said')

/** What a fit means on this computer, in a sentence. */
function fitSaid(fit: 'fits' | 'slow' | 'no', machine: Machine): string {
  const card = machine.card_mb > 0
  if (fit === 'fits') return `Fits on the graphics card (${gb(machine.card_mb)}), with ${gb(machine.margin_mb)} kept free.`
  if (fit === 'slow') return card ? 'Too large for the graphics card alone: part of it would run on the processor, several times slower.' : 'There is no graphics card Demido can use, so it would run on the processor, slowly.'
  return 'Too large for this computer, even with part of it on the processor.'
}

/** The list: a page of models from a search, and more on asking. */
export function HubList({ search, open, onOpen }: { search: Search; open: string | null; onOpen: (id: string) => void }) {
  const [models, setModels] = useState<HubModel[] | null>(null)
  const [next, setNext] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [more, setMore] = useState(false)
  const key = JSON.stringify(search)
  useEffect(() => {
    let gone = false
    setModels(null)
    setError(null)
    // Typing is waited out: one question to Hugging Face for a word, not one for each letter.
    const t = setTimeout(() => {
      searchHub(search).then((page) => {
        if (gone) return
        setModels(page.models)
        setNext(page.next)
        if (!open && page.models[0]) onOpen(page.models[0].id)
      }, (e: Error) => !gone && setError(e.message))
    }, 400)
    return () => { gone = true; clearTimeout(t) }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key])
  const loadMore = () => {
    if (!next) return
    setMore(true)
    searchHub(search, next).then((page) => { setModels((m) => [...(m ?? []), ...page.models]); setNext(page.next) }, (e: Error) => setError(e.message)).finally(() => setMore(false))
  }
  if (error) return <Signal level="caution">{error}</Signal>
  if (!models) return <p className="app-empty"><Glyph name="task-running" busy /> Asking Hugging Face…</p>
  if (!models.length) return <p className="app-empty">No model on this page matches. {next ? 'There are more pages.' : 'Loosen a filter.'}</p>
  return (
    <>
      <ul className="d-list">
        {models.map((m) => (
          <li key={m.id}>
            <button type="button" className="d-row" aria-current={m.id === open ? 'true' : undefined} onClick={() => onOpen(m.id)}>
              <span className="d-row__text">
                <span className="d-row__title">{m.id.split('/')[1]}</span>
                <span className="d-row__sub">{m.author}{billions(m.params) ? ` · ${billions(m.params)}` : ''} · {many(m.downloads)} downloads</span>
              </span>
              <span className="d-row__meta">
                {m.gated ? <span className="d-badge d-badge--outline">Sign-in</span> : m.fits ? <span className={cx('d-badge', m.fits !== 'fits' && 'd-badge--outline')}>{FIT_SAID[m.fits]}</span> : null}
              </span>
            </button>
          </li>
        ))}
      </ul>
      {next && <Button variant="ghost" size="sm" disabled={more} onClick={loadMore}>{more ? 'Asking…' : 'More'}</Button>}
    </>
  )
}

/** A quantization in the list of files. */
function QuantRow({ q, chosen, onChoose }: { q: HubQuant; chosen: boolean; onChoose: () => void }) {
  return (
    <label className="d-check" style={{ display: 'flex', width: '100%' }}>
      <input className="d-radio" type="radio" name="quant" checked={chosen} onChange={onChoose} />
      <span className="d-check__text" style={{ flex: 1 }}>
        <span>
          <span className="d-mono">{q.name}</span>{' '}
          {q.recommended && <span className="d-badge">Recommended for this computer</span>}{' '}
          {q.on_disk && <span className="d-badge d-badge--outline">On this computer</span>}
        </span>
        <small>{q.files.length > 1 ? `${q.files.length} files` : q.files[0]?.path} · {FIT_SAID[q.fits].toLowerCase()}</small>
      </span>
      <span className="d-mono d-text-secondary">{size(q.size)} · needs about {gb(q.needs_mb)}</span>
    </label>
  )
}

/** A model's page: what it is, its files, and downloading one of them. */
/** Giving Demido a Hugging Face token, for gated models. */
function TokenDialog({ onDone }: { onDone: (facts: TokenFacts | null) => void }) {
  const [value, setValue] = useState('')
  const [saving, setSaving] = useState(false)
  const save = async () => {
    if (!value.trim() || saving) return
    setSaving(true)
    const facts = await saveToken(value.trim())
    setSaving(false)
    if (facts) onDone(facts)
  }
  return (
    <Dialog open onOpenChange={(o) => !o && onDone(null)} title="Add your Hugging Face token"
      text={<>For gated models: their makers ask you to accept their terms on Hugging Face first, and Hugging Face then lets your account download them. Demido sends the token only to Hugging Face, with the requests for a gated model's files, and never shows it again: only its last four characters.</>}
      actions={<><Button onClick={() => onDone(null)}>Cancel</Button><Button variant="primary" disabled={!value.trim() || saving} onClick={() => void save()}>{saving ? 'Keeping it…' : 'Keep the token'}</Button></>}>
      <form className="app-stack" style={{ marginBlockStart: 'var(--d-space-16)' }} onSubmit={(e) => { e.preventDefault(); void save() }}>
        <label className="d-field">
          <span className="d-field__label">Token</span>
          <input className="d-input d-input--mono" type="password" autoComplete="off" spellCheck={false} autoFocus placeholder="hf_…" value={value} onChange={(e) => setValue(e.target.value)} />
          <span className="d-field__help">Make one at <a href="https://huggingface.co/settings/tokens" target="_blank" rel="noreferrer noopener">huggingface.co/settings/tokens</a>: one that may only read is enough.</span>
        </label>
      </form>
    </Dialog>
  )
}

export function HubDetail({ repo }: { repo: string | null }) {
  const [token, setToken] = useState<TokenFacts | null | undefined>(undefined)
  const [asking, setAsking] = useState(false)
  useEffect(() => { readToken().then(setToken, () => setToken(null)) }, [])
  const [detail, setDetail] = useState<Detail | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [tab, setTab] = useState<'card' | 'files'>('files')
  const [quant, setQuant] = useState<string | null>(null)
  const downloads = useDownloads((s) => s.list)
  useEffect(() => {
    if (!repo) return
    let gone = false
    setDetail(null)
    setError(null)
    hubModel(repo).then((d) => {
      if (gone) return
      setDetail(d)
      setQuant((d.quants.find((q) => q.recommended) ?? d.quants[0])?.name ?? null)
    }, (e: Error) => !gone && setError(e.message))
    return () => { gone = true }
  }, [repo])
  // A download that ends changes what is on disk: the page is asked again.
  const finished = downloads.filter((d) => d.repo === repo && d.state === 'done').length
  useEffect(() => {
    if (repo && finished) hubModel(repo).then(setDetail, () => {})
  }, [repo, finished])
  const going = useMemo(() => downloads.filter((d) => d.repo === repo && ['waiting', 'downloading'].includes(d.state)).at(-1), [downloads, repo])
  const ended = useMemo(() => downloads.filter((d) => d.repo === repo && ['failed', 'stopped'].includes(d.state)).at(-1), [downloads, repo])

  if (!repo) return <Page narrow title="Models"><p className="d-text-secondary">Pick a model on the left.</p></Page>
  if (error) return <Page narrow title={repo}><Signal level="caution">{error}</Signal></Page>
  if (!detail) return <Page narrow title={repo}><p className="d-text-secondary"><Glyph name="task-running" busy /> Asking Hugging Face…</p></Page>
  const m = detail.model
  const q = detail.quants.find((x) => x.name === quant) ?? null
  const abilities = [...m.takes, ...m.does]
  return (
    <div className="app-page">
      <div className="app-page__inner models__detail">
        <header className="models__head">
          <div className="app-stack" style={{ gap: 'var(--d-space-4)', flex: 1 }}>
            <h1 className="d-type-title-2">{m.id.split('/')[1]}</h1>
            <p className="d-text-secondary">{m.author} · {m.licence ?? 'no licence given'} · updated {day(m.updated)}</p>
          </div>
          <div className="app-row">
            <a className="d-button d-button--ghost" href={detail.page} target="_blank" rel="noreferrer noopener"><Icon icon={ExternalLink} />On Hugging Face</a>
            {going ? (
              <Button onClick={() => void stopDownload(going.id)}>{going.state === 'waiting' ? 'Waiting · Stop' : `Stop · ${Math.floor((going.done * 100) / Math.max(1, going.total))}%`}</Button>
            ) : q && (!m.gated || token) && !q.on_disk ? (
              <Button variant="primary" onClick={() => void startDownload(m.id, q.name)}><Icon icon={DownloadIcon} />Download {q.name} · {size(q.size)}</Button>
            ) : null}
          </div>
        </header>
        {going && (
          <div className="d-meter d-meter--lg" role="meter" aria-valuenow={Math.floor((going.done * 100) / Math.max(1, going.total))} aria-valuemin={0} aria-valuemax={100} aria-label="Download">
            <span style={{ '--d-meter': `${(going.done * 100) / Math.max(1, going.total)}%` } as React.CSSProperties} />
          </div>
        )}
        {ended && !going && (
          <Signal level="caution">
            {ended.problem ?? 'The download did not finish.'}{' '}
            {ended.state === 'stopped' && <>What was downloaded is kept: downloading it again goes on from there. <Button variant="ghost" size="sm" onClick={() => void stopDownload(ended.id, true)}>Remove what was downloaded</Button></>}
          </Signal>
        )}
        {m.gated && (token
          ? <Signal level="note" actions={<Button size="sm" variant="ghost" onClick={async () => { if (await removeToken()) setToken(null) }}>Delete the token</Button>}>This model is gated: Demido downloads it with your Hugging Face token (…{token.ends_with}). Accept its maker's terms <a href={detail.page} target="_blank" rel="noreferrer noopener">on its page</a> first, with the same account, or Hugging Face says no.</Signal>
          : <Signal level="caution" actions={<Button size="sm" onClick={() => setAsking(true)}>Add your token…</Button>}>This model is gated: its maker asks you to accept its terms <a href={detail.page} target="_blank" rel="noreferrer noopener">on its page</a> first. Then add your Hugging Face token, and Demido downloads it with that.</Signal>)}
        {asking && <TokenDialog onDone={(facts) => { setAsking(false); if (facts) setToken(facts) }} />}
        <div className="models__stats">
          <span><Icon icon={DownloadIcon} size="sm" />{many(m.downloads)} downloads</span>
          <span><Icon icon={Heart} size="sm" />{many(m.likes)} likes</span>
          {billions(m.params) && <span>{billions(m.params)} parameters</span>}
          {m.context_length && <span>up to {m.context_length.toLocaleString()} tokens</span>}
          {m.architecture && <span className="d-mono">{m.architecture}</span>}
        </div>
        <div className="app-row" style={{ gap: 'var(--d-space-6)' }}>
          {abilities.length ? abilities.map((a) => ABLE[a] && <span key={a} className="d-chip"><Icon icon={ABLE[a].icon} size="sm" />{ABLE[a].label}</span>) : <span className="d-type-caption d-text-tertiary">Its files say nothing of tools, thinking or images.</span>}
        </div>
        <p className="d-type-caption d-text-tertiary">What it does is read from its own files (its chat template and its tags). Demido checks once it is loaded.</p>
        <Segmented label="Show" value={tab} onChange={setTab} options={[{ value: 'files', label: `Files · ${detail.quants.length}` }, { value: 'card', label: 'Model card' }]} />
        {tab === 'files' && (
          <div className="app-stack">
            <Brief className="d-text-secondary" text={"One model, several files. A smaller file needs less memory and loses a little quality. The one recommended is the best quality that fits on the graphics card with the memory kept free."} />
            {!detail.quants.length && <p className="d-text-secondary">This repository has no GGUF file Demido can run.</p>}
            <div className="app-stack" style={{ gap: 0 }}>
              {detail.quants.map((x) => <QuantRow key={x.name} q={x} chosen={x.name === quant} onChoose={() => setQuant(x.name)} />)}
            </div>
            {q && <Signal level={q.fits === 'fits' ? 'note' : 'caution'}>{q.name}: {fitSaid(q.fits, detail.machine)} About {gb(q.needs_mb)} at a context of {detail.machine.context.toLocaleString()} tokens: an estimate from its size; the runtime measures it for real once it is loaded.</Signal>}
            <Facts rows={[
              ['Vision part', detail.vision_part ? `${detail.vision_part.path} · ${size(detail.vision_part.size)}, downloaded with any of these` : 'none'],
              ['Goes to', q ? (q.files.length > 1 || detail.vision_part ? `the folder ${q.local_id} in the models folder` : `${q.files[0]?.path.split('/').at(-1)} in the models folder`) : '—'],
              ['Checked', 'against the SHA-256 Hugging Face lists for each file, before it is used'],
              ...(detail.others.length ? [['Not offered', `${detail.others.length} more GGUF ${detail.others.length === 1 ? 'file' : 'files'} in it that ${detail.others.length === 1 ? 'is' : 'are'} not this model: ${detail.others.map((f) => f.path).join(', ')}. Smaller models that come with it, such as a draft model, or calibration data.`] as [string, string]] : []),
              ['From', `${detail.page} · commit ${detail.revision.slice(0, 10)}`],
            ]} />
          </div>
        )}
        {tab === 'card' && (detail.card ? <div className="models__card"><Prose text={detail.card} /></div> : <p className="d-text-secondary">This repository has no model card.</p>)}
        <NotBuilt block>Other sources than Hugging Face.</NotBuilt>
      </div>
    </div>
  )
}
