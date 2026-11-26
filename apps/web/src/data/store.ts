/* One store for the whole UI. Components read from it and call the functions exported here.
   In the preview those functions are a mock core: they change the store on timers, the way the real core
   will change it through API events. Swapping the mock for the real client means replacing this file's
   functions; the components do not change. */
import { create } from 'zustand'
import { startLocal } from './local-models'
import { startHub } from './hub'
import { startProviders } from './providers'
import { startSessions } from './real-sessions'
import { startSchedules } from './schedules'
import { coreGet, coreSend, look, onCoreEvent, type CoreAppearance, type CoreSection, type CoreSetting, type CoreSettings } from './core'
import { AGENTS, ARTIFACTS, CHARACTERS, MODELS, PROVIDERS, SCHEDULES, SESSIONS, SYSTEM, TOOLS } from './fixtures'
import type { Agent, Artifact, Block, Character, Message, Model, PermissionLevel, Provider, Schedule, Session, Task, Toast, Tool, SessionGroup } from './types'

const SETTINGS_KEY = 'demido-preview-settings'
/** What the core's settings were on the last visit, so the theme is right before the core has answered. */
const CORE_SETTINGS_KEY = 'demido-core-settings'
const SYSTEM_ACCENT_KEY = 'demido-core-system-accent'
export const IDLE_UNLOAD_PREVIEW_MS = 45_000

interface State {
  /** Settings the core does not declare yet. Kept in this browser; part of the scripted preview. */
  settings: Record<string, unknown>
  /** Settings the core declares, by id, with their values. Empty when no core is behind these pages. */
  core: Record<string, CoreSetting>
  coreSections: CoreSection[]
  /** The system's accent as the core reads it. `null` until a core has answered (and always, in the preview).
      `here` is false when this window is on another computer than the core: its desktop is not ours. */
  systemAccent: (CoreAppearance & { here: boolean }) | null
  sessions: Session[]
  groups: SessionGroup[]
  models: Model[]
  tools: Tool[]
  /** Tool files you edited or added: tool id, then path, then the text. */
  toolFiles: Record<string, Record<string, string>>
  schedules: Schedule[]
  characters: Character[]
  artifacts: Artifact[]
  agents: Agent[]
  providers: Provider[]
  toasts: Toast[]
  openArtifact: { id: string; version: number } | null
}

/** Which fixture sessions start out in a group, so groups are visible on first look. */
const FILED: Record<string, string> = { 's-auth': 'g-code', 's-sweater': 'g-home', 's-tax': 'g-home' }

function loadStored(key: string): Record<string, unknown> {
  try {
    return JSON.parse(localStorage.getItem(key) || '{}')
  } catch {
    return {}
  }
}
/** The core's values as last seen. Used only until the core answers. */
let remembered: Record<string, unknown> = loadStored(CORE_SETTINGS_KEY)
/** The system accent as last read through the core, for the same reason. */
let rememberedAccent: string | null = (() => {
  try {
    return localStorage.getItem(SYSTEM_ACCENT_KEY)
  } catch {
    return null
  }
})()

export const useStore = create<State>(() => ({
  settings: loadStored(SETTINGS_KEY),
  core: {},
  coreSections: [],
  systemAccent: null,
  sessions: structuredClone(SESSIONS).map((s) => ({ ...s, groupId: FILED[s.id] })),
  groups: [{ id: 'g-code', name: 'Billing service', open: true }, { id: 'g-home', name: 'Home', open: true }],
  models: structuredClone(MODELS),
  tools: structuredClone(TOOLS),
  toolFiles: {},
  schedules: structuredClone(SCHEDULES),
  characters: structuredClone(CHARACTERS),
  artifacts: structuredClone(ARTIFACTS),
  agents: structuredClone(AGENTS),
  providers: structuredClone(PROVIDERS),
  toasts: [],
  openArtifact: null,
}))

const get = useStore.getState
const set = useStore.setState
const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms))
let uid = 0
const id = (p: string) => `${p}-${Date.now().toString(36)}-${(uid++).toString(36)}`

// ───────────────────────── settings ─────────────────────────
/* Two kinds of setting live side by side while the app moves from the preview to the real thing.

   A setting the core declares is real: the core keeps it, checks it, logs the change and tells every open
   window. Everything else is still the preview's: kept in this browser, read only by scripted screens.
   Callers do not choose. `setting`, `useSetting` and `setSetting` look the id up and go to the right place. */
