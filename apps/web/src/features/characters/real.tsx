/* Characters, for real (DECISIONS E): kept by the core. A session started with one, or set to one from its
   message box, is told its instructions after Demido's own, as the person's, and answers with its model,
   thinking and temperature where it has them. */
import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router'
import { Copy, MessageSquare, MoreHorizontal, Pencil, Plus, Trash2, UserRound } from 'lucide-react'
import { changeCharacter, deleteCharacter, initials, makeCharacter, useCharacters, watchCharacters, type Character, type NewCharacter } from '@/data/characters'
import { useModelChoices } from '@/data/model-choices'
import { newSession } from '@/data/real-sessions'
import { toast } from '@/data/store'
import { Brief, Button, ContextMenu, Dialog, Empty, Facts, Icon, Menu, Page, Select, Signal, Skeleton, Slider, Switch, type MenuEntry } from '@/ui'

const BLANK: NewCharacter = { name: '', about: '', model: null, thinking: null, temperature: null, instructions: '' }
const MOST = 8000

function Editor({ character, onClose }: { character: Character | NewCharacter; onClose: () => void }) {
  const [c, setC] = useState<NewCharacter>({ name: character.name, about: character.about, model: character.model, thinking: character.thinking, temperature: character.temperature, instructions: character.instructions })
  const [problem, setProblem] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const choices = useModelChoices()
  const id = 'id' in character ? character.id : null
  const save = async () => {
    setBusy(true)
    try {
      const kept = id ? await changeCharacter(id, c) : await makeCharacter(c)
      toast(id ? `${kept.name} is changed.` : `${kept.name} is ready.`, undefined, 'task-done')
      onClose()
    } catch (e) { setProblem((e as Error).message) }
    setBusy(false)
  }
  return (
    <Dialog open wide onOpenChange={(o) => !o && onClose()} title={id ? `Change ${character.name}` : 'New character'}
      actions={<><Button onClick={onClose}>Cancel</Button><Button variant="primary" disabled={busy || !c.name.trim()} onClick={() => void save()}>{id ? 'Save' : 'Make it'}</Button></>}>
      <form className="chr__form" onSubmit={(e) => { e.preventDefault(); void save() }}>
        <label className="d-field"><span className="d-field__label">Name</span><input className="d-input" autoFocus maxLength={60} value={c.name} onChange={(e) => setC({ ...c, name: e.target.value })} placeholder="Mira" /></label>
        <label className="d-field"><span className="d-field__label">What it is, in a few words</span><input className="d-input" value={c.about} onChange={(e) => setC({ ...c, about: e.target.value })} placeholder="Reads long documents and answers with sources" /></label>
        <div className="chr__pair">
          <div className="d-field"><span className="d-field__label">Model</span>
            <Select label="Its model" value={c.model ?? ''} onChange={(v) => setC({ ...c, model: v || null })} options={[{ value: '', label: 'The one a new session would have' }, ...choices]} /></div>
          <div className="d-field"><span className="d-field__label">Thinking</span>
            <Select label="Whether it thinks first" value={c.thinking ?? ''} onChange={(v) => setC({ ...c, thinking: v || null })} options={[{ value: '', label: "As its model does" }, { value: 'off', label: 'Off' }, { value: 'on', label: 'On' }]} /></div>
        </div>
        <div className="d-field">
          <span className="d-field__label chr__temp">Temperature <Switch label="Its own temperature" checked={c.temperature !== null} onChange={(on) => setC({ ...c, temperature: on ? 0.7 : null })} /></span>
          {c.temperature === null
            ? <span className="d-field__help">Its model's own settings. Turn it on to choose: low is steady, high is varied.</span>
            : <Slider label="Temperature" min={0} max={2} step={0.05} value={c.temperature} onChange={(v) => setC({ ...c, temperature: v })} format={(v) => v.toFixed(2)} />}
        </div>
        <label className="d-field">
          <span className="d-field__label">Instructions <span className="d-text-tertiary chr__count">{c.instructions.length.toLocaleString()} / {MOST.toLocaleString()}</span></span>
          <textarea className="d-input chr__instructions" rows={8} maxLength={MOST} value={c.instructions} onChange={(e) => setC({ ...c, instructions: e.target.value })}
            placeholder={'Answer first, then the steps.\nGive your sources.'} />
          <span className="d-field__help">Told to the model after Demido's own words, as yours. Short and plain works best with a small model.</span>
        </label>
        {problem && <Signal level="caution">{problem}</Signal>}
      </form>
    </Dialog>
  )
}

