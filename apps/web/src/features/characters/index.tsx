import { useState } from 'react'
import { useNavigate } from 'react-router'
import { Plus, UserRound } from 'lucide-react'
import type { Feature } from '@/app/registry'
import { useCore } from '@/data/core'
import { RealCharacters } from './real'
import { newSession, saveCharacter, setSessionField, toast, useStore } from '@/data/store'
import type { Character } from '@/data/types'
import { Button, Dialog, Facts, Icon, NotBuilt, Page, Section, Select, Slider } from '@/ui'

function Editor({ character, onClose }: { character: Character; onClose: () => void }) {
  const models = useStore((s) => s.models).filter((m) => m.installed && !m.caps.includes('embedding'))
  const [c, setC] = useState(character)
  const set = <K extends keyof Character>(k: K, v: Character[K]) => setC({ ...c, [k]: v })
  return (
    <Dialog open wide onOpenChange={(o) => !o && onClose()} title={character.name ? `Edit ${character.name}` : 'New character'}
      actions={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" disabled={!c.name.trim()} onClick={() => { saveCharacter({ ...c, initials: c.name.trim().slice(0, 2) }); onClose() }}>Save character</Button>
        </>
      }>
      <div className="app-stack" style={{ marginBlockStart: 'var(--d-space-16)' }}>
        <div className="app-row" style={{ alignItems: 'flex-start' }}>
          <span className="d-avatar d-avatar--lg">{c.name.trim().slice(0, 2) || '?'}</span>
          <div className="app-stack" style={{ flex: 1 }}>
            <label className="d-field"><span className="d-field__label">Name</span><input className="d-input" value={c.name} onChange={(e) => set('name', e.target.value)} /></label>
            <label className="d-field"><span className="d-field__label">Description</span><input className="d-input" value={c.description} onChange={(e) => set('description', e.target.value)} /><span className="d-field__help">For you. The model does not read it.</span></label>
          </div>
        </div>
        <div><Button size="sm" disabled>Choose a picture</Button> <NotBuilt /></div>
        <label className="d-field">
          <span className="d-field__label">System prompt</span>
          <textarea className="d-input app-editor" value={c.prompt} onChange={(e) => set('prompt', e.target.value)} />
          <span className="d-field__help">For the model: short lines work best. About {Math.round(c.prompt.split(/\s+/).filter(Boolean).length * 1.3)} tokens.</span>
        </label>
        <div className="d-settings">
          <div className="d-setting"><div className="d-setting__text"><span className="d-setting__label">Model</span><span className="d-setting__desc">Sessions with this character start with it.</span></div><div className="d-setting__control"><Select label="Model" value={c.modelId} onChange={(v) => set('modelId', v)} options={models.map((m) => ({ value: m.id, label: m.name }))} /></div></div>
          <div className="d-setting"><div className="d-setting__text"><span className="d-setting__label">Temperature</span><span className="d-setting__desc">Lower is steadier.</span></div><div className="d-setting__control" style={{ width: '15rem' }}><Slider label="Temperature" value={c.temperature} onChange={(v) => set('temperature', v)} min={0} max={2} step={0.05} format={(v) => v.toFixed(2)} mark={0.7} /></div></div>
          <div className="d-setting"><div className="d-setting__text"><span className="d-setting__label">Voice</span><span className="d-setting__desc">Used when this character reads aloud.</span></div><div className="d-setting__control"><NotBuilt /></div></div>
        </div>
      </div>
    </Dialog>
  )
}

/** With a core behind the window, the person's own characters; without, the preview's. */
function CharactersPage() {
  const core = useCore()
  if (core.state === 'looking') return null
  return core.state === 'ready' ? <RealCharacters /> : <PreviewCharacters />
}

function PreviewCharacters() {
  const characters = useStore((s) => s.characters)
  const models = useStore((s) => s.models)
  const navigate = useNavigate()
  const [editing, setEditing] = useState<Character | null>(null)
  const blank: Character = { id: `c-${Date.now()}`, name: '', initials: '', role: 'Custom', description: '', modelId: 'qwen3-4b', temperature: 0.7, voice: 'Not set', prompt: 'Answer first.\nSteps numbered. One action each.' }
  return (
    <Page title="Characters" lead="A name, a way of behaving, and a model." about="Start a session with a character and it brings its prompt and its settings."
      actions={<Button variant="primary" onClick={() => setEditing(blank)}><Icon icon={Plus} />New character</Button>}>
      <Section title={`${characters.length} characters`}>
        <div className="app-grid">
          {characters.map((c) => (
            <article key={c.id} className="d-card">
              <header className="d-card__head">
                <span className="d-avatar d-avatar--lg">{c.initials}</span>
                <div className="app-stack" style={{ gap: 'var(--d-space-2)', flex: 1 }}>
                  <h3 className="d-card__title">{c.name}</h3>
                  <span className="d-card__sub">{c.role}</span>
                </div>
              </header>
              <div className="d-card__body app-stack">
                <p className="d-text-secondary">{c.description}</p>
                <Facts rows={[['Model', models.find((m) => m.id === c.modelId)?.name ?? c.modelId], ['Temperature', c.temperature.toFixed(2)], ['Voice', c.voice]]} />
                <details className="d-disclosure"><summary>System prompt</summary><div className="d-disclosure__body"><pre className="d-code">{c.prompt}</pre></div></details>
              </div>
              <footer className="d-card__foot">
                <Button onClick={() => { const id = newSession(); setSessionField(id, 'characterId', c.id); setSessionField(id, 'modelId', c.modelId); toast(`New session with ${c.name}.`); navigate(`/sessions/${id}`) }}>Start a session</Button>
                <Button variant="ghost" onClick={() => setEditing(c)}>Edit</Button>
              </footer>
            </article>
          ))}
        </div>
      </Section>
      {editing && <Editor key={editing.id} character={editing} onClose={() => setEditing(null)} />}
    </Page>
  )
}

export const feature: Feature = { id: 'characters', title: 'Characters', icon: UserRound, group: 'library', order: 2, path: 'characters', Page: CharactersPage }