function pick<T>(state: State, key: string, fallback: T): T {
  const kept = state.core[key]
  if (kept) return kept.value as T
  const v = key in remembered ? remembered[key] : state.settings[key]
  return (v === undefined ? fallback : v) as T
}
export function setting<T>(key: string, fallback: T): T {
  return pick(get(), key, fallback)
}
export function useSetting<T>(key: string, fallback: T): T {
  return useStore((s) => pick(s, key, fallback))
}
/** Changes a setting. `undefined` puts it back to its default. */
export function setSetting(key: string, value: unknown) {
  if (get().core[key]) {
    changeInCore(key, value)
    return
  }
  const settings = { ...get().settings }
  if (value === undefined) delete settings[key]
  else settings[key] = value
  set({ settings })
  try {
    localStorage.setItem(SETTINGS_KEY, JSON.stringify(settings))
  } catch {}
  applyAppearance()
}

const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b)

function keep(entry: CoreSetting) {
  const core = { ...get().core, [entry.def.id]: entry }
  set({ core })
  remembered = Object.fromEntries(Object.values(core).map((e) => [e.def.id, e.value]))
  try {
    localStorage.setItem(CORE_SETTINGS_KEY, JSON.stringify(remembered))
  } catch {}
  applyAppearance()
}

/* A slider or a colour well changes many times a second. The screen follows at once; the core is told the
   last value once the hand rests. While a change is on its way, the core's own news about that setting is
   not applied: it would be older than what is on screen. */
const SETTLE_MS = 200
const writes = new Map<string, { timer: ReturnType<typeof setTimeout> | null; busy: boolean; next: { value: unknown } | null }>()

function changeInCore(key: string, value: unknown) {
  const now = get().core[key]
  const shown = value === undefined ? now.def.default : value
  keep({ def: now.def, value: shown, changed: !same(shown, now.def.default) })
  const w = writes.get(key) ?? { timer: null, busy: false, next: null }
  writes.set(key, w)
  w.next = { value }
  if (w.busy) return
  if (w.timer) clearTimeout(w.timer)
  w.timer = setTimeout(() => void send(key), SETTLE_MS)
}

async function send(key: string) {
  const w = writes.get(key)
  if (!w?.next) return
  const { value } = w.next
  w.next = null
  w.timer = null
  w.busy = true
  try {
    const path = `/settings/${encodeURIComponent(key)}`
    const kept = value === undefined ? await coreSend<CoreSetting>('DELETE', path) : await coreSend<CoreSetting>('PUT', path, { value })
    if (!w.next) keep(kept)
  } catch (e) {
    // The core refused, in its own words. Show them, and show what the core really holds.
    w.next = null
    toast(`Not changed. ${(e as Error).message}`, undefined, 'task-failed')
    try {
      keep(await coreGet<CoreSetting>(`/settings/${encodeURIComponent(key)}`))
    } catch {}
  }
  w.busy = false
  if (w.next) void send(key)
  else writes.delete(key)
}

let started: Promise<void> | null = null
/** Looks for a core once. If one is there, its settings become the real ones and its news is followed. */
export function startCore(): Promise<void> {
  started ??= (async () => {
    const core = await look()
    if (core.state !== 'ready') {
      // No core here (the preview), or this browser is not let in: nothing remembered applies.
      if (Object.keys(remembered).length) {
        remembered = {}
        rememberedAccent = null
        try {
          localStorage.removeItem(CORE_SETTINGS_KEY)
          localStorage.removeItem(SYSTEM_ACCENT_KEY)
        } catch {}
        applyAppearance()
      }
      return
    }
    try {
      const all = await coreGet<CoreSettings>('/settings')
      set({ coreSections: all.sections })
      all.settings.forEach(keep)
      onCoreEvent('setting.changed', (e) => {
        const now = e.subject ? get().core[e.subject] : undefined
        if (!now || writes.has(now.def.id)) return
        keep({ def: now.def, value: e.data.to, changed: !same(e.data.to, now.def.default) })
      })
    } catch (e) {
      toast(`The core did not give its settings. ${(e as Error).message}`, undefined, 'task-failed')
    }
    void readSystemAccent()
    startLocal()
    startHub()
    startProviders()
    startSessions()
    startSchedules()
    // The accent is changed in the system's own settings, which means leaving this window. Ask again on return.
    document.addEventListener('visibilitychange', () => { if (!document.hidden) void readSystemAccent() })
    window.addEventListener('focus', () => void readSystemAccent())
  })()
  return started
}

/** "Automatic" follows the system's accent. The core can read it, but only its own computer's.
    Getting the focus back fires two signals at once; one reading serves both. */
let readingAccent: Promise<void> | null = null
function readSystemAccent(): Promise<void> {
  readingAccent ??= readSystemAccentNow().finally(() => { readingAccent = null })
  return readingAccent
}
async function readSystemAccentNow() {
  const here = ['127.0.0.1', 'localhost', '[::1]', '::1'].includes(location.hostname)
  let seen: CoreAppearance = { accent: null, source: null, supported: false }
  if (here) {
    try {
      seen = await coreGet<CoreAppearance>('/system/appearance')
    } catch {
      return // keep what was known; the next look may work
    }
  }
  const before = get().systemAccent
  if (before && before.accent === seen.accent && before.supported === seen.supported && before.here === here) return
  set({ systemAccent: { ...seen, here } })
  rememberedAccent = seen.accent
  try {
    if (seen.accent) localStorage.setItem(SYSTEM_ACCENT_KEY, seen.accent)
    else localStorage.removeItem(SYSTEM_ACCENT_KEY)
  } catch {}
  applyAppearance()
}

