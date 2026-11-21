/* Providers, for real: the keys the core keeps, the models chosen at each provider, and what they cost.

   A key is given once and never shown again, here or anywhere: only its last four characters. Nothing is
   sent to a model that is not chosen here, and the core stops sending when the month's count reaches the
   limit in Settings. Claude Code and OpenCode are programs on this computer with loops of their own: their
   cards say whether they are here and how they are signed in (never the login itself), and what their plan
   said of its limit. Their use is shown and is held by their own plans, not by the limit here. */
import { useEffect, useMemo, useState } from 'react'
import { Ellipsis, ExternalLink, KeyRound, ListPlus, RefreshCw } from 'lucide-react'
import { useNavigate } from 'react-router'
import { choose, dollars, holds, listModels, priceOf, readAccount, removeKey, saveKey, useProviders, type Account, type PlanLimit, type ProviderView, type RemoteModel, type Spending } from '@/data/providers'
import { Button, Dialog, Glyph, Icon, Menu, NotBuilt, Page, Section, Signal, Switch, Tip } from '@/ui'

function Meter({ label, used, limit, unit, note }: { label: string; used: number; limit: number | null; unit: string; note?: string }) {
  const pct = limit ? Math.min(100, (used / limit) * 100) : 0
  const show = (n: number) => (unit === '$' ? dollars(n) : n.toLocaleString())
  return (
    <div className="prov__usage">
      <div className="app-row" style={{ justifyContent: 'space-between' }}>
        <span className="d-type-caption">{label}</span>
        <span className="d-mono d-text-secondary">{show(used)}{limit !== null ? ` of ${show(limit)}` : ''}{note ? ` · ${note}` : ''}</span>
      </div>
      {limit !== null && <div className="d-meter" data-tone={pct >= 80 ? 'caution' : undefined} role="meter" aria-valuenow={Math.round(pct)} aria-valuemin={0} aria-valuemax={100} aria-label={label}><span style={{ '--d-meter': `${pct}%` } as React.CSSProperties} /></div>}
    </div>
  )
}

/** This month, all providers together, against the limit the person set. */
function Month({ spending }: { spending: Spending }) {
  const navigate = useNavigate()
  const until = new Date(spending.until).toLocaleDateString(undefined, { day: 'numeric', month: 'long' })
  const near = spending.limit_usd !== null && spending.spent_usd >= spending.limit_usd * 0.8
  return (
    <Section title="This month" hint={`Counted by Demido for every request it sends.${spending.by_model.length > 0 && spending.exact ? ' All of it is the providers’ own figures.' : ''} Where a provider gives no figure, it is the tokens it reported at the prices Demido read on ${spending.prices_read_on}, at the regular price where there is a lower one for a while: above the bill, never below.`}>
      <div className="app-stack">
        <Meter label={spending.limit_usd === null ? 'Spent, with no limit set' : `Spent, of the limit you set (it starts again on ${until})`} used={spending.spent_usd} limit={spending.limit_usd} unit="$" />
        {near && <Signal level="caution">Demido sends nothing more to a paid model once its count reaches the limit. Free models are not stopped.</Signal>}
        {spending.by_model.length > 0 && (
          <ul className="prov__spent">
            {spending.by_model.map((s) => (
              <li key={`${s.provider}/${s.model}`}>
                <span className="d-mono">{s.provider}/{s.model}</span>
                <span className="d-text-secondary d-type-caption">{s.requests} {s.requests === 1 ? 'request' : 'requests'} · {s.input.toLocaleString()} tokens read · {s.output.toLocaleString()} written</span>
                <Tip label={s.counted ? (s.exact ? 'The provider’s own figure.' : 'Counted by Demido from the tokens the provider reported.') : 'What the program said this cost. It is held by your own plan with its maker, not by the limit here.'}><span className="d-mono">{dollars(s.cost_usd)}{s.counted ? (s.exact ? '' : ' est.') : ' (its plan)'}</span></Tip>
              </li>
            ))}
          </ul>
        )}
        <div><Button size="sm" variant="ghost" onClick={() => { navigate('/settings'); setTimeout(() => document.getElementById('set-providers')?.scrollIntoView({ block: 'start' }), 50) }}>Change the limit</Button></div>
      </div>
    </Section>
  )
}

