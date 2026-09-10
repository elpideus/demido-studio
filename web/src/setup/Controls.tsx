import { useState } from 'react'
import { Check, FolderPlus, HardDrive, Link2, Plus, X } from 'lucide-react'

import {
  useSetup,
  type Availability,
  type Ecosystem,
  type ManifestGroup,
  type Model,
  type Reason,
  type RowState,
  type RuntimeRow,
} from './setup'
import styles from './Controls.module.css'

/**
 * The set-up controls: one component per decision, and **two hosts**.
 *
 * This is the wizard's single strongest constraint
 * (`docs/rules/setup.md` section 2): "Every step renders the same control the
 * settings page renders. One component per decision, two hosts. The accelerator
 * row in the wizard and the accelerator row in settings are the same code, so
 * they cannot come to disagree, and nothing built here is thrown away when
 * set-up is over."
 *
 * Each control takes no props at all and reads the one store, which is the
 * strongest form that rule can take: there is nothing a host could pass
 * differently, so there is nothing two hosts can disagree about. What a host
 * decides is where the control sits and what heading is over it.
 *
 * The words live here rather than in Rust, which is `demido-hardware`'s rule:
 * every reason and every availability crosses the boundary as a fact, and the
 * window writes the sentence.
 */

/**
 * The accelerator row: pre-selected from detection with the reason attached,
 * and still overridable.
 *
 * `docs/rules/setup.md` section 3: nothing detectable is ever asked as a
 * question, and a step that answers itself still says so. Every accelerator is
 * a row, including ROCm and Vulkan, which say no build is fetched for them yet
 * rather than being hidden.
 */
export function AcceleratorControl() {
  const view = useSetup((setup) => setup.view)
  const choose = useSetup((setup) => setup.choose)
  if (!view) return null
  const { rows, preselection, chosen, overridden, present } = view.accelerator

  return (
    <div className={styles.control}>
      <p className={styles.detected}>
        {detected(preselection.reason)} {label(preselection.ecosystem)} is pre-selected.
        {overridden && ' You have chosen another.'}
      </p>
      <div className={styles.rows} role="radiogroup" aria-label="Accelerator">
        {rows.map((row) => {
          const offered = row.availability.availability === 'offered'
          return (
            <button
              key={row.ecosystem}
              type="button"
              role="radio"
              aria-checked={row.ecosystem === chosen}
              // A row with no build is not disabled: it is a real row that
              // states why it cannot be taken, and a disabled button states
              // nothing. Choosing it changes nothing, which is
              // `Selector::choose`'s own refusal.
              className={styles.option}
              data-chosen={row.ecosystem === chosen}
              data-offered={offered}
              onClick={() => void choose(row.ecosystem)}
            >
              <span className={styles.optionName}>
                <span className={styles.vendor}>
                  {/* The vendor mark, at full strength only where the hardware
                      is present and in ink everywhere else, which is the rule
                      `design/tokens.css` states over the `--brand-*` block. A
                      swatch rather than a logo: the use is nominative, and a
                      picker of four logos is the logo wall that block refuses.
                      CPU carries none, because it names no vendor.

                      `present` and not `offered`: a card that is here with no
                      build fetched for it yet is still a card that is here, and
                      greying its vendor over our own manifest would be the mark
                      saying something about the hardware that is not true. */}
                  {mark(row.ecosystem) && (
                    <span
                      className={styles.mark}
                      data-brand={mark(row.ecosystem)}
                      data-present={present.includes(row.ecosystem)}
                      aria-hidden
                    />
                  )}
                  {label(row.ecosystem)}
                </span>
                {row.ecosystem === chosen && (
                  <Check className={styles.icon} strokeWidth={1.8} aria-hidden />
                )}
              </span>
              <span className={styles.optionWhy}>{availability(row.availability)}</span>
            </button>
          )
        })}
      </div>
    </div>
  )
}