/** Theme, text size and accent are applied to <html>, where the brand tokens read them. */
export function applyAppearance() {
  const el = document.documentElement
  // A theme swap is one change: every part at once, none fading at its own speed.
  el.setAttribute('data-swapping', '')
  requestAnimationFrame(() => requestAnimationFrame(() => el.removeAttribute('data-swapping')))
  const theme = setting<string>('appearance.theme', 'system')
  if (theme === 'dark' || theme === 'light') el.dataset.theme = theme
  else delete el.dataset.theme
  const size = setting('appearance.textSize', 100)
  el.style.fontSize = size === 100 ? '' : `${size}%`
  const mode = setting<string>('appearance.accent', 'auto')
  // With a core, the system accent is the real one (or none). Only the preview pretends.
  const read = get().systemAccent
  const withCore = read !== null || Object.keys(remembered).length > 0
  const system = withCore ? (read ? read.accent : rememberedAccent) ?? '' : setting('preview.systemAccent', '')
  const accent = mode === 'custom' ? setting('appearance.accentCustom', '#68efaf') : mode === 'auto' ? system : ''
  if (accent) el.style.setProperty('--d-color-accent-source', accent)
  else el.style.removeProperty('--d-color-accent-source')
}

// ───────────────────────── toasts ─────────────────────────
export function toast(text: string, action?: Toast['action'], glyph?: string) {
  const t: Toast = { id: id('toast'), text, action, glyph }
  set({ toasts: [...get().toasts.slice(-2), t] })
  setTimeout(() => dismissToast(t.id), action ? 9000 : 4200)
}
export function dismissToast(toastId: string) {
  if (!get().toasts.some((t) => t.id === toastId && !t.leaving)) return
  set({ toasts: get().toasts.map((t) => (t.id === toastId ? { ...t, leaving: true } : t)) })
  setTimeout(() => set({ toasts: get().toasts.filter((t) => t.id !== toastId) }), 180)
}

// ───────────────────────── helpers ─────────────────────────
function patchModel(modelId: string, patch: Partial<Model>) {
  set({ models: get().models.map((m) => (m.id === modelId ? { ...m, ...patch } : m)) })
}
function patchSession(sessionId: string, fn: (s: Session) => Session) {
  set({ sessions: get().sessions.map((s) => (s.id === sessionId ? fn(s) : s)) })
}
function patchLast(sessionId: string, fn: (m: Message) => Message) {
  patchSession(sessionId, (s) => ({ ...s, messages: s.messages.map((m, i) => (i === s.messages.length - 1 ? fn(m) : m)) }))
}
function pushBlock(sessionId: string, block: Block) {
  patchLast(sessionId, (m) => ({ ...m, blocks: [...(m.blocks ?? []), block] }))
}
function mapBlocks(sessionId: string, fn: (b: Block) => Block) {
  patchLast(sessionId, (m) => ({ ...m, blocks: (m.blocks ?? []).map(fn) }))
}
const mapTasks = (tasks: Task[], fn: (t: Task) => Task): Task[] => tasks.map((t) => fn({ ...t, children: t.children ? mapTasks(t.children, fn) : undefined }))
function setTask(sessionId: string, taskId: string, patch: Partial<Task>) {
  mapBlocks(sessionId, (b) => (b.kind === 'tasks' ? { ...b, tasks: mapTasks(b.tasks, (t) => (t.id === taskId ? { ...t, ...patch } : t)) } : b))
}
function setStep(sessionId: string, stepId: string, patch: object) {
  mapBlocks(sessionId, (b) => (b.kind === 'step' && b.step.id === stepId ? { ...b, step: { ...b.step, ...patch } } : b))
}
export const modelById = (modelId: string) => get().models.find((m) => m.id === modelId)
export const memoryOf = (m: Model) => m.quants.find((q) => q.id === (m.installed ?? m.quants[0].id))?.memoryGb ?? 0

/** How the 12 GB are spent right now. The same numbers feed every memory bar in the app. */
export function memoryBudget(models: Model[], agents: Agent[]) {
  const live = models.filter((m) => m.id !== 'nomic-embed' && ['loaded', 'generating', 'lent', 'loading'].includes(m.state))
  const model = live.reduce((n, m) => n + memoryOf(m), 0)
  const agent = agents.filter((a) => a.state !== 'done').reduce((n, a) => n + a.memoryGb, 0)
  const used = model + agent + SYSTEM.embeddingGb + SYSTEM.reserveGb
  return { total: SYSTEM.vramGb, model, agent, aux: SYSTEM.embeddingGb, reserve: SYSTEM.reserveGb, free: Math.max(0, SYSTEM.vramGb - used), live }
}

