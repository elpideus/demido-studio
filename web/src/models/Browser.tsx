import { useEffect, useMemo, useState } from 'react'
import { Check, Download, HardDrive, Lock, Search, X } from 'lucide-react'
import { invoke } from '@tauri-apps/api/core'

import { moving, useRows } from '@/downloads/downloads'
import { useSetup, type Model } from '@/setup/setup'
import { size } from '@/shell/bytes'
import { useDesk } from '@/shell/desk'
import { sentence } from '@/shell/failure'
import { useToasts } from '@/shell/toasts'
import { Capabilities } from './Capabilities'
import { FitVerdict } from './FitVerdict'
import {
  damage,
  download,
  matches,
  params,
  quant,
  spend,
  tokens,
  unreadable,
  useChoices,
  useSearch,
  type Choice,
  type Repo,
} from './models'
import styles from './Browser.module.css'

/** The three filter tags. `gguf` is the index, which publishes nothing else
 * this build can load. */
type Filter = 'all' | 'installed' | 'gguf'

const FILTERS: { filter: Filter; name: string }[] = [
  { filter: 'all', name: 'All' },
  { filter: 'installed', name: 'Installed' },
  { filter: 'gguf', name: 'GGUF' },
]

/** Which row the detail pane is showing. A model on disk is keyed by its
 * path, a repository by its id. */
type Selected = { kind: 'local'; path: string } | { kind: 'repo'; id: string }

/**
 * The Models browser, in two panes
 * ([#75](https://github.com/elpideus/demido-studio/issues/75)).
 *
 * **One component, three hosts.** The composer's model control and the empty
 * state's Browse models open it in [`ModelsWindow`] over the desk, and the
 * wizard's models step renders it in place. None of them has a second screen
 * for this, which is S1's one-component-two-hosts rule applied once more: what
 * the wizard offers is exactly what the desk offers. The only thing a host
 * decides is what choosing a model on disk means, which in the wizard
 * is choosing it for Finish and on the desk is loading it now.
 *
 * **Two panes, always.** v2's browser had never once laid out in two panes
 * because a container query sat on the wrong element (`done.md`). So there is
 * no query here at all: the panes are a flex row at every width, the list at
 * a fixed width and the detail taking the rest.
 *
 * **Projectors and drafts are never rows.** A model on disk is a `Local`,
 * which the scan builds from weights only and whose companions travel inside
 * it; a repository's download options are `Choice`s, which carry a projector
 * beside the weights that need it and no draft at all. Neither list has a
 * shape a projector could be offered in.
 */