/**
 * The manifest: every fetch stating its download size and its size on disk
 * before a byte is spent.
 *
 * Checkboxes, all on by default, and a row cleared is a row set-up leaves
 * alone. `docs/rules/setup.md` section 4: "Required is not a synonym for
 * forced", so leaving with less than everything is a smaller install rather
 * than a broken one.
 *
 * The two groups are drawn by one loop over data. When uv, Python, SearXNG,
 * Node, `agent-browser` and Chrome are pinned they are rows here and this file
 * does not change, which is what "the capability group is data, not a second
 * screen" means in code.
 */
export function ManifestControl() {
  const view = useSetup((setup) => setup.view)
  const busy = useSetup((setup) => setup.busy)
  const fetching = useSetup((setup) => setup.fetching)
  const failed = useSetup((setup) => setup.failed)
  const fetch = useSetup((setup) => setup.fetch)
  const cancelFetch = useSetup((setup) => setup.cancelFetch)
  if (!view) return null

  const wanted = view.manifest.flatMap((group) =>
    group.rows.filter((row) => row.ticked && row.state.state === 'absent'),
  )
  const download = total(wanted, (row) => row.downloadMib)
  const onDisk = total(wanted, (row) => row.onDiskMib)

  return (
    <div className={styles.control}>
      {view.manifest.length === 0 && (
        <p className={styles.detected}>
          Nothing is pinned for this machine, so there is nothing to fetch here.
        </p>
      )}
      {view.manifest.map((group) => (
        <div key={group.group} className={styles.group}>
          <p className={styles.groupName}>{groupName(group)}</p>
          {group.rows.map((row) => (
            <RuntimeRowControl key={row.id} row={row} />
          ))}
        </div>
      ))}
      {wanted.length > 0 && (
        <div className={styles.foot}>
          <p className={styles.total}>
            {mib(download)} to download, {mib(onDisk)} on disk.
          </p>
          {/* While a fetch runs the cancel is what this foot offers, because a
              second Fetch would be the same fetch and the thing a person wants
              at that moment is the way out. What has arrived stays on disk. */}
          {busy ? (
            <button type="button" className={styles.quiet} onClick={() => void cancelFetch()}>
              Cancel
            </button>
          ) : (
            <button type="button" className={styles.action} onClick={() => void fetch()}>
              {failed ? 'Try again' : 'Fetch'}
            </button>
          )}
        </div>
      )}
      {/* Failed is `rose` with the retry in place (`design/system.md`): the
          button above is the retry, and it says so rather than being a second
          control that appears here. */}
      {failed && !busy && (
        <p className={styles.failed} role="status">
          {/* The full stop is here because no `demido_core::Error` carries one,
              and two sentences run together without it. */}
          {failed}. What arrived is still on disk, so taking it up again costs the rest rather than
          all of it.
        </p>
      )}
      {fetching && (
        <div className={styles.progress}>
          <p className={styles.total}>
            {fetching.archive}: {mib(fetching.bytes / (1024 * 1024))} of{' '}
            {mib(fetching.total / (1024 * 1024))}
          </p>
          <div className={styles.bar}>
            <div
              className={styles.filled}
              style={{ width: `${fetching.total ? (fetching.bytes / fetching.total) * 100 : 0}%` }}
            />
          </div>
        </div>
      )}
    </div>
  )
}

/**
 * One runtime row, with the escape section 7 gives it.
 *
 * "Each runtime row offers point at one I already have", which makes the row
 * linked rather than managed: Demido reads and launches it, never writes
 * inside it, and never counts its bytes as disk Demido spent.
 */
