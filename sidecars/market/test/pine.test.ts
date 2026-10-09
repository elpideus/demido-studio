import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';

import {
  PineLibrary,
  compile,
  declaration,
  fillMessage,
  hashOf,
  messageOf,
  runnable,
  summary,
} from '../src/indicators/pine.ts';

test('compiler messages are filled in and placed', () => {
  assert.equal(
    fillMessage('Undeclared identifier "{identifier}"', { identifier: 'lenx' }),
    'Undeclared identifier "lenx"',
  );
  assert.equal(fillMessage('Keeps {unknown} as is', {}), 'Keeps {unknown} as is');
  assert.deepEqual(
    messageOf({
      code: 'CE10272',
      ctx: { identifier: 'lenx' },
      end: { column: 23, line: 3 },
      message: 'Undeclared identifier "{identifier}"',
      start: { column: 20, line: 3 },
    }),
    { line: 3, column: 20, endLine: 3, endColumn: 23, message: 'Undeclared identifier "lenx"', code: 'CE10272' },
  );
  assert.equal(messageOf({ start: { line: 1 } }), null);
  // No place: the start of the script.
  assert.deepEqual(messageOf({ message: 'Oops' }), { line: 1, column: 1, endLine: 1, endColumn: 1, message: 'Oops' });
});

test('the declaration gives the kind and the title', () => {
  assert.deepEqual(declaration('//@version=6\nindicator("RSI cross", overlay=true)\nplot(close)\n'), {
    kind: 'indicator',
    title: 'RSI cross',
  });
  assert.deepEqual(declaration("//@version=6\nstrategy(title = 'My \\'edge\\'')\n"), {
    kind: 'strategy',
    title: "My 'edge'",
  });
  assert.deepEqual(declaration('//@version=4\nstudy("Old")\n'), { kind: 'indicator', title: 'Old' });
  assert.deepEqual(declaration('//@version=6\nlibrary("Utils")\n'), { kind: 'library', title: 'Utils' });
  assert.deepEqual(declaration('plot(close)\n'), { kind: null, title: '' });
  // A call named like a declaration inside an expression is not one.
  assert.equal(declaration('x = myindicator("no")\n').kind, null);
});

test('an empty script fails to compile without asking TradingView', async () => {
  const c = await compile('   \n');
  assert.equal(c.ok, false);
  assert.match(c.errors[0]!.message, /empty/);
  await assert.rejects(runnable(''), { code: 'COMPILE_ERROR' });
});

async function tempLibrary() {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), 'demido-pine-test-'));
  const events: { event: string; params: unknown }[] = [];
  const lib = new PineLibrary(dir, { emit: (event, params) => events.push({ event, params }) });
  return { dir, lib, events };
}

const SOURCE = '//@version=6\nindicator("Close plus one", overlay=true)\nplot(close + 1)\n';

test('the library saves, renames from the title, counts revisions and deletes', async () => {
  const { dir, lib, events } = await tempLibrary();
  const s = await lib.save({ source: SOURCE });
  assert.match(s.id, /^[a-z0-9]+$/);
  assert.equal(s.name, 'Close plus one');
  assert.equal(s.revision, 1);
  assert.equal(events.at(-1)!.event, 'pine.changed');
  assert.equal((events.at(-1)!.params as { script: { name: string } }).script.name, 'Close plus one');
  // Its summary carries no source.
  assert.equal('source' in (events.at(-1)!.params as { script: object }).script, false);

  const same = await lib.save({ id: s.id, source: SOURCE });
  assert.equal(same.revision, 1);
  const edited = await lib.save({
    id: s.id,
    source: SOURCE.replace('Close plus one', 'Close plus two').replace('+ 1', '+ 2'),
  });
  assert.equal(edited.revision, 2);
  assert.equal(edited.name, 'Close plus two');
  const named = await lib.save({ id: s.id, source: edited.source, name: '  My   script ' });
  assert.equal(named.name, 'My script');
  assert.equal(named.revision, 2);

  // Kept on disk: a new library on the same folder reads it back.
  const again = new PineLibrary(dir, { emit: () => undefined });
  assert.deepEqual(
    (await again.list()).map((x) => [x.id, x.name, x.revision, x.kind]),
    [[s.id, 'My script', 2, 'indicator']],
  );
  assert.equal((await again.get(s.id)).source, edited.source);

  await lib.remove(s.id);
  assert.deepEqual(events.at(-1), { event: 'pine.changed', params: { id: s.id, script: null } });
  await assert.rejects(lib.get(s.id), { code: 'NOT_FOUND' });
  await assert.rejects(lib.save({ id: 'nothere', source: SOURCE }), { code: 'NOT_FOUND' });
  assert.deepEqual(await new PineLibrary(dir, { emit: () => undefined }).list(), []);
});

test('a link to TradingView notices local edits', async () => {
  const { lib } = await tempLibrary();
  const s = await lib.save({ source: SOURCE });
  const linked = await lib.link(s.id, 'USER;abc', '1.0');
  assert.equal(summary(linked).tradingview?.changed, false);
  assert.equal(linked.tradingview?.hash, hashOf(SOURCE));
  assert.equal((await lib.linkedTo('USER;abc'))?.id, s.id);
  const edited = await lib.save({ id: s.id, source: `${SOURCE}// more\n` });
  assert.equal(summary(edited).tradingview?.changed, true);
  assert.equal(edited.tradingview?.id, 'USER;abc');
  assert.equal(await lib.linkedTo('USER;other'), null);
});

test('chart ids of library scripts', () => {
  assert.equal(PineLibrary.idOf('DEMIDO;abc123'), 'abc123');
  assert.equal(PineLibrary.idOf('USER;abc123'), null);
  assert.equal(PineLibrary.idOf('STD;RSI'), null);
});

test('a damaged library file is kept aside and the library starts empty', async () => {
  const { dir } = await tempLibrary();
  await fs.writeFile(path.join(dir, 'library.json'), '{not json');
  const lib = new PineLibrary(dir, { emit: () => undefined });
  assert.deepEqual(await lib.list(), []);
  const files = await fs.readdir(dir);
  assert.ok(files.some((f) => f.startsWith('library.json.damaged-')));
});
