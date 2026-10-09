// What the Pine Editor colors and marks: Pine's comments, strings, numbers, keywords, built-ins
// and calls, line by line, with the compiler's errors and warnings over them. Pure, so it is
// unit-tested.

import type { PineMessage } from '@/lib/types';

export type TokenClass =
  'comment' | 'annotation' | 'string' | 'number' | 'keyword' | 'type' | 'builtin' | 'func' | 'operator';

export type MarkKind = 'error' | 'warning';

/** A run of a line's text drawn alike. */
export interface Segment {
  text: string;
  cls: TokenClass | null;
  mark?: MarkKind;
}

/** A marked stretch of a line: columns from 0, `to` exclusive. */
export interface Mark {
  from: number;
  to: number;
  kind: MarkKind;
}

const KEYWORDS = new Set([
  'and',
  'or',
  'not',
  'if',
  'else',
  'for',
  'in',
  'to',
  'by',
  'while',
  'switch',
  'var',
  'varip',
  'import',
  'export',
  'method',
  'type',
  'enum',
  'continue',
  'break',
  'true',
  'false',
]);

const TYPES = new Set([
  'int',
  'float',
  'bool',
  'string',
  'color',
  'series',
  'simple',
  'const',
  'line',
  'label',
  'box',
  'table',
  'array',
  'matrix',
  'map',
  'linefill',
  'polyline',
]);

/** Namespaces and variables Pine provides. */
const BUILTINS = new Set([
  'ta',
  'math',
  'request',
  'str',
  'input',
  'strategy',
  'syminfo',
  'timeframe',
  'barstate',
  'session',
  'chart',
  'runtime',
  'log',
  'ticker',
  'display',
  'shape',
  'location',
  'size',
  'position',
  'extend',
  'xloc',
  'yloc',
  'text',
  'font',
  'format',
  'order',
  'currency',
  'scale',
  'plot',
  'hline',
  'dayofweek',
  'open',
  'high',
  'low',
  'close',
  'volume',
  'time',
  'time_close',
  'bar_index',
  'last_bar_index',
  'hl2',
  'hlc3',
  'ohlc4',
  'hlcc4',
  'na',
  'timenow',
  'year',
  'month',
  'weekofyear',
  'dayofmonth',
  'hour',
  'minute',
  'second',
]);

