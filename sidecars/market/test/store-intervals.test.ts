import assert from 'node:assert/strict';
import { test } from 'node:test';

import { IntervalSet, type Range } from '../src/store/intervals.ts';

const set = (...ranges: Range[]) => new IntervalSet(ranges);

test('ranges merge when they overlap or touch, and stay sorted', () => {
  assert.deepEqual(set([10, 20], [0, 5]).toJSON(), [
    [0, 5],
    [10, 20],
  ]);
  assert.deepEqual(set([0, 10], [10, 20]).toJSON(), [[0, 20]]);
  assert.deepEqual(set([0, 10], [5, 15], [30, 40], [14, 31]).toJSON(), [[0, 40]]);
  assert.deepEqual(set([0, 10], [2, 3]).toJSON(), [[0, 10]]);
  // Empty and inverted ranges add nothing.
  assert.deepEqual(set([5, 5], [7, 3]).toJSON(), []);
  const s = set([0, 5], [10, 15], [20, 25]);
  s.add(4, 21);
  assert.deepEqual(s.toJSON(), [[0, 25]]);
});

test('subtract splits and trims ranges', () => {
  const s = set([0, 100]);
  s.subtract(10, 20);
  assert.deepEqual(s.toJSON(), [
    [0, 10],
    [20, 100],
  ]);
  s.subtract(0, 10);
  assert.deepEqual(s.toJSON(), [[20, 100]]);
  s.subtract(90, 200);
  assert.deepEqual(s.toJSON(), [[20, 90]]);
  s.subtract(20, 90);
  assert.equal(s.isEmpty, true);
  const t = set([0, 10], [20, 30], [40, 50]);
  t.subtract(5, 45);
  assert.deepEqual(t.toJSON(), [
    [0, 5],
    [45, 50],
  ]);
  // Touching but not overlapping ranges are left alone.
  const u = set([0, 10], [20, 30]);
  u.subtract(10, 20);
  assert.deepEqual(u.toJSON(), [
    [0, 10],
    [20, 30],
  ]);
});

test('intersect, union, minus and clip return new sets', () => {
  const a = set([0, 10], [20, 30]);
  const b = set([5, 25]);
  assert.deepEqual(a.intersect(b).toJSON(), [
    [5, 10],
    [20, 25],
  ]);
  assert.deepEqual(a.union(b).toJSON(), [[0, 30]]);
  assert.deepEqual(a.minus(b).toJSON(), [
    [0, 5],
    [25, 30],
  ]);
  assert.deepEqual(a.clip(8, 22).toJSON(), [
    [8, 10],
    [20, 22],
  ]);
  assert.deepEqual(a.toJSON(), [
    [0, 10],
    [20, 30],
  ]);
  assert.deepEqual(
    set([0, 10])
      .intersect(set([10, 20]))
      .toJSON(),
    [],
  );
});

test('covers, contains, gaps and rangeAt treat ranges as half-open', () => {
  const s = set([0, 10], [20, 30]);
  assert.equal(s.contains(0), true);
  assert.equal(s.contains(9.5), true);
  assert.equal(s.contains(10), false);
  assert.equal(s.contains(-1), false);
  assert.equal(s.contains(30), false);
  assert.equal(s.covers(0, 10), true);
  assert.equal(s.covers(2, 8), true);
  assert.equal(s.covers(5, 25), false);
  assert.equal(s.covers(10, 20), false);
  assert.equal(s.covers(12, 12), true);
  assert.deepEqual(s.gaps(-5, 35), [
    [-5, 0],
    [10, 20],
    [30, 35],
  ]);
  assert.deepEqual(s.gaps(2, 8), []);
  assert.deepEqual(s.gaps(5, 25), [[10, 20]]);
  assert.deepEqual(s.gaps(12, 15), [[12, 15]]);
  assert.deepEqual(s.gaps(7, 7), []);
  assert.deepEqual(new IntervalSet().gaps(0, 5), [[0, 5]]);
  assert.deepEqual(s.rangeAt(25), [20, 30]);
  assert.equal(s.rangeAt(15), null);
});

