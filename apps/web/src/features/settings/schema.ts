/* Settings are data. A section lists its settings; the Settings screen renders every one of them the same
   way (label, description, control, Details, Changed · Reset) and the search covers all of them.
   A feature adds settings by exporting sections from its index.tsx; it never edits the Settings screen. */
import type { ComponentType } from 'react'

interface Base {
  /** Storage key, for example "models.idleUnload". */
  id: string
  label: string
  /** One line: what it does. */
  desc: string
  /** The Reference text: when to change it, default, range, cost, trade-off. */
  details?: string
  /** True when the control is shown but nothing behind it works yet. */
  notBuilt?: boolean
  /** True for a row that holds a button and no value ("Start over", "This version"). It is not a setting. */
  action?: boolean
}
export type SettingDef =
  | (Base & { type: 'switch'; default: boolean })
  | (Base & { type: 'select' | 'segmented'; default: string; options: { value: string; label: string }[] })
  | (Base & { type: 'slider'; default: number; min: number; max: number; step: number; format: (v: number) => string })
  | (Base & { type: 'text'; default: string; mono?: boolean; secret?: boolean; placeholder?: string })
  | (Base & { type: 'custom'; default: unknown; Control: ComponentType })

export interface SettingsSection {
  id: string
  title: string
  /** One line under the title. */
  lead?: string
  order: number
  settings: SettingDef[]
}
