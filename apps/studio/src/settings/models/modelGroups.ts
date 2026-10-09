import { matchesQuery } from '@demido/ui';

import type { ModelEntry } from '@/lib/types';

/** One section of the installed list: the models on this computer, or one cloud provider's. */
export interface ModelGroupData {
  id: string;
  title: string;
  /** Label of the group's search button and field. */
  searchLabel: string;
  /** The cloud provider whose models these are; null for the local group. */
  providerId: string | null;
  models: ModelEntry[];
}

export const LOCAL_GROUP = 'local';

/**
 * Local models first (even when there are none), then one group per cloud provider in the order
 * they first appear. Providers are told apart by id, so two with the same name stay separate.
 */
export function groupModels(models: ModelEntry[]): ModelGroupData[] {
  const local: ModelGroupData = {
    id: LOCAL_GROUP,
    title: 'On this computer',
    searchLabel: 'Search on this computer',
    providerId: null,
    models: [],
  };
  const cloud = new Map<string, ModelGroupData>();
  for (const m of models) {
    if (m.source === 'local') {
      local.models.push(m);
      continue;
    }
    const id = `provider:${m.providerId ?? ''}`;
    const title = m.providerName ?? 'Cloud';
    const group = cloud.get(id) ?? {
      id,
      title,
      searchLabel: `Search ${title}`,
      providerId: m.providerId,
      models: [],
    };
    group.models.push(m);
    cloud.set(id, group);
  }
  for (const group of cloud.values()) {
    group.models.sort((a, b) => Number(b.enabled) - Number(a.enabled) || a.name.localeCompare(b.name));
  }
  return [local, ...cloud.values()];
}

/** The text a search looks through for one model. */
function haystack(m: ModelEntry): string {
  return [m.name, m.defaultName, m.providerName, m.parameters, m.quant, m.architecture, m.repo]
    .filter(Boolean)
    .join(' ');
}

/** The models matching every query; blank queries match everything. */
export function filterModels(models: ModelEntry[], ...queries: string[]): ModelEntry[] {
  const active = queries.filter((q) => q.trim() !== '');
  if (active.length === 0) return models;
  return models.filter((m) => {
    const text = haystack(m);
    return active.every((q) => matchesQuery(text, q));
  });
}
