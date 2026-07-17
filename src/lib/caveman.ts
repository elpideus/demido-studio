import type { CavemanLevel } from '../types'

/**
 * UI metadata for the caveman levels. The prompts themselves live in the backend
 * (`src-tauri/src/caveman.rs`) — only the labels are here, and the two lists must agree on the
 * level strings.
 */
export interface CavemanLevelMeta {
  value: CavemanLevel
  label: string
  icon: string
  /** One-line description shown in the dropdown row. */
  hint: string
  group: 'English' | 'Wenyan 文言'
}

export const CAVEMAN_LEVELS: CavemanLevelMeta[] = [
  { value: 'off',          label: 'Off',    icon: '○',  hint: 'Normal prose',                  group: 'English' },
  { value: 'lite',         label: 'Lite',   icon: '🪨', hint: 'Tight, but full sentences',     group: 'English' },
  { value: 'full',         label: 'Full',   icon: '🪓', hint: 'Classic caveman fragments',     group: 'English' },
  { value: 'ultra',        label: 'Ultra',  icon: '🔥', hint: 'Max compression, abbreviated',  group: 'English' },
  { value: 'wenyan-lite',  label: 'Lite',   icon: '🎋', hint: '半文言 — classical register',    group: 'Wenyan 文言' },
  { value: 'wenyan-full',  label: 'Full',   icon: '📜', hint: '文言文 — 80–90% shorter',        group: 'Wenyan 文言' },
  { value: 'wenyan-ultra', label: 'Ultra',  icon: '🐉', hint: '極簡 — classical, extreme',      group: 'Wenyan 文言' },
]

export function cavemanMeta(level: CavemanLevel): CavemanLevelMeta {
  return CAVEMAN_LEVELS.find(l => l.value === level) ?? CAVEMAN_LEVELS[0]
}

/**
 * Button text: the level, then the feature name. Off shows the bare feature name — "Off Caveman"
 * reads like a level. Wenyan levels reuse the English level names (Lite/Full/Ultra), so they
 * carry the register too or the button reads identically.
 */
export function cavemanButtonLabel(level: CavemanLevel): string {
  if (level === 'off') return 'Caveman'
  const meta = cavemanMeta(level)
  return meta.group === 'English'
    ? `${meta.label} Caveman`
    : `文言 ${meta.label} Caveman`
}

export const CAVEMAN_GROUPS: CavemanLevelMeta['group'][] = ['English', 'Wenyan 文言']
