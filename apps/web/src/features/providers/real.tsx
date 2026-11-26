/* Providers, for real: the person's accounts at each provider, the models chosen there, and what they cost.

   A provider can hold several accounts: several keys at an API, several Claude Code sign-ins, each named by
   the person. One is the default; a session can name another. A key is given once and never shown again,
   here or anywhere: only its last four characters. Nothing is sent to a model that is not chosen here, and
   the core stops sending when the month's count reaches the limit in Settings. Claude Code and OpenCode are
   programs on this computer with loops of their own: their cards say whether Demido has its copy and how each
   account is signed in (never the login itself), and what its plan said of its limit. Their use is shown and
   is held by their own plans, not by the limit here. */
import { useEffect, useMemo, useState } from 'react'
import { Ellipsis, ExternalLink, KeyRound, ListPlus, Plus, RefreshCw } from 'lucide-react'
import { useNavigate } from 'react-router'
import { addAccount, changeAccount, choose, dollars, downloadCopy, holds, latestCopy, listModels, priceOf, readAccount, removeAccount, removeCopy, removeSearchKey, replaceKey, setSearchKey, signIn, stopDownload, useProviders, type Account, type Latest, type PlanLimit, type ProviderAccount, type ProviderView, type RemoteModel, type SearchService, type Spending } from '@/data/providers'
import { setSetting } from '@/data/store'
import { Button, Dialog, Glyph, Icon, Menu, NotBuilt, Page, Section, Signal, Switch, Tip, type MenuEntry, Brief } from '@/ui'

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

/** What the account dialog does: adds one, gives one another key, or renames one. */
type Editing = { mode: 'add' } | { mode: 'key'; account: ProviderAccount } | { mode: 'rename'; account: ProviderAccount }

function AccountDialog({ p, editing, where, onClose }: { p: ProviderView; editing: Editing; where: string; onClose: () => void }) {
  const program = p.kind === 'program'
  const named = editing.mode === 'add' ? '' : editing.account.label
  const [label, setLabel] = useState(editing.mode === 'rename' ? named : '')
  const [value, setValue] = useState('')
  const [saving, setSaving] = useState(false)
  const wantsKey = !program && editing.mode !== 'rename'
  const wantsLabel = editing.mode !== 'key'
  const ready = (!wantsKey || value.trim() !== '') && (editing.mode !== 'rename' || label.trim() !== '')
  const save = async () => {
    if (!ready || saving) return
    setSaving(true)
    let done: boolean
    if (editing.mode === 'add') {
      const added = await addAccount(p.id, label, program ? undefined : value.trim())
      // A Claude Code account is signed in at once, on Anthropic's page.
      if (added && program) await signIn(p.id, added.id)
      done = added !== null
    } else if (editing.mode === 'key') {
      done = await replaceKey(p.id, editing.account.id, value.trim())
    } else {
      done = await changeAccount(p.id, editing.account.id, { label: label.trim() })
    }
    setSaving(false)
    if (done) onClose()
  }
  const title = editing.mode === 'rename' ? `Rename ${named}` : editing.mode === 'key' ? `Replace the key of ${named}` : program ? `Add a ${p.name} account` : p.accounts.length ? `Add another ${p.name} key` : `Add your ${p.name} key`
  const text = editing.mode === 'rename'
    ? 'A name of your own, to tell your accounts apart.'
    : program
      ? <>Each account signs in on Anthropic's own page and keeps its login in a profile of its own, with its own settings and history: apart from your other accounts, and from any Claude Code of your own. Demido never sees or keeps a login.</>
      : <>Paste the key {p.name} gave you. {where} It is never shown again, here or anywhere: only its last four characters. It is sent only to {p.name}, in the way its API asks for it.</>
  const action = editing.mode === 'rename' ? 'Rename' : program ? 'Add and sign in' : 'Keep the key'
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()} title={title} text={text}
      actions={<><Button onClick={onClose}>Cancel</Button><Button variant="primary" disabled={!ready || saving} onClick={() => void save()}>{saving ? 'A moment…' : action}</Button></>}>
      {/* Two fields and no button of its own: Enter is caught here, as a form would only for one field. */}
      <form className="app-stack" style={{ marginBlockStart: 'var(--d-space-16)' }} onSubmit={(e) => { e.preventDefault(); void save() }}
        onKeyDown={(e) => { if (e.key === 'Enter' && !e.nativeEvent.isComposing) { e.preventDefault(); void save() } }}>
        {wantsLabel && (
          <label className="d-field">
            <span className="d-field__label">Name</span>
            <input className="d-input" maxLength={40} autoFocus placeholder={editing.mode === 'rename' ? undefined : 'Work, Personal… (you can leave it empty)'} value={label} onChange={(e) => setLabel(e.target.value)} />
            {editing.mode === 'add' && <span className="d-field__help">To tell this account from your others. Left empty, it is numbered: Account 1, Account 2.</span>}
          </label>
        )}
        {wantsKey && (
          <label className="d-field">
            <span className="d-field__label">API key</span>
            <input className="d-input d-input--mono" type="password" autoComplete="off" spellCheck={false} autoFocus={!wantsLabel} placeholder="Paste the key" value={value} onChange={(e) => setValue(e.target.value)} />
            <span className="d-field__help">No key yet? Get one at <a href={p.keys_at} target="_blank" rel="noreferrer noopener">{p.keys_at.replace('https://', '')}</a>. Billed by {p.name}, not by Demido.</span>
          </label>
        )}
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
      text={p.kind === 'program' ? `A session can use a model once it is chosen here, and only then. ${p.name} lists what it offers on this computer; it runs the model with its own tools.` : `A session can use a model once it is chosen here, and only then. ${p.name} lists what the default account's key may use; prices are in dollars per million tokens.`}
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