export function Browser({ choose }: { choose: (path: string) => Promise<void> }) {
  const view = useSetup((setup) => setup.view)
  const [query, setQuery] = useState('')
  const [filter, setFilter] = useState<Filter>('all')
  const [selected, setSelected] = useState<Selected | null>(null)
  const { answer: index, asking } = useSearch(query)
  const said = query.trim()

  const library = view?.models
  const installed = useMemo(
    () => (library?.models ?? []).filter((model) => matches(model, query)),
    [library, query],
  )
  const repos = index?.state === 'read' ? index.found : []
  // An unreadable index lists the installed models whatever the filter: the
  // sentence saying the index is gone is only half the state without them.
  const showInstalled = filter !== 'gguf' || index?.state === 'unreadable'
  const showIndex = filter !== 'installed'
  const shownInstalled = showInstalled ? installed : []
  const shownRepos = showIndex ? repos : []

  // The right pane shows something the list is showing: the model that
  // answers, then the first installed one, then the first repository. A pane
  // that opened empty would ask for a click before it said anything, and one
  // left on a row a filter or a search took away would describe a model that
  // is not on screen.
  const visible =
    selected?.kind === 'local'
      ? shownInstalled.some((model) => model.path === selected.path)
      : selected?.kind === 'repo'
        ? shownRepos.some((found) => found.id === selected.id)
        : false
  useEffect(() => {
    if (visible) return
    const first =
      shownInstalled.find((model) => model.path === library?.chosen) ?? shownInstalled[0]
    const next: Selected | null = first
      ? { kind: 'local', path: first.path }
      : shownRepos[0]
        ? { kind: 'repo', id: shownRepos[0].id }
        : null
    if (key(next) !== key(selected)) setSelected(next)
  }, [visible, shownInstalled, shownRepos, library, selected])

  const local =
    selected?.kind === 'local'
      ? library?.models.find((model) => model.path === selected.path)
      : undefined
  const repo =
    selected?.kind === 'repo' ? repos.find((found) => found.id === selected.id) : undefined

  return (
    <div className={styles.browser}>
      <div className={styles.list}>
        <label className={styles.search}>
          <Search className={styles.icon} strokeWidth={1.8} aria-hidden />
          <input
            className={styles.field}
            aria-label="Search models"
            placeholder="Search models"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
          />
        </label>
        <div className={styles.filters} role="radiogroup" aria-label="Show">
          {FILTERS.map((tag) => (
            <button
              key={tag.filter}
              type="button"
              role="radio"
              aria-checked={filter === tag.filter}
              className={styles.filter}
              onClick={() => setFilter(tag.filter)}
            >
              {tag.name}
            </button>
          ))}
        </div>

        <div className={styles.rows}>
          {showInstalled && (
            <section className={styles.group} aria-label="Installed">
              <h3 className={styles.heading}>Installed</h3>
              {installed.length === 0 ? (
                <p className={styles.said}>
                  {library?.models.length
                    ? `No installed model matches "${said}".`
                    : 'Nothing is installed yet. A model downloaded here, or read from a folder below, is listed here.'}
                </p>
              ) : (
                installed.map((model) => (
                  <LocalRow
                    key={model.path}
                    model={model}
                    chosen={model.path === library?.chosen}
                    selected={selected?.kind === 'local' && selected.path === model.path}
                    select={() => setSelected({ kind: 'local', path: model.path })}
                  />
                ))
              )}
              {library?.damaged.map((file) => (
                <p key={file.path} className={styles.said}>
                  Not offered: {file.path}, {damage(file.damage)}.
                </p>
              ))}
            </section>
          )}

          {showIndex && (
            // First when it could not be read: the sentence saying so is the
            // news, and the installed models it points at are right below it.
            <section
              className={styles.group}
              aria-label="Hugging Face"
              data-first={index?.state === 'unreadable'}
            >
              <h3 className={styles.heading}>Hugging Face</h3>
              {!index ? (
                <p className={styles.said}>Asking Hugging Face.</p>
              ) : index.state === 'unreadable' ? (
                <div className={styles.dead} role="status">
                  <p className={styles.deadWhat}>The index could not be read.</p>
                  {/* The cause in words; what the connection itself said is
                   * a URL and a library's error, kept for whoever hovers. */}
                  <p className={styles.said} title={index.reason}>
                    {unreadable(index.cause)}{' '}
                    {filter === 'all' || filter === 'gguf'
                      ? 'The installed models below are still yours to use.'
                      : 'The installed models are still listed under Installed.'}
                  </p>
                </div>
              ) : repos.length === 0 ? (
                <p className={styles.said}>
                  {asking
                    ? 'Asking Hugging Face.'
                    : said
                      ? `Nothing in the index matches "${said}". Try a shorter name, or an author.`
                      : 'The index listed nothing.'}
                </p>
              ) : (
                repos.map((found) => (
                  <RepoRow
                    key={found.id}
                    repo={found}
                    installed={library?.models.some((model) => model.repo === found.id) ?? false}
                    selected={selected?.kind === 'repo' && selected.id === found.id}
                    select={() => setSelected({ kind: 'repo', id: found.id })}
                  />
                ))
              )}
            </section>
          )}
        </div>

        {/* The folders models are read from, named, so a model another tool
         * already fetched is found here rather than downloaded twice. */}
        <footer className={styles.folders} aria-label="Scan folders">
          <p className={styles.heading}>Read from</p>
          {library && (
            <p className={styles.folder} title={library.download}>
              <HardDrive className={styles.icon} strokeWidth={1.8} aria-hidden />
              <span className={styles.library}>Demido</span>
              <span className={styles.path}>{library.download}</span>
            </p>
          )}
          {library?.folders.map((folder) => (
            <p key={folder.path} className={styles.folder} title={folder.path}>
              <HardDrive className={styles.icon} strokeWidth={1.8} aria-hidden />
              <span className={styles.library}>{folder.library}</span>
              <span className={styles.path}>{folder.path}</span>
            </p>
          ))}
        </footer>
      </div>

      <div className={styles.detail}>
        {local ? (
          <LocalDetail model={local} chosen={local.path === library?.chosen} choose={choose} />
        ) : repo ? (
          <RepoDetail key={repo.id} repo={repo} />
        ) : (
          <p className={styles.said}>Choose a model on the left to see what it is.</p>
        )}
      </div>
    </div>
  )
}

