import { describe, expect, it } from 'vitest';

import { formatAgo, formatContext, formatLicense, formatParams } from './format';

describe('the Models window words', () => {
  it('rounds parameter counts', () => {
    expect(formatParams(8_190_735_360)).toBe('8.2B');
    expect(formatParams(30_000_000_000)).toBe('30B');
    expect(formatParams(235_000_000_000)).toBe('235B');
    expect(formatParams(596_049_920)).toBe('596M');
    expect(formatParams(null)).toBeNull();
  });

  it('gives contexts in thousands of tokens', () => {
    expect(formatContext(262_144)).toBe('256K tokens');
    expect(formatContext(40_960)).toBe('40K tokens');
  });

  it('says how long ago something was updated', () => {
    const now = Date.parse('2026-10-16T12:00:00Z');
    expect(formatAgo('2026-10-16T08:00:00Z', now)).toBe('today');
    expect(formatAgo('2026-10-11T12:00:00Z', now)).toBe('5 days ago');
    expect(formatAgo('2026-07-16T12:00:00Z', now)).toBe('3 months ago');
    expect(formatAgo('2024-10-01T12:00:00Z', now)).toBe('2 years ago');
    expect(formatAgo(null, now)).toBeNull();
  });

  it('writes licenses as their owners do', () => {
    expect(formatLicense('apache-2.0')).toBe('Apache 2.0');
    expect(formatLicense('qwen-research')).toBe('qwen-research');
  });
});