// ───────────────────────── models ─────────────────────────
const idleTimers = new Map<string, ReturnType<typeof setTimeout>>()
function armIdle(modelId: string) {
  clearTimeout(idleTimers.get(modelId))
  idleTimers.set(
    modelId,
    setTimeout(() => {
      const m = modelById(modelId)
      if (!m || m.state !== 'loaded') return
      patchModel(modelId, { state: 'unloaded' })
      toast(`${m.name} unloaded after being idle. (In this preview that takes 45 s, not 5 min.)`, { label: 'Reload', run: () => void loadModel(modelId) })
    }, IDLE_UNLOAD_PREVIEW_MS),
  )
}

export async function loadModel(modelId: string): Promise<boolean> {
  const m = modelById(modelId)
  if (!m || !m.installed) return false
  if (['loaded', 'generating', 'lent'].includes(m.state)) return true
  if (m.state === 'loading') {
    while (modelById(modelId)?.state === 'loading') await sleep(120)
    return modelById(modelId)?.state === 'loaded'
  }
  // Make room: unload other large models that would not fit beside this one.
  const need = memoryOf(m)
  for (const other of memoryBudget(get().models, get().agents).live) {
    if (other.id === modelId) continue
    const b = memoryBudget(get().models, get().agents)
    if (b.free >= need) break
    if (other.state === 'generating' || other.state === 'lent') {
      patchModel(modelId, { state: 'queued' })
      toast(`${m.name} is waiting for memory: ${other.name} is in use.`)
      while (['generating', 'lent'].includes(modelById(other.id)?.state ?? '')) await sleep(300)
    }
    patchModel(other.id, { state: 'unloaded' })
    toast(`Unloaded ${other.name} to make room for ${m.name}.`)
  }
  if (memoryBudget(get().models, get().agents).free < need) {
    toast(`${m.name} does not fit: it needs ${need.toFixed(1)} GB. Part of it will run on the CPU, slower.`)
  }
  patchModel(modelId, { state: 'loading', loadProgress: 0 })
  for (let p = 0; p <= 100; p += 4) {
    if (modelById(modelId)?.state !== 'loading') return false
    patchModel(modelId, { loadProgress: p })
    await sleep(55 + need * 6)
  }
  patchModel(modelId, { state: 'loaded', loadProgress: undefined })
  armIdle(modelId)
  return true
}
export function unloadModel(modelId: string) {
  clearTimeout(idleTimers.get(modelId))
  patchModel(modelId, { state: 'unloaded', loadProgress: undefined, lentTo: undefined })
}
export async function downloadModel(modelId: string, quant: string) {
  const m = modelById(modelId)
  if (!m || m.download) return
  for (let p = 0; p <= 100; p += 2) {
    if (p > 0 && !modelById(modelId)?.download) return // cancelled
    patchModel(modelId, { download: { quant, progress: p } })
    await sleep(90)
  }
  patchModel(modelId, { download: undefined, installed: quant })
  toast(`${m.name} ${quant} downloaded and checked.`, { label: 'Load', run: () => void loadModel(modelId) }, 'task-done')
}
export const cancelDownload = (modelId: string) => patchModel(modelId, { download: undefined })
export function deleteModel(modelId: string) {
  unloadModel(modelId)
  patchModel(modelId, { installed: undefined })
}

// ───────────────────────── sessions and the scripted answer ─────────────────────────
const runs = new Map<string, number>()
const waiters = new Map<string, (choice: 'allowed' | 'allowed-session' | 'denied') => void>()

export function newSession(): string {
  const sid = id('s')
  const s: Session = {
    id: sid, title: 'New session', tags: [], when: 'Today', updated: 'now', state: 'pending',
    modelId: setting('models.default', 'qwen3-4b'), permission: setting<PermissionLevel>('permissions.level', 'balanced'),
    thinking: 'low', contextUsed: 0, taint: [], folder: '~/Demido/sessions/new-session', messages: [],
  }
  set({ sessions: [s, ...get().sessions] })
  return sid
}
export function setSessionField<K extends keyof Session>(sessionId: string, key: K, value: Session[K]) {
  patchSession(sessionId, (s) => ({ ...s, [key]: value }))
}
// ───────────────────────── tool files ─────────────────────────
/** Saves one file of a tool. An edited tool is no longer the audited one, so its audit is cleared. */
export function saveToolFile(toolId: string, path: string, text: string) {
  const all = get().toolFiles
  set({
    toolFiles: { ...all, [toolId]: { ...all[toolId], [path]: text } },
    tools: get().tools.map((t) => (t.id !== toolId ? t : { ...t, audit: t.installed ? 'none' : t.audit, auditNote: t.installed ? undefined : t.auditNote, ...(path === 'guidance.md' ? { guidance: text.trimEnd(), guidanceEdited: true } : null) })),
  })
}
/** Back to the published text; a file you added yourself is removed. */
export function resetToolFile(toolId: string, path: string) {
  const mine = { ...get().toolFiles[toolId] }
  delete mine[path]
  set({ toolFiles: { ...get().toolFiles, [toolId]: mine } })
}

