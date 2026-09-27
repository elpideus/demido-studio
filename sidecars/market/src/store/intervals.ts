// Sets of half-open time ranges [from, to) in seconds, kept sorted and merged (touching ranges
// join, so [0, 10) + [10, 20) is one range). Coverage, empties and gaps across the market store are
// all expressed with this one type.
//
// API (for store/series/planner/jobs):
//   new IntervalSet(ranges?)          build from [from, to) pairs in any order (overlaps merge)
//   IntervalSet.fromJSON(json)        parse a stored `[[f, t], ...]`, throws on malformed input
//   set.toJSON()                      `[[f, t], ...]`, sorted and merged
//   set.add(f, t) / set.subtract(f, t)          mutate in place, return the set
//   set.addSet(o) / set.subtractSet(o)          mutate in place with another set
//   set.union(o) / set.minus(o) / set.intersect(o) / set.clip(f, t)   new sets
//   set.covers(f, t)   every instant of [f, t) is in the set (true for an empty range)
//   set.gaps(f, t)     the parts of [f, t) not in the set, as ranges
//   set.contains(t)    t is in the set
//   set.rangeAt(t)     the range containing t, or null
//   set.first() / set.last()   lowest from / highest to (null when empty)
//   set.total()        summed length;  set.count  number of ranges;  set.isEmpty
//   set.clone(), set.equals(o), iteration over the ranges

export type Range = [number, number];

export class IntervalSet implements Iterable<Range> {
  // Sorted, non-overlapping, non-touching, every range non-empty.
  #ranges: Range[] = [];

  constructor(ranges?: Iterable<readonly [number, number]>) {
    if (ranges) for (const [from, to] of ranges) this.add(from, to);
  }

  static fromJSON(json: unknown): IntervalSet {
    if (!Array.isArray(json)) throw new Error('Invalid interval set: not an array');
    const set = new IntervalSet();
    for (const item of json) {
      if (
        !Array.isArray(item) ||
        item.length !== 2 ||
        typeof item[0] !== 'number' ||
        typeof item[1] !== 'number' ||
        !Number.isFinite(item[0]) ||
        !Number.isFinite(item[1])
      ) {
        throw new Error('Invalid interval set: expected [from, to] number pairs');
      }
      set.add(item[0], item[1]);
    }
    return set;
  }

  toJSON(): Range[] {
    return this.#ranges.map(([f, t]) => [f, t]);
  }

  get count(): number {
    return this.#ranges.length;
  }

  get isEmpty(): boolean {
    return this.#ranges.length === 0;
  }

  [Symbol.iterator](): Iterator<Range> {
    return this.toJSON()[Symbol.iterator]();
  }

  clone(): IntervalSet {
    const copy = new IntervalSet();
    copy.#ranges = this.toJSON();
    return copy;
  }

  equals(other: IntervalSet): boolean {
    const a = this.#ranges;
    const b = other.#ranges;
    return a.length === b.length && a.every(([f, t], i) => f === b[i]![0] && t === b[i]![1]);
  }

  first(): number | null {
    return this.#ranges[0]?.[0] ?? null;
  }

  last(): number | null {
    return this.#ranges[this.#ranges.length - 1]?.[1] ?? null;
  }

  total(): number {
    let sum = 0;
    for (const [f, t] of this.#ranges) sum += t - f;
    return sum;
  }

  /** Index of the first range whose end is >= t (ranges ending exactly at t touch it). */
  #firstEndingAtOrAfter(t: number): number {
    let lo = 0;
    let hi = this.#ranges.length;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if (this.#ranges[mid]![1] < t) lo = mid + 1;
      else hi = mid;
    }
    return lo;
  }

  /** Index of the first range whose end is > t (the only candidate that can contain t). */
  #firstEndingAfter(t: number): number {
    let lo = 0;
    let hi = this.#ranges.length;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if (this.#ranges[mid]![1] <= t) lo = mid + 1;
      else hi = mid;
    }
    return lo;
  }

  add(from: number, to: number): this {
    if (!(to > from)) return this;
    const i = this.#firstEndingAtOrAfter(from);
    let j = i;
    let lo = from;
    let hi = to;
    // Absorb every range that overlaps or touches [from, to).
    while (j < this.#ranges.length && this.#ranges[j]![0] <= to) {
      lo = Math.min(lo, this.#ranges[j]![0]);
      hi = Math.max(hi, this.#ranges[j]![1]);
      j += 1;
    }
    this.#ranges.splice(i, j - i, [lo, hi]);
    return this;
  }

  subtract(from: number, to: number): this {
    if (!(to > from)) return this;
    const i = this.#firstEndingAfter(from);
    let j = i;
    const keep: Range[] = [];
    while (j < this.#ranges.length && this.#ranges[j]![0] < to) {
      const [f, t] = this.#ranges[j]!;
      if (f < from) keep.push([f, from]);
      if (t > to) keep.push([to, t]);
      j += 1;
    }
    this.#ranges.splice(i, j - i, ...keep);
    return this;
  }

  addSet(other: IntervalSet): this {
    for (const [f, t] of other.#ranges) this.add(f, t);
    return this;
  }

  subtractSet(other: IntervalSet): this {
    for (const [f, t] of other.#ranges) this.subtract(f, t);
    return this;
  }

  union(other: IntervalSet): IntervalSet {
    return this.clone().addSet(other);
  }

  minus(other: IntervalSet): IntervalSet {
    return this.clone().subtractSet(other);
  }

  intersect(other: IntervalSet): IntervalSet {
    const out = new IntervalSet();
    const a = this.#ranges;
    const b = other.#ranges;
    let i = 0;
    let j = 0;
    while (i < a.length && j < b.length) {
      const lo = Math.max(a[i]![0], b[j]![0]);
      const hi = Math.min(a[i]![1], b[j]![1]);
      if (hi > lo) out.#ranges.push([lo, hi]);
      if (a[i]![1] < b[j]![1]) i += 1;
      else j += 1;
    }
    return out;
  }

  clip(from: number, to: number): IntervalSet {
    return this.intersect(new IntervalSet([[from, to]]));
  }

  contains(t: number): boolean {
    const r = this.#ranges[this.#firstEndingAfter(t)];
    return r !== undefined && r[0] <= t;
  }

  rangeAt(t: number): Range | null {
    const r = this.#ranges[this.#firstEndingAfter(t)];
    return r !== undefined && r[0] <= t ? [r[0], r[1]] : null;
  }

  covers(from: number, to: number): boolean {
    if (!(to > from)) return true;
    const r = this.#ranges[this.#firstEndingAfter(from)];
    return r !== undefined && r[0] <= from && r[1] >= to;
  }

  gaps(from: number, to: number): Range[] {
    const out: Range[] = [];
    if (!(to > from)) return out;
    let cursor = from;
    for (let i = this.#firstEndingAfter(from); i < this.#ranges.length && cursor < to; i += 1) {
      const [f, t] = this.#ranges[i]!;
      if (f >= to) break;
      if (f > cursor) out.push([cursor, f]);
      cursor = Math.max(cursor, t);
    }
    if (cursor < to) out.push([cursor, to]);
    return out;
  }
}
