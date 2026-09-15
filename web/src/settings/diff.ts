/**
 * A line diff, for the one question the prompt editor asks with one.
 *
 * An edit records the base hash it was made from, so when a later build changes
 * a shipped default Demido can say so rather than leaving the improvement
 * invisible to the one person it was written for (`docs/rules/prompts.md`, "An
 * edit that predates a changed default"). Saying so is a note, which Rust
 * already writes. Showing what to do about it is this.
 *
 * **What is diffed is the edit against the wording this build ships**, and not
 * the old default against the new one. The old default is not on disk: what an
 * edit keeps is its digest, which is enough to know the wording moved and not
 * enough to reconstruct it. The comparison that can be drawn is also the one
 * worth drawing, because the gesture beside it is a reset: what the person is
 * deciding is whether to take the shipped wording instead of theirs.
 *
 * It lives in the window rather than in Rust because both texts are already
 * here, and a diff is a way of reading two strings rather than a fact about
 * them. The other diff in this application is the monitor's, and that one is
 * Rust's because the assemblies it compares are reconstructed from a log the
 * window never sees (`demido-trace`'s `replay`).
 */

/** What one line is, in the diff's own terms rather than in a diff tool's.
 *
 * Not `added` and `removed`, because neither side of this comparison is a
 * revision of the other: one is what this build ships and one is what the
 * person wrote, and the decision is which of them to keep. */
export type Change = 'unchanged' | 'shipped' | 'edited'

/** One line of the comparison. */
export type Line = { text: string; change: Change }

/**
 * `edited` against `shipped`, line by line, in reading order.
 *
 * Longest common subsequence, which is what makes an inserted paragraph read as
 * an inserted paragraph rather than as every line after it having changed. The
 * table is quadratic and the inputs are prompt paragraphs, the longest of which
 * this build ships is under a hundred lines.
 *
 * Trailing blank lines are dropped from both sides before anything is compared.
 * The register normalises line endings but not a final newline, and a diff whose
 * only row is an empty line nobody typed would report a change nobody made.
 */
export function compare(edited: string, shipped: string): Line[] {
  const mine = split(edited)
  const theirs = split(shipped)

  // The length of the longest common subsequence of mine from i and theirs
  // from j. One flat table rather than an array of arrays, filled from the end
  // so the walk below runs forwards, which is the order the lines are read in.
  const width = theirs.length + 1
  const table = new Int32Array((mine.length + 1) * width)
  const common = (i: number, j: number): number => table[i * width + j] ?? 0
  for (let i = mine.length - 1; i >= 0; i -= 1) {
    for (let j = theirs.length - 1; j >= 0; j -= 1) {
      table[i * width + j] =
        mine[i] === theirs[j]
          ? common(i + 1, j + 1) + 1
          : Math.max(common(i + 1, j), common(i, j + 1))
    }
  }

  const lines: Line[] = []
  let i = 0
  let j = 0
  while (i < mine.length && j < theirs.length) {
    if (mine[i] === theirs[j]) {
      lines.push({ text: mine[i] ?? '', change: 'unchanged' })
      i += 1
      j += 1
    } else if (common(i + 1, j) >= common(i, j + 1)) {
      // Mine first at a tie, so a line the edit no longer has is read
      // immediately before what this build ships in its place.
      lines.push({ text: mine[i] ?? '', change: 'edited' })
      i += 1
    } else {
      lines.push({ text: theirs[j] ?? '', change: 'shipped' })
      j += 1
    }
  }
  for (; i < mine.length; i += 1) lines.push({ text: mine[i] ?? '', change: 'edited' })
  for (; j < theirs.length; j += 1) lines.push({ text: theirs[j] ?? '', change: 'shipped' })

  return lines
}

/** The lines of a paragraph, without the blank tail a file ends with. */
function split(text: string): string[] {
  const lines = text.replace(/\r\n/g, '\n').replace(/\r/g, '\n').split('\n')
  while (lines.length > 0 && lines[lines.length - 1]?.trim() === '') lines.pop()
  return lines
}