// ───────────────────────── session groups ─────────────────────────
export function addGroup(name: string): string {
  const gid = id('g')
  set({ groups: [...get().groups, { id: gid, name: name.trim(), open: true }] })
  return gid
}
export function renameGroup(groupId: string, name: string) {
  set({ groups: get().groups.map((g) => (g.id === groupId ? { ...g, name: name.trim() } : g)) })
}
export function toggleGroup(groupId: string) {
  set({ groups: get().groups.map((g) => (g.id === groupId ? { ...g, open: !g.open } : g)) })
}
/** Deleting a group keeps its sessions: they go back to the plain list. */
export function removeGroup(groupId: string) {
  set({ groups: get().groups.filter((g) => g.id !== groupId), sessions: get().sessions.map((s) => (s.groupId === groupId ? { ...s, groupId: undefined } : s)) })
}
export function moveSession(sessionId: string, groupId: string | undefined) {
  patchSession(sessionId, (s) => ({ ...s, groupId }))
}

export function stopRun(sessionId: string) {
  runs.set(sessionId, (runs.get(sessionId) ?? 0) + 1)
  const s = get().sessions.find((x) => x.id === sessionId)
  patchLast(sessionId, (m) => ({ ...m, streaming: false }))
  patchSession(sessionId, (x) => ({ ...x, state: 'skipped' }))
  set({ agents: get().agents.filter((a) => a.sessionId !== sessionId) })
  if (s) {
    const m = modelById(s.modelId)
    if (m && ['generating', 'lent'].includes(m.state)) {
      patchModel(m.id, { state: 'loaded', lentTo: undefined })
      armIdle(m.id)
    }
  }
}
export function answerPermission(sessionId: string, requestId: string, choice: 'allowed' | 'allowed-session' | 'denied') {
  patchSession(sessionId, (s) => ({
    ...s,
    messages: s.messages.map((m) => ({ ...m, blocks: m.blocks?.map((b) => (b.kind === 'permission' && b.request.id === requestId ? { ...b, request: { ...b.request, state: choice } } : b)) })),
  }))
  const w = waiters.get(requestId)
  if (w) {
    waiters.delete(requestId)
    w(choice)
  } else {
    // A request that was already in the fixtures: reflect the choice in its task list.
    patchSession(sessionId, (s) => ({ ...s, state: choice === 'denied' ? 'skipped' : 'done' }))
    toast(choice === 'denied' ? 'Denied. Nothing was run.' : 'Allowed. In the preview, nothing is actually run.')
  }
}

type Scenario = {
  title: string
  tags: string[]
  taint: string[]
  thinking: string
  tasks: Task[]
  step1: { what: string; code?: string; meta: string; request: string; output: string; untrusted?: 'web' | 'email'; fullLines?: number }
  risky?: { risk: 'medium' | 'high'; riskLabel: string; title: string; what: string; why: string; raw: string }
  agentLabel: string
  answerPlain: string
  answerHtml: string
}

