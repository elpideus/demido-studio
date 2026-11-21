/* Version and updates.
   The version is always on screen, at the foot of the navigation, with a dot when a newer one exists.
   This screen holds the rest: the check, what is new, the download, the restart, the versions of the
   parts Demido downloaded for you, the history, and "about". Scripted: nothing is downloaded. */
import { create } from 'zustand'
import { NavLink, useNavigate } from 'react-router'
import { RefreshCw } from 'lucide-react'
import type { Feature } from '@/app/registry'
import { setSetting, toast, useSetting, useStore } from '@/data/store'
import { Button, Facts, Glyph, Icon, NotBuilt, Page, Section, Select, Signal, Switch, Tip } from '@/ui'

export const VERSION = '0.1.0'
const NEXT = { version: '0.2.0', released: '18 November 2026', size: '14 MB' }
const NEW = [
  'Voice mode waits for the end of your sentence, and stops when you speak over it.',
  'Markets: indicators, drawing tools and a Pine editor.',
  'Sessions can be grouped. With a session folder, @ points at its files.',
  'Fixed: a model stayed loaded after its session was closed.',
  'Fixed: the tool audit skipped files that were added after installing.',
]
const PARTS: { name: string; have: string; next?: string; from: string }[] = [
  { name: 'Demido Studio, the window', have: VERSION, next: NEXT.version, from: "Demido's releases on GitHub" },
  { name: 'Demido core, the part that runs models and tools', have: VERSION, next: NEXT.version, from: 'comes with the window' },
  { name: 'llama.cpp · CUDA 12', have: 'build 6210', next: 'build 6274', from: 'llama.cpp releases on GitHub' },
  { name: 'CUDA runtime libraries', have: '12.9', from: 'NVIDIA' },
  { name: 'Embedding model', have: 'v2', from: 'Hugging Face' },
]

type Status = 'available' | 'checking' | 'current' | 'downloading' | 'ready'
const useUpdate = create<{ status: Status; progress: number; checked: string; skipped: boolean }>(() => ({ status: 'available', progress: 0, checked: 'today at 09:00', skipped: false }))
const put = useUpdate.setState
function check() {
  put({ status: 'checking' })
  setTimeout(() => put({ status: useUpdate.getState().skipped ? 'current' : 'available', checked: 'just now' }), 1400)
}
function download() {
  put({ status: 'downloading', progress: 0 })
  const tick = () => {
    if (useUpdate.getState().status !== 'downloading') return
    const progress = useUpdate.getState().progress + 4
    if (progress >= 100) return put({ status: 'ready', progress: 100 })
    put({ progress })
    setTimeout(tick, 90)
  }
  tick()
}

/** The version, at the foot of the navigation. One press from the Updates screen. */
function RailVersion() {
  const status = useUpdate((s) => s.status)
  const waiting = status === 'available' || status === 'ready'
  return (
    <Tip side="right" label={status === 'ready' ? `Version ${NEXT.version} is ready: restart to finish` : waiting ? `Version ${NEXT.version} is available` : `Demido Studio ${VERSION}`}>
      <NavLink to="/updates" className="app-rail__version" aria-label={`Demido Studio ${VERSION}${waiting ? `, version ${NEXT.version} is available` : ''}`}>
        <span className="d-mono">{VERSION}</span>
        {waiting && <span className="app-rail__dot" />}
      </NavLink>
    </Tip>
  )
}

function OpenUpdates() {
  const navigate = useNavigate()
  return <Button onClick={() => navigate('/updates')}>Open Updates</Button>
}

