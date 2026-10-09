// The indicators a chart window keeps in its props.

import { describe, expect, it } from 'vitest';

import type { IndicatorInput } from '@/lib/types';
import {
  changedInputs,
  fromEntry,
  fromLayout,
  fromPine,
  isPlanLimit,
  pineId,
  runKey,
  savedIndicators,
} from './savedIndicators';

describe('savedIndicators', () => {
  it('keeps what is well formed, once per key', () => {
    expect(
      savedIndicators([
        { key: 'a', script: 'STD;RSI', version: '1', name: 'RSI', setup: { inputs: { in_0: 7 } }, hidden: true },
        { key: 'a', script: 'STD;SMA' },
        { key: 'b', script: 'STD;EMA', setup: [1], hidden: 'yes' },
        { key: 'c', script: '' },
        { script: 'STD;SMA' },
        'nonsense',
      ]),
    ).toEqual([
      { key: 'a', script: 'STD;RSI', version: '1', name: 'RSI', setup: { inputs: { in_0: 7 } }, hidden: true },
      { key: 'b', script: 'STD;EMA', version: null, name: 'STD;EMA', setup: {} },
    ]);
  });

  it('keeps a well formed look', () => {
    const [s] = savedIndicators([
      { key: 'a', script: 'STD;RSI', look: { plots: { plot_0: { color: '#fff' } }, legendValues: false } },
    ]);
    expect(s!.look).toEqual({ plots: { plot_0: { color: '#fff' } }, legendValues: false });
    expect(savedIndicators([{ key: 'a', script: 'STD;RSI', look: { plots: 3 } }])[0]!.look).toBeUndefined();
  });

  it('is empty for anything but a list', () => {
    expect(savedIndicators(undefined)).toEqual([]);
    expect(savedIndicators({ key: 'a' })).toEqual([]);
  });
});

describe('new indicators', () => {
  it("start from a menu entry with TradingView's defaults, under a free key", () => {
    const s = fromEntry(
      { id: 'STD;RSI', version: '1', name: 'Relative strength index', short: 'RSI', kind: 'study' },
      new Set(),
    );
    expect(s).toMatchObject({ script: 'STD;RSI', version: '1', name: 'RSI', setup: {} });
    expect(s.key).toMatch(/^[a-z0-9]+$/);
    expect(s.hidden).toBeUndefined();
  });

  it("start from a layout's study as it is set up and shown there", () => {
    const s = fromLayout(
      { id: 'PUB;x', version: '', name: 'Squeeze', hidden: true, state: { inputs: { in_0: 3 } } },
      new Set(['taken']),
    );
    expect(s).toMatchObject({
      script: 'PUB;x',
      version: null,
      name: 'Squeeze',
      setup: { inputs: { in_0: 3 } },
      hidden: true,
    });
    expect(s.key).not.toBe('taken');
  });

  it("start from a library script, with the assistant's inputs if any", () => {
    const s = fromPine({ id: 'abc123', name: 'My cross' }, new Set(), { Length: 9 });
    expect(s).toMatchObject({
      script: 'DEMIDO;abc123',
      version: null,
      name: 'My cross',
      setup: { inputs: { Length: 9 } },
    });
    expect(fromPine({ id: 'abc123', name: 'x' }, new Set(), {}).setup).toEqual({});
    expect(pineId(s.script)).toBe('abc123');
    expect(pineId('USER;abc123')).toBeNull();
  });
});

describe('runKey', () => {
  it('changes with the setup and the revision, not with the name, the look or the eye', () => {
    const s = { key: 'a', script: 'STD;RSI', version: null, name: 'RSI', setup: { inputs: { in_0: 14 } } };
    expect(runKey({ ...s, name: 'Other', hidden: true, look: { precision: 4 } })).toBe(runKey(s));
    expect(runKey({ ...s, setup: { inputs: { in_0: 7 } } })).not.toBe(runKey(s));
    expect(runKey({ ...s, revision: 2 })).not.toBe(runKey({ ...s, revision: 1 }));
  });
});

describe('changedInputs', () => {
  const inputs: IndicatorInput[] = [
    { id: 'in_0', name: 'Length', type: 'integer', value: 14, defval: 14, hidden: false, fake: false },
    { id: 'in_1', name: 'Source', type: 'source', value: 'close', defval: 'close', hidden: false, fake: false },
  ];

  it('keeps only values that differ from the defaults', () => {
    expect(changedInputs(inputs, { in_0: 14, in_1: 'hl2', other: 1 })).toEqual({ in_1: 'hl2' });
    expect(changedInputs(inputs, {})).toEqual({});
  });
});

describe('isPlanLimit', () => {
  it("recognizes the market sidecar's refusal", () => {
    expect(
      isPlanLimit(
        'Your TradingView plan has no room for another indicator on this chart. Remove one, or upgrade the plan on tradingview.com.',
      ),
    ).toBe(true);
    expect(isPlanLimit('That chart is no longer live.')).toBe(false);
    expect(isPlanLimit(undefined)).toBe(false);
  });
});