function LocalRow({
  model,
  chosen,
  selected,
  select,
}: {
  model: Model
  chosen: boolean
  selected: boolean
  select: () => void
}) {
  return (
    <button type="button" className={styles.row} aria-pressed={selected} onClick={select}>
      <span className={styles.rowName}>
        {model.label}
        {chosen && <Check className={styles.icon} strokeWidth={1.8} aria-label="Answers" />}
      </span>
      <span className={styles.rowFoot}>
        <span className={styles.meta}>
          {size(model.bytes)} · {model.library}
        </span>
        <Capabilities facts={model.capabilities} from="file" compact />
      </span>
    </button>
  )
}

function RepoRow({
  repo,
  installed,
  selected,
  select,
}: {
  repo: Repo
  installed: boolean
  selected: boolean
  select: () => void
}) {
  return (
    <button type="button" className={styles.row} aria-pressed={selected} onClick={select}>
      <span className={styles.rowName}>
        {repo.id}
        {repo.gated && <Lock className={styles.icon} strokeWidth={1.8} aria-label="Gated" />}
      </span>
      <span className={styles.rowFoot}>
        <span className={styles.meta}>
          {repo.params !== undefined && `${params(repo.params)} · `}
          {repo.downloads.toLocaleString('en')} downloads
          {installed && ' · installed'}
        </span>
        <Capabilities facts={repo.stated} from="publisher" compact />
      </span>
    </button>
  )
}

/** A model on disk: what its file says, read from the file. */
function LocalDetail({
  model,
  chosen,
  choose,
}: {
  model: Model
  chosen: boolean
  choose: (path: string) => Promise<void>
}) {
  const [busy, setBusy] = useState(false)

  return (
    <article className={styles.page} aria-label={model.label}>
      <header className={styles.head}>
        <h2 className={styles.title}>{model.label}</h2>
        <p className={styles.path}>{model.path}</p>
      </header>
      <dl className={styles.facts}>
        <Fact name="Architecture" value={model.architecture} />
        <Fact
          name="Context"
          value={model.context === undefined ? undefined : tokens(model.context)}
        />
        <Fact name="Format" value="GGUF" />
        <Fact name="On disk" value={size(model.bytes)} />
        {model.shards !== undefined && <Fact name="Pieces" value={String(model.shards)} />}
        <Fact name="Repository" value={model.repo} />
        <Fact
          name="Library"
          value={model.borrowed ? `${model.library}, read and never written` : 'Demido'}
        />
      </dl>
      <section className={styles.section} aria-label="Capabilities">
        <h3 className={styles.heading}>Capabilities, read from the file</h3>
        <Capabilities facts={model.capabilities} from="file" />
      </section>
      <div className={styles.act}>
        <button
          type="button"
          className={styles.primary}
          disabled={busy || chosen}
          onClick={() => {
            setBusy(true)
            void choose(model.path).finally(() => setBusy(false))
          }}
        >
          <Check className={styles.icon} strokeWidth={1.8} aria-hidden />
          {chosen ? 'Answering with this' : 'Answer with this'}
        </button>
        <FitVerdict weights={model.bytes} path={model.path} />
      </div>
    </article>
  )
}

/** A repository in the index: what its publisher wrote, and what a person can
 * download from it. */
