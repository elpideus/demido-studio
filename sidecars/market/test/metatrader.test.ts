// MetaTrader as the second source, without a terminal: the server clock models, the broker symbol
// names, the day encoding, and the source against a fake bridge whose "terminal" runs an Athens clock
// and answers from a synthetic EUR/USD history.

import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { afterEach, test } from 'node:test';

import { BridgeError } from '../src/metatrader/client.ts';
import {
  chooseModel,
  fxOpenUtc,
  offsetAt,
  tickEvidence,
  toServer,
  toUtc,
  weekOf,
  weekShifts,
  weeklyOpens,
  type ZoneModel,
} from '../src/metatrader/clock.ts';
import { byDay, encodeDay } from '../src/metatrader/encode.ts';
import {
  type BridgeLike,
  type MetaTraderConfig,
  MetaTraderSource,
  type MetaTraderStatus,
  parseConfig,
} from '../src/metatrader/source.ts';
import { instrumentFor, mapSymbols } from '../src/metatrader/symbols.ts';
import { type Bar } from '../src/protocol.ts';
import { decodeBucket, DukascopyStore } from '../src/store/dukascopy-store.ts';
import { type Range } from '../src/store/intervals.ts';
import { DAY, cleanup, s, tempDir } from './store-helpers.ts';

const HOUR = 3600;
afterEach(cleanup);

// ---------------------------------------------------------------------------------------------
// Clock

test('clock models: New York + 7 and Athens agree but in the weeks the US and the EU change apart', () => {
  assert.equal(offsetAt('eet', s('2024-01-15T12:00:00Z')), 2 * HOUR);
  assert.equal(offsetAt('eet', s('2024-07-15T12:00:00Z')), 3 * HOUR);
  assert.equal(offsetAt('ny+7', s('2024-01-15T12:00:00Z')), 2 * HOUR);
  assert.equal(offsetAt('ny+7', s('2024-07-15T12:00:00Z')), 3 * HOUR);
  // The US moved on 10 March 2024, the EU on 31 March.
  assert.equal(offsetAt('ny+7', s('2024-03-20T12:00:00Z')), 3 * HOUR);
  assert.equal(offsetAt('eet', s('2024-03-20T12:00:00Z')), 2 * HOUR);
  assert.equal(offsetAt('cet', s('2024-01-15T12:00:00Z')), HOUR);
  assert.equal(offsetAt('utc+3', s('2024-01-15T12:00:00Z')), 3 * HOUR);
  for (const model of ['ny+7', 'eet', 'cet', 'gmt', 'utc+2'] as ZoneModel[]) {
    for (let t = s('2024-03-01T00:00:00Z'); t < s('2024-04-05T00:00:00Z'); t += 7 * HOUR) {
      assert.equal(toUtc(model, toServer(model, t)), t, `${model} ${t}`);
    }
  }
});

test('the FX week opens Sunday 17:00 New York', () => {
  // Winter: 22:00 UTC; a New York + 7 server stamps it Monday 00:00.
  assert.equal(fxOpenUtc(s('2024-01-15T00:00:00Z')), s('2024-01-14T22:00:00Z'));
  // Summer: 21:00 UTC.
  assert.equal(fxOpenUtc(s('2024-07-15T00:00:00Z')), s('2024-07-14T21:00:00Z'));
  // Not a Sunday evening or Monday morning.
  assert.equal(fxOpenUtc(s('2024-07-17T00:00:00Z')), null);
});

/** Hourly server times of FX trading weeks, Sunday 17:00 to Friday 17:00 New York. */
function fxHours(model: ZoneModel, from: number, to: number, perWeek?: (week: number) => ZoneModel): number[] {
  const out: number[] = [];
  for (let t = from; t < to; t += HOUR) {
    if (!fxTrades(t)) continue;
    const m = perWeek ? perWeek(t) : model;
    out.push(toServer(m, t));
  }
  return out.sort((a, b) => a - b);
}

