// The MetaTrader bridge (src/metatrader/bridge.py) over its line protocol, run by this machine's
// python3 against a stub MetaTrader5 package (no terminal, no Wine). Skipped without python3.

import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { afterEach, test } from 'node:test';
import { fileURLToPath } from 'node:url';

import { Bridge } from '../src/metatrader/client.ts';
import { cleanup, tempDir } from './store-helpers.ts';

const open: Bridge[] = [];
afterEach(async () => {
  for (const b of open.splice(0)) await b.close(2000);
  cleanup();
});

const BRIDGE = fileURLToPath(new URL('../src/metatrader/bridge.py', import.meta.url));
const python = spawnSync('python3', ['--version']).status === 0 ? 'python3' : null;

const STUB = String.raw`
import json, os
from types import SimpleNamespace as NS

__version__ = "5.0.5-stub"
TIMEFRAME_M1 = 1
TIMEFRAME_H1 = 16385

def _note(*args):
    with open(os.environ["STUB_LOG"], "a") as f:
        f.write(json.dumps(args) + "\n")

_state = {"login": 111, "server": "Old-Server", "up": False, "empty_once": True}
_selected = set()

def initialize(path=None, timeout=None):
    _note("initialize", path, timeout)
    _state["up"] = True
    return True

def shutdown():
    _note("shutdown")

def last_error():
    return (-6, "Authorization failed")

def terminal_info():
    if not _state["up"]:
        return None
    return NS(connected=True, build=4755, maxbars=100000, path="C:\\MT5", data_path=os.environ.get("STUB_DATA", "C:\\data"),
              company="MetaQuotes Ltd.", name="MetaTrader 5", trade_allowed=False)

def account_info():
    return NS(login=_state["login"], server=_state["server"], name="Demo", company="MetaQuotes Ltd.",
              currency="USD", trade_mode=0)

def login(login, server=None, timeout=None, password=None):
    _note("login", login, server, password)
    if login == 999:
        return False
    _state["login"], _state["server"] = login, server
    return True

def symbols_get():
    return [
        NS(name="EURUSD", path="Forex\\EURUSD", description="Euro vs US Dollar", currency_base="EUR",
           currency_profit="USD", digits=5, visible=True),
        NS(name="GER40", path="Indices\\GER40", description="Germany 40", currency_base="EUR",
           currency_profit="EUR", digits=1, visible=False),
    ]

def symbol_info(name):
    if name not in ("EURUSD", "GER40"):
        return None
    return NS(visible=name == "EURUSD" or name in _selected, digits=5 if name == "EURUSD" else 1, time=1710300000)

def symbol_select(name, enable):
    _note("select", name, enable)
    (_selected.add if enable else _selected.discard)(name)
    return True

def copy_rates_range(name, frame, lo, hi):
    _note("rates", name, frame, int(lo.timestamp()), int(hi.timestamp()))
    if _state["empty_once"]:
        # The terminal had to download it first.
        _state["empty_once"] = False
        return []
    start = int(lo.timestamp())
    # Before the range: what a terminal answers before its history starts.
    rows = [{"time": start - 60, "open": 9.0, "high": 9.0, "low": 9.0, "close": 9.0, "tick_volume": 1}]
    for i in range(3):
        rows.append({"time": start + i * 60, "open": 18000.04, "high": 18001.06, "low": 17999.94,
                     "close": 18000.56, "tick_volume": 5 + i})
    return rows

def symbol_info_tick(name):
    return NS(time_msc=1710300000123, bid=1.1, ask=1.1001)
`;

function start(data?: string) {
  const site = tempDir('demido-bridge-');
  fs.mkdirSync(path.join(site, 'MetaTrader5'));
  fs.writeFileSync(path.join(site, 'MetaTrader5', '__init__.py'), STUB);
  const logFile = path.join(site, 'calls.jsonl');
  const bridge = new Bridge({
    program: python!,
    args: ['-I', BRIDGE, '--site', site],
    env: { STUB_LOG: logFile, PYTHONDONTWRITEBYTECODE: '1', ...(data ? { STUB_DATA: data } : {}) },
  });
  open.push(bridge);
  const calls = () =>
    fs.existsSync(logFile)
      ? fs
          .readFileSync(logFile, 'utf8')
          .trim()
          .split('\n')
          .map((l) => JSON.parse(l) as unknown[])
      : [];
  return { bridge, calls };
}