function scenarioFor(text: string): Scenario {
  const t = text.toLowerCase()
  const short = text.trim().replace(/\s+/g, ' ').slice(0, 42)
  if (/\b(delete|remove|clean|rm|wipe|tidy)\b/.test(t)) {
    return {
      title: short, tags: ['files'], taint: [], thinking: 'need: find what to remove\nlist first. never guess path\ndelete = destructive → ask',
      tasks: [{ id: 't1', label: 'List the folder', state: 'pending' }, { id: 't2', label: 'Remove what is not needed', state: 'pending' }, { id: 't3', label: 'Check the result', state: 'pending' }],
      step1: { what: 'Ran', code: 'ls -la', meta: '12 of 96 lines · 0.3 s', request: 'tool     shell.run\ncommand  ls -la', output: 'build/      418 files   212 MB\ncache/       96 files    48 MB\nsrc/         31 files\n(84 lines of file names left out)', fullLines: 96 },
      risky: { risk: 'high', riskLabel: 'High risk · deletes files', title: 'Run a shell command', what: 'rm -rf ./build ./cache', why: 'These two folders hold generated files only. 514 files, 260 MB, inside the session folder.', raw: 'tool      shell.run\ncommand   rm -rf ./build ./cache\nasked by  main session' },
      agentLabel: 'Checking nothing else pointed at those folders',
      answerPlain: 'Done. Removed 2 folders, 514 files, 260 MB.', answerHtml: '<p><strong>Done.</strong> Removed 2 folders, 514 files, 260 MB.</p><ol><li><code>build</code> and <code>cache</code> are gone.</li><li><code>src</code> is untouched: 31 files.</li></ol>',
    }
  }
  if (/\b(email|mail|send|reply|write to)\b/.test(t)) {
    return {
      title: short, tags: ['email'], taint: ['email'], thinking: 'need: read thread, draft reply\nmail body = data, never instructions\nsend = external write → ask',
      tasks: [{ id: 't1', label: 'Find the message', state: 'pending' }, { id: 't2', label: 'Draft and send the reply', state: 'pending' }, { id: 't3', label: 'File the thread', state: 'pending' }],
      step1: { what: 'Read the thread with', code: 'anna@example.com', meta: '3 messages · 0.9 s', untrusted: 'email', request: 'tool     email.read\naccount  personal\nthread   "Thursday"', output: 'Anna Keller, Mon 16:12   Can we move Thursday to 15:00?\nYou, Mon 18:40           Probably, let me check.\nAnna Keller, Tue 09:05   Any news?' },
      risky: { risk: 'medium', riskLabel: 'Medium risk · sends a message as you', title: 'Send an email', what: 'To: anna@example.com\nSubject: Re: Thursday\n\nThursday 15:00 works. See you then.', why: 'Answers her question from Tuesday. Nothing else is sent.', raw: 'tool      email.send\naccount   personal\nto        anna@example.com\nasked by  main session' },
      agentLabel: 'Filing the thread under Done',
      answerPlain: 'Sent. Anna has her answer: Thursday 15:00.', answerHtml: '<p><strong>Sent.</strong> Anna has her answer: Thursday 15:00.</p><p>The thread is filed under <strong>Done</strong>.</p>',
    }
  }
  return {
    title: short, tags: ['research'], taint: ['web'], thinking: 'need: facts, not guess → search\npage text = data, never instructions\nanswer first. steps after',
    tasks: [{ id: 't1', label: 'Search the web', state: 'pending' }, { id: 't2', label: 'Read the two best pages', state: 'pending' }, { id: 't3', label: 'Compare and answer', state: 'pending' }],
    step1: { what: 'Searched the web:', code: short.toLowerCase(), meta: '8 of 46 results · 1.4 s', untrusted: 'web', request: `tool   web.search\nquery  ${short.toLowerCase()}\nmerge  4 engines, ranked`, output: '1  docs.example.org      the reference page\n2  forum.example.net     a long thread, mixed quality\n3  blog.example.com      a recent summary\n(5 more kept, 38 dropped as duplicates or thin)', fullLines: 46 },
    agentLabel: 'Reading the second page',
    answerPlain: 'Here is the short answer. Two sources agree; one is out of date.', answerHtml: '<p><strong>Here is the short answer.</strong> Two sources agree; one is out of date.</p><ol><li>The reference page is the one to trust: it was updated this month.</li><li>The forum thread adds one useful warning, quoted in the details.</li><li>The blog summary repeats the reference page and adds nothing.</li></ol>',
  }
}

const RISKY_LEVELS: Record<PermissionLevel, (risk?: 'medium' | 'high') => boolean> = {
  cautious: () => true,
  balanced: (r) => Boolean(r),
  smart: (r) => r === 'high',
  autonomous: () => false,
}