/** FX trades at a UTC instant: Sunday 17:00 to Friday 17:00 New York. */
function fxTrades(utc: number): boolean {
  const ny = new Date((utc + offsetAt('ny+7', utc) - 7 * HOUR) * 1000);
  const day = ny.getUTCDay();
  const hour = ny.getUTCHours();
  if (day === 6) return false;
  if (day === 0) return hour >= 17;
  if (day === 5) return hour < 17;
  return true;
}

test('weekly opens tell an Athens server from a New York + 7 one', () => {
  const from = s('2024-01-01T00:00:00Z');
  const to = s('2025-01-01T00:00:00Z');
  const eet = chooseModel(weeklyOpens(fxHours('eet', from, to)));
  assert.equal(eet.model, 'eet');
  assert.equal(eet.fit, eet.of);
  assert.ok(eet.of >= 50);
  const ny = chooseModel(weeklyOpens(fxHours('ny+7', from, to)));
  assert.equal(ny.model, 'ny+7');
  assert.equal(ny.fit, ny.of);
  assert.deepEqual(chooseModel([]), { model: 'ny+7', fit: 0, of: 0, assumed: true });
});

test("weekShifts: the weeks an Athens server's history ran New York + 7", () => {
  const from = s('2024-02-01T00:00:00Z');
  const to = s('2024-05-01T00:00:00Z');
  // Old history: New York + 7 in March, Athens otherwise.
  const march = (t: number): ZoneModel =>
    t >= s('2024-03-09T00:00:00Z') && t < s('2024-03-31T00:00:00Z') ? 'ny+7' : 'eet';
  const times = fxHours('eet', from, to, march);
  const shifts = weekShifts('eet', times);
  assert.deepEqual(
    [...shifts.keys()].map((w) => new Date(w * 1000).toISOString().slice(0, 10)),
    ['2024-03-09', '2024-03-16', '2024-03-23'],
  );
  for (const offset of shifts.values()) assert.equal(offset, 3 * HOUR);
  // Every hour of those weeks lands where it belongs.
  for (const server of times) {
    const shift = shifts.get(weekOf(server));
    const utc = shift === undefined ? toUtc('eet', server) : server - shift;
    assert.ok(fxTrades(utc), new Date(utc * 1000).toISOString());
  }
  assert.equal(weekShifts('cet', times).size, 0);
});

test('a fresh tick measures the offset while FX trades, never on a Saturday', () => {
  const wednesday = s('2025-03-12T12:00:00Z') * 1000;
  assert.deepEqual(tickEvidence(wednesday + 2 * HOUR * 1000 - 3000, wednesday), {
    at: wednesday / 1000,
    offset: 2 * HOUR,
  });
  assert.equal(tickEvidence(wednesday + 2 * HOUR * 1000 - 60_000, wednesday), null, 'a stale tick');
  const saturday = s('2025-03-15T12:00:00Z') * 1000;
  assert.equal(tickEvidence(saturday + 2 * HOUR * 1000, saturday), null);
});

// ---------------------------------------------------------------------------------------------
// Symbols and days

test("broker symbols: decorations, index tickers and aliases; the undecorated, the instrument's own name wins", () => {
  const sym = (name: string, more: Record<string, unknown> = {}) => ({ name, digits: 5, ...more });
  assert.equal(instrumentFor(sym('EURUSDm')), 'eurusd');
  assert.equal(instrumentFor(sym('EURUSD.a')), 'eurusd');
  assert.equal(instrumentFor(sym('EURUSD#')), 'eurusd');
  assert.equal(instrumentFor(sym('GER40.cash')), 'deuidxeur');
  assert.equal(instrumentFor(sym('US30')), 'usa30idxusd');
  assert.equal(instrumentFor(sym('USOIL')), 'lightcmdusd');
  // Stock CFDs are never a history source.
  assert.equal(instrumentFor(sym('AAPL')), null);
  assert.equal(instrumentFor(sym('NOTATHING')), null);
  // A pair must agree with the currencies the broker states.
  assert.equal(instrumentFor(sym('EURUSD', { base: 'EUR', profit: 'USD' })), 'eurusd');
  assert.equal(instrumentFor(sym('EURUSD', { base: 'GBP', profit: 'USD' })), null);
  const map = mapSymbols([
    sym('EURUSDm', { visible: true }),
    sym('EURUSD'),
    sym('EURUSD.raw'),
    sym('GOLD', { visible: true }),
    sym('XAUUSD'),
    sym('GER40.cash'),
    sym('DE40'),
  ]);
  assert.equal(map.get('eurusd')!.name, 'EURUSD');
  assert.equal(map.get('xauusd')!.name, 'XAUUSD');
  assert.equal(map.get('deuidxeur')!.name, 'DE40', 'undecorated over decorated');
  assert.equal(map.size, 3);
});

