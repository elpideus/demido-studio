// Real-time data from TradingView through TradingView-API
// (https://github.com/Mathieu2301/Tradingview-API).
//
// One websocket client serves everything. It exists only while a session is set: Demido
// requires a TradingView sign-in for live data, and the session cookies (`sessionid`,
// `sessionid_sign`) come from the sign-in window of the app.

import TradingView, { type ChartSession, type Client, type MarketInfos } from '@mathieuc/tradingview';
import miscRequests from '@mathieuc/tradingview/src/miscRequests.js';

import { type Bar, RpcError, emit, log } from './protocol.ts';
import { type TvStore, coverOf } from './store/tv-store.ts';
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

/** Where every bar TradingView sends is kept (set by main.ts; tests run without one). */
let store: TvStore | null = null;

export function useStore(s: TvStore | null): void {
  store = s;
}

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

export const WEBVIEW_USER_AGENT =
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

/** The session cookies for requests to tradingview.com made on the user's behalf. */
export async function sessionCookie(): Promise<string> {
  if (authPending) await authPending.catch(() => undefined);
  if (!credentials) throw new RpcError('NOT_LOGGED_IN', 'Not signed in to TradingView.');
  const { session, signature } = credentials;
  return signature ? `sessionid=${session};sessionid_sign=${signature}` : `sessionid=${session}`;
}

/** The connection being opened, shared by every request that arrives meanwhile. */
let connecting: Promise<Client> | null = null;

const CONNECT_TIMEOUT_MS = 15_000;

async function requireClient(): Promise<Client> {
  if (authPending) await authPending.catch(() => undefined);
  if (!credentials) {
    throw new RpcError('NOT_LOGGED_IN', 'Not signed in to TradingView.');
  }
  if (client && client.isOpen) return client;
  if (!connecting) {
    connecting = connect().finally(() => {
      connecting = null;
    });
  }
  return connecting;
}

/**
 * Opens the websocket and waits until it is up. TradingView's data host resolves to several
 * servers; one can be unreachable, so a failed attempt is retried (the next lookup usually
 * lands elsewhere).
 */
