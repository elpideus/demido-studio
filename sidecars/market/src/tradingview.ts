// Real-time data from TradingView through TradingView-API
// (https://github.com/Mathieu2301/Tradingview-API).
//
// One websocket client serves everything. It exists only while a session is set: Demido
// requires a TradingView sign-in for live data, and the session cookies (`sessionid`,
// `sessionid_sign`) come from the sign-in window of the app.

import TradingView, { type ChartSession, type Client, type MarketInfos } from '@mathieuc/tradingview';
import miscRequests from '@mathieuc/tradingview/src/miscRequests.js';

import { type Bar, RpcError, emit, log } from './protocol.ts';
import { TIMEFRAMES, type Timeframe } from './timeframes.ts';

interface Credentials {
  session: string;
  signature: string;
}

let credentials: Credentials | null = null;
let username: string | null = null;
let client: Client | null = null;
/** Pending validation of a session, so requests wait for it instead of failing. */
let authPending: Promise<unknown> | null = null;

const PAGE = 5000;

export function isLoggedIn(): boolean {
  return credentials !== null;
}

function closeClient(): void {
  if (client) {
    client.end().catch(() => undefined);
    client = null;
  }
  for (const id of [...streams.keys()]) closeStream(id, 'The connection to TradingView was reset.');
}

/** Retries transient network failures (DNS hiccups, resets) twice. */
async function withRetry<T>(fn: () => Promise<T>): Promise<T> {
  for (let attempt = 1; ; attempt += 1) {
    try {
      return await fn();
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      if (attempt >= 3 || !/ENOTFOUND|EAI_AGAIN|ECONNRESET|ETIMEDOUT|socket hang up/i.test(message)) throw error;
      await new Promise((r) => setTimeout(r, 800 * attempt));
    }
  }
}

/** The page whose HTML carries the signed-in user's websocket auth token. */
const USER_PAGE = 'https://www.tradingview.com/chart/';

interface TvUser {
  username: string;
  authToken: string;
}

/**
 * Reads the signed-in user from TradingView's chart page.
 *
 * TradingView-API's own getUser() scrapes the homepage, which CloudFront now refuses to
 * non-browser clients (403) or serves without the token, and the library then keeps
 * re-requesting it (upstream issue #321). The chart page carries the same token; redirects
 * are followed only when they are real ones.
 */
export async function fetchUser(session: string, signature: string): Promise<TvUser> {
  try {
    return await fetchUserAs(session, signature, undefined);
  } catch (error) {
    // The cookies come from the sign-in window's browser; if the page served to the app
    // differs, ask for it the way that browser did before calling the session invalid.
    if (error instanceof RpcError && error.code === 'SESSION_EXPIRED') {
      return fetchUserAs(session, signature, WEBVIEW_USER_AGENT);
    }
    throw error;
  }
}

const WEBVIEW_USER_AGENT =
  'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36 Edg/140.0.0.0';

async function fetchUserAs(session: string, signature: string, userAgent: string | undefined): Promise<TvUser> {
  let url = USER_PAGE;
  for (let hop = 0; hop < 5; hop += 1) {
    const res = await fetch(url, {
      redirect: 'manual',
      headers: {
        cookie: signature ? `sessionid=${session};sessionid_sign=${signature}` : `sessionid=${session}`,
        accept: 'text/html,application/xhtml+xml',
        ...(userAgent ? { 'user-agent': userAgent } : {}),
      },
    });
    const location = res.headers.get('location');
    if (res.status >= 300 && res.status < 400 && location) {
      url = new URL(location, url).href;
      continue;
    }
    const html = await res.text();
    if (res.status === 403 || res.status >= 500) {
      throw new RpcError('NETWORK', `TradingView refused the request (HTTP ${res.status}). Try again in a moment.`);
    }
    return parseUser(html);
  }
  throw new RpcError('NETWORK', 'TradingView kept redirecting the sign-in check.');
}

/** Extracts the auth token and the username next to it from a TradingView page. */
export function parseUser(html: string): TvUser {
  const token = /"auth_token"\s*:\s*"([^"]+)"/.exec(html);
  if (!token) {
    throw new RpcError('SESSION_EXPIRED', 'TradingView did not accept the session. Sign in again.');
  }
  // The username belongs to the same user object as the token: take the closest one.
  const names = [...html.matchAll(/"username"\s*:\s*"([^"]+)"/g)];
  let username = 'TradingView user';
  let best = Number.MAX_SAFE_INTEGER;
  for (const m of names) {
    const distance = Math.abs((m.index ?? 0) - token.index);
    if (distance < best) {
      best = distance;
      username = m[1]!;
    }
  }
  return { username, authToken: token[1]! };
}