test('a day of MetaTrader bars is a candle response the store decodes back', () => {
  const day = s('2024-03-13T00:00:00Z');
  const bars: Bar[] = [
    { t: day, o: 1.0912, h: 1.09135, l: 1.09101, c: 1.0913, v: 41 },
    { t: day + 60, o: 1.0913, h: 1.0915, l: 1.0912, c: 1.09148, v: 17 },
    { t: day + 3600, o: 1.09001, h: 1.09012, l: 1.08997, c: 1.09, v: 3 },
  ];
  assert.deepEqual(decodeBucket('m1', day, encodeDay(bars, 5)), bars);
  const yen = bars.map((b) => ({ ...b, o: b.o * 100, h: b.h * 100, l: b.l * 100, c: b.c * 100 }));
  const back = decodeBucket('m1', day, encodeDay(yen, 3));
  for (const [i, b] of back.entries()) {
    assert.equal(b.t, yen[i]!.t);
    assert.ok(Math.abs(b.c - yen[i]!.c) < 1e-9);
  }
  const grouped = byDay([...bars, { t: day + DAY, o: 1, h: 1, l: 1, c: 1, v: 1 }]);
  assert.deepEqual([...grouped.keys()], [day, day + DAY]);
});

// ---------------------------------------------------------------------------------------------
// The source

const NOW = s('2025-03-12T12:00:00Z');

interface World {
  /** UTC of the first bar the terminal answers with. */
  start: number;
  /** UTC days it lacks. */
  gaps: Set<number>;
  connected: boolean;
  price: number;
  /** Max bars in chart. */
  maxBars: number;
  /** UTC of the first and after the last bar in its history files. */
  files: Range;
}

/** Bars with server times in [from, to) every `step` while FX trades, but the days the world lacks. */
function barsOf(world: World, from: number, to: number, step: number, has: (utc: number) => boolean) {
  const out = {
    time: [] as number[],
    open: [] as number[],
    high: [] as number[],
    low: [] as number[],
    close: [] as number[],
    volume: [] as number[],
  };
  for (let server = Math.ceil(from / step) * step; server < to; server += step) {
    const utc = toUtc('eet', server);
    if (!has(utc) || !fxTrades(utc) || world.gaps.has(Math.floor(utc / DAY) * DAY)) continue;
    out.time.push(server);
    for (const key of ['open', 'high', 'low', 'close'] as const) out[key].push(world.price);
    out.volume.push(10);
  }
  return out;
}

/** A fake bridge whose terminal runs Athens' clock, with EUR/USD bars every 30 min while FX trades,
 *  answered from `world.start` and read from its files over `world.files`. */
class FakeBridge implements BridgeLike {
  alive = true;
  calls: Array<{ method: string; params: Record<string, unknown> }> = [];
  handlers: Record<string, (params: Record<string, unknown>) => unknown>;