test(
  'the bridge: connect switches the account, rates are asked once more when empty, EOF switches back',
  { skip: !python },
  async () => {
    const { bridge, calls } = start();
    const hello = await bridge.call<{ bridge: number; package: string }>('hello', {}, 20_000);
    assert.equal(hello.bridge, 1);
    assert.equal(hello.package, '5.0.5-stub');

    const terminal = 'C:\\MT5\\terminal64.exe';
    const connected = await bridge.call<{
      account: { login: number; server: string };
      terminal: { maxbars: number; connected: boolean };
      switched: boolean;
    }>('connect', { path: terminal, login: 222, server: 'New-Server', allowSwitch: true, settle: 1 }, 20_000);
    assert.equal(connected.switched, true);
    assert.deepEqual([connected.account.login, connected.account.server], [222, 'New-Server']);
    assert.equal(connected.terminal.maxbars, 100000);
    // No password: the terminal's saved one.
    assert.deepEqual(
      calls().find((c) => c[0] === 'login'),
      ['login', 222, 'New-Server', null],
    );
    await assert.rejects(
      bridge.call(
        'connect',
        { path: terminal, login: 333, server: 'New-Server', allowSwitch: false, settle: 1 },
        20_000,
      ),
      (e: Error & { code?: string }) => e.code === 'OTHER_ACCOUNT',
    );
    await assert.rejects(
      bridge.call('connect', { path: terminal, login: 999, server: 'New-Server', settle: 1 }, 20_000),
      (e: Error & { code?: string }) => e.code === 'AUTH_FAILED' && /Save password/.test(e.message),
    );

    const symbols = await bridge.call<Array<{ name: string; base: string; profit: string; digits: number }>>(
      'symbols',
      {},
      20_000,
    );
    assert.deepEqual(
      symbols.map((s) => [s.name, s.base, s.profit, s.digits]),
      [
        ['EURUSD', 'EUR', 'USD', 5],
        ['GER40', 'EUR', 'EUR', 1],
      ],
    );

    const from = 1710288000;
    const rates = await bridge.call<{
      time: number[];
      close: number[];
      volume: number[];
      last: number;
      connected: boolean;
      digits: number;
    }>('rates', { symbol: 'GER40', from, to: from + 3600, pause: 0 }, 20_000);
    assert.deepEqual(rates.time, [from, from + 60, from + 120], 'the bar before the range is dropped');
    assert.deepEqual(rates.close, [18000.6, 18000.6, 18000.6], 'rounded to the symbol digits');
    assert.deepEqual(rates.volume, [5, 6, 7]);
    assert.equal(rates.last, 1710300000);
    assert.equal(rates.connected, true);
    const asked = calls().filter((c) => c[0] === 'rates');
    assert.equal(asked.length, 2, 'empty once, asked once more, never in a loop');
    assert.deepEqual(asked[0], ['rates', 'GER40', 1, from, from + 3600]);
    assert.deepEqual(
      calls().find((c) => c[0] === 'select'),
      ['select', 'GER40', true],
    );
    await bridge.call('rates', { symbol: 'EURUSD', from, to: from + 3600, timeframe: 'H1', pause: 0 }, 20_000);
    assert.equal(
      calls()
        .filter((c) => c[0] === 'rates')
        .at(-1)![2],
      16385,
    );
    await assert.rejects(
      bridge.call('rates', { symbol: 'EURUSD', from, to: from + 60, timeframe: 'M5' }, 20_000),
      /Unknown timeframe/,
    );
    await assert.rejects(
      bridge.call('rates', { symbol: 'NOPE', from, to: from + 60 }, 20_000),
      (e: Error & { code?: string }) => e.code === 'NO_SYMBOL',
    );

    assert.deepEqual(await bridge.call('tick', { symbol: 'EURUSD' }, 20_000), {
      timeMsc: 1710300000123,
      bid: 1.1,
      ask: 1.1001,
    });
    assert.deepEqual(await bridge.call('io', { pid: 1 }, 20_000), { written: null }, 'not measurable off Windows');
    await assert.rejects(
      bridge.call('trade', {}, 20_000),
      (e: Error & { code?: string }) => e.code === 'UNKNOWN_METHOD',
    );

    // Leaving: what it selected is hidden again, the terminal goes back to the account it was on.
    await bridge.close(20_000);
    assert.equal(bridge.alive, false);
    const tail = calls().slice(-3);
    assert.deepEqual(tail, [['select', 'GER40', false], ['login', 111, 'Old-Server', null], ['shutdown']]);
  },
);

type HccBar = [time: number, open: number, high: number, low: number, close: number, volume: number];

/** One of the terminal's yearly history files (layout 502): `days` in the order stored. */
function hcc(symbol: string, days: HccBar[][], version = 502): Buffer {
  const header = Buffer.alloc(228);
  header.writeUInt32LE(version, 0);
  // One entry more, empty: it ends the table.
  const table = Buffer.alloc(18 * (days.length + 1));
  const blocks: Buffer[] = [];
  let offset = header.length + table.length;
  for (const [i, bars] of days.entries()) {
    const head = Buffer.alloc(189);
    head.writeUInt16LE(0x81, 0);
    head.write(symbol, 2, 'utf16le');
    const body = Buffer.alloc(60 * bars.length);
    for (const [j, [t, o, h, l, c, v]] of bars.entries()) {
      const at = j * 60;
      body.writeBigInt64LE(BigInt(t), at);
      for (const [k, price] of [o, h, l, c].entries()) body.writeDoubleLE(price, at + 8 + k * 8);
      body.writeBigInt64LE(BigInt(v), at + 40);
      body.writeInt32LE(10, at + 48);
    }
    table.writeUInt16LE(i + 1, i * 18);
    table.writeUInt16LE(bars.length, i * 18 + 2);
    table.writeUInt32LE(offset, i * 18 + 14);
    blocks.push(head, body);
    offset += head.length + body.length;
  }
  return Buffer.concat([header, table, ...blocks]);
}

