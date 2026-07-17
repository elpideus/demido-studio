import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { DEFAULT_TOOLS } from './builtinTools'

/**
 * The Tools panel can only switch off a tool that DEFAULT_TOOLS knows about: `disabledKeys()`
 * derives `builtin:<id>` from that list alone, and the backend keeps any tool it isn't told to
 * drop. So a tool the backend offers but this registry omits is permanently on — `read_contact`
 * was, which is why a model reached for contacts on an account that was never connected.
 *
 * Rather than mirror the Rust list by hand and hope, read it.
 *
 * `skills_tool_defs` is not in this check: those tools are only offered when a skill claims them
 * with a `{"type":"builtin"}` entry, so the skill's own toggle switches them off, not this
 * registry. `skills_tool_defs_are_not_offered_unconditionally` (below) is what keeps that true.
 */
function backendToolNames(fnName: string): string[] {
  const src = readFileSync(resolve(__dirname, '../../src-tauri/src/agent/mod.rs'), 'utf8')
  const start = src.indexOf(`pub fn ${fnName}()`)
  if (start === -1) throw new Error(`${fnName} not found — did agent/mod.rs move or get renamed?`)
  // Each def opens with `name: "x".into(),`; stop at the next top-level `pub fn`.
  const next = src.indexOf('\npub fn ', start + 1)
  const body = src.slice(start, next === -1 ? undefined : next)
  return [...body.matchAll(/name:\s*"([a-z_]+)"\.into\(\)/g)].map(m => m[1])
}

describe('builtin tool registry', () => {
  it('lists every unconditionally offered tool, so each one can be switched off', () => {
    const backend = [
      ...backendToolNames('web_tool_defs'),
      ...backendToolNames('google_tool_defs'),
    ]
    expect(backend.length).toBeGreaterThan(0)
    expect([...DEFAULT_TOOLS.map(t => t.id)].sort()).toEqual([...backend].sort())
  })

  /**
   * `install_skill`/`delete_skill` follow skill-manager's toggle, which only works while nothing
   * else offers them. Re-adding the `.chain(skills_tool_defs())` that used to sit in
   * `optional_builtin_tools` would put them back in every conversation with no way to switch them
   * off — they are absent from DEFAULT_TOOLS now, so `disabledKeys` could never reach them.
   */
  it('does not offer skills_tool_defs unconditionally — a skill claims those', () => {
    const src = readFileSync(resolve(__dirname, '../../src-tauri/src/commands.rs'), 'utf8')
    const start = src.indexOf('fn optional_builtin_tools(')
    expect(start).toBeGreaterThan(-1)
    const body = src.slice(start, src.indexOf('\nfn ', start + 1))
    expect(body).not.toContain('skills_tool_defs')
  })

  /** The allowlist is the boundary: a skill may surface these and nothing else. */
  it('only lets a skill claim tools that are already offered in every mode', () => {
    const src = readFileSync(resolve(__dirname, '../../src-tauri/src/agent/mod.rs'), 'utf8')
    const start = src.indexOf('pub fn exposable_builtin_defs()')
    expect(start).toBeGreaterThan(-1)
    const body = src.slice(start, src.indexOf('\npub fn ', start + 1))
    // Widening this to builtin_tool_defs would let a model-authored skill hand itself
    // run_command in Off mode, which is exactly what agent_mode exists to prevent.
    expect(body).toContain('skills_tool_defs()')
    expect(body).not.toContain('builtin_tool_defs()')
  })
})
