import { create } from 'zustand'
import { load as loadStore } from '@tauri-apps/plugin-store'
import { skills as skillsApi, skillMcp } from '../lib/tauri'
import { useMcpTools } from './mcpTools'

/** One declared parameter of a command, positional in schema order. */
export interface SkillCommandParam {
  name: string
  description?: string
  /** Invoking without it is an error rather than an empty substitution. */
  required?: boolean
  /** Swallows every remaining token, so a trailing free-text param can contain spaces. */
  rest?: boolean
}

export interface SkillCommandDef {
  name: string
  description: string
  file?: string
  prompt?: string
  params?: SkillCommandParam[]
}

/**
 * A `{"type": "prompt"}` entry in a skill's `tools.json` — a tool the *model* calls, as opposed to
 * a command the *user* types. Same body shape; calling it returns the expanded body as the tool
 * result. The backend builds the defs (`skills::skill_tool_defs`), so nothing here mirrors the
 * schema.
 */
export interface SkillPromptToolDef {
  type: 'prompt'
  name: string
  description: string
  file?: string
  prompt?: string
  params?: SkillCommandParam[]
}

/**
 * A `{"type": "mcp"}` entry — one MCP server the skill brings. Contributes however many tools that
 * server reports, so its own tools arrive through `mcpTools`, not from this declaration.
 */
export interface SkillMcpToolDef {
  type: 'mcp'
  name: string
  description?: string
  command: string
  args?: string[]
  env?: Record<string, string>
  /** Whether this server's tools skip the agent-mode gate. Defaults false. */
  bypassAgentMode?: boolean
}

/**
 * A `{"type": "builtin"}` entry — a backend-implemented tool this skill surfaces (`install_skill`,
 * `delete_skill`). Offered under its real name while the skill is enabled; the skill's toggle is
 * its switch.
 */
export interface SkillBuiltinToolDef {
  type: 'builtin'
  name: string
  description?: string
}

/** One entry in a skill's `tools.json`, discriminated by `type`. */
export type SkillToolDef = SkillPromptToolDef | SkillMcpToolDef | SkillBuiltinToolDef

export interface SkillEntry {
  id: string
  name: string
  description: string
  version: string
  commands: SkillCommandDef[]
  tools: SkillToolDef[]
  /** Raw `skill.json` text — the only skill content an enabled skill puts in the prompt. */
  metaJson: string
  /** Absolute paths of the skill's files, `skill.json` excluded. */
  files: string[]
  /** Absolute path of the skill folder on disk. */
  path: string
  enabled: boolean
}

/** One slash command, flattened out of its skill and ready for the input popup. */
export interface SkillCommandEntry extends SkillCommandDef {
  skillId: string
  skillName: string
  skillPath: string
  /** What the user types after `/`. Qualified as `skillId:name` when two skills collide. */
  invocation: string
}

// A backslash escapes any placeholder. Needed because a prompt may legitimately *talk about*
// $ARGUMENTS or $1 — a skill that documents commands would otherwise have its own prose
// substituted. Matches $ARGUMENTS, $1..$9, and $name for declared params.
const PLACEHOLDER_RE = /\\?\$(ARGUMENTS|[1-9]|[A-Za-z_][A-Za-z0-9_]*)/g

/**
 * Split an argument string into tokens, honouring double quotes so a single param can hold spaces:
 * `/cmd "two words" tail` -> ['two words', 'tail'].
 */
export function tokenizeArgs(args: string): string[] {
  const out: string[] = []
  const re = /"([^"]*)"|(\S+)/g
  let m: RegExpExecArray | null
  while ((m = re.exec(args))) out.push(m[1] ?? m[2])
  return out
}