  constructor(
    world: World,
    clock: { ms: number },
    more: Record<string, (params: Record<string, unknown>) => unknown> = {},
  ) {
    const model: ZoneModel = 'eet';
    const bars = (from: number, to: number, step: number, has: (utc: number) => boolean) =>
      barsOf(world, from, to, step, has);
    this.handlers = {
      hello: () => ({ bridge: 1, python: '3.13.1', platform: 'win32', package: '5.0.5' }),
      running: () => ({ pid: 4242 }),
      connect: (p) => ({
        account: {
          login: p.login,
          server: p.server,
          name: 'Demo',
          company: 'MetaQuotes Ltd.',
          currency: 'USD',
          trade_mode: 0,
        },
        terminal: { connected: world.connected, build: 4755, maxbars: world.maxBars },
        switched: false,
      }),
      symbols: () => [
        { name: 'EURUSD', base: 'EUR', profit: 'USD', digits: 5, visible: true },
        { name: 'GER40', base: 'EUR', profit: 'EUR', digits: 1, visible: false },
      ],
      tick: () => ({
        timeMsc: toServer(model, Math.floor(clock.ms / 1000)) * 1000,
        bid: world.price,
        ask: world.price,
      }),
      io: () => ({ written: null }),
      rates: (p) => {
        const nowSec = Math.floor(clock.ms / 1000);
        const step = p.timeframe === 'H1' ? HOUR : 1800;
        const out = bars(Number(p.from), Number(p.to), step, (utc) => utc <= nowSec && utc >= world.start);
        return { ...out, last: out.time.at(-1) ?? null, connected: world.connected, digits: 5 };
      },
      history: (p) => {
        const [first, end] = world.files;
        const out = bars(Number(p.from), Number(p.to), 1800, (utc) => utc >= first && utc < end);
        return { ...out, last: null, connected: false, digits: 5, files: 2 };
      },
      ...more,
    };
  }

  async call<T>(method: string, params: Record<string, unknown>): Promise<T> {
    this.calls.push({ method, params });
    await Promise.resolve();
    const handler = this.handlers[method];
    if (!handler) throw new BridgeError('UNKNOWN_METHOD', method);
    return handler(params) as T;
  }

  async close(): Promise<void> {
    this.alive = false;
  }

  rates(timeframe = 'M1') {
    return this.calls.filter((c) => c.method === 'rates' && c.params.timeframe === timeframe);
  }

  files() {
    return this.calls.filter((c) => c.method === 'history');
  }
}

const CONFIG: MetaTraderConfig = {
  mode: 'fallback',
  bridge: { program: 'python.exe', args: ['bridge.py'] },
  terminal: 'C:\\Program Files\\MetaTrader 5\\terminal64.exe',
  login: 5012345,
  server: 'MetaQuotes-Demo',
};

async function source(
  opts: {
    world?: Partial<World>;
    handlers?: Record<string, (params: Record<string, unknown>) => unknown>;
    writes?: () => Promise<number | null>;
    store?: DukascopyStore;
    cacheDir?: string;
    log?: (level: 'info' | 'warn' | 'error', message: string) => void;
  } = {},
) {
  const cacheDir = opts.cacheDir ?? tempDir('demido-mt5-');
  const clock = { ms: NOW * 1000 };
  const world: World = {
    start: s('2023-06-01T00:00:00Z'),
    gaps: new Set(),
    connected: true,
    price: 1.0905,
    // About 23 months of minutes: the terminal answers from late April 2023.
    maxBars: 1_000_000,
    files: [s('2020-01-01T00:00:00Z'), s('2025-03-12T00:00:00Z')],
    ...opts.world,
  };
  const bridges: FakeBridge[] = [];
  const statuses: MetaTraderStatus[] = [];
  const store = opts.store ?? new DukascopyStore(cacheDir, { now: () => clock.ms });
  let ready = 0;
  const mt = new MetaTraderSource({
    store,
    cacheDir,
    now: () => clock.ms,
    bridge: () => {
      const b = new FakeBridge(world, clock, opts.handlers);
      bridges.push(b);
      return b;
    },
    writes: opts.writes ?? (async () => null),
    log: opts.log,
    onStatus: (st) => statuses.push(st),
    onReady: () => (ready += 1),
  });
  await mt.configure(CONFIG);
  const signal = new AbortController().signal;
  return {
    mt,
    store,
    clock,
    world,
    bridges,
    statuses,
    cacheDir,
    ready: () => ready,
    chunk: (start: number, instrument = 'eurusd') => mt.fetchChunk(instrument, start, signal),
  };
}

