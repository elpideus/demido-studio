import assert from 'node:assert/strict';
import { test } from 'node:test';

import { parseUser } from '../src/tradingview.ts';
import { RpcError } from '../src/protocol.ts';

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
