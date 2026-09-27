import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';

import { type Bar, RpcError } from '../src/protocol.ts';
import { type PageSource, parseUser, searchFilter, walkBack } from '../src/tradingview.ts';

test('funds search both ETFs and mutual funds', () => {
  assert.equal(searchFilter('fund'), 'funds');
  assert.equal(searchFilter('Funds'), 'funds');
  assert.equal(searchFilter('etf'), 'etf');
  assert.equal(searchFilter('mutual_fund'), 'mutual_fund');
  assert.equal(searchFilter('commodity'), 'cfd');
});

test('an unknown or missing asset class searches every class', () => {
  assert.equal(searchFilter('mutualfund'), '');
  assert.equal(searchFilter(undefined), '');
  assert.equal(searchFilter(''), '');
});

test('every asset class the market_search tool offers has a filter', () => {
  const rust = readFileSync(new URL('../../../apps/studio/src-tauri/src/tools/market.rs', import.meta.url), 'utf8');
  const schema = /"type": \{"type": "string", "enum": \[([^\]]+)\]/.exec(rust);
  assert.ok(schema, 'market_search type enum not found in market.rs');
  const names = [...schema[1]!.matchAll(/"([^"]+)"/g)].map((m) => m[1]!);
  assert.ok(names.includes('fund'));
  for (const name of names) assert.notEqual(searchFilter(name), '', `${name} has no TradingView filter`);
});

test('the username next to the auth token wins', () => {
  const html =
    '<script>window.ideas=[{"username":"someauthor"}];' +
    'window.user = {"id":42,"username":"stefan","auth_token":"eyJhbGciOi.abc.def","is_pro":false};</script>';
  assert.deepEqual(parseUser(html), { username: 'stefan', authToken: 'eyJhbGciOi.abc.def' });
});

test('a page without a token means the session was not accepted', () => {
  assert.throws(
    () => parseUser('<html>signed out</html>'),
    (e: unknown) => e instanceof RpcError && e.code === 'SESSION_EXPIRED',
  );
});

const D = 86400;
const daily = (from: number, to: number): Bar[] => {
  const out: Bar[] = [];
  for (let t = from; t < to; t += D) out.push({ t, o: 1, h: 1, l: 1, c: 1, v: 1 });
  return out;
};

/** A chart that serves `all` newest first, `page` bars at a time. */
function pages(all: Bar[], page: number): PageSource & { asked: number } {
  let shown = Math.min(page, all.length);
  const src = {
    asked: 0,
    first: all.slice(all.length - shown),
    firstSeconds: 0,
    async more() {
      src.asked += 1;
      shown = Math.min(all.length, shown + page);
      return all.slice(all.length - shown);
    },
  };
  return src;
}

test('a history walk stops after two empty pages and reports the start', async () => {
  const t0 = Date.UTC(2020, 0, 1) / 1000;
  const src = pages(daily(t0, t0 + 30 * D), 10);
  const kept: Array<[number, number] | null> = [];
  const walked = await walkBack(src, '1d', { keep: (_bars, cover) => kept.push(cover) });
  assert.deepEqual(walked, { reachedStart: true, oldest: t0 });
  assert.equal(src.asked, 4, 'two pages with bars, then two empty ones');
  assert.equal(kept.length, 3);
});

test('a walk down a hole stops where it meets stored bars and never reports a start', async () => {
  const t0 = Date.UTC(2020, 0, 1) / 1000;
  const src = pages(daily(t0, t0 + 60 * D), 10);
  const stopAt = t0 + 35 * D;
  const walked = await walkBack(src, '1d', { stopAt, keep: () => {} });
  assert.equal(walked.reachedStart, false);
  assert.ok(walked.oldest! <= stopAt && walked.oldest! > stopAt - 10 * D, 'one page past the edge at most');
  assert.equal(src.asked, 2);
  // Running dry above the edge is not TradingView's start either.
  const short = pages(daily(t0 + 50 * D, t0 + 60 * D), 10);
  assert.equal((await walkBack(short, '1d', { stopAt, keep: () => {} })).reachedStart, false);
});
