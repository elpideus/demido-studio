import { describe, expect, it } from 'vitest';

import type { ModelEntry } from '@/lib/types';
import { filterModels, groupModels } from './modelGroups';

function model(patch: Partial<ModelEntry>): ModelEntry {
  return {
    id: 'm',
    source: 'local',
    providerId: null,
    providerName: null,
    name: 'Model',
    defaultName: 'model',
    description: null,
    avatarPath: null,
    enabled: true,
    isDefault: false,
    path: null,
    size: null,
    quant: null,
    architecture: null,
    parameters: null,
    maxContext: null,
    repo: null,
    removable: false,
    capabilities: { vision: null, audio: null, tools: null, thinking: null },
    checkingCapabilities: false,
    settings: {} as ModelEntry['settings'],
    effective: {} as ModelEntry['effective'],
    ...patch,
  };
}

const cloud = (id: string, name: string, providerId: string, providerName: string, enabled = true) =>
  model({ id, name, source: 'gemini', providerId, providerName, enabled });

describe('groupModels', () => {
  it('always starts with the local group, even when it is empty', () => {
    const groups = groupModels([cloud('a', 'Flash', 'p1', 'Gemini')]);
    expect(groups.map((g) => g.title)).toEqual(['On this computer', 'Gemini']);
    expect(groups[0]!.models).toEqual([]);
  });

  it('keeps two providers with the same name apart', () => {
    const groups = groupModels([cloud('a', 'Flash', 'p1', 'Gemini'), cloud('b', 'Pro', 'p2', 'Gemini')]);
    expect(groups.slice(1).map((g) => g.models.map((m) => m.id))).toEqual([['a'], ['b']]);
    expect(new Set(groups.map((g) => g.id)).size).toBe(3);
  });

  it('sorts cloud models active first, then by name, and leaves local order alone', () => {
    const groups = groupModels([
      model({ id: 'z', name: 'Zeta' }),
      model({ id: 'a', name: 'Alpha' }),
      cloud('c', 'Charlie', 'p1', 'Gemini'),
      cloud('b', 'Bravo', 'p1', 'Gemini', false),
      cloud('a2', 'Alpha', 'p1', 'Gemini'),
    ]);
    expect(groups[0]!.models.map((m) => m.id)).toEqual(['z', 'a']);
    expect(groups[1]!.models.map((m) => m.id)).toEqual(['a2', 'c', 'b']);
  });
});

describe('filterModels', () => {
  const qwen = model({ id: 'q', name: 'My assistant', defaultName: 'Qwen3.5 9B', quant: 'Q4_K_M', repo: 'unsloth/x' });
  const gemma = model({ id: 'g', name: 'Gemma 4', architecture: 'gemma3' });
  const flash = cloud('f', 'Gemini Flash', 'p1', 'Work account');

  it('returns the same list when every query is blank', () => {
    const list = [qwen, gemma];
    expect(filterModels(list, '', '  ')).toBe(list);
  });

  it('finds a model by its original name, quant, repo or provider', () => {
    expect(filterModels([qwen, gemma, flash], 'qwen').map((m) => m.id)).toEqual(['q']);
    expect(filterModels([qwen, gemma, flash], 'q4_k').map((m) => m.id)).toEqual(['q']);
    expect(filterModels([qwen, gemma, flash], 'unsloth').map((m) => m.id)).toEqual(['q']);
    expect(filterModels([qwen, gemma, flash], 'work').map((m) => m.id)).toEqual(['f']);
  });

  it('needs every query to match', () => {
    expect(filterModels([qwen, gemma, flash], 'gem', 'flash').map((m) => m.id)).toEqual(['f']);
    expect(filterModels([qwen, gemma, flash], 'gem', 'nothing')).toEqual([]);
  });
});
