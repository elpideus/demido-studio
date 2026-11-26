import { useMemo, useState, type ComponentType } from 'react'
import { SearchX, Search, Settings as SettingsIcon } from 'lucide-react'
import { allFeatures, type Feature } from '@/app/registry'
import type { CoreSetting } from '@/data/core'
import { setSetting, useSetting, useStore } from '@/data/store'
import { Button, Icon, NotBuilt, Page, Segmented, Select, SettingRow, Slider, Switch, Empty } from '@/ui'
import type { SettingDef, SettingsSection } from './schema'

// ───────────── the accent control: the one setting with a shape of its own ─────────────
function AccentControl() {
  const mode = useSetting<string>('appearance.accent', 'auto')
  const custom = useSetting<string>('appearance.accentCustom', '#68efaf')
  return (
    <>
      {mode === 'custom' && <input className="d-colorwell" type="color" aria-label="Custom accent colour" value={custom} onChange={(e) => setSetting('appearance.accentCustom', e.target.value)} />}
      <Select label="Accent colour" value={mode} onChange={(v) => setSetting('appearance.accent', v)} options={[{ value: 'auto', label: 'Automatic' }, { value: 'green', label: 'Demido green' }, { value: 'custom', label: 'Custom' }]} />
    </>
  )
}
function SystemAccentControl() {
  const sim = useSetting<string>('preview.systemAccent', '')
  return <Select label="Pretend the system accent is" value={sim} onChange={(v) => setSetting('preview.systemAccent', v)} options={[{ value: '', label: 'Nothing reported' }, { value: '#e6a5d9', label: 'Your KDE accent today' }, { value: '#3daee9', label: 'Plasma blue' }, { value: '#f67400', label: 'Orange' }, { value: '#1b2a4a', label: 'Very dark navy' }]} />
}
function ResetPreview() {
  return (
    <Button variant="danger" onClick={() => { localStorage.removeItem('demido-preview-settings'); location.reload() }}>
      Reset the preview
    </Button>
  )
}
const pct = (v: number) => `${v}%`
export const CORE_SECTIONS: SettingsSection[] = [
  {
    id: 'appearance', title: 'Appearance', order: 0,
    settings: [
      { id: 'appearance.theme', type: 'segmented', label: 'Theme', desc: 'Dark, light, or whatever your system is set to.', default: 'system', options: [{ value: 'system', label: 'System' }, { value: 'dark', label: 'Dark' }, { value: 'light', label: 'Light' }] },
      { id: 'appearance.accent', type: 'custom', label: 'Accent colour', desc: "Uses your system's accent colour when it has one. Otherwise Demido green.", default: 'auto', Control: AccentControl,
        details: 'Default Automatic. Automatic follows the system accent whenever the system reports one. Whichever colour comes in, Demido keeps its hue and strength and holds its lightness where it reads on the current theme. Success stays Demido green whatever you pick.' },
      { id: 'preview.systemAccent', type: 'custom', label: 'System accent (preview only)', desc: 'A browser cannot read your system accent. Pick what to pretend it reports.', default: '', Control: SystemAccentControl, notBuilt: true,
        details: 'In the app Demido reads the accent from the system itself. "Your KDE accent today" is the value your desktop reported when the preview was built: #e6a5d9.' },
      { id: 'appearance.textSize', type: 'slider', label: 'Text size', desc: 'Scales the whole interface, not only the text.', default: 100, min: 85, max: 130, step: 5, format: pct,
        details: 'Default 100% · Range 85 to 130%. Every size in Demido is relative to this, so spacing and controls grow with the text.' },
    ],
  },
  {
    id: 'answers', title: 'Answers', lead: 'How Demido writes to you, and how it writes to models.', order: 1,
    settings: [
      { id: 'answers.style', type: 'select', label: 'Answer style', desc: 'Manual: the answer first, then short numbered steps.', default: 'manual', options: [{ value: 'manual', label: 'Manual' }, { value: 'plain', label: 'Plain prose' }],
        details: 'Default Manual. Written for attention that is short and valuable: one idea per sentence, exact values with units, no filler. Plain prose removes the style guidance and lets the model write as it likes; small models then ramble more.' },
      { id: 'answers.signals', type: 'switch', label: 'Signal words', desc: 'Marks risk with NOTE, CAUTION and WARNING, each used exactly.', default: true },
      { id: 'answers.caveman', type: 'switch', label: 'Short internal language', desc: 'Prompts, tool guidance, thinking and messages between agents use the fewest words that keep the meaning.', default: true,
        details: 'Default on. Small models lose the thread in long prose. This never changes what is written for you. Turning it off costs context: this system prompt goes from 212 to about 690 tokens.' },
      { id: 'answers.showPrompts', type: 'switch', label: 'Show what the model was told', desc: 'Adds a line under each answer that opens the exact prompt.', default: false },
    ],
  },
  {
    id: 'models', title: 'Models and memory', lead: 'Models load only when needed and unload when idle.', order: 2,
    settings: [
      { id: 'models.default', type: 'select', label: 'Default model', desc: 'New sessions start with this model.', default: 'qwen3-4b', options: [{ value: 'qwen3-4b', label: 'Qwen3 4B Instruct' }, { value: 'gemma-3n-e4b', label: 'Gemma 3n E4B' }, { value: 'phi-4-mini', label: 'Phi-4 mini' }] },
      { id: 'models.idleUnload', type: 'select', label: 'Idle unload', desc: 'Unloads a model after this long without use.', default: '5', options: [{ value: '1', label: '1 min' }, { value: '5', label: '5 min' }, { value: '15', label: '15 min' }, { value: '60', label: '1 h' }, { value: 'never', label: 'Never' }],
        details: 'Default 5 min · Range 0 to 120 min, or Never. Lower frees memory sooner. Higher avoids reload waits: about 4 s for a 4B model on this computer. In the preview a model unloads after 45 s.' },
      { id: 'models.lend', type: 'switch', label: 'Lend to agents and schedules', desc: 'Lets an agent or a schedule use the loaded model while the session waits for it.', default: true,
        details: 'Default on. Off means every agent needs memory of its own: on this GPU that allows no agents beside a 6 GB model.' },
      { id: 'models.reserve', type: 'slider', label: 'Memory kept free', desc: 'Never used by Demido, so your desktop stays smooth.', default: 0.9, min: 0.5, max: 3, step: 0.1, format: (v) => `${v.toFixed(1)} GB`,
        details: 'Default 0.9 GB, computed from your display setup. Lower fits larger models; too low and the desktop stutters or the load fails.' },
      { id: 'models.agents', type: 'select', label: 'Agents at once', desc: 'How many agents may run beside the main session.', default: 'auto', options: [{ value: 'auto', label: 'Automatic' }, { value: '0', label: 'None' }, { value: '1', label: '1' }, { value: '2', label: '2' }],
        details: 'Default Automatic: as many as fit. With Qwen3 4B loaded on 12 GB that is 1 agent sharing the model, or 5 if they only borrow it in turn.' },
    ],
  },
  {
    id: 'permissions', title: 'Permissions', lead: 'What Demido may do without asking.', order: 3,
    settings: [
      /* The same id as the core's registry: with a core, this row is drawn from its description. */
      { id: 'permissions.level', type: 'segmented', label: 'New sessions start at', desc: 'How much a model may do in a new session without asking you. Each session can be changed by itself.', default: 'balanced', options: [{ value: 'cautious', label: 'Cautious' }, { value: 'balanced', label: 'Balanced' }, { value: 'autonomous', label: 'Autonomous' }, { value: 'smart', label: 'Smart' }],
        details: 'Cautious always asks. Balanced asks only before things that could cause damage. Autonomous never asks. Smart lets the model decide and logs every decision. A model can never raise its own level: only you can.' },
      { id: 'permissions.sandbox', type: 'switch', label: 'Run commands in a sandbox', desc: 'Commands cannot leave the session folder or reach the network unless you allow it.', default: true, notBuilt: true },
      { id: 'permissions.untrusted', type: 'switch', label: 'Treat outside content as data', desc: 'Web pages, mail, files and messages can inform an answer but cannot give orders.', default: true,
        details: 'Default on, and it should stay on. This is the core of the protection against injected instructions. Off is for testing only.' },
    ],
  },
  {
    id: 'context', title: 'Context and compaction', order: 4,
    settings: [
      /* The same two ids as the core's registry, so that with a core these rows are drawn from its description, in this order. */
      { id: 'context.autoCompact', type: 'switch', label: 'Make long conversations shorter by themselves', desc: 'When a session nears what the model can hold, the older messages are replaced by a short account of them.', default: true },
      { id: 'context.compactAt', type: 'slider', label: 'Make shorter at', desc: 'How long a conversation may get before its older part is replaced by an account.', default: 24, min: 4, max: 32, step: 1, format: (v) => `${v}k`,
        details: 'Default 24k tokens, computed from the default model (32k context) and your free memory. Lower keeps answers fast and leaves room for agents; higher keeps more of the conversation word for word.' },
      { id: 'context.showSummary', type: 'switch', label: 'Show the summary', desc: 'After compaction, shows what was kept so you can correct it.', default: true },
      { id: 'context.compressOutput', type: 'switch', label: 'Compress command output', desc: 'Long output is shortened before the model sees it. The full output stays one press away.', default: true },
    ],
  },
  {
    id: 'server', title: 'Server and remote use', lead: 'Use Demido from a browser or another computer.', order: 8,
    settings: [
      { id: 'server.enabled', type: 'switch', label: 'Allow other devices to connect', desc: 'Off means only this computer can reach Demido.', default: false, notBuilt: true,
        details: 'Default off. On needs a passkey on every device and an encrypted connection. Demido can run commands, read mail and hold keys: do not expose it without both.' },
      { id: 'server.address', type: 'text', label: 'Listen on', desc: 'Address and port the server answers on.', default: '127.0.0.1:7341', mono: true, notBuilt: true },
    ],
  },
  // "Updates" is contributed by the updates feature (features/updates).
  {
    id: 'preview', title: 'This preview', lead: 'Everything here is scripted. Nothing leaves your browser.', order: 10,
    settings: [{ id: 'preview.reset', type: 'custom', label: 'Start over', desc: 'Clears your preview settings and reloads the fake data.', default: null, Control: ResetPreview, action: true }],
  },
]

