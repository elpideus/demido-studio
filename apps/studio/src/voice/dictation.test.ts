import { describe, expect, it } from 'vitest';

import { insertWords } from './dictation';

describe('insertWords', () => {
  it('fills an empty composer', () => {
    expect(insertWords('', { start: 0, end: 0 }, ' Buy EURUSD. ')).toEqual({ text: 'Buy EURUSD.', caret: 11 });
  });

  it('goes at the caret, spaced from the words around it', () => {
    expect(insertWords('Check this', { start: 10, end: 10 }, 'on the daily chart')).toEqual({
      text: 'Check this on the daily chart',
      caret: 29,
    });
    expect(insertWords('Check please', { start: 5, end: 5 }, 'the chart')).toEqual({
      text: 'Check the chart please',
      caret: 15,
    });
    expect(insertWords('Check .', { start: 6, end: 6 }, 'it')).toEqual({ text: 'Check it.', caret: 8 });
  });

  it('replaces the selection', () => {
    expect(insertWords('Sell GBPUSD now', { start: 5, end: 11 }, 'EURJPY')).toEqual({
      text: 'Sell EURJPY now',
      caret: 11,
    });
  });

  it('leaves the text as it was when nothing was said', () => {
    expect(insertWords('Hello', { start: 5, end: 5 }, '  ')).toEqual({ text: 'Hello', caret: 5 });
    expect(insertWords('Hi', { start: 9, end: 9 }, 'there')).toEqual({ text: 'Hi there', caret: 8 });
  });
});