/** What a provider itself says about one account, asked when the card opens. `withNote`: say once what it does not say. */
function AccountLines({ p, a, withNote }: { p: ProviderView; a: ProviderAccount; withNote: boolean }) {
  const [account, setAccount] = useState<Account | null>(null)
  const [error, setError] = useState<string | null>(null)
  const keyed = a.key?.added_at
  useEffect(() => {
    if (!keyed) return
    let alive = true
    readAccount(p.id, a.id).then((r) => { if (alive) setAccount(r) }, (e: Error) => { if (alive) setError(e.message) })
    return () => { alive = false }
  }, [p.id, a.id, keyed])
  if (!keyed) return null
  if (error) return <Signal level="caution">{p.name} did not say what this key has spent: {error}</Signal>
  if (!account || (!account.lines.length && !withNote)) return null
  return (
    <div className="app-stack" style={{ gap: 'var(--d-space-6)' }}>
      {account.lines.map((l) => <Meter key={l.label} label={`${l.label} (as ${p.name} says)`} used={l.used} limit={l.limit} unit={l.unit} note={l.reset ?? undefined} />)}
      {withNote && account.note && <p className="d-type-caption d-text-secondary">{account.note}</p>}
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
        ? <Meter label={`Its plan's limit, as Claude Code said at ${said}`} used={Math.round(plan.used * 100)} limit={100} unit="%" note={resets ? `resets ${resets}` : undefined} />
        : <p className="d-type-caption d-text-secondary">Claude Code said at {said} that this plan's limit is {plan.status === 'rejected' ? 'reached' : plan.status === 'allowed_warning' ? 'nearly reached' : 'not reached'}{resets ? `; it resets ${resets}` : ''}.</p>}
      {plan.status === 'rejected' && <Signal level="caution">This plan's limit is reached: Claude Code answers again when it resets, with usage credits if you turned those on with Anthropic, or with another of your accounts.</Signal>}
    </div>
  )
}

const MB = (n: number) => `${Math.round(n / 1e6)} MB`

/** Demido's own copy of a program: download it, update it, delete it. Nothing goes on the system. */
function OwnCopy({ p }: { p: ProviderView }) {
  const [latest, setLatest] = useState<Latest | null>(null)
  const maker = p.id === 'claude-code' ? 'Anthropic' : 'OpenCode\'s package on npm'
  useEffect(() => {
    let alive = true
    latestCopy(p.id).then((l) => { if (alive) setLatest(l) }, () => {})
    return () => { alive = false }
  }, [p.id, p.copy?.version])
  if (p.downloading) {
    const d = p.downloading
    return (
      <div className="app-stack" style={{ gap: 'var(--d-space-6)' }}>
        <Meter label={`Downloading ${p.name}${d.version ? ` ${d.version}` : ''} from ${maker}`} used={Math.round(d.done / 1e6)} limit={d.total ? Math.round(d.total / 1e6) : null} unit=" MB" />
        <div><Button size="sm" variant="ghost" onClick={() => void stopDownload(p.id)}>Stop</Button></div>
      </div>
    )
  }
  if (!p.copy)
    return (
      <div className="app-stack" style={{ gap: 'var(--d-space-8)' }}>
        <p className="d-type-small d-text-secondary">Demido keeps a copy of {p.name} of its own: downloaded from {maker} when you ask, checked against its published checksum, and kept unchanged in Demido's folder. Nothing is installed on your system, and none of your own settings, skills or plugins come with it.</p>
        <div><Button size="sm" onClick={() => void downloadCopy(p.id)}>Download {p.name}{latest ? ` ${latest.version}${latest.size ? ` (${MB(latest.size)})` : ''}` : ''}</Button></div>
      </div>
    )
  const newer = latest && latest.version !== p.copy.version ? latest : null
  return (
    <div className="app-stack" style={{ gap: 'var(--d-space-6)' }}>
      <p className="d-type-caption d-text-secondary">Demido's own copy, {p.copy.version}, {MB(p.copy.size)}, checked against the {p.copy.checked}. It runs with {p.takes_accounts ? 'a profile of its own for each account' : 'a profile of its own'} and does not update itself: Demido updates it.</p>
      <div className="app-row" style={{ gap: 'var(--d-space-8)' }}>
        {newer && <Button size="sm" onClick={() => void downloadCopy(p.id)}>Update to {newer.version}</Button>}
        <Tip label={p.takes_accounts ? 'The accounts\' profiles, with their settings and logins, are kept: downloaded again, it finds them.' : 'Its profile, with its settings, is kept: downloaded again, it finds it.'}><Button size="sm" variant="ghost" onClick={() => void removeCopy(p.id)}>Delete the copy</Button></Tip>
      </div>
    </div>
  )
}

/** Claude Code's own sign-in for one account, on Anthropic's page. Demido starts it and never sees the login. */
function SignInPart({ p, a }: { p: ProviderView; a: ProviderAccount }) {
  const s = a.signing
  const needed = a.signed_in === 'not signed in'
  if ((!s || s.state === 'done') && !needed) return null
  const command = a.sign_in_command && (
    <span style={{ display: 'block', marginBlockStart: 'var(--d-space-6)' }}>If Anthropic's page shows a code to paste, sign in from a terminal instead: <code className="d-mono" style={{ userSelect: 'all' }}>{a.sign_in_command}</code></span>
  )
  if (s?.state === 'started')
    return (
      <Signal level="note">
        Anthropic's sign-in page opened in your browser. Sign in there with the account for {a.label}; Claude Code keeps the login in this account's own profile, and Demido never sees it.
        {s.url && <span style={{ display: 'block', marginBlockStart: 'var(--d-space-6)' }}>No page opened? <a href={s.url} target="_blank" rel="noreferrer noopener">Open Anthropic's sign-in page</a>.</span>}
        {command}
      </Signal>
    )
  if (s?.state === 'failed')
    return <Signal level="caution" actions={<Button size="sm" onClick={() => void signIn(p.id, a.id)}>Try again</Button>}>The sign-in did not finish. {s.said}{command}</Signal>
  return <div><Button size="sm" variant="primary" onClick={() => void signIn(p.id, a.id)}>Sign in to Claude</Button></div>
}

/** One account: the default's radio, its name, what it holds and cost, and what can be done with it. */
function AccountRow({ p, a, first, onEdit }: { p: ProviderView; a: ProviderAccount; first: boolean; onEdit: (e: Editing) => void }) {
  const [sure, setSure] = useState(false)
  const program = p.kind === 'program'
  const holdsWords = program
    ? (!a.signed_in ? 'not looked at yet' : a.signed_in === 'not signed in' ? 'not signed in yet' : `signed in ${a.signed_in}`)
    : a.key ? `key …${a.key.ends_with} · in ${a.key.kept_in === 'keyring' ? 'the system keyring' : 'a file only your user can read'}` : 'its key is missing: replace it'
  const items: MenuEntry[] = [
    { label: 'Rename…', onSelect: () => onEdit({ mode: 'rename', account: a }) },
    program ? { label: 'Sign in again', hint: 'on Anthropic\'s page', onSelect: () => void signIn(p.id, a.id) } : { label: 'Replace the key…', onSelect: () => onEdit({ mode: 'key', account: a }) },
    { kind: 'sep' },
    { label: program ? 'Delete the account' : 'Delete the key', hint: program ? 'signs out, deletes its profile' : `from ${a.key?.kept_in === 'keyring' ? 'the keyring' : 'the file'}`, danger: true, onSelect: () => setSure(true) },
  ]
  const others = p.accounts.length > 1
  return (
    <li>
      <label className="d-check">
        <input className="d-radio" type="radio" name={`default-${p.id}`} checked={a.is_default} disabled={!others} onChange={() => void changeAccount(p.id, a.id, { default: true })} aria-label={`${a.label}: the default`} />
        <span className="d-check__text">
          <span>{a.label}{a.is_default && others && <span className="d-badge" style={{ marginInlineStart: 'var(--d-space-6)' }}>Default</span>}</span>
          <small className="d-type-caption d-text-secondary">{holdsWords}</small>
        </span>
      </label>
      <div className="app-stack" style={{ gap: 'var(--d-space-6)' }}>
        <span className="d-type-caption d-text-secondary">{program
          ? (a.spent_usd > 0 ? `${dollars(a.spent_usd)} this month, as ${p.name} said it` : `${p.name} has said nothing of a cost this month.`)
          : (a.spent_usd > 0 ? `Demido counted ${dollars(a.spent_usd)} for it this month.` : 'Demido has counted nothing for it this month.')}</span>
        {program ? <SignInPart p={p} a={a} /> : <AccountLines p={p} a={a} withNote={first} />}
        {a.plan && <Plan plan={a.plan} />}
        {sure && (
          <Signal level="warning" actions={<><Button size="sm" onClick={() => setSure(false)}>Keep it</Button><Button size="sm" variant="danger" onClick={() => { setSure(false); void removeAccount(p.id, a) }}>{program ? 'Delete the account' : 'Delete the key'}</Button></>}>
            {program
              ? <>{a.label} is signed out and its profile is deleted: its login, its settings and Claude Code's history of its sessions. Sessions that used it use the default account from then on.</>
              : <>The key is deleted from where Demido keeps it. Sessions that used {a.label} use the default account from then on{others ? '' : `; with no key left, sessions with ${p.name} models stop until you add one`}. The key itself still works at {p.name}: revoke it there if it should not.</>}
          </Signal>
        )}
      </div>
      <Menu align="end" trigger={<Button variant="ghost" size="sm" iconOnly aria-label={`More for ${a.label}`}><Icon icon={Ellipsis} /></Button>} items={items} />
    </li>
  )
}

/** The chosen models of a provider, each to let go. */
function Chosen({ p }: { p: ProviderView }) {
  if (!p.chosen.length) return null
  return (
    <ul className="prov__models">
      {p.chosen.map((m) => (
        <li key={m.id}>
          <span className="app-stack" style={{ gap: 'var(--d-space-2)', minWidth: 0 }}>
            <span>{m.name}{m.free && <span className="d-badge" style={{ marginInlineStart: 'var(--d-space-6)' }}>Free</span>}</span>
            <span className="d-type-caption d-text-secondary">{p.kind === 'program' ? m.note ?? priceOf(m) : `${priceOf(m)} · ${thinks(m)}`}</span>
          </span>
          <Tip label="Sessions can no longer use it. Those that do keep their messages."><Button size="sm" variant="ghost" onClick={() => void choose(p.id, m.id, false)}>Let go</Button></Tip>
        </li>
      ))}
    </ul>
  )
}

const accountsWords = (n: number, one: string, many: string) => (n === 0 ? `no ${one} yet` : n === 1 ? `1 ${one}` : `${n} ${many}`)

/** A program on this computer: Demido's copy, its accounts and how each is signed in, its models. */
function ProgramCard({ p }: { p: ProviderView }) {
  const [choosing, setChoosing] = useState(false)
  const [editing, setEditing] = useState<Editing | null>(null)
  const found = p.program
  const here = Boolean(found?.path)
  const sub = [p.copy ? 'Demido\'s own copy' : 'not downloaded yet', p.copy ? found?.version : null, p.takes_accounts ? accountsWords(p.accounts.length, 'account', 'accounts') : null].filter(Boolean).join(' · ')
  // What is said about signing in is said by each account.
  const problem = found?.path && found.problem && !/signed in|no account/.test(found.problem) ? found.problem : null
  const ok = here && !problem && (!p.takes_accounts || p.accounts.some((a) => a.signed_in?.startsWith('with')))
  return (
    <article className="d-card">
      <header className="d-card__head">
        <Glyph name={ok ? 'task-done' : 'task-pending'} size="1.25rem" style={{ marginTop: '0.125rem' }} label={ok ? 'Ready' : 'Not ready yet'} />
        <div className="app-stack" style={{ gap: 'var(--d-space-2)', flex: 1 }}>
          <h3 className="d-card__title">{p.name}</h3>
          <span className="d-card__sub">{sub}{p.spent_usd > 0 ? ` · ${dollars(p.spent_usd)} this month, by its own count` : ''}</span>
        </div>
        {p.takes_accounts && here && p.accounts.length > 0 && <Button size="sm" onClick={() => setEditing({ mode: 'add' })}><Icon icon={Plus} size="sm" />Add an account</Button>}
      </header>
      <div className="d-card__body app-stack">
        <Brief text={p.note} />
        <OwnCopy p={p} />
        {problem && <Signal level="caution">{problem}</Signal>}
        {p.takes_accounts && here && p.accounts.length === 0 && (
          <div className="app-stack" style={{ gap: 'var(--d-space-6)' }}>
            <div><Button size="sm" variant="primary" onClick={() => setEditing({ mode: 'add' })}>Sign in to Claude</Button></div>
            <Brief className="d-type-caption d-text-secondary" text={"Opens Anthropic's own sign-in page. Use your Claude plan's account; Demido never sees or keeps the login. You can add more accounts afterwards, each signed in on its own."} />
          </div>
        )}
        {p.accounts.length > 0 && <ul className="prov__accounts">{p.accounts.map((a, i) => <AccountRow key={a.id} p={p} a={a} first={i === 0} onEdit={setEditing} />)}</ul>}
        <Chosen p={p} />
        <div className="app-row" style={{ gap: 'var(--d-space-8)', flexWrap: 'wrap' }}>
          <Button size="sm" disabled={!here} onClick={() => setChoosing(true)}><Icon icon={ListPlus} size="sm" />Choose models…</Button>
        </div>
      </div>
      {choosing && <ModelsDialog p={p} onClose={() => setChoosing(false)} />}
      {editing && <AccountDialog p={p} editing={editing} where="" onClose={() => setEditing(null)} />}
    </article>
  )
}

function ProviderCard({ p, where }: { p: ProviderView; where: string }) {
  const [editing, setEditing] = useState<Editing | null>(null)
  const [choosing, setChoosing] = useState(false)
  const has = p.accounts.length > 0
  const keyWords = p.accounts.length === 1 && p.accounts[0].key ? `key …${p.accounts[0].key.ends_with}` : accountsWords(p.accounts.length, 'key', 'keys')
  return (
    <article className="d-card">
      <header className="d-card__head">
        <Glyph name={has ? 'task-done' : 'task-pending'} size="1.25rem" style={{ marginTop: '0.125rem' }} label={has ? 'Has a key' : 'No key yet'} />
        <div className="app-stack" style={{ gap: 'var(--d-space-2)', flex: 1 }}>
          <h3 className="d-card__title">{p.name}</h3>
          <span className="d-card__sub">over the network · {keyWords}{p.spent_usd > 0 ? ` · ${dollars(p.spent_usd)} this month${p.accounts.length > 1 ? ' together' : ''}${p.spent_exact ? '' : ' (estimated)'}` : ''}</span>
        </div>
        <Button size="sm" onClick={() => setEditing({ mode: 'add' })}><Icon icon={has ? Plus : KeyRound} size="sm" />{has ? 'Add another key' : 'Add your key'}</Button>
      </header>
      <div className="d-card__body app-stack">
        <Brief text={p.note} />
        {has && <ul className="prov__accounts">{p.accounts.map((a, i) => <AccountRow key={a.id} p={p} a={a} first={i === 0} onEdit={setEditing} />)}</ul>}
        {p.accounts.length > 1 && <Brief className="d-type-caption d-text-secondary" text={"A session uses the default unless you pick another account in its header. Each key is sent only for the sessions that use it, and what each cost is counted on its own; the month's limit holds them together."} />}
        <Chosen p={p} />
        <div className="app-row" style={{ gap: 'var(--d-space-8)' }}>
          <Button size="sm" disabled={!has && p.id !== 'openrouter'} onClick={() => setChoosing(true)}><Icon icon={ListPlus} size="sm" />Choose models…</Button>
          {!has && <a className="d-button d-button--ghost d-button--sm" href={p.keys_at} target="_blank" rel="noreferrer noopener"><Icon icon={ExternalLink} size="sm" />Get a key</a>}
        </div>
        {!has && p.chosen.length > 0 && <Signal level="note">These models are chosen, and there is no key: sessions with them cannot be answered until you add one.</Signal>}
      </div>
      {editing && <AccountDialog p={p} editing={editing} where={where} onClose={() => setEditing(null)} />}
      {choosing && <ModelsDialog p={p} onClose={() => setChoosing(false)} />}
    </article>
  )
}

/** A search service the person can give a key for: asked first, before the free ones, and counted with the spending. */
function SearchCard({ s, where }: { s: SearchService; where: string }) {
  const [adding, setAdding] = useState(false)
  const [value, setValue] = useState('')
  const [saving, setSaving] = useState(false)
  const [removing, setRemoving] = useState(false)
  const save = async () => {
    if (!value.trim() || saving) return
    setSaving(true)
    const done = await setSearchKey(s, value.trim())
    setSaving(false)
    if (done) { setAdding(false); setValue('') }
  }
  const perThousand = dollars(s.price_usd * 1000)
  return (
    <article className="d-card">
      <header className="d-card__head">
        <Glyph name={s.key ? (s.on ? 'task-done' : 'task-skipped') : 'task-pending'} size="1.25rem" style={{ marginTop: '0.125rem' }} label={s.key ? (s.on ? 'Has a key, in use' : 'Has a key, turned off') : 'No key yet'} />
        <div className="app-stack" style={{ gap: 'var(--d-space-2)', flex: 1 }}>
          <h3 className="d-card__title">{s.name}</h3>
          <span className="d-card__sub">{perThousand} per 1,000 searches · free: {s.free}{s.key ? ` · key …${s.key.ends_with}` : ''}{s.searches > 0 ? ` · ${s.searches} ${s.searches === 1 ? 'search' : 'searches'}, ${dollars(s.spent_usd)} this month` : ''}</span>
        </div>
        {s.key
          ? <Switch checked={s.on} onChange={(v) => setSetting(s.setting, v)} label={`Use ${s.name}`} />
          : <Button size="sm" onClick={() => setAdding(true)}><Icon icon={KeyRound} size="sm" />Add your key</Button>}
      </header>
      {s.key && (
        <div className="d-card__body app-row" style={{ gap: 'var(--d-space-8)' }}>
          <Button size="sm" variant="ghost" onClick={() => setAdding(true)}>Replace the key</Button>
          <Button size="sm" variant="ghost" onClick={() => setRemoving(true)}>Delete the key</Button>
        </div>
      )}
      {adding && (
        <Dialog open onOpenChange={(o) => !o && setAdding(false)} title={s.key ? `Replace the ${s.name} key` : `Add your ${s.name} key`}
          text={<>Paste the key {s.name} gave you. {where} It is never shown again: only its last four characters. It is sent only to {s.name}, with the words of a search.</>}
          actions={<><Button onClick={() => setAdding(false)}>Cancel</Button><Button variant="primary" disabled={!value.trim() || saving} onClick={() => void save()}>{saving ? 'A moment…' : 'Keep the key'}</Button></>}>
          <form className="app-stack" style={{ marginBlockStart: 'var(--d-space-16)' }} onSubmit={(e) => { e.preventDefault(); void save() }}>
            <label className="d-field">
              <span className="d-field__label">API key</span>
              <input className="d-input d-input--mono" type="password" autoComplete="off" spellCheck={false} autoFocus placeholder="Paste the key" value={value} onChange={(e) => setValue(e.target.value)} />
              <span className="d-field__help">No key yet? Get one at <a href={s.keys_at} target="_blank" rel="noreferrer noopener">{s.keys_at.replace('https://', '')}</a>. Billed by {s.name}, not by Demido.</span>
            </label>
          </form>
        </Dialog>
      )}
      {removing && (
        <Dialog open onOpenChange={(o) => !o && setRemoving(false)} title={`Delete the ${s.name} key?`}
          text={<>Searches go on with the free services. The key still works at {s.name} until you revoke it there.</>}
          actions={<><Button onClick={() => setRemoving(false)}>Cancel</Button><Button variant="danger" onClick={() => { setRemoving(false); void removeSearchKey(s) }}>Delete the key</Button></>} />
      )}
    </article>
  )
}

export function RealProviders() {
  const { providers, vault, spending, search, error } = useProviders()
  return (
    <Page title="Providers" lead="Where models can run besides this computer."
      about={<><p>A model elsewhere is sent what is said in a session that uses it, and costs money. A model here is free, and nothing leaves the computer.</p><p>Demido sends nothing to a model you did not choose here.</p></>}>
      {error && <Signal level="caution">The core did not say. {error}</Signal>}
      {!providers && !error && <p className="app-empty"><Glyph name="task-running" busy /> Reading the providers…</p>}
      {spending && <Month spending={spending} />}
      {providers && vault && (
        <Section title="Providers" hint={`${vault.where_words} A provider can hold several accounts, each with its own key.`}>
          <div className="app-stack">
            {providers.filter((p) => p.kind !== 'program').map((p) => <ProviderCard key={p.id} p={p} where={vault.where_words} />)}
          </div>
        </Section>
      )}
      {search && vault && (
        <Section title="Search with your own key" hint="A search goes first to the services you gave a key for, in this order, then to the free ones (Settings › The internet). Each search is counted with the spending above, at the service's list price, and stops at the month's limit; their free credits are not taken off, so the count is above the bill. The words of a search go to the service; nothing else does. Brave's and Exa's terms forbid keeping their results, so a search there is asked again rather than kept.">
          <div className="app-stack">
            {search.map((s) => <SearchCard key={s.id} s={s} where={vault.where_words} />)}
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
      <NotBuilt block>A limit of its own for each account, a provider's own spending figures for Anthropic and Google (an ordinary key cannot ask), OpenCode signed in to an account, pictures sent to a model, OpenCode's levels of thinking, and a Claude plan's limit before Claude Code has run once with it.</NotBuilt>
    </Page>
  )
}