function RuntimeRowControl({ row }: { row: RuntimeRow }) {
  const tick = useSetup((setup) => setup.tick)
  const link = useSetup((setup) => setup.link)
  const busy = useSetup((setup) => setup.busy)
  const [pointing, setPointing] = useState(false)
  const [path, setPath] = useState('')

  return (
    <div className={styles.row}>
      <label className={styles.check}>
        <input
          type="checkbox"
          className={styles.checkbox}
          checked={row.ticked}
          // A row that is already on disk is not a fetch anybody is choosing,
          // and a checkbox that could clear it would read as a delete.
          disabled={row.state.state !== 'absent'}
          onChange={(event) => void tick(row.id, event.target.checked)}
        />
        <span className={styles.rowName}>{row.id}</span>
      </label>
      <p className={styles.rowWhy}>{state(row)}</p>
      <ul className={styles.archives}>
        {row.archives.map((archive) => (
          <li key={archive.name} className={styles.archive}>
            <span className={styles.archiveName}>{archive.name}</span>
            <span className={styles.archiveSize}>
              {mib(archive.downloadMib)} down, {mib(archive.onDiskMib)} on disk, {archive.license}
            </span>
          </li>
        ))}
      </ul>
      {pointing ? (
        <div className={styles.point}>
          <input
            type="text"
            className={styles.path}
            value={path}
            placeholder="Path to the binary"
            aria-label="Path to a binary you already have"
            onChange={(event) => setPath(event.target.value)}
          />
          <button
            type="button"
            className={styles.action}
            disabled={busy || path.trim() === ''}
            onClick={() => void link(row.id, path.trim()).then(() => setPointing(false))}
          >
            {busy ? 'Checking' : 'Use it'}
          </button>
          <button type="button" className={styles.quiet} onClick={() => setPointing(false)}>
            Cancel
          </button>
        </div>
      ) : (
        <button type="button" className={styles.quiet} onClick={() => setPointing(true)}>
          <Link2 className={styles.icon} strokeWidth={1.8} aria-hidden />
          Point at one I already have
        </button>
      )}
    </div>
  )
}

/**
 * The model folder, pre-filled from a readable folder already on the machine.
 *
 * Section 7's first escape and the brief's own mechanism, so a person with
 * models from LM Studio moves no files and makes no symlinks. A folder is
 * confirmed, never adopted: what is offered here is what was found, and
 * nothing is read from a folder nobody said yes to.
 */
export function ModelFolderControl() {
  const view = useSetup((setup) => setup.view)
  const add = useSetup((setup) => setup.addFolder)
  const remove = useSetup((setup) => setup.removeFolder)
  const [typed, setTyped] = useState('')
  if (!view) return null
  const { folders, suggested, models } = view.models

  return (
    <div className={styles.control}>
      <div className={styles.rows}>
        {folders.map((folder) => {
          const held = models.filter((model) => model.folder === folder).length
          return (
            <div key={folder} className={styles.folder}>
              <HardDrive className={styles.icon} strokeWidth={1.8} aria-hidden />
              <span className={styles.folderPath}>{folder}</span>
              <span className={styles.folderCount}>
                {held === 0 ? 'nothing readable' : `${held} model${held === 1 ? '' : 's'}`}
              </span>
              <button
                type="button"
                className={styles.remove}
                aria-label={`Stop reading models from ${folder}`}
                onClick={() => void remove(folder)}
              >
                <X className={styles.icon} strokeWidth={1.8} aria-hidden />
              </button>
            </div>
          )
        })}
        {folders.length === 0 && (
          <p className={styles.detected}>No folder is being read for models yet.</p>
        )}
      </div>
      {suggested.length > 0 && (
        <div className={styles.rows}>
          <p className={styles.detected}>Already on this machine:</p>
          {suggested.map((folder) => (
            <button
              key={folder}
              type="button"
              className={styles.suggested}
              onClick={() => void add(folder)}
            >
              <FolderPlus className={styles.icon} strokeWidth={1.8} aria-hidden />
              <span className={styles.folderPath}>{folder}</span>
            </button>
          ))}
        </div>
      )}
      <div className={styles.point}>
        <input
          type="text"
          className={styles.path}
          value={typed}
          placeholder="Another folder"
          aria-label="Another folder to read models from"
          onChange={(event) => setTyped(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === 'Enter' && typed.trim() !== '') {
              void add(typed.trim()).then(() => setTyped(''))
            }
          }}
        />
        <button
          type="button"
          className={styles.action}
          disabled={typed.trim() === ''}
          onClick={() => void add(typed.trim()).then(() => setTyped(''))}
        >
          <Plus className={styles.icon} strokeWidth={1.8} aria-hidden />
          Add
        </button>
      </div>
    </div>
  )
}

