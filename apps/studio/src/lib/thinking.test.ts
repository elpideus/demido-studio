// How hard a model thinks: what each kind of model offers, and what a choice saves.

import { describe, expect, it } from 'vitest';

import { canThink, currentThinking, thinkingChoices, thinkingInfo, withThinking } from '@/lib/thinking';
import type { EffortLevel, ModelEntry, ModelSettings, ModelSource } from '@/lib/types';

const model = (
  source: ModelSource,
  efforts: EffortLevel[],
  settings: ModelSettings = {},
  alwaysThinks = source === 'anthropic' || source === 'claudecode',
  defaultEffort: EffortLevel | null = null,
) =>
  ({
    id: 'm',
    source,
    name: 'M',
    efforts,
    defaultEffort,
    alwaysThinks,
    capabilities: { vision: null, audio: null, tools: null, thinking: true },
    settings,
    effective: { thinking: settings.thinking ?? true, effort: settings.effort ?? null },
  }) as unknown as ModelEntry;

const claude: EffortLevel[] = ['low', 'medium', 'high', 'xhigh', 'max'];

describe('thinking choices', () => {
  it('offers Claude its levels, without turning thinking off', () => {
    expect(thinkingChoices(model('claudecode', claude))).toEqual(['default', ...claude]);
    expect(thinkingChoices(model('anthropic', ['low', 'high']))).toEqual(['default', 'low', 'high']);
  });

  it('lets OpenRouter turn thinking off, unless the model always thinks', () => {
    expect(thinkingChoices(model('openrouter', ['low', 'high']))).toEqual(['default', 'low', 'high', 'off']);
    expect(thinkingChoices(model('openrouter', ['low', 'high'], {}, true))).toEqual(['default', 'low', 'high']);
  });

  it('offers Gemini 3 its levels, and 2.5 Flash off as well', () => {
    const levels: EffortLevel[] = ['minimal', 'low', 'medium', 'high'];
    expect(thinkingChoices(model('gemini', levels, {}, true))).toEqual(['default', ...levels]);
    expect(thinkingChoices(model('gemini', ['low', 'medium', 'high']))).toEqual([
      'default',
      'low',
      'medium',
      'high',
      'off',
    ]);
  });

  it('names the level the default stands for, when it is known', () => {
    const flash = model('gemini', ['low', 'medium', 'high'], {}, true, 'medium');
    expect(thinkingInfo('default', flash).description).toBe('The model’s own level: Medium.');
    expect(thinkingInfo('default', model('claudecode', claude)).description).toBe(
      'The level the provider sets for this model.',
    );
  });

  it('offers a model without levels on and off', () => {
    expect(thinkingChoices(model('local', []))).toEqual(['on', 'off']);
    expect(thinkingChoices(model('gemini', []))).toEqual(['on', 'off']);
  });

  it('is only for models that think', () => {
    const plain = { ...model('local', []), capabilities: { thinking: false } } as ModelEntry;
    expect(canThink(plain)).toBe(false);
    expect(canThink(model('local', []))).toBe(true);
  });
});

describe('current thinking', () => {
  it('reads the level, or the default', () => {
    expect(currentThinking(model('claudecode', claude, { effort: 'max' }))).toBe('max');
    expect(currentThinking(model('claudecode', claude))).toBe('default');
    // A level the model no longer takes is its default.
    expect(currentThinking(model('anthropic', ['low', 'high'], { effort: 'max' }))).toBe('default');
  });

  it('shows thinking turned off, on a model that always thinks, as the lowest level it sends', () => {
    expect(currentThinking(model('claudecode', claude, { thinking: false }))).toBe('low');
    expect(currentThinking(model('gemini', ['minimal', 'low'], { thinking: false }, true))).toBe('minimal');
    expect(currentThinking(model('openrouter', ['low'], { thinking: false }))).toBe('off');
    expect(currentThinking(model('local', [], { thinking: false }))).toBe('off');
    expect(currentThinking(model('local', []))).toBe('on');
  });

  it('reads a draft in place of what is saved', () => {
    const m = model('claudecode', claude, { effort: 'high' });
    expect(currentThinking(m, { effort: 'low' })).toBe('low');
  });
});

describe('choosing', () => {
  it('saves a level and clears answering directly', () => {
    expect(withThinking({ thinking: false, temperature: 1 }, 'xhigh')).toEqual({
      thinking: null,
      effort: 'xhigh',
      temperature: 1,
    });
  });

  it('turns thinking off, or back to the default', () => {
    expect(withThinking({ effort: 'high' }, 'off')).toEqual({ thinking: false, effort: null });
    expect(withThinking({ effort: 'high', thinking: false }, 'default')).toEqual({ thinking: null, effort: null });
    expect(withThinking({ thinking: false }, 'on')).toEqual({ thinking: null, effort: null });
  });
});
