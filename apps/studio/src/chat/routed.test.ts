import { describe, expect, it } from 'vitest';

import type { Message, MessageStats, ModelEntry } from '@/lib/types';
import { routedPicks } from './routed';

const ROUTER = 'openrouter:or1:openrouter/free';

function step(stats: MessageStats | null, role: Message['role'] = 'assistant'): Message {
  return { role, modelId: ROUTER, stats } as Message;
}

const models = [
  { id: 'openrouter:or1:qwen/qwen3-coder:free', name: 'Qwen: Qwen3 Coder (free)' },
  { id: 'openrouter:or1:meta-llama/llama-3.3-70b-instruct:free', name: 'Meta: Llama 3.3 70B (free)' },
  { id: 'openrouter:or2:z-ai/glm-4.5-air:free', name: 'Another key: GLM 4.5 Air' },
] as ModelEntry[];

describe('routedPicks', () => {
  it('lists the models a router picked, with their names and steps', () => {
    const picks = routedPicks(
      [
        step({ routed: true, providerModel: 'qwen/qwen3-coder:free', provider: 'Chutes' }),
        step(null, 'tool'),
        step({ routed: true, providerModel: 'meta-llama/llama-3.3-70b-instruct', provider: 'Venice' }),
        step({ routed: true, providerModel: 'qwen/qwen3-coder:free', provider: 'Chutes' }),
        step({ routed: true, providerModel: 'z-ai/glm-4.5-air:free', provider: null }),
      ],
      models,
    );
    expect(picks).toEqual([
      { model: 'qwen/qwen3-coder:free', name: 'Qwen: Qwen3 Coder (free)', provider: 'Chutes', steps: 2 },
      // The reported id lacks the listed one's `:free`.
      {
        model: 'meta-llama/llama-3.3-70b-instruct',
        name: 'Meta: Llama 3.3 70B (free)',
        provider: 'Venice',
        steps: 1,
      },
      // Listed only under another provider: the id is all there is.
      { model: 'z-ai/glm-4.5-air:free', name: null, provider: null, steps: 1 },
    ]);
  });

  it('says nothing for a model that is not a router', () => {
    expect(routedPicks([step({ providerModel: 'qwen/qwen3-coder:free' })], models)).toEqual([]);
    expect(routedPicks([step({ durationMs: 900, routed: true })], models)).toEqual([]);
  });
});
