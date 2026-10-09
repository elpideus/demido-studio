// The Pine Editor's coloring and marks.

import { describe, expect, it } from 'vitest';

import { lineAt, lineClasses, marksByLine, offsetAt, segmentsOf } from './pineSyntax';

const runs = (line: string) => segmentsOf(line).filter((s) => s.cls).map((s) => [s.text, s.cls]);

describe('lineClasses', () => {
  it('colors declarations, calls, strings and numbers', () => {
    expect(runs('indicator("RSI cross", overlay = true)')).toEqual([
      ['indicator', 'func'],
      ['"RSI cross"', 'string'],
      ['=', 'operator'],
      ['true', 'keyword'],
    ]);
    expect(runs('len = input.int(14, "Length")')).toEqual([
      ['=', 'operator'],
      ['input', 'builtin'],
      ['int', 'func'],
      ['14', 'number'],
      ['"Length"', 'string'],
    ]);
  });

  it('tells types from namespaces, and leaves member names alone', () => {
    expect(runs('var float x = color.new(#2962ff80, 50)')).toEqual([
      ['var', 'keyword'],
      ['float', 'type'],
      ['=', 'operator'],
      ['color', 'builtin'],
      ['new', 'func'],
      ['#2962ff80', 'number'],
      ['50', 'number'],
    ]);
    expect(runs('x = syminfo.ticker')).toEqual([
      ['=', 'operator'],
      ['syminfo', 'builtin'],
    ]);
  });

  it('runs comments and annotations to the end of the line', () => {
    expect(runs('//@version=6')).toEqual([['//@version=6', 'annotation']]);
    expect(runs('a = 1 // "not a string"')).toEqual([
      ['=', 'operator'],
      ['1', 'number'],
      ['// "not a string"', 'comment'],
    ]);
  });

  it('does not take digits of a name for a number, and ends strings at their quote', () => {
    expect(lineClasses('ma2')).toEqual([null, null, null]);
    expect(runs("s = 'it\\'s' + x")).toEqual([
      ['=', 'operator'],
      ["'it\\'s'", 'string'],
      ['+', 'operator'],
    ]);
  });
});

describe('marks', () => {
  const src = ['//@version=6', 'indicator("x")', 'plot(foo)'];

  it('place the compiler messages on their lines, errors over warnings', () => {
    const marks = marksByLine(
      src,
      [{ line: 3, column: 6, endLine: 3, endColumn: 8, message: "Undeclared identifier 'foo'" }],
      [{ line: 3, column: 1, endLine: 3, endColumn: 9, message: 'w' }],
    );
    expect(marks.get(2)).toEqual([
      { from: 0, to: 9, kind: 'warning' },
      { from: 5, to: 8, kind: 'error' },
    ]);
    const segs = segmentsOf(src[2]!, marks.get(2));
    expect(segs.map((s) => [s.text, s.mark ?? null])).toEqual([
      ['plot', 'warning'],
      ['(', 'warning'],
      ['foo', 'error'],
      [')', 'warning'],
    ]);
  });

  it('show past the end of a line, and across lines', () => {
    const marks = marksByLine(src, [{ line: 1, column: 13, endLine: 2, endColumn: 3, message: 'm' }]);
    expect(marks.get(0)).toEqual([{ from: 12, to: 13, kind: 'error' }]);
    expect(marks.get(1)).toEqual([{ from: 0, to: 3, kind: 'error' }]);
    expect(segmentsOf(src[0]!, marks.get(0)).at(-1)).toEqual({ text: ' ', cls: null, mark: 'error' });
  });
});

describe('positions', () => {
  const source = 'ab\ncdef\ng';

  it('go from line and column to offset and back', () => {
    expect(offsetAt(source, 1, 1)).toBe(0);
    expect(offsetAt(source, 2, 3)).toBe(5);
    expect(offsetAt(source, 2, 99)).toBe(7);
    expect(offsetAt(source, 9, 1)).toBe(source.length);
    expect(lineAt(source, 5)).toBe(2);
    expect(lineAt(source, 8)).toBe(3);
  });
});
