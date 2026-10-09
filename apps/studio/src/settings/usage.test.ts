import { describe, expect, it } from 'vitest';

import type { Allowance } from '@/lib/types';
import { amount, leftInShort, resets, share, tightest, tone } from './usage';

const free = (remaining: number, limit = 50, resetsAt: number | null = null): Allowance => ({
  kind: 'freeRequests',
  remaining,
  limit,
  resetsAt,
});

const credit = (remaining: number, limit = 10): Allowance => ({ kind: 'keyCredit', remaining, limit, resetsAt: null });

describe('allowances', () => {
  it('count requests and dollars', () => {
    expect(amount(free(38), 38)).toBe('38');
    expect(amount(credit(7.25), 7.25)).toBe('$7.25');
    expect(leftInShort(free(38))).toBe('38 free requests left');
    expect(leftInShort(free(1))).toBe('1 free request left');
    expect(leftInShort(credit(7.25))).toBe('$7.25 credit left');
  });

  it('turn warning, then danger, as they run out', () => {
    expect(tone(free(38))).toBe('accent');
    expect(tone(free(12))).toBe('warning');
    expect(tone(free(5))).toBe('danger');
    expect(tone(free(0))).toBe('danger');
  });

  it('never show more than full or less than empty', () => {
    expect(share(free(60))).toBe(1);
    expect(share(credit(-0.5))).toBe(0);
    expect(share(free(0, 0))).toBe(0);
  });

  it('say the time of a reset within a day, and the day of a later one', () => {
    const now = new Date(2026, 9, 9, 15, 0).getTime();
    expect(resets(free(1, 50, new Date(2026, 9, 10, 2, 0).getTime()), now)).toMatch(/^resets at /);
    expect(resets(free(1, 50, new Date(2026, 9, 12, 2, 0).getTime()), now)).toMatch(/^resets (?!at )/);
    expect(resets(free(1), now)).toBeNull();
  });

  it('show the one closest to running out', () => {
    expect(tightest([free(40), credit(1)])).toEqual(credit(1));
    expect(tightest([free(2), credit(9)])).toEqual(free(2));
    expect(tightest([])).toBeNull();
  });
});
