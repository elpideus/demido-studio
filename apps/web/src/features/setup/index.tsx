import { useState } from 'react'
import { HardDrive } from 'lucide-react'
import type { Feature } from '@/app/registry'
import { useCore, useCoreData } from '@/data/core'
import type { CoreSystem } from '@/data/core'
import { SYSTEM } from '@/data/fixtures'
import { toast } from '@/data/store'
import { Button, Facts, Glyph, NotBuilt, Page, Section, Signal, Tasks } from '@/ui'

interface Part {
  id: string
  name: string
  what: string
  from: string
  size: string
  state: 'installed' | 'available' | 'unsuitable'
  why?: string
  recommended?: boolean
}
const PARTS: Part[] = [
  { id: 'llama-cuda', name: 'llama.cpp · CUDA 12', what: 'Runs models on your NVIDIA GPU.', from: 'llama.cpp releases on GitHub', size: '0.4 GB', state: 'installed', recommended: true, why: 'Recommended: you have an NVIDIA GPU with a CUDA 12 driver.' },
  { id: 'cuda-rt', name: 'CUDA runtime libraries', what: 'Needed by the CUDA build.', from: 'NVIDIA', size: '1.1 GB', state: 'installed' },
  { id: 'llama-vulkan', name: 'llama.cpp · Vulkan', what: 'Runs on any modern GPU. Slower than CUDA on NVIDIA.', from: 'llama.cpp releases on GitHub', size: '0.1 GB', state: 'available' },
  { id: 'llama-rocm', name: 'llama.cpp · ROCm', what: 'For AMD GPUs.', from: 'llama.cpp releases on GitHub', size: '0.6 GB', state: 'unsuitable', why: 'Not for this computer: no AMD GPU found.' },
  { id: 'embed', name: 'Embedding model', what: 'Lets Demido search and pick tools by meaning.', from: 'Hugging Face', size: '0.3 GB', state: 'installed' },
  { id: 'browser', name: 'Built-in browser', what: 'For pages that need JavaScript, and for the Browser screen.', from: 'Chrome for Testing, from Google', size: '0.2 GB', state: 'available' },
  { id: 'stt', name: 'Speech to text', what: 'For dictation and voice mode.', from: 'Hugging Face', size: '0.5 GB', state: 'available' },
  { id: 'tts', name: 'Voice', what: 'Reads answers aloud.', from: 'Hugging Face', size: '0.3 GB', state: 'available' },
]

/** This computer, read by the core. The first screen that shows something real. */
function RealComputer({ version }: { version: string }) {
  const { data, error, again } = useCoreData<CoreSystem>('/system', true)
  if (error) return <Signal level="caution" actions={<Button size="sm" onClick={again}>Try again</Button>}>The core could not read this computer: {error}</Signal>
  if (!data) return <p className="d-text-secondary"><Glyph name="task-running" /> Reading this computer…</p>
  const r = data.runtime
  return (
    <div className="d-card"><div className="d-card__body app-stack">
      <div className="app-row"><span className="d-badge" data-tone="success"><Glyph name="task-done" />Read from this computer</span><span className="d-type-caption d-text-tertiary">by Demido's core {version}, just now</span><span className="app-spacer" /><Button size="sm" variant="ghost" onClick={again}>Read again</Button></div>
      <Facts rows={[
        ...data.gpus.map((g): [string, string] => ['Graphics', [g.name, g.shares_memory ? "shares the computer's memory" : g.vram_gb != null ? `${g.vram_gb.toFixed(1)} GB${g.vram_used_gb != null ? ` (${g.vram_used_gb.toFixed(1)} GB in use)` : ''}` : null, g.driver && `driver ${g.driver}`, g.cuda && `supports CUDA ${g.cuda}`].filter(Boolean).join(' · ')]),
        ...(data.gpus.length ? [] : [['Graphics', 'none that can run models was found'] as [string, string]]),
        ['Processor', `${data.cpu.model} · ${data.cpu.threads} threads${data.cpu.features.length ? ` · ${data.cpu.features.join(', ')}` : ''}`],
        ['Memory', `${data.memory.total_gb.toFixed(1)} GB, ${data.memory.available_gb.toFixed(1)} GB free`],
        ['System', `${data.os.name} ${data.os.version} · kernel ${data.os.kernel} · ${data.os.arch}`],
      ]} />
      <div>
        <p className="d-list__label" style={{ paddingInline: 0 }}>Model runtime for this computer</p>
        <p><strong>{r.recommended.name}</strong> <span className="d-badge">Recommended</span></p>
        <p className="d-text-secondary">{r.recommended.why}</p>
        <p className="d-type-caption d-text-tertiary d-mono">{r.recommended.file}</p>
        {r.others.length > 0 && <p className="d-type-caption d-text-secondary">Would also run: {r.others.map((b) => b.name.replace(`${r.runtime} · `, '')).join(', ')}.</p>}
        <p className="d-type-caption d-text-tertiary">From {r.source.replace('https://', '')}. Demido downloads it from there; it does not ship it.</p>
      </div>
      <NotBuilt block>Downloading and starting the runtime. The list of parts below is still scripted.</NotBuilt>
    </div></div>
  )
}

