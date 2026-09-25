// Prepares assistant Markdown for remark-math, which reads `$...$` as math. Market answers are
// full of prices, so "$4,416 to $4,270" would otherwise render as a formula. Outside code:
//   - a `$` directly before a number is a currency sign and is escaped, unless the number is
//     the start of a formula (`$2^{10}$`, `$2x + 1$`);
//   - LaTeX's own delimiters, `\(...\)` and `\[...\]`, become `$...$` and `$$...$$`.

/** A line opening a fence: quote or list markers, then three or more backticks or tildes. */
const FENCE_OPEN = /^(?:[ \t]*(?:>|(?:[-*+]|\d{1,9}[.)])(?=[ \t])))*[ \t]*(`{3,}(?=[^`]*$)|~{3,})/;
const FENCE_CLOSE = /^(?:[ \t]*>)*[ \t]*(`{3,}|~{3,})\s*$/;
/** A list item's marker and the spaces after it, which together set where its content starts. */
const LIST_ITEM = /^([ \t]*(?:[-*+]|\d{1,9}[.)]))(?:([ \t]+)|(?=\s*$))/;

/**
 * Inline code spans (closed by a backtick run as long as the one that opened them), then
 * LaTeX display and inline math.
 */
const INLINE = /(?<!`)(`+)(?!`)(?:(?!\n[ \t]*\n)[\s\S])*?(?<!`)\1(?!`)|\\\[([\s\S]+?)\\\]|\\\(([\s\S]+?)\\\)/g;

/** Characters that, right after a number, show the number is part of a formula. */
const FORMULA_NEXT = new Set(['^', '\\', '{', '}']);

/**
 * A currency code that a `$` ends, like US$, R$ or HK$: one to three capitals starting a word.
 * A capital after `_` or `^` is a subscript or superscript (`$1 - F_X$`), not a code.
 */
const CURRENCY_CODE = /(?<![\p{L}\p{N}_^])[A-Z]{1,3}$/u;

export function prepareMath(text: string): string {
  if (!text.includes('$') && !text.includes('\\(') && !text.includes('\\[')) return text;
  return splitBlocks(text)
    .map((part, i) => (i % 2 === 1 ? part : prose(part)))
    .join('');
}

/**
 * Splits text into prose and code blocks, alternating, prose first. Code blocks are fences
 * (closed or still streaming) and indented blocks; every part starts at the start of a line.
 */
function splitBlocks(text: string): string[] {
  const parts = [''];
  const add = (line: string, code: boolean) => {
    if (code !== (parts.length % 2 === 0)) parts.push('');
    parts[parts.length - 1] += line;
  };
  let fence = '';
  let indented = false;
  let afterBlank = true;
  // Where the content of each open list item starts: text indented under an item is part of
  // it, so a code block inside needs four columns more than that.
  const items: number[] = [];
  for (const line of text.split(/(?<=\n)/)) {
    if (fence) {
      const close = FENCE_CLOSE.exec(line)?.[1];
      if (close && close[0] === fence[0] && close.length >= fence.length) fence = '';
      add(line, true);
      afterBlank = false;
      continue;
    }
    if (!line.trim()) {
      add(line, indented);
      afterBlank = true;
      continue;
    }
    const indent = columns(/^[ \t]*/.exec(line)![0]);
    if (afterBlank) while ((items.at(-1) ?? 0) > indent) items.pop();
    // An indented block cannot interrupt a paragraph, so it starts only after a blank line.
    indented = (indented || afterBlank) && indent >= (items.at(-1) ?? 0) + 4;
    afterBlank = false;
    if (indented) {
      add(line, true);
      continue;
    }
    const item = LIST_ITEM.exec(line);
    if (item) {
      while ((items.at(-1) ?? 0) > indent) items.pop();
      const marker = columns(item[1]!);
      const gap = columns(item[1]! + (item[2] ?? '')) - marker;
      items.push(marker + (gap >= 1 && gap <= 4 ? gap : 1));
    }
    fence = FENCE_OPEN.exec(line)?.[1] ?? '';
    add(line, fence !== '');
  }
  return parts;
}