/** Checks a session with TradingView and switches to it. */
export async function setCredentials(c: Credentials): Promise<{ username: string }> {
  const check = withRetry(() => fetchUser(c.session, c.signature)).then(
    (user) => user,
    (error: unknown) => {
      if (error instanceof RpcError) throw error;
      const message = error instanceof Error ? error.message : String(error);
      log('warn', `sign-in check failed: ${message}`);
      throw new RpcError('NETWORK', `Could not reach TradingView: ${message}`);
    },
  );
  authPending = check;
  try {
    const user = await check;
    closeClient();
    credentials = c;
    username = user.username;
    log('info', `signed in as ${username}`);
    emit('auth', { loggedIn: true, username });
    return { username };
  } finally {
    if (authPending === check) authPending = null;
  }
}

/** Restores the session handed over at startup. A network failure keeps it; a rejection drops it. */
export function restoreFromEnvironment(): void {
  const session = process.env.DEMIDO_TV_SESSION;
  if (!session) return;
  const signature = process.env.DEMIDO_TV_SIGNATURE ?? '';
  // Assume it works until TradingView says otherwise, so requests are not refused meanwhile.
  credentials = { session, signature };
  setCredentials({ session, signature }).catch((error: unknown) => {
    if (error instanceof RpcError && error.code === 'SESSION_EXPIRED') {
      credentials = null;
      username = null;
      emit('auth', { loggedIn: false, expired: true });
    } else {
      log('warn', `Could not verify the TradingView session: ${String(error)}`);
    }
  });
}

export function clearCredentials(): void {
  closeClient();
  credentials = null;
  username = null;
  emit('auth', { loggedIn: false });
}

async function requireClient(): Promise<Client> {
  if (authPending) await authPending.catch(() => undefined);
  if (!credentials) {
    throw new RpcError('NOT_LOGGED_IN', 'Not signed in to TradingView.');
  }
  if (client && client.isOpen) return client;
  if (client) closeClient();
  const created = new TradingView.Client({
    token: credentials.session,
    signature: credentials.signature,
    // The client looks the user up itself; point it at the page that carries the token.
    location: USER_PAGE,
  });
  created.onError((...err: unknown[]) => {
    const text = err.map(String).join(' ');
    log('warn', `TradingView: ${text}`);
    if (/credentials error/i.test(text)) {
      credentials = null;
      username = null;
      emit('auth', { loggedIn: false, expired: true });
    }
  });
  created.onDisconnected(() => {
    if (client === created) {
      client = null;
      for (const id of [...streams.keys()]) {
        closeStream(id, 'The connection to TradingView dropped.');
      }
    }
  });
  client = created;
  return created;
}

function stripTags(text: string): string {
  return text.replace(/<[^>]+>/g, '');
}

export interface SymbolMatch {
  symbol: string;
  ticker: string;
  description: string;
  exchange: string;
  type: string;
}

/** Symbol search. Works without signing in (it is a public endpoint). */
export async function search(query: string, type?: string): Promise<SymbolMatch[]> {
  if (!query.trim()) return [];
  const results = await TradingView.searchMarketV3(query.trim(), type ?? '');
  const seen = new Set<string>();
  const out: SymbolMatch[] = [];
  for (const r of results) {
    const symbol = stripTags(r.id);
    if (seen.has(symbol)) continue;
    seen.add(symbol);
    out.push({
      symbol,
      ticker: stripTags(r.symbol),
      description: stripTags(r.description ?? ''),
      exchange: r.fullExchange || r.exchange,
      type: r.type,
    });
  }
  return out.slice(0, 30);
}

export interface Quote {
  symbol: string;
  description?: string;
  price?: number;
  change?: number;
  changePercent?: number;
  bid?: number;
  ask?: number;
  high?: number;
  low?: number;
  open?: number;
  prevClose?: number;
  volume?: number;
  currency?: string;
  exchange?: string;
  type?: string;
  time?: string;
  error?: string;
}

function num(v: unknown): number | undefined {
  return typeof v === 'number' && Number.isFinite(v) ? v : undefined;
}

