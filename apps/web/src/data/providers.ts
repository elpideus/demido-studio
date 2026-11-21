/* Models elsewhere, for real: the keys the core keeps for them, the models chosen there, and what they cost.

   A key goes to the core once and never comes back: what this page can know of one is its name, where it is
   kept and its last four characters. Read once when a core is found, and again when the core says a key, a
   choice or a cost changed. */
import { create } from 'zustand'
import { coreGet, coreSend, onCoreEvent } from './core'
import { toast } from './store'

export interface KeyFacts { name: string; label: string; kept_in: 'keyring' | 'file'; ends_with: string; added_at: number }
export interface VaultView { new_keys_go_to: 'keyring' | 'file'; where_words: string; keys: KeyFacts[]; known: { name: string; label: string }[] }
/** `price_from`: `provider` (it says so), `table` (Demido's table, read on a stated day), `highest` (not in the table: the provider's highest is counted), `unknown`. Prices in dollars per million tokens. */
export interface RemoteModel {
  id: string; provider: string; name: string; context_length: number | null; most_written: number | null
  price_in: number | null; price_out: number | null; price_from: 'provider' | 'table' | 'highest' | 'plan' | 'program' | 'unknown'; free: boolean
  thinking: string[]; always_thinks: boolean; takes: string[]; chosen: boolean
  /** What to know first: what a free model does with what is sent to it. */
  note?: string | null
}
/** A program on this computer (Claude Code, OpenCode): where it is, its version, how it is signed in (never the login itself). */
export interface Found { provider: string; path: string | null; version: string | null; signed_in: string | null; problem: string | null }
/** A program's plan limit, as it last said it while it ran. `used` from 0 to 1. */
export interface PlanLimit { status: 'allowed' | 'allowed_warning' | 'rejected' | string; resets_at: number | null; used: number | null; said_at: number }
/** Demido's own copy of a program, kept unchanged in its folder. */
export interface Kept { provider: string; version: string; path: string; size: number; checked: string; from: string; kept_at: number }
export interface Downloading { version: string; done: number; total: number | null }
export interface Latest { provider: string; version: string; size: number | null }
/** Claude Code's own sign-in, under way: `url` is Anthropic's page, for when no browser opened. */
export interface SignIn { state: 'started' | 'done' | 'failed'; url: string | null; said: string | null }
/** `kind`: `api` (Demido asks the model itself) or `program` (a program runs its own loop: always Demido's own copy). */
export interface ProviderView {
  id: string; name: string; kind: 'api' | 'program'; program: Found | null; plan: PlanLimit | null
  copy: Kept | null; downloading: Downloading | null; signing: SignIn | null; sign_in_command: string | null
  keys_at: string; note: string; key: KeyFacts | null; chosen: RemoteModel[]; spent_usd: number; spent_exact: boolean
}
/** `counted`: whether it counts toward the month's limit. A program's use does not: its plan holds it. */
export interface Spent { provider: string; model: string; requests: number; input: number; output: number; cost_usd: number; exact: boolean; counted: boolean }
export interface Spending { since: number; until: number; limit_usd: number | null; spent_usd: number; exact: boolean; prices_read_on: string; by_model: Spent[] }
export interface AccountLine { label: string; used: number; limit: number | null; unit: string; reset: string | null }
export interface Account { provider: string; lines: AccountLine[]; note: string | null }

interface State {
  providers: ProviderView[] | null
  vault: VaultView | null
  spending: Spending | null
  error: string | null
}

export const useProviders = create<State>(() => ({ providers: null, vault: null, spending: null, error: null }))
const set = useProviders.setState

let reading: Promise<void> | null = null
let again = false
/** Asks the core for the providers, the vault and the month's spending. Several askers share one answer; one
    who asks while an answer is on its way gets one more, read after theirs: what they heard of may have
    happened after the first was asked (a turn's cost, written just after its plan limit was told). */
export function readProviders(): Promise<void> {
  if (reading) {
    again = true
    return reading
  }
  reading = (async () => {
    do {
      again = false
      try {
        const [providers, vault, spending] = await Promise.all([coreGet<ProviderView[]>('/providers'), coreGet<VaultView>('/vault'), coreGet<Spending>('/usage')])
        set({ providers, vault, spending, error: null })
      } catch (e) {
        set({ error: (e as Error).message })
      }
    } while (again)
  })().finally(() => { reading = null })
  return reading
}

/** Gives the core a key to keep. Returns false, with the reason shown, when it was not kept. */
export async function saveKey(name: string, value: string): Promise<boolean> {
  try {
    const facts = await coreSend<KeyFacts>('PUT', `/vault/${encodeURIComponent(name)}`, { value })
    toast(`Kept: the key ending in …${facts.ends_with}, in ${facts.kept_in === 'keyring' ? 'the system keyring' : 'a file only your user can read'}.`, undefined, 'task-done')
    await readProviders()
    return true
  } catch (e) {
    toast(`The key was not kept. ${(e as Error).message}`, undefined, 'task-failed')
    return false
  }
}

