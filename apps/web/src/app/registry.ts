/* The feature registry. A feature is a folder in src/features with an index.tsx that exports `feature`.
   Adding a folder adds the feature to the navigation, the routes and the settings; removing the folder removes
   it everywhere. Nothing else lists features by hand. */
import type { ComponentType } from 'react'
import type { LucideIcon } from 'lucide-react'
import type { SettingsSection } from '@/features/settings/schema'

export interface Feature {
  id: string
  title: string
  icon: LucideIcon
  /** Where it sits in the navigation rail. */
  group: 'work' | 'library' | 'connect' | 'system'
  order: number
  /** Route base, for example "models" for #/models. */
  path: string
  Page: ComponentType
  /** Settings this feature contributes. They appear in Settings automatically. */
  settings?: SettingsSection[]
  /** Where "open this feature" should land, if not the bare path. */
  home?: string
  /** Routes that take the whole window, without the navigation rail. For example ["voice/talk"]. */
  immersive?: string[]
  /** Something small it always shows at the foot of the navigation. The version label, for example. */
  rail?: ComponentType
  /** Its places in the navigation, when it wants more than one, or none ([]). Replaces the single entry made from title, icon and path. */
  nav?: { title: string; icon: LucideIcon; path: string }[]
}

export const GROUPS = ['work', 'library', 'connect', 'system'] as const

const modules = import.meta.glob<{ feature: Feature }>('../features/*/index.tsx', { eager: true })

let cache: Feature[] | null = null
/** All features, in navigation order. Read lazily, so features may import the registry themselves. */
export function allFeatures(): Feature[] {
  cache ??= Object.values(modules)
    .map((m) => m.feature)
    .filter(Boolean)
    .sort((a, b) => GROUPS.indexOf(a.group) - GROUPS.indexOf(b.group) || a.order - b.order)
  return cache
}