export async function quote(symbols: string[]): Promise<Quote[]> {
  const c = await requireClient();
  const session = new c.Session.Quote({ fields: 'all' });
  try {
    return await Promise.all(
      symbols.map(
        (symbol) =>
          new Promise<Quote>((resolve) => {
            const market = new session.Market(symbol.trim().toUpperCase());
            let data: Record<string, unknown> = {};
            let loaded = false;
            let done = false;
            const finish = (error?: string) => {
              if (done) return;
              done = true;
              clearTimeout(timer);
              try {
                market.close();
              } catch {
                // already closed
              }
              if (error && data.lp === undefined) {
                resolve({ symbol, error });
                return;
              }
              const lpTime = num(data.lp_time);
              resolve({
                symbol: typeof data.pro_name === 'string' ? data.pro_name : symbol,
                description: typeof data.description === 'string' ? data.description : undefined,
                price: num(data.lp),
                change: num(data.ch),
                changePercent: num(data.chp),
                bid: num(data.bid),
                ask: num(data.ask),
                high: num(data.high_price),
                low: num(data.low_price),
                open: num(data.open_price),
                prevClose: num(data.prev_close_price),
                volume: num(data.volume),
                currency: typeof data.currency_code === 'string' ? data.currency_code : undefined,
                exchange: typeof data.exchange === 'string' ? data.exchange : undefined,
                type: typeof data.type === 'string' ? data.type : undefined,
                time: lpTime ? new Date(lpTime * 1000).toISOString() : undefined,
              });
            };
            const timer = setTimeout(
              () => finish('No data arrived for this symbol. Check it with market_search.'),
              12_000,
            );
            market.onData((d) => {
              data = d;
              if (loaded && d.lp !== undefined) finish();
            });
            market.onLoaded(() => {
              loaded = true;
              if (data.lp !== undefined) finish();
              else setTimeout(() => finish('The symbol has no price.'), 2500);
            });
            market.onError((...err: unknown[]) => finish(err.map(String).join(' ')));
          }),
      ),
    );
  } finally {
    setTimeout(() => session.delete(), 50);
  }
}

function toBar(p: { time: number; open: number; max: number; min: number; close: number; volume: number }): Bar {
  return { t: p.time, o: p.open, h: p.max, l: p.min, c: p.close, v: p.volume };
}

/** Bars of a chart session, oldest first. */
function barsOf(chart: ChartSession): Bar[] {
  return chart.periods.map(toBar).sort((a, b) => a.t - b.t);
}

export interface ChartInfo {
  symbol: string;
  description: string;
  exchange: string;
  currency: string;
  type: string;
  pricescale: number;
  timezone: string;
}

function infoOf(infos: MarketInfos, requested: string): ChartInfo {
  return {
    symbol: (infos.pro_name as string) || (infos.full_name as string) || requested,
    description: (infos.description as string) || '',
    exchange: (infos.exchange as string) || '',
    currency: (infos.currency_code as string) || '',
    type: (infos.type as string) || '',
    pricescale: Number(infos.pricescale) || 100,
    timezone: (infos.timezone as string) || 'Etc/UTC',
  };
}

/** Waits until a chart session has settled: data arrived and nothing new for `quietMs`. */
function settle(chart: ChartSession, timeoutMs: number, quietMs = 450): Promise<void> {
  return new Promise((resolve, reject) => {
    let quiet: ReturnType<typeof setTimeout> | undefined;
    let done = false;
    const finish = (error?: RpcError) => {
      if (done) return;
      done = true;
      clearTimeout(timer);
      clearTimeout(quiet);
      if (error) reject(error);
      else resolve();
    };
    const timer = setTimeout(() => {
      if (chart.periods.length > 0) finish();
      else finish(new RpcError('TIMEOUT', 'TradingView did not send any data in time.'));
    }, timeoutMs);
    // The library has no way to remove listeners, so finished waits ignore later events.
    chart.onUpdate(() => {
      if (done) return;
      clearTimeout(quiet);
      quiet = setTimeout(() => finish(), quietMs);
    });
    chart.onError((...err: unknown[]) => {
      if (done) return;
      const text = err.map(String).join(' ');
      finish(new RpcError('SYMBOL_ERROR', `TradingView: ${text}`));
    });
  });
}