async function connect(): Promise<Client> {
  if (client) closeClient();
  let lastError = '';
  for (let attempt = 1; attempt <= 3; attempt += 1) {
    if (!credentials) throw new RpcError('NOT_LOGGED_IN', 'Not signed in to TradingView.');
    const created = new TradingView.Client({
      token: credentials.session,
      signature: credentials.signature,
      // The client looks the user up itself; point it at the page that carries the token.
      location: USER_PAGE,
    });
    try {
      await new Promise<void>((resolve, reject) => {
        const timer = setTimeout(() => reject(new Error('timed out')), CONNECT_TIMEOUT_MS);
        created.onConnected(() => {
          clearTimeout(timer);
          resolve();
        });
        created.onError((...err: unknown[]) => {
          const text = err.map(String).join(' ');
          log('warn', `TradingView: ${text}`);
          if (/credentials error/i.test(text)) {
            credentials = null;
            username = null;
            emit('auth', { loggedIn: false, expired: true });
            clearTimeout(timer);
            reject(new RpcError('SESSION_EXPIRED', 'TradingView did not accept the session. Sign in again.'));
          } else if (/websocket error/i.test(text)) {
            clearTimeout(timer);
            reject(new Error(text));
          }
        });
      });
    } catch (error) {
      created.end().catch(() => undefined);
      if (error instanceof RpcError) throw error;
      lastError = error instanceof Error ? error.message : String(error);
      log('warn', `connecting to TradingView failed (attempt ${attempt}): ${lastError}`);
      await new Promise((r) => setTimeout(r, 600 * attempt));
      continue;
    }
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
  throw new RpcError('NETWORK', `Could not connect to TradingView (${lastError}). Check the internet connection.`);
}

/** Drops a client whose socket died, so the next request opens a fresh one. */
function dropIfDead(c: Client): void {
  if (client === c && !c.isOpen) closeClient();
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

/**
 * The filters TradingView's search accepts, and the names people (and models) use for them:
 * gold and oil are listed as `cfd`. `funds` returns ETFs and mutual funds together, `etf` and
 * `mutual_fund` one kind each; `fund` is not a filter TradingView knows.
 */
const SEARCH_TYPES: Record<string, string> = {
  stock: 'stock',
  forex: 'forex',
  crypto: 'crypto',
  index: 'index',
  futures: 'futures',
  cfd: 'cfd',
  commodity: 'cfd',
  fund: 'funds',
  funds: 'funds',
  etf: 'etf',
  mutual_fund: 'mutual_fund',
  bond: 'bond',
  economic: 'economic',
};

/** The TradingView search filter for an asset class name, or '' (every class) when there is none. */
export function searchFilter(type?: string): string {
  // An unknown filter makes TradingView answer with an error the library cannot read.
  return (type && SEARCH_TYPES[type.toLowerCase()]) || '';
}

/** Symbol search. Works without signing in (it is a public endpoint). */
export async function search(query: string, type?: string): Promise<SymbolMatch[]> {
  if (!query.trim()) return [];
  const results = await TradingView.searchMarketV3(query.trim(), searchFilter(type));
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

function toBar(p: { time: number; open: number; max: number; min: number; close: number; volume: number | null }): Bar {
  // Some brokers (Saxo forex CFDs, for one) report no volume at all — as NaN, not null or
  // undefined, so `??` doesn't catch it; it then serializes to JSON `null` and the chart's
  // histogram series throws on that. Treat anything non-finite as "no data" (zero).
  return { t: p.time, o: p.open, h: p.max, l: p.min, c: p.close, v: Number.isFinite(p.volume) ? p.volume! : 0 };
}

/** Bars of a chart session, oldest first. */
function barsOf(chart: ChartSession): Bar[] {
  return chart.periods.map(toBar).sort((a, b) => a.t - b.t);
}

const nowSec = () => Math.floor(Date.now() / 1000);

/** Keeps what a chart session returned, and which symbol it answered for. */
function keep(
  asked: string,
  info: ChartInfo,
  tf: Timeframe,
  bars: readonly Bar[],
  cover: [number, number] | null,
): void {
  if (!store) return;
  store.setAlias(asked, info.symbol);
  store.setInfo(info.symbol, info);
  store.record(info.symbol, tf, bars, cover);
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
    const ready = settle(chart, 30_000);
    chart.setMarket(symbol.trim().toUpperCase(), {
      timeframe: TIMEFRAMES[tf].tradingview,
      range: Math.min(count, PAGE),
      ...(to ? { to } : {}),
    });
    await ready.catch((error: unknown) => {
      dropIfDead(c);
      throw error;
    });
    let bars = barsOf(chart);
    while (bars.length < count) {
      const before = bars.length;
      const more = settle(chart, 20_000);
      chart.fetchMore(Math.min(PAGE, count - bars.length));
      await more.catch(() => undefined);
      bars = barsOf(chart);
      if (bars.length <= before) break; // History exhausted.
    }
    const info = infoOf(chart.infos, symbol);
    // Everything the session fetched is contiguous and worth keeping, not just what was asked for.
    keep(symbol, info, tf, bars, coverOf(bars, tf, nowSec()));
    return { info, bars: bars.slice(-count) };
  } finally {
    chart.delete();
  }
}

/**
 * Runs `job` on a chart session of its own, opened on the newest `count` bars and deleted after:
 * a scratch chart for work that must not touch the live ones (testing a script, say).
 */
export async function withChart<T>(
  symbol: string,
  tf: Timeframe,
  count: number,
  job: (chart: ChartSession, info: ChartInfo, bars: Bar[]) => Promise<T>,
): Promise<T> {
  const c = await requireClient();
  const chart = new c.Session.Chart();
  try {
    const ready = settle(chart, 30_000);
    chart.setMarket(symbol.trim().toUpperCase(), {
      timeframe: TIMEFRAMES[tf].tradingview,
      range: Math.min(count, PAGE),
    });
    await ready.catch((error: unknown) => {
      dropIfDead(c);
      throw error;
    });
    const bars = barsOf(chart);
    const info = infoOf(chart.infos, symbol);
    keep(symbol, info, tf, bars, coverOf(bars, tf, nowSec()));
    return await job(chart, info, bars);
  } finally {
    try {
      chart.delete();
    } catch {
      // the socket is already gone
    }
  }
}

/**
 * Pages back through TradingView's whole history for one timeframe (a download job), storing every
 * page as it lands. Stops after two consecutive empty pages (the start of TradingView's history,
 * recorded as `reachedStart`) or when `signal` aborts. `to` starts the walk at an older bar; `stopAt` ends it
 * where the pages reach back to stored bars (a hole's lower edge; see walkBack).
 */
export async function pageHistory(
  symbol: string,
  tf: Timeframe,
  opts: {
    to?: number;
    /** Stop once the pages reach back to this time (a hole's lower edge, where stored bars go on). */
    stopAt?: number;
    signal?: AbortSignal;
    onPage?: (bars: number, seconds: number) => void;
  } = {},
): Promise<{ info: ChartInfo; reachedStart: boolean; oldest: number | null }> {
  const c = await requireClient();
  const chart = new c.Session.Chart();
  try {
    const started = Date.now();
    const ready = settle(chart, 30_000);
    chart.setMarket(symbol.trim().toUpperCase(), {
      timeframe: TIMEFRAMES[tf].tradingview,
      range: PAGE,
      ...(opts.to ? { to: opts.to } : {}),
    });
    await ready.catch((error: unknown) => {
      dropIfDead(c);
      throw error;
    });
    const info = infoOf(chart.infos, symbol);
    const walked = await walkBack(
      {
        first: barsOf(chart),
        firstSeconds: (Date.now() - started) / 1000,
        async more() {
          const more = settle(chart, 10_000);
          chart.fetchMore(PAGE);
          await more.catch(() => undefined);
          return barsOf(chart);
        },
      },
      tf,
      { ...opts, keep: (bars, cover) => keep(symbol, info, tf, bars, cover) },
    );
    if (walked.reachedStart && store) store.setReachedStart(info.symbol, tf, walked.oldest ?? nowSec());
    return { info, ...walked };
  } finally {
    chart.delete();
  }
}

/** A chart session seen as pages: its first answer, and every bar it holds after one more page. */
export interface PageSource {
  first: readonly Bar[];
  firstSeconds: number;
  more(): Promise<readonly Bar[]>;
}

/**
 * The paging walk of pageHistory: stores each page (as one contiguous covered run with the page
 * above it) and stops after two consecutive empty pages (`reachedStart`), once a page reaches back
 * to `stopAt`, or when `signal` aborts. A walk with `stopAt` never reports a start: running dry
 * above a hole's lower edge only means TradingView serves nothing older from there.
 * A walk started at `to` covers its first page up to `to`: TradingView serves the bars before `to`
 * (exclusive), so nothing lies between the page's last bar and `to`, even across a market closure the
 * gap tolerance does not know (a stock's night), and the page joins the run stored above it.
 */
export async function walkBack(
  source: PageSource,
  tf: Timeframe,
  opts: {
    to?: number;
    stopAt?: number;
    signal?: AbortSignal;
    onPage?: (bars: number, seconds: number) => void;
    keep: (bars: readonly Bar[], cover: [number, number] | null) => void;
  },
): Promise<{ reachedStart: boolean; oldest: number | null }> {
  const first = source.first;
  const now = nowSec();
  const cover = coverOf(first, tf, now);
  const top = opts.to !== undefined && opts.to <= now && first[0] && opts.to > first[0].t ? opts.to : null;
  opts.keep(first, top !== null ? [first[0]!.t, Math.max(cover?.[1] ?? top, top)] : cover);
  opts.onPage?.(first.length, source.firstSeconds);
  let oldest = first[0]?.t ?? null;
  let empties = 0;
  const met = () => opts.stopAt !== undefined && oldest !== null && oldest <= opts.stopAt;
  while (empties < 2 && !met() && !opts.signal?.aborted) {
    const started = Date.now();
    const all = await source.more();
    if (opts.signal?.aborted) break;
    const older = oldest === null ? [...all] : all.filter((b) => b.t < oldest!);
    if (older.length === 0) {
      empties += 1;
    } else {
      empties = 0;
      // Contiguous with the page before it, so the whole walk is one covered run.
      const end = oldest ?? coverOf(older, tf, nowSec())?.[1] ?? older[older.length - 1]!.t;
      opts.keep(older, [older[0]!.t, end]);
      oldest = older[0]!.t;
    }
    opts.onPage?.(older.length, (Date.now() - started) / 1000);
  }
  return { reachedStart: empties >= 2 && opts.stopAt === undefined, oldest };
}

interface Stream {
  chart: ChartSession;
  symbol: string;
  tf: Timeframe;
  lastSent: string;
  /** First bar of the opening snapshot: a live stream covers everything from there on. */
  coveredFrom: number | null;
  /** Time of the newest bar seen, to notice a rollover. */
  lastT: number | null;
}

const streams = new Map<string, Stream>();
let nextStream = 1;
const closedListeners: ((id: string) => void)[] = [];

/** The chart session of a live stream (indicators run on it), or null once it closed. */
export function streamChart(id: string): ChartSession | null {
  return streams.get(id)?.chart ?? null;
}

/** Calls `cb` with the id of every stream that closes, whatever the reason. */
export function onStreamClosed(cb: (id: string) => void): void {
  closedListeners.push(cb);
}

/** Most bars one chart session holds. */
const STREAM_MAX_BARS = PAGE;

/**
 * Loads `count` more history bars into a live stream's session, so the indicators running on it
 * reach further back. Returns how many it asked for (0 once the session is full).
 */
export function extendStream(id: string, count: number): number {
  const stream = streams.get(id);
  if (!stream) throw new RpcError('NO_STREAM', 'That chart is no longer live.');
  const room = STREAM_MAX_BARS - stream.chart.periods.length;
  const asked = Math.max(0, Math.min(count, room));
  if (asked > 0) stream.chart.fetchMore(asked);
  return asked;
}

/** Opens a live chart: returns the first bars; `stream.update` events carry every change. */
export async function openStream(
  symbol: string,
  tf: Timeframe,
  count: number,
): Promise<{ id: string; info: ChartInfo; bars: Bar[] }> {
  const c = await requireClient();
  const chart = new c.Session.Chart();
  const ready = settle(chart, 30_000);
  chart.setMarket(symbol.trim().toUpperCase(), {
    timeframe: TIMEFRAMES[tf].tradingview,
    range: Math.min(count, PAGE),
  });
  try {
    await ready;
  } catch (error) {
    chart.delete();
    dropIfDead(c);
    throw error;
  }
  const info = infoOf(chart.infos, symbol);
  const bars = barsOf(chart);
  keep(symbol, info, tf, bars, coverOf(bars, tf, nowSec()));
  const id = `s${nextStream++}`;
  // The canonical symbol (not whatever spelling the caller used) keys the store, so re-opening the
  // same chart from a different alias still finds what was stored.
  const stream: Stream = {
    chart,
    symbol: info.symbol,
    tf,
    lastSent: '',
    coveredFrom: bars[0]?.t ?? null,
    lastT: bars[bars.length - 1]?.t ?? null,
  };
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
      if (stream.lastT !== null && bar.t > stream.lastT) {
        // Rollover: the bar that just closed may have ticked in the last 200 ms; keep its final state.
        const closed = chart.periods[1] ? toBar(chart.periods[1]) : null;
        if (closed && closed.t === stream.lastT) {
          store?.live(stream.symbol, tf, closed);
          emit('stream.update', { id, bar: closed });
        }
        stream.coveredFrom ??= stream.lastT;
        store?.cover(stream.symbol, tf, stream.coveredFrom, bar.t);
      }
      stream.coveredFrom ??= bar.t;
      stream.lastT = Math.max(stream.lastT ?? bar.t, bar.t);
      const key = JSON.stringify(bar);
      if (key === stream.lastSent) return;
      stream.lastSent = key;
      store?.live(stream.symbol, tf, bar);
      emit('stream.update', { id, bar });
    }, 200);
  });
  chart.onError((...err: unknown[]) => {
    emit('stream.error', { id, message: err.map(String).join(' ') });
  });
  return { id, info, bars };
}

export function closeStream(id: string, reason?: string): void {
  const stream = streams.get(id);
  if (!stream) return;
  streams.delete(id);
  for (const cb of closedListeners) cb(id);
  try {
    stream.chart.delete();
  } catch {
    // the socket is already gone
  }
  void store?.flush().catch(() => undefined);
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