function KeyDialog({ p, where, onClose }: { p: ProviderView; where: string; onClose: () => void }) {
  const [value, setValue] = useState('')
  const [saving, setSaving] = useState(false)
  const save = async () => {
    if (!value.trim() || saving) return
    setSaving(true)
    if (await saveKey(p.id, value.trim())) onClose()
    setSaving(false)
  }
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()} title={p.key ? `Replace the ${p.name} key` : `Add your ${p.name} key`}
      text={<>Paste the key {p.name} gave you. {where} It is never shown again, here or anywhere: only its last four characters. It is sent only to {p.name}, in the way its API asks for it.</>}
      actions={<><Button onClick={onClose}>Cancel</Button><Button variant="primary" disabled={!value.trim() || saving} onClick={() => void save()}>{saving ? 'Keeping it…' : 'Keep the key'}</Button></>}>
      <form className="app-stack" style={{ marginBlockStart: 'var(--d-space-16)' }} onSubmit={(e) => { e.preventDefault(); void save() }}>
        <label className="d-field">
          <span className="d-field__label">API key</span>
          <input className="d-input d-input--mono" type="password" autoComplete="off" spellCheck={false} autoFocus placeholder="Paste the key" value={value} onChange={(e) => setValue(e.target.value)} />
          <span className="d-field__help">No key yet? Get one at <a href={p.keys_at} target="_blank" rel="noreferrer noopener">{p.keys_at.replace('https://', '')}</a>. Billed by {p.name}, not by Demido.</span>
        </label>
      </form>
    </Dialog>
  )
}

/** How a model thinks, in a few words. */
function thinks(m: RemoteModel): string {
  if (!m.thinking.length) return m.provider === 'opencode' ? 'thinks as OpenCode sets it' : 'does not think'
  const levels = m.thinking.join(', ')
  return m.always_thinks ? `always thinks (${levels})` : m.thinking[0] === 'on' ? 'can think' : `can think: ${levels}`
}

function ModelsDialog({ p, onClose }: { p: ProviderView; onClose: () => void }) {
  const [list, setList] = useState<RemoteModel[] | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [find, setFind] = useState('')
  const [freeOnly, setFreeOnly] = useState(false)
  const [turn, setTurn] = useState(0)
  const chosen = useProviders((s) => s.providers?.find((x) => x.id === p.id)?.chosen ?? [])
  useEffect(() => {
    let alive = true
    setList(null)
    setError(null)
    listModels(p.id, turn > 0).then((l) => { if (alive) setList(l) }, (e: Error) => { if (alive) setError(e.message) })
    return () => { alive = false }
  }, [p.id, turn])
  const shown = useMemo(() => {
    const words = find.toLowerCase().split(/\s+/).filter(Boolean)
    return (list ?? []).filter((m) => (!freeOnly || m.free) && words.every((w) => `${m.name} ${m.id}`.toLowerCase().includes(w)))
  }, [list, find, freeOnly])
  const most = 150
  return (
    <Dialog open wide onOpenChange={(o) => !o && onClose()} title={`Choose ${p.name} models`}
      text={p.kind === 'program' ? `A session can use a model once it is chosen here, and only then. ${p.name} lists what it offers on this computer; it runs the model with its own tools.` : `A session can use a model once it is chosen here, and only then. ${p.name} lists what this key may use; prices are in dollars per million tokens.`}
      actions={<Button variant="primary" onClick={onClose}>Done</Button>}>
      <div className="app-stack" style={{ marginBlockStart: 'var(--d-space-12)' }}>
        <div className="app-row" style={{ gap: 'var(--d-space-8)' }}>
          <input className="d-input" style={{ flex: 1, minWidth: 0 }} placeholder="Find a model" aria-label="Find a model" value={find} onChange={(e) => setFind(e.target.value)} autoFocus />
          {p.id === 'openrouter' && <label className="app-row d-type-small" style={{ gap: 'var(--d-space-6)' }}><Switch label="Free only" checked={freeOnly} onChange={setFreeOnly} /> Free only</label>}
          <Tip label="Ask the provider again now"><Button size="sm" variant="ghost" iconOnly aria-label="Ask again" onClick={() => setTurn((n) => n + 1)}><Icon icon={RefreshCw} size="sm" /></Button></Tip>
        </div>
        {!list && !error && <p className="app-empty"><Glyph name="task-running" busy /> Asking {p.name}…</p>}
        {error && <Signal level="caution">{error}</Signal>}
        {list && (
          <ul className="prov__models">
            {shown.slice(0, most).map((m) => {
              const on = chosen.some((c) => c.id === m.id)
              return (
                <li key={m.id}>
                  <span className="app-stack" style={{ gap: 'var(--d-space-2)', minWidth: 0 }}>
                    <span>{m.name}{m.free && <span className="d-badge" style={{ marginInlineStart: 'var(--d-space-6)' }}>Free</span>}</span>
                    <span className="d-type-caption d-text-secondary"><span className="d-mono">{m.id.slice(p.id.length + 1)}</span> · {priceOf(m)}{m.price_from === 'highest' ? ' (not in Demido’s table: counted at the highest)' : ''}{m.context_length ? ` · holds ${holds(m.context_length)} tokens` : ''} · {thinks(m)}</span>
                    {m.note && <span className="d-type-caption d-text-secondary">{m.note}</span>}
                  </span>
                  <Button size="sm" variant={on ? 'ghost' : undefined} pressed={on} onClick={() => void choose(p.id, m.id, !on)}>{on ? 'Chosen' : 'Choose'}</Button>
                </li>
              )
            })}
            {shown.length > most && <li className="d-type-caption d-text-secondary">{(shown.length - most).toLocaleString()} more. Find narrows the list.</li>}
            {shown.length === 0 && <li className="d-type-caption d-text-secondary">None that match.</li>}
          </ul>
        )}
      </div>
    </Dialog>
  )
}

