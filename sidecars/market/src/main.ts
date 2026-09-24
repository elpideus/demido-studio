// Demido Studio's market data service. Started by the app, it reads requests on stdin and
// answers on stdout (see protocol.ts). It exits when the app closes its stdin.

import { createInterface } from 'node:readline';

import './net.ts';
import './http.ts';
import { candles } from './candles.ts';
import * as dukascopy from './dukascopy.ts';
import { RpcError, emit, fail, log, reply } from './protocol.ts';
import { TIMEFRAMES, parseDate, timeframe } from './timeframes.ts';
import * as tv from './tradingview.ts';

// Libraries must never write to stdout: it carries the protocol.
console.log = (...args: unknown[]) => process.stderr.write(`${args.map(String).join(' ')}\n`);
console.info = console.log;

const VERSION = '0.1.0';

type Params = Record<string, unknown>;

function str(params: Params, key: string, required = true): string {
  const v = params[key];
  if (typeof v === 'string' && v.trim()) return v.trim();
  if (required) throw new RpcError('BAD_REQUEST', `"${key}" is required.`);
  return '';
}

const handlers: Record<string, (params: Params) => Promise<unknown> | unknown> = {
  ping: () => ({ version: VERSION, loggedIn: tv.isLoggedIn(), username: tv.currentUser() }),

  'auth.set': (p) => tv.setCredentials({ session: str(p, 'session'), signature: str(p, 'signature', false) }),

  'auth.clear': () => {
    tv.clearCredentials();
    return {};
  },

  search: (p) => tv.search(str(p, 'query'), typeof p.type === 'string' ? p.type : undefined),

  quote: (p) => {
    const symbols = Array.isArray(p.symbols) ? p.symbols.map(String).filter(Boolean) : [];
    if (!symbols.length) throw new RpcError('BAD_REQUEST', 'Give at least one symbol.');
    return tv.quote(symbols.slice(0, 20));
  },

  candles: (p) => {
    const tf = timeframe(p.timeframe);
    const count = Math.max(1, Math.min(Number(p.bars) || 300, 20_000));
    return candles(str(p, 'symbol'), tf, count, parseDate(p.from), parseDate(p.to, true));
  },

  history: async (p) => {
    const tf = timeframe(p.timeframe);
    const from = parseDate(p.from);
    if (from === null) throw new RpcError('BAD_REQUEST', '"from" is required (YYYY-MM-DD).');
    const to = parseDate(p.to, true) ?? Math.floor(Date.now() / 1000);
    const result = await dukascopy.history(str(p, 'instrument'), tf, from, to);
    const meta = dukascopy.instrumentInfo(result.instrument);
    return {
      symbol: meta.name,
      info: { symbol: meta.name, description: meta.description, exchange: 'Dukascopy' },
      timeframe: tf,
      bars: result.bars,
      sources: [{ source: 'dukascopy', count: result.bars.length }],
    };
  },

  'dukascopy.resolve': (p) => {
    const instrument = dukascopy.resolveInstrument(str(p, 'symbol'));
    return instrument ? { instrument, ...dukascopy.instrumentInfo(instrument) } : null;
  },

  'stream.open': (p) => {
    const tf = timeframe(p.timeframe);
    const count = Math.max(10, Math.min(Number(p.bars) || 500, 5000));
    return tv.openStream(str(p, 'symbol'), tf, count);
  },

  // Older bars for a chart: TradingView first, then Dukascopy once TradingView runs out.
  'stream.more': async (p) => {
    const id = str(p, 'id');
    const count = Math.max(1, Math.min(Number(p.count) || 500, 5000));
    const info = tv.streamInfo(id);
    if (!info) throw new RpcError('NOT_FOUND', 'That chart is closed.');
    const live = await tv.moreHistory(id, count);
    if (!live.exhausted) return { bars: live.bars, source: 'tradingview', exhausted: false };
    const before = typeof p.before === 'number' ? p.before : null;
    const instrument = dukascopy.resolveInstrument(info.symbol);
    if (!instrument || before === null) return { bars: [], source: 'tradingview', exhausted: true };
    const step = TIMEFRAMES[info.tf].seconds;
    const span = Math.min(count * step * 1.6, TIMEFRAMES[info.tf].maxDays * 86400);
    const older = await dukascopy.history(instrument, info.tf, Math.floor(before - span), before);
    const bars = older.bars.filter((b) => b.t < before);
    return { bars, source: 'dukascopy', exhausted: bars.length === 0 };
  },

  'stream.close': (p) => {
    tv.closeStream(str(p, 'id'));
    return {};
  },
};

async function handle(line: string): Promise<void> {
  let message: { id?: number; method?: string; params?: Params };
  try {
    message = JSON.parse(line);
  } catch {
    log('warn', `ignored a line that is not JSON: ${line.slice(0, 120)}`);
    return;
  }
  const { id, method, params } = message;
  if (typeof id !== 'number' || typeof method !== 'string') return;
  const handler = handlers[method];
  if (!handler) {
    fail(id, new RpcError('UNKNOWN_METHOD', `Unknown method ${method}.`));
    return;
  }
  try {
    reply(id, await handler(params ?? {}));
  } catch (error) {
    fail(id, error);
  }
}

if (process.env.DEMIDO_CACHE_DIR) dukascopy.setCacheDir(process.env.DEMIDO_CACHE_DIR);
tv.restoreFromEnvironment();

process.on('uncaughtException', (error) => log('error', `uncaught: ${error.stack ?? error.message}`));
process.on('unhandledRejection', (reason) => log('error', `unhandled: ${String(reason)}`));

const rl = createInterface({ input: process.stdin, crlfDelay: Infinity });
rl.on('line', (line) => {
  if (line.trim()) void handle(line);
});
rl.on('close', () => process.exit(0));
emit('ready', { version: VERSION });