/**
 * Which model answers.
 *
 * The same list the composer will be talking to, because the wizard's last
 * step and the composer are told by one place (`demido_setup::target`).
 */
export function ModelControl() {
  const view = useSetup((setup) => setup.view)
  const choose = useSetup((setup) => setup.chooseModel)
  if (!view) return null
  const { models, chosen } = view.models

  if (models.length === 0) {
    return (
      <p className={styles.detected}>
        No model was read out of those folders. Add a folder that holds a GGUF file, or download one
        into one of them.
      </p>
    )
  }

  return (
    <div className={styles.rows} role="radiogroup" aria-label="Model">
      {models.map((model: Model) => (
        <button
          key={model.path}
          type="button"
          role="radio"
          aria-checked={model.path === chosen}
          className={styles.option}
          data-chosen={model.path === chosen}
          data-offered
          onClick={() => void choose(model.path)}
        >
          <span className={styles.optionName}>
            {model.name}
            {model.path === chosen && (
              <Check className={styles.icon} strokeWidth={1.8} aria-hidden />
            )}
          </span>
          <span className={styles.optionWhy}>
            {mib(model.sizeMib)} in {model.folder}
          </span>
        </button>
      ))}
    </div>
  )
}

/** What detection saw, in this application's own words. */
function detected(reason: Reason): string {
  switch (reason.reason) {
    case 'cuda-driver':
      return `${reason.adapter}, driver reports CUDA ${reason.driver.major}.${reason.driver.minor}.`
    case 'no-cuda-driver':
      return `${reason.adapter}, and its driver reports no CUDA.`
    case 'adapter':
      return `${reason.adapter}.`
    case 'no-adapter':
      return 'No display adapter was found.'
  }
}

/** What a row can offer, or why it cannot. */
function availability(availability: Availability): string {
  switch (availability.availability) {
    case 'offered':
      return `${mib(availability.download_mib)} to download, ${mib(availability.on_disk_mib)} on disk.`
    case 'needs-cuda-driver':
      return availability.runs === null
        ? 'No CUDA driver is installed, so no build here can load.'
        : `The driver runs CUDA ${availability.runs.major}.${availability.runs.minor}, which is older than the pinned build.`
    case 'no-build-yet':
      return 'No build is fetched for this yet.'
  }
}

/** What the ledger says a row is. */
function state(row: RuntimeRow): string {
  const at: RowState = row.state
  switch (at.state) {
    case 'managed':
      return `Fetched at ${at.pin}, ${mib(at.on_disk_mib)} on disk.`
    case 'linked':
      return `Using ${at.path}. Demido did not fetch it and will never delete it.`
    case 'absent':
      return at.reason === null
        ? 'Not fetched yet.'
        : `Not in use: ${at.reason} Demido kept nothing that does not work.`
  }
}

function groupName(group: ManifestGroup): string {
  return group.group === 'required'
    ? 'Required, and nothing answers without it'
    : 'Capabilities, each a feature that silently does not exist without it'
}

/** Every size in this application is MiB, one decimal, the way section 4
 * writes them. */
function mib(value: number): string {
  return `${value.toFixed(1)} MiB`
}

function total(rows: RuntimeRow[], of: (row: RuntimeRow) => number): number {
  return rows.reduce((sum, row) => sum + of(row), 0)
}

/** What an accelerator is called, which is not what its slug is. */
/** Whose hardware a row is about, or nothing when it names no vendor. */
function mark(ecosystem: Ecosystem): 'nvidia' | 'amd' | 'vulkan' | null {
  switch (ecosystem) {
    case 'cuda':
      return 'nvidia'
    case 'rocm':
      return 'amd'
    case 'vulkan':
      return 'vulkan'
    case 'cpu':
      return null
  }
}

function label(ecosystem: Ecosystem): string {
  switch (ecosystem) {
    case 'cuda':
      return 'CUDA'
    case 'rocm':
      return 'ROCm'
    case 'vulkan':
      return 'Vulkan'
    case 'cpu':
      return 'CPU'
  }
}
