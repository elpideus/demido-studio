import { describe, expect, it } from 'vitest';

import type { ContextUsage } from '@/lib/types';
import { contextView, estimateTokens, unreachableNote } from './contextView';

function usage(fields: Partial<ContextUsage>): ContextUsage {
  return {
    window: 32_768,
    windowEstimated: false,
    windowBy: 'model',
    modelLimit: 32_768,
    reserve: 8_192,
    used: 4_000,
    counted: false,
    threshold: 20_889,
    ceiling: 20_889,
    custom: null,
    canCompact: true,
    compactions: 0,
    parts: { system: 1_000, tools: 1_000, summary: 0, files: 0, conversation: 2_000 },
    ...fields,
  };
}

describe('the context ring', () => {
  it('fills up toward the compaction point', () => {
    const view = contextView(usage({ used: 10_000 }));
    expect(view.limit).toBe(20_889);
    expect(view.percent).toBe(47);
    expect(view.level).toBe('low');
    expect(view.headline).toBe('11K tokens until auto-compact');
  });

  it('turns amber, then red, as compaction nears', () => {
    expect(contextView(usage({ used: 16_000 })).level).toBe('near');
    expect(contextView(usage({ used: 19_500 })).level).toBe('full');
  });

  it('counts the message being written', () => {
    const view = contextView(usage({ used: 20_000 }), 900);
    expect(view.used).toBe(20_900);
    expect(view.percent).toBe(100);
    expect(view.headline).toBe('Compacts with your next message');
    expect(contextView(usage({ used: 20_900 }), 0, true).headline).toBe('Compacts before the next step');
  });

  it('shows 100% only once the point is reached', () => {
    expect(contextView(usage({ used: 20_888 })).percent).toBe(99);
    expect(contextView(usage({ used: 30_000 })).share).toBe(1);
    expect(contextView(usage({ used: 100 })).percentLabel).toBe('<1%');
    expect(
      contextView(usage({ used: 0, parts: { system: 0, tools: 0, summary: 0, files: 0, conversation: 0 } }))
        .percentLabel,
    ).toBe('0%');
  });

  it('says when there is nothing older to summarize', () => {
    const view = contextView(usage({ used: 22_000, canCompact: false }));
    expect(view.headline).toBe('Nothing older to compact yet');
    expect(view.outlook).toMatch(/oldest messages/);
  });

  it('measures against the whole room with auto-compact off', () => {
    const view = contextView(usage({ used: 12_288, threshold: null }));
    expect(view.limit).toBe(24_576);
    expect(view.percent).toBe(50);
    expect(view.headline).toBe('12K tokens until the window fills');
    expect(contextView(usage({ used: 25_000, threshold: null })).headline).toBe(
      'Window full: oldest messages left out',
    );
  });

  it('estimates tokens as the backend does', () => {
    expect(estimateTokens('')).toBe(0);
    expect(estimateTokens('abcdef')).toBe(3);
    // Three bytes a character in UTF-8.
    expect(estimateTokens('日本語')).toBe(4);
  });

  it('warns when the threshold set is beyond what the window allows', () => {
    const set = { tokens: 100_000, scope: 'model' as const };
    const memory = usage({ windowBy: 'memory', modelLimit: 262_144, custom: set });
    // Numbers as this machine writes them: 32,768 or 32.768.
    const n = (x: number) => x.toLocaleString();
    expect(contextView(memory, 0, false, 'Qwen').warning).toBe(
      `With the memory free, Qwen has a window of ${n(32_768)} tokens, so chats compact at ${n(20_889)} tokens: ` +
        `${n(100_000)} is never reached. Free up GPU memory, or lower the threshold.`,
    );
    expect(unreachableNote(usage({ custom: set }), 'Qwen')).toContain(`Qwen reads at most ${n(32_768)} tokens`);
    expect(unreachableNote(usage({ windowBy: 'setting', custom: set }), 'Qwen')).toMatch(/context length is set/);
  });

  it('does not warn about a threshold that is reached, or with auto-compact off', () => {
    expect(contextView(usage({ custom: { tokens: 12_000, scope: 'all' }, threshold: 12_000 })).warning).toBeNull();
    expect(contextView(usage({ custom: { tokens: 100_000, scope: 'all' }, threshold: null })).warning).toBeNull();
    expect(unreachableNote(usage({}), 'Qwen')).toBeNull();
    expect(unreachableNote(usage({}), 'Qwen', 50_000)).toContain(`${(50_000).toLocaleString()} is never reached`);
  });
});