/** The column `text` ends at, with tab stops every four columns. */
function columns(text: string): number {
  let col = 0;
  for (const ch of text) col = ch === '\t' ? col + 4 - (col % 4) : col + 1;
  return col;
}

function prose(text: string): string {
  let out = '';
  let last = 0;
  for (const m of text.matchAll(INLINE)) {
    const [whole, , display, inline] = m;
    out += escapeCurrency(text.slice(last, m.index));
    // Converted math goes out as it is: `\(5x + 3\)` is a formula, not a price. An empty pair,
    // like the escaped checkbox `\[ \]`, is not math at all.
    if (display?.trim()) out += displayMath(text, m.index, m.index + whole.length, display);
    else if (inline?.trim()) out += `$${inline.trim()}$`;
    else out += whole;
    last = m.index + whole.length;
  }
  return out + escapeCurrency(text.slice(last));
}

/**
 * `\[...\]` on lines of its own becomes a `$$` block that keeps the line's indent and quote
 * markers, so it stays inside its list item or quote. Anywhere else (mid-sentence, in a table
 * cell) it becomes inline `$$...$$` on one line.
 */
function displayMath(text: string, start: number, end: number, body: string): string {
  // The body's own continuation lines carry the source's indent and quote markers.
  const lines = body
    .split(/\n[ \t>]*/)
    .map((line) => line.trim())
    .filter(Boolean);
  const prefix = text.slice(text.lastIndexOf('\n', start - 1) + 1, start);
  const lineEnd = text.indexOf('\n', end);
  const rest = text.slice(end, lineEnd < 0 ? text.length : lineEnd);
  if (!/^[ \t>]*$/.test(prefix) || rest.trim()) return `$$${lines.join(' ')}$$`;
  return ['$$', ...lines.map((line) => `${prefix}${line}`.trimEnd()), `${prefix}$$`].join('\n');
}

function escapeCurrency(text: string): string {
  let out = '';
  for (let i = 0; i < text.length; i++) {
    const ch = text[i];
    if (ch !== '$') {
      out += ch;
      continue;
    }
    // `$$` opens or closes display math; `\$` is already escaped.
    if (text[i + 1] === '$') {
      out += '$$';
      i++;
      continue;
    }
    if (text[i - 1] === '\\' || !/\d/.test(text[i + 1] ?? '')) {
      out += ch;
      continue;
    }
    out += opensFormula(text, i) ? '$' : '\\$';
  }
  return out;
}

/** Whether the `$` at `at`, which a digit follows, opens a formula rather than a price. */
function opensFormula(text: string, at: number): boolean {
  let end = at + 1;
  while (end < text.length && /[\d,.]/.test(text.charAt(end))) end++;
  const next = text.charAt(end);
  // `_` is a subscript only before a letter, digit or brace: in "_$4,416_" it closes emphasis.
  if (FORMULA_NEXT.has(next) || (next === '_' && /[{a-z\d]/i.test(text.charAt(end + 1)))) return true;
  // Pandoc's rule: a formula closes at the next `$` on its line, which has a non-space character
  // before it and no digit after it. After a price, the next `$` is the next price, or ends a
  // currency code: "$4,416 an ounce (in US$)". The cost is that a formula ending in a lone capital
  // after a space, `$2 + A$`, reads as a price; `$2X$` has no word start before the X and stays math.
  const close = text.indexOf('$', end);
  const lineEnd = text.indexOf('\n', end);
  if (close < 0 || (lineEnd >= 0 && close > lineEnd)) return false;
  return (
    !/[\s\\]/.test(text.charAt(close - 1)) &&
    !/[\d$]/.test(text.charAt(close + 1)) &&
    !CURRENCY_CODE.test(text.slice(at + 1, close))
  );
}