function Control({ def }: { def: SettingDef }) {
  const value = useSetting<unknown>(def.id, def.default)
  switch (def.type) {
    case 'switch':
      return <Switch label={def.label} checked={Boolean(value)} onChange={(v) => setSetting(def.id, v)} />
    case 'select':
      return <Select label={def.label} value={String(value)} onChange={(v) => setSetting(def.id, v)} options={def.options} />
    case 'segmented':
      return <Segmented label={def.label} small value={String(value)} onChange={(v) => setSetting(def.id, v)} options={def.options} />
    case 'slider':
      return <Slider label={def.label} value={Number(value)} onChange={(v) => setSetting(def.id, v)} min={def.min} max={def.max} step={def.step} format={def.format} mark={def.default} />
    case 'text':
      return <input className={`d-input${def.mono ? ' d-input--mono' : ''}`} style={{ width: '14rem' }} aria-label={def.label} type={def.secret ? 'password' : 'text'} placeholder={def.placeholder} value={String(value)} onChange={(e) => setSetting(def.id, e.target.value)} />
    case 'custom':
      return <def.Control />
  }
}

/** `preview` is true when a core is behind these pages and this setting is not one of the core's: nothing real reads it yet. */
function Row({ def, preview }: { def: SettingDef; preview: boolean }) {
  const all = useStore((s) => s.settings)
  const changed = def.type !== 'custom' ? all[def.id] !== undefined && all[def.id] !== def.default : def.id === 'appearance.accent' && all[def.id] !== undefined && all[def.id] !== 'auto'
  return (
    <SettingRow label={def.label} desc={<>{def.desc} {(def.notBuilt || (preview && !def.action)) && <NotBuilt />}</>} details={def.details} changed={changed} onReset={() => setSetting(def.id, undefined)} wide={def.type === 'slider'}>
      <Control def={def} />
    </SettingRow>
  )
}

