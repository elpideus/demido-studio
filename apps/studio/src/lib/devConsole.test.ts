// What the terminal gets of a console message in development.

import { describe, expect, it } from 'vitest';

import { formatConsole } from '@/lib/devConsole';

describe('console messages', () => {
  it("writes React's key into its warning", () => {
    expect(formatConsole(['Encountered two children with the same key, `%s`.', 'local:a.gguf'])).toEqual([
      'Encountered two children with the same key, `local:a.gguf`.',
    ]);
  });

  it('keeps the values no placeholder stands for', () => {
    const err = new Error('x');
    expect(formatConsole(['%s failed:', 'load', err])).toEqual(['load failed:', err]);
  });

  it('writes numbers and objects, drops styles and unescapes %%', () => {
    expect(formatConsole(['%c%d%% of %o', 'color: red', 50, { a: 1 }])).toEqual(['50% of {"a":1}']);
  });

  it('leaves a placeholder without a value, and a message without placeholders', () => {
    expect(formatConsole(['`%s`'])).toEqual(['`%s`']);
    const err = new Error('x');
    expect(formatConsole([err, 'more'])).toEqual([err, 'more']);
    expect(formatConsole(['plain', 1])).toEqual(['plain', 1]);
  });
});