const key = (t: number) => new Date(t * 1000).toISOString().slice(0, 10);

test('a chunk is the calendar year around the day, in UTC, once; the clock is read and kept', async () => {
  const { mt, chunk, bridges, statuses, cacheDir, ready } = await source({
    world: { gaps: new Set([s('2024-05-15T00:00:00Z'), s('2024-12-25T00:00:00Z')]) },
  });
  assert.equal(ready(), 1, 'configured and usable');
  const got = await chunk(s('2024-03-13T00:00:00Z'));
  assert.deepEqual(got.range, [s('2024-01-01T00:00:00Z'), s('2025-01-01T00:00:00Z')]);
  const [bridge] = bridges;
  assert.equal(bridge!.rates('H1').length, 1, 'the clock, from a year of hours');
  const [m1] = bridge!.rates('M1');
  // Four days' margin each side, in server time (Athens, +2 then).
  assert.equal(m1!.params.from, s('2023-12-28T02:00:00Z'));
  assert.equal(m1!.params.to, s('2025-01-05T02:00:00Z'));
  assert.equal(m1!.params.symbol, 'EURUSD');

  const wed = got.days.get(s('2024-03-13T00:00:00Z'))!;
  assert.equal(wed.final, true);
  const bars = decodeBucket('m1', s('2024-03-13T00:00:00Z'), wed.buffer!);
  assert.equal(bars.length, 48);
  assert.equal(bars[0]!.t, s('2024-03-13T00:00:00Z'));
  // Sunday 17 March: the US already on summer time, the EU not yet. Athens' clock read as Athens'
  // puts the open at 21:00 UTC (New York + 7 would say 20:00).
  const sunday = decodeBucket('m1', s('2024-03-17T00:00:00Z'), got.days.get(s('2024-03-17T00:00:00Z'))!.buffer!);
  assert.equal(sunday[0]!.t, s('2024-03-17T21:00:00Z'));
  // Saturday: empty. A Wednesday gap: the broker's, unknown. Christmas: empty.
  assert.deepEqual(got.days.get(s('2024-03-16T00:00:00Z')), { buffer: null, final: true, builtAt: NOW });
  assert.equal(got.days.has(s('2024-05-15T00:00:00Z')), false);
  assert.equal(got.days.get(s('2024-12-25T00:00:00Z'))!.buffer, null);
  assert.equal(got.days.size, 365);

  // Asked once: another day of that year is not asked again.
  const again = await chunk(s('2024-06-05T00:00:00Z'));
  assert.equal(again.days.size, 0);
  assert.equal(bridge!.rates('M1').length, 1);

  const st = mt.status();
  assert.equal(st.state, 'ready');
  assert.deepEqual(st.clock, { model: 'eet', assumed: false, fit: st.clock!.of, of: st.clock!.of });
  assert.equal(st.account!.login, CONFIG.login);
  assert.equal(st.markets, 2);
  assert.equal(st.days, 365);
  assert.deepEqual(
    [...new Set(statuses.map((x) => x.state))],
    ['idle', 'starting', 'connecting', 'ready', 'downloading'],
  );
  const saved = JSON.parse(fs.readFileSync(path.join(cacheDir, 'metatrader', 'clock.json'), 'utf8'));
  assert.equal(saved.servers['MetaQuotes-Demo'].model, 'eet');

  // A new service on the same cache keeps the clock: no hours asked.
  const second = await source({ cacheDir });
  await second.chunk(s('2024-03-13T00:00:00Z'));
  assert.equal(second.bridges[0]!.rates('H1').length, 0);
  assert.equal(second.mt.status().clock!.model, 'eet');
});