export async function removeKey(name: string) {
  try {
    await coreSend('DELETE', `/vault/${encodeURIComponent(name)}`)
    toast('The key is deleted.', undefined, 'task-done')
  } catch (e) {
    toast(`The key was not deleted. ${(e as Error).message}`, undefined, 'task-failed')
  }
  await readProviders()
}

/** The models a provider has, as it says. `fresh`: ask it now, not from what the core kept. Throws with the core's words. */
export function listModels(provider: string, fresh = false): Promise<RemoteModel[]> {
  return coreGet<RemoteModel[]>(`/providers/${encodeURIComponent(provider)}/models${fresh ? '?fresh=true' : ''}`)
}

/** Chooses a model so that sessions may use it, or lets it go. */
export async function choose(provider: string, model: string, chosen: boolean): Promise<boolean> {
  try {
    await coreSend('PUT', `/providers/${encodeURIComponent(provider)}/chosen`, { model, chosen })
    await readProviders()
    return true
  } catch (e) {
    toast(`${chosen ? 'Not chosen' : 'Not let go'}. ${(e as Error).message}`, undefined, 'task-failed')
    return false
  }
}

/** Downloads Demido's own copy of a program. Its progress comes as news. */
export async function downloadCopy(provider: string) {
  try {
    await coreSend('POST', `/providers/${provider}/program`)
  } catch (e) {
    toast(`Not downloaded. ${(e as Error).message}`, undefined, 'task-failed')
  }
  await readProviders()
}

export async function stopDownload(provider: string) {
  await coreSend('DELETE', `/providers/${provider}/program/download`).catch(() => {})
  await readProviders()
}

export async function removeCopy(provider: string) {
  try {
    await coreSend('DELETE', `/providers/${provider}/program`)
    toast('Demido\'s copy is deleted. Its profile (its settings and login) is kept.', undefined, 'task-done')
  } catch (e) {
    toast(`Not deleted. ${(e as Error).message}`, undefined, 'task-failed')
  }
  await readProviders()
}

export function latestCopy(provider: string): Promise<Latest> {
  return coreGet<Latest>(`/providers/${provider}/program/latest`)
}

/** Starts the program's own sign-in, on its maker's page. */
export async function signIn(provider: string) {
  try {
    await coreSend('POST', `/providers/${provider}/sign-in`)
  } catch (e) {
    toast(`The sign-in did not start. ${(e as Error).message}`, undefined, 'task-failed')
  }
  await readProviders()
}

export function readAccount(provider: string): Promise<Account> {
  return coreGet<Account>(`/providers/${encodeURIComponent(provider)}/account`)
}

/** The models chosen at every provider: the ones a session may use. */
export function chosenModels(providers: ProviderView[] | null): RemoteModel[] {
  return providers?.flatMap((p) => p.chosen) ?? []
}

/** Dollars, with as many places as an amount that small needs: $0.00, $0.0148, $3.40. */
export function dollars(n: number): string {
  if (n <= 0) return '$0.00'
  if (n < 1) return `$${n.toFixed(4)}`
  return `$${n.toFixed(2)}`
}

/** How many tokens a model holds, short: 131k, 1M. */
export function holds(tokens: number): string {
  return tokens >= 1_000_000 ? `${+(tokens / 1_000_000).toFixed(1)}M` : `${Math.round(tokens / 1000)}k`
}

/** A price per million tokens, in and out. */
export function priceOf(m: RemoteModel): string {
  if (m.free) return 'free'
  if (m.price_from === 'plan') return 'billed by your Claude plan'
  if (m.price_from === 'program') return 'billed through the provider you set up in OpenCode'
  if (m.price_in === null || m.price_out === null) return 'price not known in advance'
  const p = (n: number) => (n < 0.1 ? n.toFixed(3) : n < 10 ? n.toFixed(2) : n.toFixed(0))
  return `$${p(m.price_in)} in · $${p(m.price_out)} out, a million tokens`
}

let started = false
/** Called once, when a core is found. */
export function startProviders() {
  if (started) return
  started = true
  void readProviders()
  for (const kind of ['vault.key.added', 'vault.key.removed', 'provider.model.chosen', 'provider.model.dropped', 'provider.used', 'provider.plan']) onCoreEvent(kind, () => void readProviders())
  onCoreEvent('setting.changed', (e) => { if (e.subject === 'providers.monthlyLimit') void readProviders() })
  for (const kind of ['program.downloaded', 'program.removed', 'program.signin']) onCoreEvent(kind, () => void readProviders())
  onCoreEvent('program.download.failed', (e) => { toast(`The download stopped. ${String(e.data.problem ?? '')}`, undefined, 'task-failed'); void readProviders() })
  // How far a download has come, told often: written into the list without asking the core again.
  onCoreEvent('program.download', (e) => {
    const providers = useProviders.getState().providers
    if (!providers || !e.subject) return
    const downloading = { version: String(e.data.version ?? ''), done: Number(e.data.done ?? 0), total: e.data.total == null ? null : Number(e.data.total) }
    set({ providers: providers.map((p) => (p.id === e.subject ? { ...p, downloading } : p)) })
  })
}
