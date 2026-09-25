import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';

import { parseUser, searchFilter } from '../src/tradingview.ts';
import { RpcError } from '../src/protocol.ts';

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
