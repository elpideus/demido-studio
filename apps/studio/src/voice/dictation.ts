// Dictated words go into the composer's text where the caret was, as they are written down.

/** Where the caret (or the selection, which the words replace) was when recording started. */
export interface Caret {
  start: number;
  end: number;
}

/**
 * `text` with `words` in place of the selection at `caret`, spaced from what is around it.
 * Returns the new text and where the caret goes: after the words.
 */
export function insertWords(text: string, caret: Caret, words: string): { text: string; caret: number } {
  const start = Math.max(0, Math.min(caret.start, text.length));
  const end = Math.max(start, Math.min(caret.end, text.length));
  const before = text.slice(0, start);
  const after = text.slice(end);
  const said = words.trim();
  if (!said) return { text: before + after, caret: start };
  const lead = before && !/\s$/.test(before) ? ' ' : '';
  const trail = after && !/^[\s.,;:!?)\]}]/.test(after) ? ' ' : '';
  const inserted = lead + said + trail;
  return { text: before + inserted + after, caret: start + lead.length + said.length };
}
