// Which models a router (OpenRouter's Free Models Router, say) picked to write a turn: each step
// of the turn is a request of its own, and the router may pick another model for each.

import type { Message, ModelEntry } from '@/lib/types';

export interface RoutedPick {
  /** The model id the provider reported, e.g. `qwen/qwen3-coder:free`. */
  model: string;
  /** Its name in the provider's list of models, when it is there. */
  name: string | null;
  /** Who ran it, e.g. `Chutes`. */
  provider: string | null;
  /** Steps of the turn it wrote. */
  steps: number;
}

/** The models a router picked for the steps of a turn, first used first; none when the turn's
 * model is not a router. */
export function routedPicks(messages: Message[], models: ModelEntry[]): RoutedPick[] {
  const picks: RoutedPick[] = [];
  for (const m of messages) {
    const s = m.stats;
    if (m.role !== 'assistant' || !s?.routed || !s.providerModel) continue;
    const provider = s.provider ?? null;
    const same = picks.find((p) => p.model === s.providerModel && p.provider === provider);
    if (same) {
      same.steps += 1;
    } else {
      const name = listedName(s.providerModel, m.modelId, models);
      picks.push({ model: s.providerModel, name, provider, steps: 1 });
    }
  }
  return picks;
}

/** The name `model` has in the router's provider's list. Cloud model ids are
 * `<kind>:<provider>:<model id>`; the id a router reports may lack the `:free` of the listed one. */
function listedName(model: string, routerId: string | null, models: ModelEntry[]): string | null {
  const parts = routerId?.split(':') ?? [];
  if (parts.length < 3) return null;
  const prefix = `${parts[0]}:${parts[1]}:`;
  const bare = (id: string) => id.replace(/:free$/, '');
  const listed =
    models.find((e) => e.id === prefix + model) ??
    models.find((e) => e.id.startsWith(prefix) && bare(e.id.slice(prefix.length)) === bare(model));
  return listed?.name ?? null;
}