/** Bind tokens to declared params positionally; a `rest` param swallows what's left. */
function bindParams(params: SkillCommandParam[], args: string): Record<string, string> {
  const tokens = tokenizeArgs(args)
  const bound: Record<string, string> = {}
  params.forEach((p, i) => {
    bound[p.name] = p.rest ? tokens.slice(i).join(' ') : (tokens[i] ?? '')
  })
  const missing = params.filter(p => p.required && !bound[p.name])
  if (missing.length) {
    throw new Error(
      `missing required argument${missing.length > 1 ? 's' : ''}: ${missing.map(p => p.name).join(', ')}`,
    )
  }
  return bound
}

/** Human-readable call shape for the popup: `/name <a> [b...]`. */
export function usageOf(cmd: { invocation?: string; name: string; params?: SkillCommandParam[] }): string {
  const parts = (cmd.params ?? []).map(p => {
    const body = p.rest ? `${p.name}...` : p.name
    return p.required ? `<${body}>` : `[${body}]`
  })
  return `/${cmd.invocation ?? cmd.name}${parts.length ? ' ' + parts.join(' ') : ''}`
}

/**
 * Substitute a command body's placeholders with the invocation's arguments.
 *
 * `$ARGUMENTS` is the whole raw argument string; `$1`..`$9` are positional tokens; `$name` resolves
 * against `params` declared in skill.json. If the body substitutes nothing, trailing args are
 * appended instead, so a command whose body never mentions arguments still receives them. A body
 * containing only escaped placeholders counts as having none. Throws when a required param is
 * absent — silently sending a half-filled prompt is worse than refusing.
 */
export function expandCommand(body: string, args: string, params: SkillCommandParam[] = []): string {
  const bound = bindParams(params, args)
  const tokens = tokenizeArgs(args)
  let substituted = false

  const out = body.replace(PLACEHOLDER_RE, (m, key: string) => {
    if (m.startsWith('\\')) return m.slice(1)
    if (key === 'ARGUMENTS') { substituted = true; return args }
    if (/^[1-9]$/.test(key)) { substituted = true; return tokens[Number(key) - 1] ?? '' }
    // An unknown $word is left alone: prompt prose is full of dollar-prefixed words that are not
    // placeholders, and only declared params may claim one.
    if (key in bound) { substituted = true; return bound[key] }
    return m
  })

  if (substituted) return out
  return args ? `${out.trimEnd()}\n\n${args}` : out
}

/**
 * Commands routinely say "read the file at X". The model has no idea where a skill lives on disk,
 * so relative paths in a command body are unresolvable without this. Absolute paths are passed
 * straight through by the agent's `resolve_path`, so naming the folder makes its files reachable.
 */
export function withSkillLocation(body: string, skillName: string, skillPath: string): string {
  if (!skillPath) return body
  return `[Skill "${skillName}" is installed at ${skillPath} — resolve any relative path in this prompt against that folder, using absolute paths when reading files.]\n\n${body}`
}

/**
 * The prompt block for the enabled skills: each skill's `skill.json` verbatim plus the absolute
 * paths of its other files.
 *
 * SKILL.md is deliberately *not* inlined. A skill body runs to thousands of tokens and was being
 * paid for on every message of every conversation, whether or not the skill applied. The metadata
 * is enough for the model to decide the skill is relevant, and the paths let it read the body with
 * `read_file` at that point — the same load-on-demand shape the agent already uses for the repo.
 */
export function skillsContext(skills: SkillEntry[]): string {
  const enabled = skills.filter(s => s.enabled && s.metaJson)
  if (!enabled.length) return ''
  const blocks = enabled.map(s => {
    const files = s.files.length
      ? s.files.map(f => `- ${f}`).join('\n')
      : '(none)'
    return `# Skill: ${s.name}\n\n${'```'}json\n${s.metaJson.trim()}\n${'```'}\n\nFiles (read with read_file when this skill applies):\n${files}`
  })
  return [
    'The following skills are installed and enabled. Only their metadata is shown. When a skill is relevant to the request, read its files at the absolute paths listed before acting on it.',
    ...blocks,
  ].join('\n\n---\n\n')
}