// ───────────── settings the core declares: drawn from the core's own description ─────────────
/** Controls with a shape of their own, by setting id. Everything else is drawn from the setting's kind. */
const OWN_CONTROL: Record<string, ComponentType> = { 'appearance.accent': AccentControl }
/** What the system's accent is, said under the Accent setting: Automatic follows it. */
function SystemAccentNote() {
  const seen = useStore((s) => s.systemAccent)
  if (!seen) return null
  if (!seen.here) return <> This window is on another computer than the core, so your system's accent cannot be read here.</>
  if (!seen.supported) return <> Reading the system accent on this system: <NotBuilt /></>
  if (!seen.accent) return <> Your system reports no accent colour.</>
  return <> Your system reports <span className="app-swatch" style={{ background: seen.accent }} aria-hidden="true" /> <code>{seen.accent}</code>.</>
}
/** Words added under a setting's description, by setting id. */
const OWN_NOTE: Record<string, ComponentType> = { 'appearance.accent': SystemAccentNote }
/** Rows that only exist so the preview can pretend. With a core they are gone: the real thing is read instead. */
const PREVIEW_ONLY = new Set(['preview.systemAccent'])
/** Settings that are shown inside another setting's control, so they get no row of their own. */
const INSIDE_ANOTHER = new Set(['appearance.accentCustom'])
/** A choice between a few short words reads faster side by side than in a list. */
const fitsSideBySide = (options: { label: string }[]) => options.length <= 4 && options.reduce((n, o) => n + o.label.length, 0) <= 32