test(
  "the bridge reads the terminal's history files: the range's minutes, in order, never daily bars",
  { skip: !python },
  async () => {
    const data = tempDir('demido-mt5-data-');
    const folder = path.join(data, 'bases', 'Old-Server', 'history', 'EURUSD');
    fs.mkdirSync(folder, { recursive: true });
    const t = (iso: string) => Date.parse(iso) / 1000;
    const bar = (iso: string, c = 1.0912345): HccBar => [t(iso), c, c + 0.0001, c - 0.0001, c, 7];
    fs.writeFileSync(
      path.join(folder, '2024.hcc'),
      hcc('EURUSD', [
        // A day kept as one bar at midnight: older, daily history.
        [[t('2024-01-02T00:00:00Z'), 1.1, 1.11, 1.09, 1.105, 900]],
        // Fetched backwards: the newest day first.
        [bar('2024-03-13T00:00:00Z'), bar('2024-03-13T12:00:00Z')],
        [
          bar('2024-03-12T00:00:00Z'),
          [t('2024-03-12T00:01:00Z'), 1.09, 1.08, 1.1, 1.09, 1],
          bar('2024-03-12T00:02:00Z'),
        ],
      ]),
    );
    fs.writeFileSync(path.join(folder, '2023.hcc'), hcc('EURUSD', [[bar('2023-03-13T00:00:00Z')]], 501));
    const { bridge, calls } = start(data);
    await bridge.call(
      'connect',
      { path: 'C:\\MT5\\terminal64.exe', login: 111, server: 'Old-Server', settle: 1 },
      20_000,
    );

    type Files = { time: number[]; close: number[]; volume: number[]; last: null; connected: boolean; files: number };
    const got = await bridge.call<Files>(
      'history',
      { symbol: 'EURUSD', from: t('2024-03-12T00:00:00Z'), to: t('2024-03-13T06:00:00Z') },
      20_000,
    );
    assert.deepEqual(
      got.time,
      [t('2024-03-12T00:00:00Z'), t('2024-03-12T00:02:00Z'), t('2024-03-13T00:00:00Z')],
      'ascending, a broken bar and the bars after the range left out',
    );
    assert.deepEqual(got.close, [1.09123, 1.09123, 1.09123], 'rounded to the symbol digits');
    assert.deepEqual(got.volume, [7, 7, 7]);
    assert.deepEqual([got.last, got.connected, got.files], [null, false, 1]);
    const all = await bridge.call<Files>(
      'history',
      { symbol: 'EURUSD', from: t('2024-01-01T00:00:00Z'), to: t('2025-01-01T00:00:00Z') },
      20_000,
    );
    assert.equal(all.time.length, 4, 'not the daily bar');
    const none = await bridge.call<Files>(
      'history',
      { symbol: 'EURUSD', from: t('2022-03-01T00:00:00Z'), to: t('2022-04-01T00:00:00Z') },
      20_000,
    );
    assert.deepEqual([none.time, none.files], [[], 0], 'a year without a file');
    await assert.rejects(
      bridge.call(
        'history',
        { symbol: 'EURUSD', from: t('2023-03-01T00:00:00Z'), to: t('2023-04-01T00:00:00Z') },
        20_000,
      ),
      (e: Error & { code?: string }) => e.code === 'HISTORY_LAYOUT',
    );
    assert.equal(calls().filter((c) => c[0] === 'rates').length, 0, 'the terminal asked nothing');
  },
);

test('the bridge without the package says so, and a bridge that ended fails its calls', { skip: !python }, async () => {
  const empty = tempDir('demido-bridge-');
  const bridge = new Bridge({ program: python!, args: ['-I', BRIDGE, '--site', empty] });
  open.push(bridge);
  const hello = await bridge.call<{ package: string | null }>('hello', {}, 20_000);
  assert.equal(hello.package, null);
  await assert.rejects(bridge.call('symbols', {}, 20_000), (e: Error & { code?: string }) => e.code === 'NO_PACKAGE');
  await bridge.close(20_000);
  await assert.rejects(bridge.call('hello', {}, 1000), (e: Error & { code?: string }) => e.code === 'BRIDGE_EXITED');

  const missing = new Bridge({ program: path.join(empty, 'no-such-python'), args: [] });
  await assert.rejects(missing.call('hello', {}, 5000), (e: Error & { code?: string }) => e.code === 'BRIDGE_EXITED');
});