export function RealCharacters() {
  const navigate = useNavigate()
  const list = useCharacters((s) => s.list)
  const error = useCharacters((s) => s.error)
  const choices = useModelChoices()
  const [editing, setEditing] = useState<Character | NewCharacter | null>(null)
  const [deleting, setDeleting] = useState<Character | null>(null)
  useEffect(() => watchCharacters(), [])
  const start = async (c: Character) => {
    const id = await newSession(undefined, c.id)
    if (id) navigate(`/sessions/${id}`)
  }
  const modelName = (m: string | null) => (m ? choices.find((x) => x.value === m)?.label.replace(/, loaded$/, '') ?? m : 'the session’s')
  const menu = (c: Character): MenuEntry[] => [
    { label: 'Start a session', lead: <Icon icon={MessageSquare} size="sm" />, onSelect: () => void start(c) },
    { label: 'Change…', lead: <Icon icon={Pencil} size="sm" />, onSelect: () => setEditing(c) },
    { label: 'Make a copy', lead: <Icon icon={Copy} size="sm" />, onSelect: () => setEditing({ name: `${c.name} (copy)`.slice(0, 60), about: c.about, model: c.model, thinking: c.thinking, temperature: c.temperature, instructions: c.instructions }) },
    { kind: 'sep' },
    { label: 'Delete…', lead: <Icon icon={Trash2} size="sm" />, danger: true, onSelect: () => setDeleting(c) },
  ]
  return (
    <Page title="Characters" lead="A name, a way of behaving, and a model." about="Start a session with a character and it is told the character's instructions, with its model, its thinking and its temperature. A session can also be set to one from its message box."
      actions={list && list.length > 0 ? <Button variant="primary" onClick={() => setEditing(BLANK)}><Icon icon={Plus} />New character</Button> : undefined}>
      {error && <Signal level="caution">The core did not say. {error}</Signal>}
      {!list && !error && <Skeleton rows={3} lead />}
      {list && list.length === 0 && <Empty icon={UserRound} title="No characters yet" text="A name, a model and your instructions: a session started with it follows them." action={<Button variant="primary" onClick={() => setEditing(BLANK)}><Icon icon={Plus} />New character</Button>} />}
      {list && list.length > 0 && (
        <div className="app-grid">
          {list.map((c) => (
            <ContextMenu key={c.id} items={menu(c)}>
              <article className="d-card chr__card">
                <header className="d-card__head">
                  <span className="d-avatar d-avatar--lg">{initials(c.name)}</span>
                  <div className="app-stack" style={{ gap: 'var(--d-space-2)', flex: 1, minWidth: 0 }}>
                    <h3 className="d-card__title">{c.name}</h3>
                    {c.about && <span className="d-card__sub">{c.about}</span>}
                  </div>
                  <Menu align="end" trigger={<Button size="sm" variant="ghost" iconOnly aria-label={`More for ${c.name}`}><Icon icon={MoreHorizontal} size="sm" /></Button>} items={menu(c)} />
                </header>
                <div className="d-card__body app-stack">
                  <Facts rows={[['Model', modelName(c.model)], ['Temperature', c.temperature === null ? 'its model’s own' : c.temperature.toFixed(2)], ['Thinking', c.thinking ?? 'as its model does']]} />
                  {c.instructions ? <Brief className="d-type-caption d-text-secondary chr__said" text={c.instructions} /> : <p className="d-type-caption d-text-tertiary">No instructions: only its name.</p>}
                </div>
                <footer className="d-card__foot">
                  <Button variant="primary" onClick={() => void start(c)}><Icon icon={MessageSquare} size="sm" />Start a session</Button>
                  <Button variant="ghost" onClick={() => setEditing(c)}>Change</Button>
                </footer>
              </article>
            </ContextMenu>
          ))}
        </div>
      )}
      {editing && <Editor character={editing} onClose={() => setEditing(null)} />}
      {deleting && <Dialog open onOpenChange={(o) => !o && setDeleting(null)} title={`Delete ${deleting.name}?`} text="Sessions set to it stay, with everything said in them, and are set to none."
        actions={<><Button onClick={() => setDeleting(null)}>Keep</Button><Button variant="danger" onClick={() => { const gone = deleting; setDeleting(null); void deleteCharacter(gone.id).then(() => toast(`${gone.name} is deleted.`)) }}>Delete</Button></>} />}
    </Page>
  )
}