/**
 * Up to `count` candles ending at `to` (seconds; now when omitted), paging back through
 * TradingView's history. Returns fewer when TradingView's history runs out.
 */
export async function candles(
  symbol: string,
  tf: Timeframe,
  count: number,
  to?: number,
): Promise<{ info: ChartInfo; bars: Bar[] }> {
  const c = await requireClient();
  const chart = new c.Session.Chart();
  try {
    const ready = settle(chart, 45_000);
    chart.setMarket(symbol.trim().toUpperCase(), {
      timeframe: TIMEFRAMES[tf].tradingview,
      range: Math.min(count, PAGE),
      ...(to ? { to } : {}),
    });
    await ready;
    let bars = barsOf(chart);
    while (bars.length < count) {
      const before = bars.length;
      const more = settle(chart, 20_000);
      chart.fetchMore(Math.min(PAGE, count - bars.length));
      await more.catch(() => undefined);
      bars = barsOf(chart);
      if (bars.length <= before) break; // History exhausted.
    }
    return { info: infoOf(chart.infos, symbol), bars: bars.slice(-count) };
  } finally {
    chart.delete();
  }
}

interface Stream {
  chart: ChartSession;
  symbol: string;
  tf: Timeframe;
  lastSent: string;
}

const streams = new Map<string, Stream>();
let nextStream = 1;

/** Opens a live chart: returns the first bars; `stream.update` events carry every change. */
export async function openStream(
  symbol: string,
  tf: Timeframe,
  count: number,
): Promise<{ id: string; info: ChartInfo; bars: Bar[] }> {
  const c = await requireClient();
  const chart = new c.Session.Chart();
  const ready = settle(chart, 45_000);
  chart.setMarket(symbol.trim().toUpperCase(), {
    timeframe: TIMEFRAMES[tf].tradingview,
    range: Math.min(count, PAGE),
  });
  try {
    await ready;
  } catch (error) {
    chart.delete();
    throw error;
  }
  const id = `s${nextStream++}`;
  const stream: Stream = { chart, symbol, tf, lastSent: '' };
  streams.set(id, stream);
  let pending: ReturnType<typeof setTimeout> | undefined;
  chart.onUpdate(() => {
    if (!streams.has(id)) return;
    // Coalesce bursts: the UI needs a few frames a second, not every tick.
    if (pending) return;
    pending = setTimeout(() => {
      pending = undefined;
      const latest = chart.periods[0];
      if (!latest) return;
      const bar = toBar(latest);
      const key = JSON.stringify(bar);
      if (key === stream.lastSent) return;
      stream.lastSent = key;
      emit('stream.update', { id, bar });
    }, 200);
  });
  chart.onError((...err: unknown[]) => {
    emit('stream.error', { id, message: err.map(String).join(' ') });
  });
  return { id, info: infoOf(chart.infos, symbol), bars: barsOf(chart) };
}

/** Older bars for a stream. Empty when TradingView has no more history. */
export async function moreHistory(id: string, count: number): Promise<{ bars: Bar[]; exhausted: boolean }> {
  const stream = streams.get(id);
  if (!stream) throw new RpcError('NOT_FOUND', 'That chart is closed.');
  const before = barsOf(stream.chart);
  const earliest = before[0]?.t ?? Number.MAX_SAFE_INTEGER;
  const more = settle(stream.chart, 20_000);
  stream.chart.fetchMore(Math.min(PAGE, Math.max(1, count)));
  await more.catch(() => undefined);
  const older = barsOf(stream.chart).filter((b) => b.t < earliest);
  return { bars: older, exhausted: older.length === 0 };
}

export function streamInfo(id: string): { symbol: string; tf: Timeframe } | null {
  const s = streams.get(id);
  return s ? { symbol: s.symbol, tf: s.tf } : null;
}

export function closeStream(id: string, reason?: string): void {
  const stream = streams.get(id);
  if (!stream) return;
  streams.delete(id);
  try {
    stream.chart.delete();
  } catch {
    // the socket is already gone
  }
  if (reason) emit('stream.closed', { id, reason });
}

export function currentUser(): string | null {
  return username;
}

// The websocket client looks the user up with the library's getUser (the homepage scrape
// described above). Route it through fetchUser so every path uses the same check.
miscRequests.getUser = async (session: string, signature?: string) => {
  const user = await fetchUser(session, signature ?? '');
  return { id: 0, username: user.username, authToken: user.authToken };
};