function CoreControl({ entry }: { entry: CoreSetting }) {
  const { def, value } = entry
  const Own = OWN_CONTROL[def.id]
  if (Own) return <Own />
  const kind = def.kind
  switch (kind.type) {
    case 'switch':
      return <Switch label={def.label} checked={Boolean(value)} onChange={(v) => setSetting(def.id, v)} />
    case 'select':
      return fitsSideBySide(kind.options)
        ? <Segmented label={def.label} small value={String(value)} onChange={(v) => setSetting(def.id, v)} options={kind.options} />
        : <Select label={def.label} value={String(value)} onChange={(v) => setSetting(def.id, v)} options={kind.options} />
    case 'slider':
      return <Slider label={def.label} value={Number(value)} onChange={(v) => setSetting(def.id, v)} min={kind.min} max={kind.max} step={kind.step} format={(v) => (kind.unit === '%' ? `${v}%` : `${v} ${kind.unit}`)} mark={Number(def.default)} />
    case 'text':
      return <input className={`d-input${kind.code ? ' d-input--mono' : ''}`} style={{ width: '14rem' }} aria-label={def.label} type={kind.secret ? 'password' : 'text'} value={String(value)} onChange={(e) => setSetting(def.id, e.target.value)} />
    case 'colour':
      return <input className="d-colorwell" type="color" aria-label={def.label} value={String(value)} onChange={(e) => setSetting(def.id, e.target.value)} />
  }
}

function CoreRow({ entry }: { entry: CoreSetting }) {
  const { def } = entry
  const Note = OWN_NOTE[def.id]
  return (
    <SettingRow label={def.label} desc={<>{def.desc}{Note && <Note />} {!def.built && <NotBuilt />}</>} details={def.details ?? undefined} changed={entry.changed} onReset={() => setSetting(def.id, undefined)} wide={def.kind.type === 'slider'}>
      <CoreControl entry={entry} />
    </SettingRow>
  )
}

interface Shown { key: string; words: string; core?: CoreSetting; def?: SettingDef }
interface ShownSection { id: string; title: string; lead?: string; order: number; rows: Shown[] }

