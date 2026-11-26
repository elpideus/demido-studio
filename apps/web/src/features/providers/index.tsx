/* Providers: where models can run besides this computer.

   With a core behind the pages, this is real (`real.tsx`): the keys Demido keeps, the models chosen at each
   provider, and what they cost this month. Without one, the preview below shows where it is going: several
   accounts per provider, and Claude Code and OpenCode. Accounts live in a small store of their own, so they
   stay while you move around the preview. */
import { useState } from 'react'
import { create } from 'zustand'
import { Cloud, Ellipsis, Plus } from 'lucide-react'
import type { Feature } from '@/app/registry'
import { useCore } from '@/data/core'
import { toast, useStore } from '@/data/store'
import type { Provider } from '@/data/types'
import { Button, Dialog, Glyph, Icon, Menu, NotBuilt, Page, Section, Signal } from '@/ui'
import { RealProviders } from './real'

type Usage = NonNullable<Provider['usage']>
interface Account { id: string; provider: string; label: string; hint: string; isDefault: boolean; usage?: Usage }

const SEED: Account[] = [
  { id: 'a1', provider: 'anthropic', label: 'Personal', hint: 'key ending in h4Qd', isDefault: true, usage: [{ label: 'This month', used: 4.18, limit: 25, unit: '$', reset: 'limit you set', source: 'reported' }] },
  { id: 'a2', provider: 'anthropic', label: 'Studio', hint: 'key ending in 9xKe', isDefault: false, usage: [{ label: 'This month', used: 41.6, limit: 100, unit: '$', reset: 'limit you set', source: 'reported' }] },
  { id: 'a3', provider: 'claude-code', label: 'you@example.com', hint: 'the login of your own Claude Code', isDefault: true, usage: [{ label: 'Programmatic credit', used: 17.4, limit: 20, unit: '$', reset: 'resets 1 Dec', source: 'estimated' }] },
  { id: 'a4', provider: 'openrouter', label: 'Personal', hint: 'key ending in Lm2a', isDefault: true, usage: [{ label: 'Credits', used: 3.12, limit: 10, unit: '$', source: 'reported' }] },
]
const useAccounts = create<{ accounts: Account[] }>(() => ({ accounts: SEED }))
const setAccounts = (fn: (a: Account[]) => Account[]) => useAccounts.setState({ accounts: fn(useAccounts.getState().accounts) })

function AddAccount({ p, edit, onClose }: { p: Provider; edit?: Account; onClose: () => void }) {
  const cli = p.kind === 'cli'
  const [label, setLabel] = useState(edit?.label ?? '')
  const [secret, setSecret] = useState('')
  const save = () => {
    if (!label.trim()) return
    if (edit) setAccounts((a) => a.map((x) => (x.id === edit.id ? { ...x, label: label.trim() } : x)))
    else
      setAccounts((a) => [...a, { id: `a${Date.now()}`, provider: p.id, label: label.trim(), hint: cli ? `profile ${secret.trim() || '~/.config/' + p.id + '-' + label.trim().toLowerCase()}` : `key ending in ${(secret.trim().slice(-4) || 'new1')}`, isDefault: !a.some((x) => x.provider === p.id) }])
    toast(edit ? 'Renamed.' : `${p.name}: account "${label.trim()}" added. Not checked in the preview.`)
    onClose()
  }
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()} title={edit ? 'Rename the account' : `Add a ${p.name} account`}
      text={edit ? undefined : cli ? `Demido runs the ${p.name} program that is installed on this computer, once per account, each with a profile folder of its own. It never copies a login and never pretends to be the program.` : 'A name for you, and a key. The key is stored in your system keyring, never in a file.'}
      actions={<><Button onClick={onClose}>Cancel</Button><Button variant="primary" disabled={!label.trim()} onClick={save}>{edit ? 'Rename' : 'Add the account'}</Button></>}>
      <form className="app-stack" style={{ marginBlockStart: 'var(--d-space-16)' }} onSubmit={(e) => { e.preventDefault(); save() }}>
        <label className="d-field"><span className="d-field__label">Name</span><input className="d-input" autoFocus placeholder="For example: Work" value={label} onChange={(e) => setLabel(e.target.value)} /></label>
        {!edit && (cli ? (
          <label className="d-field"><span className="d-field__label">Profile folder</span><input className="d-input d-input--mono" placeholder={`~/.config/${p.id}-work`} value={secret} onChange={(e) => setSecret(e.target.value)} /><span className="d-field__help">You sign in inside {p.name} itself, the first time this account is used.</span></label>
        ) : (
          <label className="d-field"><span className="d-field__label">API key</span><input className="d-input d-input--mono" type="password" placeholder="Paste the key" value={secret} onChange={(e) => setSecret(e.target.value)} /><span className="d-field__help">Billed by {p.name}, not by Demido.</span></label>
        ))}
      </form>
    </Dialog>
  )
}

function Meter({ name, u }: { name: string; u: Usage[number] }) {
  const pct = (u.used / u.limit) * 100
  return (
    <div className="prov__usage">
      <div className="app-row" style={{ justifyContent: 'space-between' }}>
        <span className="d-type-caption">{u.label} <span className="d-badge d-badge--outline">{u.source}</span></span>
        <span className="d-mono d-text-secondary">{u.unit}{u.used.toFixed(2)} / {u.unit}{u.limit.toFixed(2)}{u.reset ? ` · ${u.reset}` : ''}</span>
      </div>
      <div className="d-meter" data-tone={pct > 80 ? 'caution' : undefined} role="meter" aria-valuenow={Math.round(pct)} aria-valuemin={0} aria-valuemax={100} aria-label={`${name} ${u.label}`}><span style={{ '--d-meter': `${pct}%` } as React.CSSProperties} /></div>
    </div>
  )
}