test('first, last, total, count, clone and equals', () => {
  const s = set([0, 10], [20, 25]);
  assert.equal(s.first(), 0);
  assert.equal(s.last(), 25);
  assert.equal(s.total(), 15);
  assert.equal(s.count, 2);
  const copy = s.clone();
  copy.add(10, 20);
  assert.equal(s.count, 2);
  assert.equal(copy.count, 1);
  assert.equal(s.equals(set([20, 25], [0, 10])), true);
  assert.equal(s.equals(copy), false);
  assert.equal(new IntervalSet().first(), null);
  assert.equal(new IntervalSet().last(), null);
  assert.deepEqual(
    [...s],
    [
      [0, 10],
      [20, 25],
    ],
  );
});

test('JSON round-trips and malformed JSON throws', () => {
  const s = set([100, 200], [0, 50]);
  assert.deepEqual(IntervalSet.fromJSON(JSON.parse(JSON.stringify(s))).toJSON(), s.toJSON());
  // Stored sets that were never merged come back merged.
  assert.deepEqual(
    IntervalSet.fromJSON([
      [0, 10],
      [10, 20],
    ]).toJSON(),
    [[0, 20]],
  );
  for (const bad of [null, {}, [[1]], [[1, 'x']], [[0, Infinity]], [1, 2], 'x']) {
    assert.throws(() => IntervalSet.fromJSON(bad), /Invalid interval set/);
  }
});

// Every operation against a brute-force model: a bitmap of unit cells over a small domain.
test('every operation matches a bitmap model on random sets', () => {
  const N = 40;
  let seed = 12345;
  const rand = (n: number) => {
    seed = (seed * 1103515245 + 12345) % 2147483648;
    return seed % n;
  };
  const randomRanges = (): Range[] =>
    Array.from({ length: rand(5) }, () => {
      const a = rand(N + 1);
      const b = rand(N + 1);
      return [Math.min(a, b), Math.max(a, b)];
    });
  const bitmap = (ranges: Iterable<Range>) => {
    const cells = new Array<boolean>(N).fill(false);
    for (const [f, t] of ranges) for (let i = f; i < t; i += 1) cells[i] = true;
    return cells;
  };
  const toRanges = (cells: boolean[]): Range[] => {
    const out: Range[] = [];
    for (let i = 0; i < N; i += 1) {
      if (!cells[i]) continue;
      const last = out[out.length - 1];
      if (last && last[1] === i) last[1] = i + 1;
      else out.push([i, i + 1]);
    }
    return out;
  };
  for (let round = 0; round < 2000; round += 1) {
    const ra = randomRanges();
    const rb = randomRanges();
    const a = new IntervalSet(ra);
    const b = new IntervalSet(rb);
    const ma = bitmap(ra);
    const mb = bitmap(rb);
    assert.deepEqual(a.toJSON(), toRanges(ma));
    assert.deepEqual(a.union(b).toJSON(), toRanges(ma.map((x, i) => x || mb[i]!)));
    assert.deepEqual(a.intersect(b).toJSON(), toRanges(ma.map((x, i) => x && mb[i]!)));
    assert.deepEqual(a.minus(b).toJSON(), toRanges(ma.map((x, i) => x && !mb[i]!)));
    assert.equal(a.total(), ma.filter(Boolean).length);
    const f = rand(N + 1);
    const t = rand(N + 1);
    const lo = Math.min(f, t);
    const hi = Math.max(f, t);
    const window = ma.map((x, i) => i >= lo && i < hi && x);
    const holes = ma.map((x, i) => i >= lo && i < hi && !x);
    assert.equal(
      a.covers(lo, hi),
      holes.every((x) => !x),
      `covers ${lo}-${hi} of ${JSON.stringify(ra)}`,
    );
    assert.deepEqual(a.gaps(lo, hi), toRanges(holes));
    assert.deepEqual(a.clip(lo, hi).toJSON(), toRanges(window));
    assert.equal(a.contains(lo), lo < N && ma[lo]!);
    const sub = a.clone().subtract(lo, hi);
    assert.deepEqual(sub.toJSON(), toRanges(ma.map((x, i) => x && !(i >= lo && i < hi))));
  }
});