test('this year: up to today, what the store has at the start left out, today provisional and asked again', async () => {
  const { chunk, bridges, store } = await source();
  for (let d = s('2025-01-01T00:00:00Z'); d < s('2025-03-01T00:00:00Z'); d += DAY) {
    await store.put('eurusd', 'm1', d, null, { builtAt: d + 2 * DAY, final: true });
  }
  const today = s('2025-03-12T00:00:00Z');
  const got = await chunk(today);
  assert.deepEqual(got.range, [s('2025-03-01T00:00:00Z'), today + DAY]);
  assert.deepEqual(got.days.get(today)!.final, false);
  assert.equal(got.days.get(today)!.builtAt, NOW);
  assert.equal(got.days.get(today - DAY)!.final, true);
  // Today again: only today.
  const again = await chunk(today);
  assert.deepEqual(again.range, [today, today + DAY]);
  assert.equal(bridges[0]!.rates('M1').length, 2);
});

test('a history Max bars in chart cut: the days before its first full day, and older years, come from the files', async () => {
  // A Wednesday noon.
  const { chunk, bridges } = await source({ world: { start: s('2024-08-14T12:00:00Z') } });
  const got = await chunk(s('2024-03-13T00:00:00Z'));
  assert.deepEqual(got.range, [s('2024-01-01T00:00:00Z'), s('2025-01-01T00:00:00Z')]);
  assert.equal(got.days.size, 366, 'all of 2024');
  const [files] = bridges[0]!.files();
  // Up to the cut day's end, with the margin each side.
  assert.equal(files!.params.from, s('2023-12-28T02:00:00Z'));
  assert.equal(files!.params.to, s('2024-08-19T03:00:00Z'));
  const cut = decodeBucket('m1', s('2024-08-14T00:00:00Z'), got.days.get(s('2024-08-14T00:00:00Z'))!.buffer!);
  assert.equal(cut.length, 48, 'the cut day whole, from the files');
  // Older years: the files only, the terminal asked nothing.
  const older = await chunk(s('2023-05-02T00:00:00Z'));
  assert.deepEqual(older.range, [s('2023-01-01T00:00:00Z'), s('2024-01-01T00:00:00Z')]);
  assert.equal(older.days.size, 365);
  assert.equal(bridges[0]!.rates('M1').length, 1);
  assert.equal(bridges[0]!.files().length, 2);
});

test('beyond Max bars in chart: pieces it allows from the terminal, older days from its files', async () => {
  // 100,000 bars: 69 days of minutes whatever the market.
  const { mt, chunk, bridges, statuses } = await source({ world: { maxBars: 100_000 } });
  const today = s('2025-03-12T00:00:00Z');
  const reach = s('2025-01-07T00:00:00Z');
  const recent = await chunk(today);
  assert.deepEqual(recent.range, [reach, today + DAY]);
  const [m1] = bridges[0]!.rates('M1');
  const oldest = toUtc('eet', Number(m1!.params.from));
  assert.ok((NOW - oldest) / 60 <= 100_000, 'no bar further back than Max bars');
  assert.equal(bridges[0]!.files().length, 0);

  const year = await chunk(s('2024-03-13T00:00:00Z'));
  assert.deepEqual(year.range, [s('2024-01-01T00:00:00Z'), s('2025-01-01T00:00:00Z')]);
  assert.equal(year.days.size, 366);
  assert.equal(year.days.get(s('2024-03-13T00:00:00Z'))!.final, true);
  // The days of this year before the reach.
  const start = await chunk(s('2025-01-06T00:00:00Z'));
  assert.deepEqual(start.range, [s('2025-01-01T00:00:00Z'), reach]);
  assert.equal(bridges[0]!.rates('M1').length, 1, 'the terminal asked once');
  assert.equal(bridges[0]!.files().length, 2);
  assert.ok(statuses.some((st) => /history files, 2024-01-01 to 2024-12-31/.test(st.message ?? '')));
  assert.equal(mt.status().state, 'ready');
});