/** What a provider itself says about the key's account, asked when the card opens. */
function AccountLines({ p }: { p: ProviderView }) {
  const [account, setAccount] = useState<Account | null>(null)
  const [error, setError] = useState<string | null>(null)
  const keyed = p.key?.added_at
  useEffect(() => {
    if (!keyed) return
    let alive = true
    readAccount(p.id).then((a) => { if (alive) setAccount(a) }, (e: Error) => { if (alive) setError(e.message) })
    return () => { alive = false }
  }, [p.id, keyed])
  if (!keyed) return null
  if (error) return <Signal level="caution">{p.name} did not say what the key has spent: {error}</Signal>
  if (!account) return null
  return (
    <div className="app-stack" style={{ gap: 'var(--d-space-6)' }}>
      {account.lines.map((l) => <Meter key={l.label} label={`${l.label} (as ${p.name} says)`} used={l.used} limit={l.limit} unit={l.unit} note={l.reset ?? undefined} />)}
      {account.note && <p className="d-type-caption d-text-secondary">{account.note}</p>}
    </div>
  )
}

/** A program's plan limit, as it said it: how much is used, and when it resets. */
function Plan({ plan }: { plan: PlanLimit }) {
  const resets = plan.resets_at ? new Date(plan.resets_at).toLocaleString(undefined, { weekday: 'short', hour: '2-digit', minute: '2-digit' }) : null
  const said = new Date(plan.said_at).toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' })
  return (
    <div className="app-stack" style={{ gap: 'var(--d-space-6)' }}>
      {plan.used !== null
        ? <Meter label={`Your plan's limit, as Claude Code said at ${said}`} used={Math.round(plan.used * 100)} limit={100} unit="%" note={resets ? `resets ${resets}` : undefined} />
        : <p className="d-type-caption d-text-secondary">Claude Code said at {said} that your plan's limit is {plan.status === 'rejected' ? 'reached' : plan.status === 'allowed_warning' ? 'nearly reached' : 'not reached'}{resets ? `; it resets ${resets}` : ''}.</p>}
      {plan.status === 'rejected' && <Signal level="caution">Your plan's limit is reached: Claude Code answers again when it resets, or with usage credits if you turned those on with Anthropic.</Signal>}
    </div>
  )
}

/** A program on this computer: whether it is here, how it is signed in, its models. */
function ProgramCard({ p }: { p: ProviderView }) {
  const [choosing, setChoosing] = useState(false)
  const found = p.program
  const here = Boolean(found?.path)
  const sub = here ? ['a program on this computer', found?.version, found?.signed_in ? `signed in ${found.signed_in}` : null].filter(Boolean).join(' · ') : 'not found on this computer'
  return (
    <article className="d-card">
      <header className="d-card__head">
        <Glyph name={here && !found?.problem ? 'task-done' : 'task-pending'} size="1.25rem" style={{ marginTop: '0.125rem' }} label={here ? 'Found' : 'Not found'} />
        <div className="app-stack" style={{ gap: 'var(--d-space-2)', flex: 1 }}>
          <h3 className="d-card__title">{p.name}</h3>
          <span className="d-card__sub">{sub}{p.spent_usd > 0 ? ` · ${dollars(p.spent_usd)} this month, by its own count` : ''}</span>
        </div>
      </header>
      <div className="d-card__body app-stack">
        <p className="d-text-secondary">{p.note}</p>
        {found?.problem && <Signal level="caution">{found.problem}</Signal>}
        {found?.path && <p className="d-type-caption d-text-tertiary">Runs <span className="d-mono">{found.path}</span>. Demido never downloads, changes or bundles it, and never sees or keeps its login.</p>}
        {p.plan && <Plan plan={p.plan} />}
        {p.chosen.length > 0 && (
          <ul className="prov__models">
            {p.chosen.map((m) => (
              <li key={m.id}>
                <span className="app-stack" style={{ gap: 'var(--d-space-2)', minWidth: 0 }}>
                  <span>{m.name}{m.free && <span className="d-badge" style={{ marginInlineStart: 'var(--d-space-6)' }}>Free</span>}</span>
                  <span className="d-type-caption d-text-secondary">{m.note ?? priceOf(m)}</span>
                </span>
                <Tip label="Sessions can no longer use it. Those that do keep their messages."><Button size="sm" variant="ghost" onClick={() => void choose(p.id, m.id, false)}>Let go</Button></Tip>
              </li>
            ))}
          </ul>
        )}
        <div className="app-row" style={{ gap: 'var(--d-space-8)' }}>
          <Button size="sm" disabled={!here} onClick={() => setChoosing(true)}><Icon icon={ListPlus} size="sm" />Choose models…</Button>
          {!here && <a className="d-button d-button--ghost d-button--sm" href={p.keys_at} target="_blank" rel="noreferrer noopener"><Icon icon={ExternalLink} size="sm" />How to install it</a>}
        </div>
      </div>
      {choosing && <ModelsDialog p={p} onClose={() => setChoosing(false)} />}
    </article>
  )
}

function ProviderCard({ p, where }: { p: ProviderView; where: string }) {
  const [editing, setEditing] = useState(false)
  const [choosing, setChoosing] = useState(false)
  const [sure, setSure] = useState(false)
  const keyWords = p.key ? `key …${p.key.ends_with}, in ${p.key.kept_in === 'keyring' ? 'the system keyring' : 'a file only your user can read'}` : 'no key yet'
  return (
    <article className="d-card">
      <header className="d-card__head">
        <Glyph name={p.key ? 'task-done' : 'task-pending'} size="1.25rem" style={{ marginTop: '0.125rem' }} label={p.key ? 'Has a key' : 'No key yet'} />
        <div className="app-stack" style={{ gap: 'var(--d-space-2)', flex: 1 }}>
          <h3 className="d-card__title">{p.name}</h3>
          <span className="d-card__sub">over the network · {keyWords}{p.spent_usd > 0 ? ` · ${dollars(p.spent_usd)} this month${p.spent_exact ? '' : ' (estimated)'}` : ''}</span>
        </div>
        {p.key
          ? <Menu align="end" trigger={<Button variant="ghost" size="sm" iconOnly aria-label={`More for ${p.name}`}><Icon icon={Ellipsis} /></Button>}
              items={[{ label: 'Replace the key…', onSelect: () => setEditing(true) }, { kind: 'sep' }, { label: 'Delete the key', hint: `from ${p.key.kept_in === 'keyring' ? 'the keyring' : 'the file'}`, danger: true, onSelect: () => setSure(true) }]} />
          : <Button size="sm" onClick={() => setEditing(true)}><Icon icon={KeyRound} size="sm" />Add your key</Button>}
      </header>
      <div className="d-card__body app-stack">
        <p className="d-text-secondary">{p.note}</p>
        {sure && (
          <Signal level="warning" actions={<><Button size="sm" onClick={() => setSure(false)}>Keep it</Button><Button size="sm" variant="danger" onClick={() => { setSure(false); void removeKey(p.id) }}>Delete the key</Button></>}>
            The key is deleted from where Demido keeps it. Sessions with {p.name} models stop working until you add one again. The key itself still works at {p.name}: revoke it there if it should not.
          </Signal>
        )}
        {p.key && <AccountLines p={p} />}
        {p.chosen.length > 0 && (
          <ul className="prov__models">
            {p.chosen.map((m) => (
              <li key={m.id}>
                <span className="app-stack" style={{ gap: 'var(--d-space-2)', minWidth: 0 }}>
                  <span>{m.name}{m.free && <span className="d-badge" style={{ marginInlineStart: 'var(--d-space-6)' }}>Free</span>}</span>
                  <span className="d-type-caption d-text-secondary">{priceOf(m)} · {thinks(m)}</span>
                </span>
                <Tip label="Sessions can no longer use it. Those that do keep their messages."><Button size="sm" variant="ghost" onClick={() => void choose(p.id, m.id, false)}>Let go</Button></Tip>
              </li>
            ))}
          </ul>
        )}
        <div className="app-row" style={{ gap: 'var(--d-space-8)' }}>
          <Button size="sm" disabled={!p.key && p.id !== 'openrouter'} onClick={() => setChoosing(true)}><Icon icon={ListPlus} size="sm" />Choose models…</Button>
          {!p.key && <a className="d-button d-button--ghost d-button--sm" href={p.keys_at} target="_blank" rel="noreferrer noopener"><Icon icon={ExternalLink} size="sm" />Get a key</a>}
        </div>
        {!p.key && p.chosen.length > 0 && <Signal level="note">These models are chosen, and there is no key: sessions with them cannot be answered until you add one.</Signal>}
      </div>
      {editing && <KeyDialog p={p} where={where} onClose={() => setEditing(false)} />}
      {choosing && <ModelsDialog p={p} onClose={() => setChoosing(false)} />}
    </article>
  )
}

export function RealProviders() {
  const { providers, vault, spending, error } = useProviders()
  return (
    <Page title="Providers" lead="Where models can run besides this computer. A model elsewhere is sent what is said in a session that uses it, and costs money; a model here is free and nothing leaves the computer. Demido sends nothing to a model you did not choose here.">
      {error && <Signal level="caution">The core did not say. {error}</Signal>}
      {!providers && !error && <p className="app-empty"><Glyph name="task-running" busy /> Reading the providers…</p>}
      {spending && <Month spending={spending} />}
      {providers && vault && (
        <Section title="Providers" hint={vault.where_words}>
          <div className="app-stack">
            {providers.filter((p) => p.kind !== 'program').map((p) => <ProviderCard key={p.id} p={p} where={vault.where_words} />)}
          </div>
        </Section>
      )}
      {providers && (
        <Section title="Programs on this computer" hint="They run their own loop with their own tools; Demido gives them your messages, shows what they do, and asks you what they ask. Demido's own help for small models does not run inside them.">
          <div className="app-stack">
            {providers.filter((p) => p.kind === 'program').map((p) => <ProgramCard key={p.id} p={p} />)}
          </div>
        </Section>
      )}
      <NotBuilt block>Several keys for one provider, a provider's own spending figures for Anthropic and Google (an ordinary key cannot ask), pictures sent to a model, OpenCode's levels of thinking, and your Claude plan's limit before Claude Code has run once.</NotBuilt>
    </Page>
  )
}