const IDENT_START = /[A-Za-z_]/;
const IDENT = /[A-Za-z0-9_]/;
const NUMBER = /^(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?/;
const HEX_COLOR = /^#(?:[0-9a-fA-F]{8}|[0-9a-fA-F]{6})(?![0-9a-zA-Z_])/;
const OPERATORS = new Set(['=', '+', '-', '*', '/', '%', '<', '>', '!', '?', ':']);

/** The class of every character of a line. */
export function lineClasses(line: string): Array<TokenClass | null> {
  const out: Array<TokenClass | null> = new Array(line.length).fill(null);
  const fill = (from: number, to: number, cls: TokenClass) => {
    for (let k = from; k < to; k++) out[k] = cls;
  };
  let i = 0;
  while (i < line.length) {
    const c = line[i]!;
    if (c === '/' && line[i + 1] === '/') {
      fill(i, line.length, line[i + 2] === '@' ? 'annotation' : 'comment');
      break;
    }
    if (c === '"' || c === "'") {
      let j = i + 1;
      while (j < line.length && line[j] !== c) j += line[j] === '\\' ? 2 : 1;
      const end = Math.min(line.length, j + 1);
      fill(i, end, 'string');
      i = end;
      continue;
    }
    if (c === '#') {
      const hex = HEX_COLOR.exec(line.slice(i));
      if (hex) {
        fill(i, i + hex[0].length, 'number');
        i += hex[0].length;
        continue;
      }
    }
    if (/[\d.]/.test(c) && !(i > 0 && IDENT.test(line[i - 1]!))) {
      const num = NUMBER.exec(line.slice(i));
      if (num) {
        fill(i, i + num[0].length, 'number');
        i += num[0].length;
        continue;
      }
    }
    if (IDENT_START.test(c)) {
      let j = i + 1;
      while (j < line.length && IDENT.test(line[j]!)) j++;
      const word = line.slice(i, j);
      let k = j;
      while (line[k] === ' ') k++;
      const afterDot = i > 0 && line[i - 1] === '.';
      const cls: TokenClass | null =
        line[k] === '('
          ? 'func'
          : afterDot
            ? null
            : KEYWORDS.has(word)
              ? 'keyword'
              : TYPES.has(word) && line[k] !== '.'
                ? 'type'
                : BUILTINS.has(word) || TYPES.has(word)
                  ? 'builtin'
                  : null;
      if (cls) fill(i, j, cls);
      i = j;
      continue;
    }
    if (OPERATORS.has(c)) out[i] = 'operator';
    i++;
  }
  return out;
}

/** A line's runs of alike text; a mark past its end shows on a space after it. */
export function segmentsOf(line: string, marks: readonly Mark[] = []): Segment[] {
  const reach = marks.reduce((m, x) => Math.max(m, x.to), 0);
  const text = reach > line.length ? line + ' ' : line;
  const classes = lineClasses(line);
  const markAt = (k: number): MarkKind | undefined => {
    let found: MarkKind | undefined;
    for (const m of marks) {
      if (k < m.from || k >= m.to) continue;
      if (m.kind === 'error') return 'error';
      found = 'warning';
    }
    return found;
  };
  const out: Segment[] = [];
  for (let k = 0; k < text.length; k++) {
    const cls = classes[k] ?? null;
    const mark = markAt(k);
    const last = out[out.length - 1];
    if (last && last.cls === cls && last.mark === mark) last.text += text[k];
    else out.push({ text: text[k]!, cls, ...(mark ? { mark } : {}) });
  }
  return out;
}

/**
 * The compiler's messages as marks on the lines they cover (by line index from 0). Its lines and
 * columns count from 1 and its ends are inclusive; a message with no width still marks a character.
 */
export function marksByLine(
  lines: readonly string[],
  errors: readonly PineMessage[],
  warnings: readonly PineMessage[] = [],
): Map<number, Mark[]> {
  const out = new Map<number, Mark[]>();
  const add = (index: number, mark: Mark) => {
    if (index < 0 || index >= lines.length) return;
    const list = out.get(index) ?? [];
    list.push(mark);
    out.set(index, list);
  };
  const place = (m: PineMessage, kind: MarkKind) => {
    const first = Math.max(1, m.line) - 1;
    const last = Math.max(first, (m.endLine || m.line) - 1);
    for (let l = first; l <= last && l < lines.length; l++) {
      const from = l === first ? Math.max(0, m.column - 1) : 0;
      const to = l === last ? Math.max(from + 1, m.endColumn || m.column) : Math.max(from + 1, lines[l]!.length);
      add(l, { from, to, kind });
    }
  };
  for (const w of warnings) place(w, 'warning');
  for (const e of errors) place(e, 'error');
  return out;
}

/** The offset in the source of a line and column (both from 1), within bounds. */
export function offsetAt(source: string, line: number, column: number): number {
  let offset = 0;
  for (let l = 1; l < line; l++) {
    const next = source.indexOf('\n', offset);
    if (next < 0) return source.length;
    offset = next + 1;
  }
  const end = source.indexOf('\n', offset);
  const lineEnd = end < 0 ? source.length : end;
  return Math.min(lineEnd, offset + Math.max(0, column - 1));
}

/** The line (from 1) an offset of the source is on. */
export function lineAt(source: string, offset: number): number {
  let line = 1;
  for (let k = 0; k < offset && k < source.length; k++) if (source[k] === '\n') line++;
  return line;
}