test("files a bridge cannot read: those days are Dukascopy's, and the files are not tried again", async () => {
  const warnings: string[] = [];
  const env = await source({
    world: { maxBars: 100_000 },
    log: (level, message) => level === 'warn' && warnings.push(message),
    handlers: {
      history: () => {
        throw new BridgeError('UNKNOWN_METHOD', 'Unknown method history');
      },
    },
  });
  const got = await env.chunk(s('2024-03-13T00:00:00Z'));
  assert.equal(got.days.size, 0);
  assert.equal(env.mt.status().state, 'ready', 'not a failure of the terminal');
  assert.equal(env.mt.serves('eurusd'), true);
  const older = await env.chunk(s('2023-03-01T00:00:00Z'));
  assert.equal(older.days.size, 0);
  assert.equal(env.bridges[0]!.files().length, 1);
  assert.equal(warnings.length, 1);
  assert.match(warnings[0]!, /history files cannot be read \(Unknown method history\)/);
  // The terminal still answers within Max bars.
  const recent = await env.chunk(s('2025-03-12T00:00:00Z'));
  assert.ok(recent.days.size > 0);
});

test('a terminal answering nothing for closed days within its reach: the files answer them', async () => {
  const { chunk, bridges } = await source({ world: { start: s('2026-01-01T00:00:00Z') } });
  const got = await chunk(s('2024-03-13T00:00:00Z'));
  assert.equal(got.days.size, 366);
  assert.equal(bridges[0]!.rates('M1').length, 1);
  assert.equal(bridges[0]!.files().length, 1);
});

test('a disconnected terminal: its last day with bars and later are unknown', async () => {
  const { chunk } = await source({ world: { connected: false } });
  const today = s('2025-03-12T00:00:00Z');
  const got = await chunk(today);
  assert.equal(got.days.has(today), false);
  assert.equal(got.days.get(today - DAY)!.final, true);
});

test('prices far from Dukascopy mean another market: the instrument is not served', async () => {
  const cacheDir = tempDir('demido-mt5-');
  const store = new DukascopyStore(cacheDir, { now: () => NOW * 1000 });
  const day = s('2024-03-12T00:00:00Z');
  const yen = Array.from({ length: 10 }, (_, i) => ({ t: day + i * 60, o: 150.1, h: 150.2, l: 150, c: 150.1, v: 1 }));
  await store.put('eurusd', 'm1', day, encodeDay(yen, 3), { builtAt: day + 2 * DAY, final: true });
  const { mt, chunk } = await source({ store, cacheDir });
  await assert.rejects(chunk(s('2024-03-13T00:00:00Z')), /EURUSD on MetaQuotes-Demo is not EUR\/USD/);
  assert.equal(mt.serves('eurusd'), false);
  assert.equal(mt.serves('deuidxeur'), true);
  assert.equal(mt.serves('usdjpy'), false, 'the broker has no symbol for it');
});

test('a terminal that is not running is launched; one that refuses the account cools MetaTrader down', async () => {
  const launched = await source({
    handlers: { running: () => ({ pid: null }), launch: () => ({ pid: 77, tied: false, launched: true }) },
  });
  await launched.chunk(s('2024-03-13T00:00:00Z'));
  const calls = launched.bridges[0]!.calls;
  const launch = calls.find((c) => c.method === 'launch')!;
  assert.equal(launch.params.path, CONFIG.terminal);
  assert.equal(calls.find((c) => c.method === 'connect')!.params.settle, 60, 'a terminal just started logs in slowly');

  const refused = await source({
    handlers: {
      connect: () => {
        throw new BridgeError('AUTH_FAILED', 'MetaTrader could not log into 5012345 on MetaQuotes-Demo');
      },
    },
  });
  await assert.rejects(refused.chunk(s('2024-03-13T00:00:00Z')), /could not log into/);
  const st = refused.mt.status();
  assert.equal(st.state, 'error');
  assert.match(st.message!, /could not log into/);
  assert.equal(st.retryAt, NOW * 1000 + 120_000);
  assert.equal(refused.mt.serves('eurusd'), false);
  assert.equal(refused.bridges[0]!.alive, false, 'its bridge was closed');
  // After the cool-down it may be asked again.
  refused.clock.ms += 121_000;
  assert.equal(refused.mt.serves('eurusd'), true);
});

