import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { CAVEMAN_LEVELS, cavemanMeta, cavemanButtonLabel } from './caveman'

/**
 * The levels exist twice: as prompts in `caveman.rs` and as UI rows here. A level in one list and
 * not the other is silently broken — the backend would inject nothing for a level the dropdown
 * happily offers, or a prompt would be unreachable. Parse the Rust and compare.
 */
function rustLevels(): string[] {
  const src = readFileSync(join(__dirname, '../../src-tauri/src/caveman.rs'), 'utf-8')
  const block = src.match(/pub const LEVELS: &\[&str\] = &\[([\s\S]*?)\];/)
  if (!block) throw new Error('LEVELS not found in caveman.rs — did the const get renamed?')
  return [...block[1].matchAll(/"([^"]+)"/g)].map(m => m[1])
}

describe('caveman levels', () => {
  it('match the backend list exactly, in order', () => {
    expect(CAVEMAN_LEVELS.map(l => l.value)).toEqual(rustLevels())
  })

  it('has a prompt arm in caveman.rs for every level except off', () => {
    const src = readFileSync(join(__dirname, '../../src-tauri/src/caveman.rs'), 'utf-8')
    for (const level of CAVEMAN_LEVELS.filter(l => l.value !== 'off')) {
      expect(src).toContain(`"${level.value}" =>`)
    }
  })

  it('falls back to off for an unknown level rather than throwing', () => {
    expect(cavemanMeta('nonsense' as never).value).toBe('off')
  })

  it('labels the button by feature name when off and qualifies wenyan levels', () => {
    expect(cavemanButtonLabel('off')).toBe('Caveman')
    expect(cavemanButtonLabel('ultra')).toBe('Ultra Caveman')
    expect(cavemanButtonLabel('wenyan-full')).toBe('文言 Full Caveman')
  })
})