function SetupPage() {
  const core = useCore()
  const [parts, setParts] = useState(PARTS)
  const [progress, setProgress] = useState<Record<string, number>>({})
  const [detecting, setDetecting] = useState(false)
  const download = (id: string) => {
    let p = 0
    const tick = () => {
      p += 5
      setProgress((x) => ({ ...x, [id]: p }))
      if (p < 100) return setTimeout(tick, 110)
      setParts((list) => list.map((x) => (x.id === id ? { ...x, state: 'installed' } : x)))
      setProgress((x) => { const { [id]: _, ...rest } = x; return rest })
      toast('Downloaded from its official source and checked. (Scripted.)', undefined, 'task-done')
    }
    tick()
  }
  return (
    <Page narrow title="Setup" lead="Demido itself is small. The heavy parts are downloaded to your computer from the people who make them, so you get exactly their files and only the ones you need."
      actions={<Button disabled={detecting} onClick={() => { setDetecting(true); setTimeout(() => { setDetecting(false); toast('Nothing changed since the last look.') }, 1500) }}>{detecting ? 'Looking…' : 'Look again'}</Button>}>
      {core.state === 'ready' && <Section title="This computer" hint="Read by the core."><RealComputer version={core.version} /></Section>}
      {core.state === 'locked' && (
        <Signal level="note">Demido's core {core.version} is running here, but this browser is not signed in to it, so what follows is the scripted preview. Run <code>demido open</code> in a terminal to get in.</Signal>
      )}
      <Section title={core.state === 'ready' ? 'Setup steps' : 'This computer'} hint={core.state === 'ready' ? 'Scripted in the preview.' : 'Scripted in the preview: no core is behind these pages.'}>
        <div className="d-card"><div className="d-card__body app-stack">
          {core.state !== 'ready' && <Facts rows={[['Graphics', `${SYSTEM.gpu} · ${SYSTEM.vramGb.toFixed(1)} GB`], ['Driver', 'NVIDIA 580 · CUDA 12 ready'], ['Processor', `${SYSTEM.cpu} · AVX2`], ['Memory', `${SYSTEM.ramGb} GB`], ['System', SYSTEM.os]]} />}
          <Tasks tasks={[
            { id: '1', label: 'Looked at your hardware', state: 'done' },
            { id: '2', label: 'Chose the CUDA build over Vulkan and ROCm', state: 'done', meta: 'NVIDIA GPU' },
            { id: '3', label: 'Downloaded what is needed to run models', state: 'done', meta: '1.8 GB' },
            { id: '4', label: 'Optional parts', state: parts.every((p) => p.state !== 'available') ? 'done' : 'pending', meta: `${parts.filter((p) => p.state === 'available').length} left` },
          ]} />
        </div></div>
      </Section>
      <Section title="Parts" hint="Nothing here is bundled with Demido.">
        <div className="d-settings">
          {parts.map((p) => (
            <div key={p.id} className="d-setting">
              <div className="d-setting__text">
                <span className="d-setting__label">
                  <Glyph name={p.state === 'installed' ? 'task-done' : p.state === 'unsuitable' ? 'task-skipped' : 'task-pending'} label={p.state} />
                  {p.name}
                  {p.recommended && <span className="d-badge">Recommended</span>}
                </span>
                <span className="d-setting__desc">{p.what} {p.why}</span>
                <span className="d-setting__desc d-text-tertiary">From {p.from} · {p.size}</span>
              </div>
              <div className="d-setting__control">
                {progress[p.id] !== undefined ? (
                  <span className="d-mono d-text-secondary">{progress[p.id]}%</span>
                ) : p.state === 'installed' ? (
                  <Button variant="ghost" size="sm" onClick={() => toast('Removing a part: not built in the preview.')}>Remove</Button>
                ) : p.state === 'available' ? (
                  <Button size="sm" onClick={() => download(p.id)}>Download</Button>
                ) : (
                  <span className="d-type-caption d-text-tertiary">Not needed</span>
                )}
              </div>
              {progress[p.id] !== undefined && <div className="d-meter" style={{ gridColumn: '1 / -1' }} role="meter" aria-valuenow={progress[p.id]} aria-valuemin={0} aria-valuemax={100} aria-label={`Downloading ${p.name}`}><span style={{ '--d-meter': `${progress[p.id]}%` } as React.CSSProperties} /></div>}
            </div>
          ))}
        </div>
      </Section>
      <Signal level="note">On a server without a screen the same setup runs in the terminal: <code>demido setup</code>. It asks the same questions and downloads the same parts.</Signal>
    </Page>
  )
}

export const feature: Feature = { id: 'setup', title: 'Setup', icon: HardDrive, group: 'system', order: 5, path: 'setup', Page: SetupPage }