function RepoDetail({ repo }: { repo: Repo }) {
  const choices = useChoices(repo.id)
  const found = choices?.state === 'read' ? choices.found : []
  const [name, setName] = useState<string>()
  const choice = found.find((one) => one.name === name) ?? preferred(found)
  const queue = useRows()
  const [queuing, setQueuing] = useState(false)
  const queued = choice
    ? queue.some((row) => row.repo === repo.id && row.name === choice.name && moving(row))
    : false

  return (
    <article className={styles.page} aria-label={repo.id}>
      <header className={styles.head}>
        <h2 className={styles.title}>{repo.id}</h2>
        {repo.author && <p className={styles.path}>Published by {repo.author}</p>}
      </header>

      {/* Before the download, never after it: a keyless fetch of a gated file
       * is refused, so the refusal is said here while nothing has been spent. */}
      {repo.gated && (
        <p className={styles.gate} role="note">
          <Lock className={styles.icon} strokeWidth={1.8} aria-hidden />
          Gated. The publisher asks for a Hugging Face account and accepted terms before these files
          are fetched, and Demido downloads without an account, so it cannot be downloaded here.
        </p>
      )}

      <dl className={styles.facts}>
        <Fact name="Params" value={repo.params === undefined ? undefined : params(repo.params)} />
        <Fact name="Architecture" value={repo.architecture} />
        <Fact name="Domain" value={repo.pipeline} />
        <Fact name="Format" value="GGUF" />
        <Fact
          name="Context"
          value={repo.context === undefined ? undefined : tokens(repo.context)}
        />
      </dl>
      <section className={styles.section} aria-label="Capabilities">
        <h3 className={styles.heading}>Capabilities, as the publisher states them</h3>
        <Capabilities facts={repo.stated} from="publisher" />
      </section>

      <section className={styles.section} aria-label="Download options">
        <h3 className={styles.heading}>Download options</h3>
        {!choices ? (
          <p className={styles.said}>Reading the repository's files.</p>
        ) : choices.state === 'unreadable' ? (
          <p className={styles.said} title={choices.reason}>
            The files could not be read. {unreadable(choices.cause)}
          </p>
        ) : found.length === 0 ? (
          <p className={styles.said}>This repository publishes no weights this build can load.</p>
        ) : (
          <div className={styles.options} role="radiogroup" aria-label="Quantisation">
            {found.map((one) => (
              <button
                key={one.name}
                type="button"
                role="radio"
                aria-checked={one.name === choice?.name}
                className={styles.option}
                onClick={() => setName(one.name)}
              >
                <span className={styles.optionName}>{quant(one)}</span>
                <span className={styles.meta}>
                  {size(one.bytes)}
                  {one.pieces.length > 1 && ` · ${one.pieces.length} pieces`}
                  {one.projector && ' · with its projector'}
                </span>
              </button>
            ))}
          </div>
        )}
      </section>

      {choice && (
        <div className={styles.act}>
          <button
            type="button"
            className={styles.primary}
            disabled={repo.gated || queued || queuing}
            onClick={() => {
              setQueuing(true)
              void download(repo.id, choice).finally(() => setQueuing(false))
            }}
          >
            <Download className={styles.icon} strokeWidth={1.8} aria-hidden />
            {queued ? 'In the queue' : spend(choice)}
          </button>
          <FitVerdict weights={choice.weights} />
        </div>
      )}
    </article>
  )
}

/** One of a model's facts. A fact nobody stated says so rather than being
 * left out, so a pane of one kind has the same rows in the same places on
 * every model. */
function Fact({ name, value }: { name: string; value?: string }) {
  return (
    <div className={styles.fact}>
      <dt className={styles.factName}>{name}</dt>
      <dd className={styles.factValue} data-unstated={value === undefined}>
        {value ?? 'not stated'}
      </dd>
    </div>
  )
}

/** A selection as one comparable value. */
function key(selected: Selected | null): string {
  if (!selected) return ''
  return selected.kind === 'local' ? `local:${selected.path}` : `repo:${selected.id}`
}

/** The option a person is shown first: `Q4_K_M` where the repository has it,
 * which is what most people run, and otherwise the most faithful. */
function preferred(choices: Choice[]): Choice | undefined {
  return choices.find((choice) => choice.quant?.label === 'Q4_K_M') ?? choices[0]
}

/**
 * The browser as a window over the desk, which is where the composer's model
 * control and the empty state's Browse models open it.
 *
 * Answering with a model here loads it at once: the same two steps the
 * wizard's last page takes, so there is one way a model becomes the
 * conversation's.
 */
export function ModelsWindow() {
  const close = useDesk((desk) => desk.close)

  useEffect(() => {
    const dismiss = (event: KeyboardEvent) => {
      if (event.key === 'Escape') close()
    }
    document.addEventListener('keydown', dismiss)
    return () => document.removeEventListener('keydown', dismiss)
  }, [close])

  async function choose(path: string) {
    try {
      await invoke('setup_choose_model', { path })
    } catch (error) {
      useToasts.getState().show(sentence(error))
      return
    }
    await useSetup.getState().finish()
    close()
  }

  return (
    <>
      <div className={styles.scrim} aria-hidden />
      <section className={styles.window} role="dialog" aria-label="Models">
        <header className={styles.bar}>
          <h1 className={styles.name}>Models</h1>
          <button type="button" className={styles.close} aria-label="Close" onClick={close}>
            <X className={styles.icon} strokeWidth={1.8} aria-hidden />
          </button>
        </header>
        <div className={styles.body}>
          <Browser choose={choose} />
        </div>
      </section>
    </>
  )
}