export async function sendMessage(sessionId: string, text: string) {
  const run = (runs.get(sessionId) ?? 0) + 1
  runs.set(sessionId, run)
  const alive = () => runs.get(sessionId) === run
  const sc = scenarioFor(text)
  const first = get().sessions.find((s) => s.id === sessionId)!
  patchSession(sessionId, (s) => ({
    ...s,
    title: s.messages.length ? s.title : sc.title.charAt(0).toUpperCase() + sc.title.slice(1),
    tags: s.messages.length ? s.tags : sc.tags,
    state: 'running', updated: 'now', when: 'Today',
    messages: [...s.messages, { id: id('m'), from: 'user', text }, { id: id('m'), from: 'assistant', blocks: [], streaming: true }],
  }))
  // 1. the model wakes up only now
  const ok = await loadModel(first.modelId)
  if (!alive()) return
  if (!ok) {
    pushBlock(sessionId, { kind: 'signal', level: 'error', text: 'The model is not on this computer. Download it in Models, or pick another model below.' })
    patchLast(sessionId, (m) => ({ ...m, streaming: false }))
    patchSession(sessionId, (s) => ({ ...s, state: 'failed' }))
    return
  }
  clearTimeout(idleTimers.get(first.modelId))
  patchModel(first.modelId, { state: 'generating' })
  const session = () => get().sessions.find((s) => s.id === sessionId)!
  if (session().thinking !== 'off') {
    await sleep(900)
    if (!alive()) return
    pushBlock(sessionId, { kind: 'thinking', seconds: session().thinking === 'high' ? 9 : session().thinking === 'medium' ? 5 : 2, text: sc.thinking })
  }
  await sleep(400)
  if (!alive()) return
  pushBlock(sessionId, { kind: 'tasks', tasks: sc.tasks })
  setTask(sessionId, 't1', { state: 'running' })
  // 2. first step
  const s1 = id('st')
  pushBlock(sessionId, { kind: 'step', step: { id: s1, state: 'running', what: sc.step1.what, code: sc.step1.code, meta: 'running', request: sc.step1.request, untrusted: sc.step1.untrusted } })
  await sleep(1500)
  if (!alive()) return
  setStep(sessionId, s1, { state: 'done', meta: sc.step1.meta, output: sc.step1.output, fullLines: sc.step1.fullLines })
  setTask(sessionId, 't1', { state: 'done' })
  patchSession(sessionId, (s) => ({ ...s, taint: Array.from(new Set([...s.taint, ...sc.taint])), contextUsed: s.contextUsed + 1840 }))
  setTask(sessionId, 't2', { state: 'running' })
  // 3. ask first, if this permission level asks here
  const level = session().permission
  const asks = RISKY_LEVELS[level](sc.risky?.risk)
  if (asks) {
    const reqId = id('p')
    const req = sc.risky ?? { risk: 'low' as const, riskLabel: 'Low risk · reads only', title: 'Read two web pages', what: 'web.fetch  docs.example.org\nweb.fetch  forum.example.net', why: 'Cautious asks before every action, including ones that only read.', raw: 'tool      web.fetch\nasked by  main session' }
    pushBlock(sessionId, { kind: 'permission', request: { id: reqId, ...req, state: 'waiting' } })
    setTask(sessionId, 't2', { state: 'needs-input' })
    patchSession(sessionId, (s) => ({ ...s, state: 'needs-input' }))
    patchModel(first.modelId, { state: 'loaded' }) // nothing to generate while it waits for you
    const choice = await new Promise<'allowed' | 'allowed-session' | 'denied'>((resolve) => waiters.set(reqId, resolve))
    if (!alive()) return
    if (choice === 'denied') {
      setTask(sessionId, 't2', { state: 'skipped' })
      setTask(sessionId, 't3', { state: 'skipped' })
      pushBlock(sessionId, { kind: 'text', plain: 'Stopped. Nothing was changed.', html: '<p><strong>Stopped.</strong> Nothing was changed. Tell me what to do instead.</p>' })
      patchLast(sessionId, (m) => ({ ...m, streaming: false }))
      patchSession(sessionId, (s) => ({ ...s, state: 'skipped' }))
      armIdle(first.modelId)
      return
    }
    patchSession(sessionId, (s) => ({ ...s, state: 'running' }))
    patchModel(first.modelId, { state: 'generating' })
    setTask(sessionId, 't2', { state: 'running' })
  } else if (sc.risky && level !== 'cautious') {
    pushBlock(sessionId, { kind: 'signal', level: 'note', text: level === 'autonomous' ? `Autonomous: ran "${sc.risky.what.split('\n')[0]}" without asking. It is in the activity log.` : `Smart: judged "${sc.risky.title.toLowerCase()}" safe enough here and did not ask. It is in the activity log.` })
  }
  await sleep(1300)
  if (!alive()) return
  setTask(sessionId, 't2', { state: 'done' })
  // 4. a mechanical sub-task goes to an agent, which borrows the loaded model
  const agentId = id('agent')
  setTask(sessionId, 't3', { state: 'delegated', meta: 'agent 1' })
  set({ agents: [...get().agents, { id: agentId, sessionId, label: sc.agentLabel, state: 'running', memoryGb: 0.7, borrows: first.modelId }] })
  patchModel(first.modelId, { state: 'lent', lentTo: 'agent 1' })
  await sleep(2600)
  if (!alive()) return
  set({ agents: get().agents.filter((a) => a.id !== agentId) })
  patchModel(first.modelId, { state: 'generating', lentTo: undefined })
  setTask(sessionId, 't3', { state: 'done', meta: 'agent 1 · 2.6 s' })
  // 5. the answer streams
  pushBlock(sessionId, { kind: 'text', plain: sc.answerPlain, html: sc.answerHtml, shown: 0 })
  const words = sc.answerPlain.split(' ').length
  for (let i = 1; i <= words; i++) {
    await sleep(55)
    if (!alive()) return
    mapBlocks(sessionId, (b) => (b.kind === 'text' && b.shown !== undefined ? { ...b, shown: i } : b))
  }
  mapBlocks(sessionId, (b) => (b.kind === 'text' ? { ...b, shown: undefined } : b))
  pushBlock(sessionId, { kind: 'signal', level: 'note', text: 'This is the preview. The steps and the answer are scripted; no model ran and nothing was changed.' })
  patchLast(sessionId, (m) => ({ ...m, streaming: false }))
  patchSession(sessionId, (s) => ({ ...s, state: 'done', contextUsed: s.contextUsed + 620 }))
  patchModel(first.modelId, { state: 'loaded' })
  armIdle(first.modelId)
}