function SettingsPage() {
  const [q, setQ] = useState('')
  const core = useStore((s) => s.core)
  const coreSections = useStore((s) => s.coreSections)
  const live = Object.keys(core).length > 0
  /* One list from two sources. A setting the core declares is drawn from the core's description, in the place
     this screen already had for it; the preview's own copy of it is dropped. What the core declares and this
     screen never had is added under its section. */
  const sections = useMemo<ShownSection[]>(() => {
    const placed = new Set<string>()
    const row = (title: string, core?: CoreSetting, def?: SettingDef): Shown => {
      const d = core?.def ?? def!
      return { key: d.id, words: `${d.label} ${d.desc} ${d.details ?? ''} ${title}`.toLowerCase(), core, def }
    }
    const out: ShownSection[] = [...CORE_SECTIONS, ...allFeatures().flatMap((f: Feature) => f.settings ?? [])].map((s) => ({
      id: s.id, title: s.title, lead: s.lead, order: s.order,
      rows: s.settings.filter((d) => (!INSIDE_ANOTHER.has(d.id) || !core[d.id]) && !(live && PREVIEW_ONLY.has(d.id))).map((d) => {
        if (core[d.id]) placed.add(d.id)
        return core[d.id] ? row(s.title, core[d.id]) : row(s.title, undefined, d)
      }),
    }))
    for (const entry of Object.values(core)) {
      if (placed.has(entry.def.id) || INSIDE_ANOTHER.has(entry.def.id)) continue
      let section = out.find((s) => s.id === entry.def.section)
      if (!section) {
        const known = coreSections.find((s) => s.id === entry.def.section)
        section = { id: entry.def.section, title: known?.title ?? entry.def.section, lead: known?.lead ?? undefined, order: 9, rows: [] }
        out.push(section)
      }
      section.rows.push(row(section.title, entry))
    }
    return out.sort((a, b) => a.order - b.order)
  }, [core, coreSections, live])
  const needle = q.trim().toLowerCase()
  const shown = sections.map((s) => ({ ...s, rows: s.rows.filter((r) => !needle || r.words.includes(needle)) })).filter((s) => s.rows.length)
  const total = sections.reduce((n, s) => n + s.rows.length, 0)
  return (
    <div className="app-split">
      <aside className="app-side" aria-label="Settings sections">
        <div className="app-side__head"><span className="d-type-headline">Settings</span></div>
        <label className="d-input-wrap">
          <Icon icon={Search} />
          <input className="d-input" type="search" placeholder={`Search ${total} settings`} aria-label="Search settings" value={q} onChange={(e) => setQ(e.target.value)} />
        </label>
        <div className="app-side__scroll">
          <ul className="d-list">
            {shown.map((s) => (
              <li key={s.id}>
                <a className="d-row d-row--compact" href={`#/settings`} onClick={(e) => { e.preventDefault(); document.getElementById(`set-${s.id}`)?.scrollIntoView({ block: 'start' }) }}>
                  <span className="d-row__text"><span className="d-row__title">{s.title}</span></span>
                  <span className="d-row__meta">{s.rows.length}</span>
                </a>
              </li>
            ))}
          </ul>
        </div>
        <p className="app-side__foot">
          Every setting is here. Nothing is hidden behind an advanced mode: detail is one step away, under Details.
          {live && ' Settings the core keeps are saved on this computer and are the same in every window. The others are marked Not built yet: they still belong to the scripted preview and stay in this browser.'}
        </p>
      </aside>
      <Page narrow title="Settings" lead="Each row says what it does. A changed one resets with one press." about="Details, under a row, gives the default, the range and the trade-off.">
        {shown.map((s) => (
          <section key={s.id} id={`set-${s.id}`} className="d-settings" style={{ scrollMarginTop: 'var(--d-space-16)' }}>
            <h2 className="d-settings__title">{s.title}{s.lead && <span style={{ textTransform: 'none', letterSpacing: 0, fontWeight: 'var(--d-font-weight-small)' }}> · {s.lead}</span>}</h2>
            {s.rows.map((r) => (r.core ? <CoreRow key={r.key} entry={r.core} /> : <Row key={r.key} def={r.def!} preview={live} />))}
          </section>
        ))}
        {!shown.length && <Empty icon={SearchX} title={`No setting matches “${q}”`} text="Try another word: every setting's name and description is searched." />}
      </Page>
    </div>
  )
}

export const feature: Feature = { id: 'settings', title: 'Settings', icon: SettingsIcon, group: 'system', order: 9, path: 'settings', Page: SettingsPage }