function ProvidersPage() {
  const core = useCore()
  if (core.state === 'looking') return <Page title="Providers"><p className="app-empty"><Glyph name="task-running" /> Looking for the core…</p></Page>
  return core.state === 'ready' ? <RealProviders /> : <PreviewProviders />
}

function PreviewProviders() {
  const providers = useStore((s) => s.providers)
  const accounts = useAccounts((s) => s.accounts)
  const [adding, setAdding] = useState<{ p: Provider; edit?: Account } | null>(null)
  const remove = (a: Account) => setAccounts((list) => {
    const rest = list.filter((x) => x.id !== a.id)
    // The default goes to the next account of that provider, so there is always one.
    if (a.isDefault) { const next = rest.find((x) => x.provider === a.provider); if (next) return rest.map((x) => (x.id === next.id ? { ...x, isDefault: true } : x)) }
    return rest
  })
  return (
    <Page title="Providers" lead="Where models can run besides this computer." about="And what each account has cost you. Local models are free and private; everything else is someone else's computer.">
      <Section title="Providers and accounts" hint="A provider can hold several accounts. Usage is reported by the provider where it tells us, estimated from token counts where it does not.">
        <div className="app-stack">
          {providers.map((p) => {
            const mine = accounts.filter((a) => a.provider === p.id)
            const local = p.id === 'local'
            const installed = p.status !== 'not installed'
            const spent = mine.flatMap((a) => a.usage ?? []).filter((u) => u.unit === '$').reduce((n, u) => n + u.used, 0)
            return (
              <article key={p.id} className="d-card">
                <header className="d-card__head">
                  <Glyph name={local || mine.length ? 'task-done' : 'task-pending'} size="1.25rem" style={{ marginTop: '0.125rem' }} label={local || mine.length ? 'Connected' : 'No account yet'} />
                  <div className="app-stack" style={{ gap: 'var(--d-space-2)', flex: 1 }}>
                    <h3 className="d-card__title">{p.name}</h3>
                    <span className="d-card__sub">{local ? 'no network · free' : `${p.kind === 'cli' ? 'a program on this computer' : 'over the network'} · ${p.billing.toLowerCase()} · ${mine.length} ${mine.length === 1 ? 'account' : 'accounts'}${mine.length > 1 && spent ? ` · $${spent.toFixed(2)} together` : ''}`}</span>
                  </div>
                  {!local && installed && <Button size="sm" onClick={() => setAdding({ p })}><Icon icon={Plus} size="sm" />Add an account</Button>}
                </header>
                <div className="d-card__body app-stack">
                  <p className="d-text-secondary">{p.note}</p>
                  {!installed && <Signal level="note">{p.name} is not on this computer yet. Install it from its own site, then come back. Demido does not install or bundle it.</Signal>}
                  {mine.length > 0 && (
                    <ul className="prov__accounts">
                      {mine.map((a) => (
                        <li key={a.id}>
                          <label className="d-check"><input className="d-radio" type="radio" name={`default-${p.id}`} checked={a.isDefault} onChange={() => setAccounts((list) => list.map((x) => (x.provider === p.id ? { ...x, isDefault: x.id === a.id } : x)))} /><span className="d-check__text">{a.label}{a.isDefault && <span className="d-badge">Default</span>}<small className="d-mono">{a.hint}</small></span></label>
                          <div className="app-stack" style={{ gap: 'var(--d-space-6)' }}>{a.usage?.length ? a.usage.map((u) => <Meter key={u.label} name={`${p.name} ${a.label}`} u={u} />) : <span className="d-type-caption d-text-tertiary">No usage yet.</span>}</div>
                          <Menu align="end" trigger={<Button variant="ghost" size="sm" iconOnly aria-label={`More for ${a.label}`}><Icon icon={Ellipsis} /></Button>}
                            items={[{ label: 'Rename…', onSelect: () => setAdding({ p, edit: a }) }, { label: 'Usage by session', onSelect: () => toast('Per-session usage: not built in the preview.') }, { kind: 'sep' }, { label: 'Remove the account', hint: p.kind === 'cli' ? 'keeps its profile folder' : 'deletes the key', danger: true, onSelect: () => remove(a) }]} />
                        </li>
                      ))}
                    </ul>
                  )}
                  {p.id === 'claude-code' && mine.length > 0 && <Signal level="caution">87% of this month's programmatic credit is used. Past it, Claude Code bills extra usage if you switched that on with Anthropic.</Signal>}
                </div>
              </article>
            )
          })}
        </div>
      </Section>
      <p className="d-type-caption d-text-secondary">A session uses the provider's default account. You will be able to pick another one next to the model, per session and per schedule. <NotBuilt>Picking an account per session</NotBuilt></p>
      <NotBuilt block>More providers come later. Adding one that speaks a known API is a settings file, not new code.</NotBuilt>
      {adding && <AddAccount p={adding.p} edit={adding.edit} onClose={() => setAdding(null)} />}
    </Page>
  )
}

export const feature: Feature = { id: 'providers', title: 'Providers', icon: Cloud, group: 'system', order: 0, path: 'providers', Page: ProvidersPage }
