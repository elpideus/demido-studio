// "Search settings": what it finds, in what order, and where a result leads.

import { describe, expect, it } from 'vitest';

import type { ModelEntry, Skill } from '@/lib/types';
import { highlight, searchSettings, settingEntries } from './searchIndex';

function model(over: Partial<ModelEntry>): ModelEntry {
  return {
    id: 'm',
    name: 'Model',
    defaultName: 'Model',
    source: 'local',
    providerName: null,
    enabled: true,
    isDefault: false,
    parameters: null,
    quant: null,
    ...over,
  } as ModelEntry;
}

function skill(over: Partial<Skill>): Skill {
  return { id: 'sk', name: 'Skill', description: '', enabled: true, ...over } as Skill;
}

const MODELS = [
  model({ id: 'gemini', name: 'Gemini 2.5 Flash', source: 'gemini' as ModelEntry['source'], providerName: 'Google' }),
  model({ id: 'qwen', name: 'Qwen3 8B', isDefault: true }),
  model({ id: 'gemma', name: 'Gemma 3 4B' }),
];
const SKILLS = [skill({ id: 'report', name: 'Weekly report', description: 'Sums up the week in markets' })];
const entries = settingEntries(MODELS, SKILLS);
const labels = (query: string) => searchSettings(entries, query).map((e) => e.label);

describe('searchSettings', () => {
  it('finds nothing for an empty query', () => {
    expect(searchSettings(entries, '   ')).toEqual([]);
  });

  it('puts the label that is the query first, then labels that start with it', () => {
    const found = labels('microphone');
    expect(found[0]).toBe('Microphone');
    expect(labels('compact')[0]).toMatch(/^Compact/);
  });

  it('finds a setting by the words people use for it', () => {
    expect(labels('dark mode')).toEqual([]);
    expect(labels('imap')).toContain('Mail accounts');
    expect(labels('vram')).toContain('GPU layers');
    expect(labels('prompt injection')).toContain('Safety notice');
  });

  it('needs every word of the query', () => {
    expect(labels('compact model')).toContain('Compact at, for one model');
    expect(labels('compact zebra')).toEqual([]);
  });

  it('ignores case and accents', () => {
    expect(labels('MICRO')).toContain('Microphone');
    expect(labels('micróphone')).toContain('Microphone');
  });

  it('caps the results', () => {
    expect(searchSettings(entries, 'e').length).toBeLessThanOrEqual(12);
    expect(searchSettings(entries, 'e', 3)).toHaveLength(3);
  });
});

describe('settingEntries', () => {
  it('points a model setting at the model new chats use', () => {
    const temperature = searchSettings(entries, 'temperature')[0]!;
    expect(temperature.target).toEqual({ tab: 'models', model: 'qwen', focus: 'model-temperature' });
    expect(temperature.where).toBe('Qwen3 8B · Sampling');
  });

  it('points a setting only local models have at a local model', () => {
    const cloudFirst = settingEntries(
      [
        model({ id: 'cloud', name: 'Cloud', source: 'gemini' as ModelEntry['source'], isDefault: true }),
        model({ id: 'local' }),
      ],
      [],
    );
    const layers = searchSettings(cloudFirst, 'gpu layers')[0]!;
    expect(layers.target.model).toBe('local');
    const temperature = searchSettings(cloudFirst, 'temperature')[0]!;
    expect(temperature.target.model).toBe('cloud');
  });

  it('still lists the model settings when there is no model', () => {
    const none = settingEntries([], []);
    const temperature = searchSettings(none, 'temperature')[0]!;
    expect(temperature.target).toEqual({ tab: 'models' });
    expect(temperature.where).toBe('Each model');
  });

  it('finds models and skills by name', () => {
    const gemma = searchSettings(entries, 'gemma')[0]!;
    expect(gemma.kind).toBe('model');
    expect(gemma.target).toEqual({ tab: 'models', model: 'gemma' });
    expect(gemma.where).toBe('Local model');
    expect(searchSettings(entries, 'gemini flash')[0]!.where).toBe('Google');

    const report = searchSettings(entries, 'weekly')[0]!;
    expect(report.kind).toBe('skill');
    expect(report.target).toEqual({ tab: 'skills', skill: 'report' });
    expect(searchSettings(entries, 'markets')[0]!.label).toBe('Weekly report');
  });

  it('puts a setting before a model that matches as well', () => {
    const withThinker = settingEntries([model({ id: 't', name: 'Thinking' })], []);
    const found = searchSettings(withThinker, 'thinking');
    expect(found[0]!.kind).toBe('setting');
    expect(found[1]!.kind).toBe('model');
  });

  it('gives every setting a page the rail has', () => {
    const pages = new Set([
      'providers',
      'models',
      'skills',
      'chats',
      'permissions',
      'voice',
      'mail',
      'web',
      'runtime',
      'updates',
      'about',
    ]);
    for (const e of entries) expect(pages.has(e.target.tab)).toBe(true);
  });
});

describe('highlight', () => {
  it('marks where the query words are', () => {
    expect(highlight('Compact at', 'at comp')).toEqual([
      { text: 'Comp', match: true },
      { text: 'act ', match: false },
      { text: 'at', match: true },
    ]);
  });

  it('marks nothing when nothing matches', () => {
    expect(highlight('Microphone', 'zebra')).toEqual([{ text: 'Microphone', match: false }]);
  });

  it('matches across case and accents', () => {
    expect(highlight('Résumé', 'resu')).toEqual([
      { text: 'Résu', match: true },
      { text: 'mé', match: false },
    ]);
  });
});