/** Commands from enabled skills only — matching how skill content itself is gated. */
export function commandsOf(skills: SkillEntry[]): SkillCommandEntry[] {
  const enabled = skills.filter(s => s.enabled)
  const counts = new Map<string, number>()
  for (const s of enabled) {
    for (const c of s.commands) counts.set(c.name, (counts.get(c.name) ?? 0) + 1)
  }
  return enabled.flatMap(s =>
    s.commands.map(c => ({
      ...c,
      skillId: s.id,
      skillName: s.name,
      skillPath: s.path,
      invocation: (counts.get(c.name) ?? 0) > 1 ? `${s.id}:${c.name}` : c.name,
    })),
  )
}

/** Prefix of a server id synthesised from a skill's `mcp.json` (`skills::skill_mcp_server_id`). */
export const SKILL_SERVER_PREFIX = 'skill:'

/** The skill a server belongs to, or null for one the user configured in Settings. */
export function skillIdOfServer(serverId: string): string | null {
  if (!serverId.startsWith(SKILL_SERVER_PREFIX)) return null
  // id shape is `skill:<skill id>:<server name>`; the skill id is the middle segment, and a
  // server name may itself contain ':', so split off only the first two.
  return serverId.slice(SKILL_SERVER_PREFIX.length).split(':')[0] || null
}

/**
 * Push the enabled set to the backend so it spawns or kills each skill's MCP servers, then reload
 * the MCP tool list — a skill's tools do not exist until its server is up and has answered
 * `tools/list`, so the popup would show a stale list without this.
 */
async function syncSkillMcp(skills: SkillEntry[]) {
  try {
    await skillMcp.sync(skills.filter(s => s.enabled).map(s => s.id))
    await useMcpTools.getState().load()
  } catch (e) {
    // A skill whose server fails to spawn must not take the skills list down with it.
    console.error('skill MCP sync failed', e)
  }
}

interface SkillsStore {
  skills: SkillEntry[]
  load: () => Promise<void>
  toggle: (id: string) => Promise<void>
  delete: (id: string) => Promise<void>
  enabledContext: () => string
  enabledCommands: () => SkillCommandEntry[]
  /** Ids of the enabled skills — what the backend needs to know whose tools to offer. */
  enabledIds: () => string[]
}

let _storePromise: ReturnType<typeof loadStore> | null = null
function getStore() {
  if (!_storePromise) _storePromise = loadStore('prefs.json', { defaults: {}, autoSave: true })
  return _storePromise
}

export const useSkills = create<SkillsStore>((set, get) => ({
  skills: [],

  load: async () => {
    const raw = await skillsApi.list()
    const store = await getStore()
    const saved = (await store.get<Record<string, boolean>>('skill_enabled')) ?? {}
    const skills: SkillEntry[] = raw.map(s => ({ ...s, tools: s.tools ?? [], enabled: saved[s.id] ?? true }))
    set({ skills })
    await syncSkillMcp(skills)
  },

  delete: async (id) => {
    await skillsApi.delete(id)
    const updated = get().skills.filter(s => s.id !== id)
    const saved: Record<string, boolean> = {}
    updated.forEach(s => { saved[s.id] = s.enabled })
    const store = await getStore()
    await store.set('skill_enabled', saved)
    set({ skills: updated })
    // Deleting a skill must also stop the server it brought, or the process outlives its skill.
    await syncSkillMcp(updated)
  },

  toggle: async (id) => {
    const updated = get().skills.map(s => s.id === id ? { ...s, enabled: !s.enabled } : s)
    const saved: Record<string, boolean> = {}
    updated.forEach(s => { saved[s.id] = s.enabled })
    const store = await getStore()
    await store.set('skill_enabled', saved)
    set({ skills: updated })
    await syncSkillMcp(updated)
  },

  enabledContext: () => skillsContext(get().skills),

  enabledCommands: () => commandsOf(get().skills),

  enabledIds: () => get().skills.filter(s => s.enabled).map(s => s.id),
}))