// ───────────────────────── tools ─────────────────────────
function patchTool(toolId: string, patch: Partial<Tool>) {
  set({ tools: get().tools.map((t) => (t.id === toolId ? { ...t, ...patch } : t)) })
}
export const setToolEnabled = (toolId: string, enabled: boolean) => patchTool(toolId, { enabled })
export const saveGuidance = (toolId: string, guidance: string) => patchTool(toolId, { guidance, guidanceEdited: true })
export async function auditTool(toolId: string) {
  const t = get().tools.find((x) => x.id === toolId)
  if (!t) return
  patchTool(toolId, { audit: 'running' })
  await loadModel('qwen3-4b')
  await sleep(2600)
  const bad = t.kind === 'mcp'
  patchTool(toolId, {
    audit: bad ? 'failed' : 'passed',
    auditNote: bad ? 'Its one function description is 31 words long and tells the model to "always call this first". Publisher is unknown. Not installed as a Tool.' : 'Reads what it says it reads. Network use matches the manifest. No instructions hidden in descriptions.',
  })
}
export async function installTool(toolId: string) {
  if (setting('tools.auditBeforeInstall', true)) await auditTool(toolId)
  if (get().tools.find((x) => x.id === toolId)?.audit === 'failed') return
  patchTool(toolId, { installed: true, enabled: true })
  toast('Tool installed and switched on.', undefined, 'task-done')
}
export async function updateTool(toolId: string) {
  const t = get().tools.find((x) => x.id === toolId)
  if (!t?.update) return
  if (setting('tools.auditBeforeInstall', true)) await auditTool(toolId)
  patchTool(toolId, { version: t.update, update: undefined })
  toast(`${t.name} updated to ${t.update}.`, undefined, 'task-done')
}
export async function convertTool(toolId: string) {
  const t = get().tools.find((x) => x.id === toolId)
  if (!t) return
  patchTool(toolId, { audit: 'running' })
  await sleep(2200)
  patchTool(toolId, {
    kind: 'external', audit: 'none', indexTokens: 12, enabled: true,
    name: t.kind === 'skill' ? 'PDF' : 'Weather',
    summary: t.kind === 'skill' ? 'Read, split and merge PDF files. Converted from a classic skill.' : 'Forecasts by place. Converted from an MCP server: pinned, indexed, started only when used.',
    guidance: t.kind === 'skill' ? 'Text from PDF → pdf.read.\nMany files → pdf.merge.\nScanned pages → say so; no text inside.' : 'Place unclear → ask.\nForecast → weather.forecast. Days max 7.',
  })
  toast(`Converted. Its index entry went from ${t.rawTokens?.toLocaleString() ?? t.indexTokens} tokens to 12.`, undefined, 'task-done')
}

// ───────────────────────── schedules ─────────────────────────
function patchSchedule(sid: string, patch: Partial<Schedule>) {
  set({ schedules: get().schedules.map((s) => (s.id === sid ? { ...s, ...patch } : s)) })
}
export const setScheduleEnabled = (sid: string, enabled: boolean) => patchSchedule(sid, { enabled })
export async function runSchedule(sid: string) {
  const s = get().schedules.find((x) => x.id === sid)
  if (!s || s.state === 'running') return
  patchSchedule(sid, { state: 'running' })
  await loadModel(s.modelId)
  patchModel(s.modelId, { state: 'generating' })
  await sleep(3200)
  patchModel(s.modelId, { state: 'loaded' })
  armIdle(s.modelId)
  patchSchedule(sid, { state: 'idle', runs: [{ at: 'Just now', result: s.kind === 'monitor' ? 'no change' : 'ok', note: 'Run by you · scripted in the preview · 3.2 s' }, ...s.runs] })
  toast(`${s.title}: finished. The model unloads again when idle.`, undefined, 'task-done')
}
export function addSchedule(title: string, when: string, kind: Schedule['kind'], prompt: string) {
  const s: Schedule = { id: id('sch'), title, kind, when, next: 'Tomorrow, 07:00', modelId: setting('models.default', 'qwen3-4b'), enabled: true, state: 'idle', prompt, runs: [] }
  set({ schedules: [s, ...get().schedules] })
  return s.id
}

// ───────────────────────── artifacts, characters ─────────────────────────
export const showArtifact = (artifactId: string | null, version?: number) => {
  const a = get().artifacts.find((x) => x.id === artifactId)
  set({ openArtifact: a ? { id: a.id, version: version ?? a.versions.length } : null })
}
export function saveCharacter(c: Character) {
  const exists = get().characters.some((x) => x.id === c.id)
  set({ characters: exists ? get().characters.map((x) => (x.id === c.id ? c : x)) : [...get().characters, c] })
}
export type { Artifact, Provider }