function UpdatesPage() {
  const { status, progress, checked } = useUpdate()
  const tools = useStore((s) => s.tools)
  const sessions = useStore((s) => s.sessions)
  const navigate = useNavigate()
  const daily = useSetting('updates.check', true)
  const auto = useSetting('updates.auto', false)
  const channel = useSetting<string>('updates.channel', 'stable')
  const toolUpdates = tools.filter((t) => t.installed && t.update)
  const busy = sessions.filter((s) => s.state === 'running' || s.state === 'needs-input')
  const details = `Demido Studio ${VERSION} (preview build)\nChannel: ${channel}\nSystem: Linux, NVIDIA RTX 3060, CUDA 12\nRuntime: llama.cpp build 6210`
  return (
    <Page narrow title="Updates" lead="Which version you have, what is newer, and the versions of the parts Demido downloaded for you."
      actions={<Button disabled={status === 'checking' || status === 'downloading'} onClick={check}><Icon icon={RefreshCw} />{status === 'checking' ? 'Checking…' : 'Check now'}</Button>}>
      <div className="d-card"><div className="d-card__body app-stack">
        <div className="app-row">
          <Glyph name={status === 'checking' || status === 'downloading' ? 'task-running' : status === 'current' ? 'task-done' : 'task-needs-input'} tone={status === 'available' || status === 'ready' ? 'caution' : undefined} size="1.5rem" />
          <div className="app-stack" style={{ gap: 0, flex: 1 }}>
            <span className="d-type-title-3">Demido Studio {VERSION}</span>
            <span className="d-type-caption d-text-secondary">Preview build · installed 12 November 2026 · last checked {checked}</span>
          </div>
        </div>
        {status === 'current' && <p className="d-text-secondary">You have the newest version on the {channel} channel.</p>}
        {status === 'checking' && <p className="d-text-secondary">Asking the release page…</p>}
        {(status === 'available' || status === 'downloading') && (
          <>
            <Signal level="note" actions={status === 'available' ? <><Button variant="primary" size="sm" onClick={download}>Download and install</Button><Button size="sm" variant="ghost" onClick={() => put({ status: 'current', skipped: true })}>Skip this version</Button></> : undefined}>
              Version {NEXT.version} is available. Released {NEXT.released} · {NEXT.size}.
            </Signal>
            {status === 'downloading' && (
              <div className="app-stack" style={{ gap: 'var(--d-space-6)' }}>
                <div className="d-meter d-meter--lg" role="meter" aria-valuenow={progress} aria-valuemin={0} aria-valuemax={100} aria-label="Downloading the update"><span style={{ '--d-meter': `${progress}%` } as React.CSSProperties} /></div>
                <div className="app-row"><span className="d-type-caption d-text-secondary">Downloading {NEXT.version}: {progress}%. You can keep working.</span><span className="app-spacer" /><Button size="sm" variant="ghost" onClick={() => put({ status: 'available', progress: 0 })}>Cancel</Button></div>
              </div>
            )}
          </>
        )}
        {status === 'ready' && (
          <Signal level="note" actions={<><Button variant="primary" size="sm" disabled={busy.length > 0} onClick={() => toast('Restarts Demido and opens it again where you were. Not built in the preview.')}>Restart now</Button><Button size="sm" variant="ghost" onClick={() => toast('It will be installed the next time you start Demido.')}>Later</Button></>}>
            Version {NEXT.version} is downloaded and checked. Restart to finish. Your sessions, settings and models stay as they are.
            {busy.length > 0 && <> {busy.length === 1 ? `"${busy[0].title}" is` : `${busy.length} sessions are`} still at work or waiting for you, so the restart waits.</>}
          </Signal>
        )}
        {status !== 'current' && status !== 'checking' && (
          <div>
            <p className="d-list__label" style={{ paddingInline: 0 }}>What is new in {NEXT.version}</p>
            <ul className="d-prose" style={{ margin: 0 }}>{NEW.map((n) => <li key={n}>{n}</li>)}</ul>
          </div>
        )}
      </div></div>

      <Section title="Parts" hint="Demido downloads these from the people who make them. Version numbers here are made up.">
        <div className="d-card"><div className="d-card__body">
          <table className="app-table">
            <thead><tr><th>Part</th><th>You have</th><th>Newest</th><th /></tr></thead>
            <tbody>
              {PARTS.map((p) => (
                <tr key={p.name}>
                  <td>{p.name}<br /><span className="d-type-caption d-text-tertiary">From {p.from}</span></td>
                  <td className="d-mono">{p.have}</td>
                  <td className="d-mono">{p.next && status !== 'current' ? p.next : <span className="d-text-tertiary">same</span>}</td>
                  <td style={{ textAlign: 'end', paddingInlineEnd: 0 }}>{p.next && p.name.startsWith('llama') && <Button size="sm" onClick={() => toast('Updates the runtime when no model is loaded. Not built in the preview.')}>Update</Button>}</td>
                </tr>
              ))}
              <tr>
                <td>Tools<br /><span className="d-type-caption d-text-tertiary">Each from its own publisher</span></td>
                <td className="d-mono">{tools.filter((t) => t.installed).length} installed</td>
                <td className="d-mono">{toolUpdates.length ? `${toolUpdates.length} with an update` : <span className="d-text-tertiary">same</span>}</td>
                <td style={{ textAlign: 'end', paddingInlineEnd: 0 }}>{toolUpdates.length > 0 && <Button size="sm" onClick={() => navigate(`/tools/${toolUpdates[0].id}`)}>See in Tools</Button>}</td>
              </tr>
            </tbody>
          </table>
        </div></div>
      </Section>

      <Section title="How updates behave">
        <div className="d-settings">
          <div className="d-setting"><div className="d-setting__text"><span className="d-setting__label">Check for updates</span><span className="d-setting__desc">Looks for a new version once a day, and when Demido starts.</span></div><div className="d-setting__control"><Switch label="Check for updates" checked={daily} onChange={(v) => setSetting('updates.check', v)} /></div></div>
          <div className="d-setting"><div className="d-setting__text"><span className="d-setting__label">Install updates automatically</span><span className="d-setting__desc">Downloads in the background and installs when no session is running. Off: Demido tells you and waits.</span></div><div className="d-setting__control"><Switch label="Install updates automatically" checked={auto} onChange={(v) => setSetting('updates.auto', v)} /></div></div>
          <div className="d-setting"><div className="d-setting__text"><span className="d-setting__label">Channel</span><span className="d-setting__desc">Stable gets finished versions. Beta gets them earlier, with rough edges.</span></div><div className="d-setting__control" style={{ width: '12rem' }}><Select label="Channel" value={channel} onChange={(v) => setSetting('updates.channel', v)} options={[{ value: 'stable', label: 'Stable' }, { value: 'beta', label: 'Beta' }]} /></div></div>
        </div>
        <p className="d-type-caption d-text-secondary">An update never interrupts a running session. The download is checked against the project's signature before anything is replaced, and the old version is kept until the new one has started once.</p>
      </Section>

      <Section title="History">
        <div className="d-card"><div className="d-card__body">
          <table className="app-table"><tbody>
            <tr><td className="d-mono">{VERSION}</td><td>Installed 12 November 2026</td><td style={{ textAlign: 'end', paddingInlineEnd: 0 }}><span className="d-badge">Running now</span></td></tr>
            <tr><td className="d-mono">0.0.9</td><td className="d-text-secondary">Kept, in case the new version does not start</td><td style={{ textAlign: 'end', paddingInlineEnd: 0 }}><NotBuilt>Going back a version</NotBuilt></td></tr>
          </tbody></table>
        </div></div>
      </Section>

      <Section title="About">
        <div className="d-card"><div className="d-card__body app-stack">
          <Facts rows={[['Version', `${VERSION} · preview build`], ['Licence', 'GPL-3.0-or-later'], ['Source', 'not public yet'], ['On a server, without a window', 'demido --version · demido update']]} />
          <div className="app-row">
            <Button size="sm" onClick={() => { navigator.clipboard?.writeText(details).then(() => toast('Details copied. Paste them into a bug report.'), () => toast(details)) }}>Copy details for a bug report</Button>
            <Button size="sm" variant="ghost" onClick={() => toast('Lists every part and its licence. Not built in the preview.')}>Third-party notices</Button>
            <NotBuilt>Scripted: nothing is downloaded</NotBuilt>
          </div>
        </div></div>
      </Section>
    </Page>
  )
}

export const feature: Feature = {
  id: 'updates', title: 'Updates', icon: RefreshCw, group: 'system', order: 6, path: 'updates', Page: UpdatesPage,
  // No entry of its own among the screens: the version at the foot of the navigation leads here.
  nav: [],
  rail: RailVersion,
  settings: [
    {
      id: 'updates', title: 'Updates', order: 9,
      settings: [
        { id: 'updates.check', type: 'switch', label: 'Check for updates', desc: 'Looks for a new version once a day, and when Demido starts.', default: true },
        { id: 'updates.auto', type: 'switch', label: 'Install updates automatically', desc: 'Installs when no session is running.', default: false, details: 'Default off: Demido tells you a version is ready and waits. On: it downloads in the background and installs the next time nothing is running. Either way a running session is never interrupted.' },
        { id: 'updates.channel', type: 'select', label: 'Update channel', desc: 'Which versions you are offered.', default: 'stable', options: [{ value: 'stable', label: 'Stable' }, { value: 'beta', label: 'Beta' }] },
        { id: 'updates.open', type: 'custom', label: 'This version', desc: `Demido Studio ${VERSION}. Release notes, parts and history are on the Updates screen.`, default: null, Control: OpenUpdates, action: true },
      ],
    },
  ],
}