test('the write guard: a terminal writing over 20 MB/s pauses MetaTrader for 15 min', async () => {
  let written = 0;
  const env = await source({
    writes: async () => written,
    handlers: {},
  });
  const rates = env.bridges.length;
  assert.equal(rates, 0);
  // The first chunk opens the session; make its M1 request slow and heavy.
  const original = FakeBridge.prototype.call;
  FakeBridge.prototype.call = async function <T>(this: FakeBridge, method: string, params: Record<string, unknown>) {
    const result = await original.call(this, method, params);
    if (method === 'rates' && params.timeframe === 'M1') {
      env.clock.ms += 6_000;
      written += 200 * 1024 * 1024;
    }
    return result as T;
  };
  try {
    await env.chunk(s('2024-03-13T00:00:00Z'));
  } finally {
    FakeBridge.prototype.call = original;
  }
  const st = env.mt.status();
  assert.equal(st.state, 'paused');
  assert.match(st.message!, /200 MB/);
  assert.equal(st.retryAt, env.clock.ms + 15 * 60_000);
  assert.equal(env.mt.serves('eurusd'), false);
});

test('config: incomplete is off; another account starts over', async () => {
  assert.equal(parseConfig({ mode: 'preferred', bridge: null, terminal: 'x', login: 1, server: 's' })!.bridge, null);
  assert.equal(parseConfig('nope'), null);
  const parsed = parseConfig({ ...CONFIG, login: '5012345', extra: true })!;
  assert.equal(parsed.login, 5012345);
  assert.equal(parsed.allowSwitch, true);
  const { mt, chunk, bridges } = await source();
  assert.equal(mt.mode(), 'fallback');
  await chunk(s('2024-03-13T00:00:00Z'));
  await mt.configure({ ...CONFIG, login: 0 });
  assert.equal(mt.mode(), 'off');
  assert.equal(mt.status().state, 'off');
  assert.equal(mt.serves('eurusd'), false);
  assert.equal(bridges[0]!.alive, false);
  await mt.configure({ ...CONFIG, login: 7 });
  // Asked again under the new account.
  await chunk(s('2024-03-13T00:00:00Z'));
  assert.equal(bridges.at(-1)!.rates('M1').length, 1);
  await mt.close();
});

test('only: it says what it waits for and what it lacks; a day asked comes back without the terminal', async () => {
  assert.equal(parseConfig({ ...CONFIG, mode: 'only' })!.mode, 'only');
  const only = { ...CONFIG, mode: 'only' as const };
  const { mt, chunk, bridges, ready } = await source();
  await mt.configure(only);
  assert.equal(mt.mode(), 'only');
  assert.equal(mt.blocked(), null);
  const day = s('2024-03-13T00:00:00Z');
  assert.ok((await chunk(day)).days.size > 0);
  // The session ended (the terminal was closed, say): a day already asked needs no new one.
  bridges[0]!.alive = false;
  assert.deepEqual(await chunk(day), { range: [day, day + DAY], days: new Map() });
  assert.equal(bridges.length, 1);
  // A market the account has no symbol for, and why.
  assert.equal(mt.lacks('eurusd'), null);
  assert.match(mt.lacks('usdjpy')!, /^MetaQuotes-Demo has no symbol for /);
  assert.equal(mt.serves('usdjpy'), false);
  // An incomplete setup keeps it the only source (Dukascopy does not step in meanwhile), serving
  // nothing, and the fetcher is told to look again.
  const before = ready();
  await mt.configure({ ...only, login: 0 });
  assert.equal(ready(), before + 1);
  assert.equal(mt.mode(), 'only');
  assert.equal(mt.serves('eurusd'), false);
  assert.equal(mt.blocked(), 'Choose a MetaTrader account');
  await mt.configure({ ...only, bridge: null });
  assert.equal(mt.blocked(), "MetaTrader's bridge is not installed yet");
  await mt.configure({ ...only, terminal: '' });
  assert.equal(mt.blocked(), 'The MetaTrader terminal chosen is not on this computer');
  // Any other mode is off until it is set up.
  await mt.configure({ ...CONFIG, mode: 'preferred', login: 0 });
  assert.equal(mt.mode(), 'off');
  await mt.close();
  assert.equal(mt.blocked(), 'MetaTrader is closed');
});
