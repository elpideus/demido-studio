// MetaTrader 5 as the fetcher's second source of 1-minute days (store/fetcher.ts AlternateSource),
// through the bridge (metatrader/bridge.py) the app installs. The app sends what the user chose
// (`configure`): the mode, the terminal and the account. Nothing starts before they consented: the
// app sends mode 'off' until then, and only `connect` (their "Test connection") opens a session.
//
// Session (one at a time; opened by the first chunk, closed after 5 min without one):
//   start the bridge -> the terminal is running, or is launched minimized without focus -> connect:
//   attach and make the chosen account the active one with the password the terminal saved (it may
//   switch from another account; closing the bridge switches back) -> map the broker's symbols to
//   instruments (metatrader/symbols) -> the server's clock (metatrader/clock), from a year of H1
//   weekly opens and a fresh tick of a currency pair, kept per server for 30 days. A session that
//   fails cools MetaTrader down 2 min, doubling up to 30.
// Chunk: the UTC calendar year around the day asked, up to tomorrow, minus what was already asked
//   since the service started, with the store's covered head and tail trimmed, at least 3 s after
//   the previous one, from one place:
//   - the terminal, within its reach: it answers only from its newest Max bars in chart bars and
//     refuses a range reaching further, so from as many days back as Max bars has minutes (no
//     market has more bars a day), less the margin, or from where its history was found cut. One M1
//     request (4 days' margin each side, server times), a piece Max bars allows; what it did not
//     answer at the start of the range (cut or empty, closed days) is read from its files at once;
//   - before that, its history files on disk (bridge `history`, a year a chunk): the terminal is
//     asked nothing and writes nothing. A bridge or a layout that cannot read them: not tried again.
//   The terminal rewrites its history files when asked for the same ranges over and over, so a
//   range is asked once: a day of it that neither knew goes back to Dukascopy (in 'only' mode it
//   stays missing), and a day asked already is answered at once, without a session.
// A day of the range is:
//   bars  - it has them (in UTC), unless the day is before `trust`: the day after the first bar when
//           no bar came before the range (the history was cut or starts there);
//   empty - no bars, with history before it, and bars or quotes after it, or a connected terminal
//           whose last quote is at most 4 days before it (a weekend or a holiday, not a dead
//           symbol); a currency pair's weekday (but Christmas and New Year) is never empty;
//   unknown otherwise: left out, so Dukascopy is asked (unless MetaTrader is the only source) and
//           nothing MetaTrader lacks is recorded empty. A disconnected terminal's days from its last
//           bar on are unknown.
//   Final once closed for a minute, provisional until then.
// The first chunk of each instrument is checked against Dukascopy's nearest stored day: prices
//   outside a factor of 1.33 (2 when further than 30 days) mean the broker's symbol is something
//   else, and MetaTrader is not used for that instrument.
// Write guard: the terminal's disk writes during each request (Linux: /proc of its Wine process;
//   Windows: through the bridge). Above 20 MB/s over a request of 5 s or more MetaTrader pauses
//   15 min; above 2 GB in all it stops until it is set up again.
//
// Modes: 'fallback', 'preferred' and 'only' (Dukascopy never asked) as the fetcher reads them; 'only'
//   holds while the setup is incomplete (the bridge still installing), so nothing goes to Dukascopy
//   meanwhile: `blocked` says why it waits.
//
// API: new MetaTraderSource({store, cacheDir, log, onStatus, onReady}); configure(config) -> status;
//      status(); connect() (opens a session now) -> status; close(); parseConfig(json).
//      AlternateSource: name, mode(), serves(instrument), lacks(instrument), blocked(), readyAt(),
//      fetchChunk(instrument, start, signal)

import fs from 'node:fs';
import path from 'node:path';

import { instrumentInfo } from '../dukascopy.ts';
import { type Bar, RpcError } from '../protocol.ts';
import { bucketKey } from '../store/buckets.ts';
import { CLOSE_GRACE_SECONDS, type DukascopyStore, writeAtomic } from '../store/dukascopy-store.ts';
import { type AlternateChunk, type AlternateDay, type AlternateMode, type AlternateSource } from '../store/fetcher.ts';
import { IntervalSet, type Range } from '../store/intervals.ts';
import { Bridge, BridgeError, type LaunchSpec } from './client.ts';
import {
  type Evidence,
  MODELS,
  type ModelChoice,
  type ZoneModel,
  chooseModel,
  tickEvidence,
  toServer,
  toUtc,
  weekOf,
  weekShifts,
  weeklyOpens,
} from './clock.ts';
import { byDay, encodeDay } from './encode.ts';
import { procWrites } from './guard.ts';
import { type BrokerSymbol, mapSymbols } from './symbols.ts';

export interface MetaTraderConfig {
  /** What the user chose; 'off' until they consented. */
  mode: AlternateMode;
  /** How to start the bridge; null until it is installed. */
  bridge: LaunchSpec | null;
  /** terminal64.exe as the bridge sees it (a Windows path, under Wine too). */
  terminal: string;
  /** Extra arguments when Demido launches it (`/portable` for a portable install). */
  terminalArgs?: string[];
  login: number;
  server: string;
  /** May log the terminal into this account when it is on another (default true). */
  allowSwitch?: boolean;
}

export type MetaTraderState = 'off' | 'idle' | 'starting' | 'connecting' | 'ready' | 'downloading' | 'paused' | 'error';

export interface MetaTraderStatus {
  mode: AlternateMode;
  state: MetaTraderState;
  message: string | null;
  account: { login: number; server: string; name: string; company: string; currency: string } | null;
  terminal: { build: number | null; maxBars: number | null; connected: boolean } | null;
  clock: { model: ZoneModel; assumed: boolean; fit: number; of: number } | null;
  /** The broker's symbols that are Demido markets (null before the first connection). */
  markets: number | null;
  /** Days it answered since the service started. */
  days: number;
  /** When it is used again after a failure or a pause (ms). */
  retryAt: number | null;
  /** The terminal was on another account and was logged into this one. */
  switched: boolean;
  /** Windows: the terminal Demido launched closes with Demido (it could not leave Demido's job). */
  tied: boolean;
}

export interface BridgeLike {
  readonly alive: boolean;
  call<T = unknown>(method: string, params: Record<string, unknown>, timeoutMs: number): Promise<T>;
  close(graceMs?: number): Promise<void>;
}

export interface MetaTraderOptions {
  store: DukascopyStore;
  cacheDir: string;
  log?: (level: 'info' | 'warn' | 'error', message: string) => void;
  onStatus?: (status: MetaTraderStatus) => void;
  /** It can take days again after a cool-down or a pause, or its setup changed (the fetcher should
   *  look). */
  onReady?: () => void;
  /** Clock in ms (tests). */
  now?: () => number;
  /** Tests: another bridge. */
  bridge?: (spec: LaunchSpec) => BridgeLike;
  /** Tests: bytes the terminal wrote so far (null: not measurable). */
  writes?: () => Promise<number | null>;
}

interface Account {
  login: number;
  server: string;
  name: string;
  company: string;
  currency: string;
}

interface ConnectAnswer {
  account: (Account & { trade_mode?: number }) | null;
  terminal: { connected?: boolean; build?: number; maxbars?: number } | null;
  switched: boolean;
}

interface RatesAnswer {
  time: number[];
  open: number[];
  high: number[];
  low: number[];
  close: number[];
  volume: number[];
  /** Server time of the symbol's last quote (null from the files). */
  last: number | null;
  connected: boolean;
  digits: number;
}

/** Days of a chunk and the bars they came from. */
interface Answered {
  days: Map<number, AlternateDay>;
  bars: Bar[];
}

/** The bridge cannot read the terminal's history files: an older bridge, or another layout. */
const NO_FILES = new Set(['UNKNOWN_METHOD', 'HISTORY_LAYOUT', 'NO_HISTORY']);

interface Known {
  account: Account;
  terminal: MetaTraderStatus['terminal'];
  clock: ModelChoice;
  switched: boolean;
  tied: boolean;
}

interface Session extends Known {
  key: string;
  bridge: BridgeLike;
  /** The terminal's process id as the bridge sees it (for its I/O counters on Windows). */
  pid: number | null;
}

const DAY = 86400;
const PACE_MS = 3000;
const IDLE_MS = 5 * 60_000;
const COOL_MIN_MS = 2 * 60_000;
const COOL_MAX_MS = 30 * 60_000;
const MARGIN = 4 * DAY;
/** A connected terminal's last quote this close before a day without bars: a weekend or holiday. */
const QUIET = 4 * DAY;
const RATES_TIMEOUT_MS = 180_000;
const CLOCK_MAX_AGE_MS = 30 * DAY * 1000;
const CLOCK_VERSION = 1;
const WRITE_RATE_LIMIT = 20 * 1024 * 1024;
const WRITE_TOTAL_LIMIT = 2 * 1024 ** 3;
const WRITE_PAUSE_MS = 15 * 60_000;
const METALS = new Set(['XAU', 'XAG', 'XPT', 'XPD']);

const dayOf = (t: number) => Math.floor(t / DAY) * DAY;
const messageOf = (error: unknown) => (error instanceof Error ? error.message : String(error));

/** A pair of two currencies (not a metal): its week opens Sunday 17:00 New York. */
function isCurrencyPair(instrument: string): boolean {
  const m = /^([A-Z]{3})\/([A-Z]{3})$/.exec(instrumentInfo(instrument).name);
  return m !== null && !METALS.has(m[1]!) && !METALS.has(m[2]!);
}

/** A day a currency pair may have no bars on: the weekend, Christmas, New Year's Day. */
function quietDay(day: number): boolean {
  const d = new Date(day * 1000);
  const weekday = d.getUTCDay();
  if (weekday === 0 || weekday === 6) return true;
  const md = d.getUTCMonth() * 100 + d.getUTCDate();
  return md === 1125 || md === 1;
}

function median(values: number[]): number {
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.floor(sorted.length / 2)] ?? Number.NaN;
}

/** The config the app sent, or null when it is not one. */
export function parseConfig(json: unknown): MetaTraderConfig | null {
  if (!json || typeof json !== 'object') return null;
  const c = json as Record<string, unknown>;
  const mode = c.mode === 'fallback' || c.mode === 'preferred' || c.mode === 'only' ? c.mode : 'off';
  const b = c.bridge as Record<string, unknown> | null | undefined;
  const bridge: LaunchSpec | null =
    b && typeof b === 'object' && typeof b.program === 'string' && Array.isArray(b.args)
      ? {
          program: b.program,
          args: b.args.map(String),
          env:
            b.env && typeof b.env === 'object'
              ? Object.fromEntries(Object.entries(b.env as Record<string, unknown>).map(([k, v]) => [k, String(v)]))
              : undefined,
          cwd: typeof b.cwd === 'string' ? b.cwd : undefined,
        }
      : null;
  const login = Number(c.login);
  return {
    mode,
    bridge,
    terminal: typeof c.terminal === 'string' ? c.terminal : '',
    terminalArgs: Array.isArray(c.terminalArgs) ? c.terminalArgs.map(String) : [],
    login: Number.isSafeInteger(login) && login > 0 ? login : 0,
    server: typeof c.server === 'string' ? c.server.trim() : '',
    allowSwitch: c.allowSwitch !== false,
  };
}

function keyOf(c: MetaTraderConfig): string {
  return JSON.stringify([c.bridge, c.terminal, c.terminalArgs ?? [], c.login, c.server, c.allowSwitch ?? true]);
}

function complete(c: MetaTraderConfig | null): c is MetaTraderConfig & { bridge: LaunchSpec } {
  return c !== null && c.bridge !== null && c.terminal !== '' && c.login > 0 && c.server !== '';
}

/** Rejects when `signal` aborts first (the call itself goes on; the bridge answers into the void). */
function abortable<T>(promise: Promise<T>, signal: AbortSignal): Promise<T> {
  if (signal.aborted) return Promise.reject(signal.reason);
  return new Promise<T>((resolve, reject) => {
    const onAbort = () => reject(signal.reason);
    signal.addEventListener('abort', onAbort, { once: true });
    promise.then(
      (value) => {
        signal.removeEventListener('abort', onAbort);
        resolve(value);
      },
      (error: unknown) => {
        signal.removeEventListener('abort', onAbort);
        reject(error);
      },
    );
  });
}

export class MetaTraderSource implements AlternateSource {
  readonly name = 'MetaTrader';
  readonly #store: DukascopyStore;
  readonly #clockFile: string;
  readonly #log: NonNullable<MetaTraderOptions['log']>;
  readonly #onStatus: MetaTraderOptions['onStatus'];
  readonly #onReady: MetaTraderOptions['onReady'];
  readonly #now: () => number;
  readonly #makeBridge: (spec: LaunchSpec) => BridgeLike;
  readonly #writesOption: MetaTraderOptions['writes'];

  #config: MetaTraderConfig | null = null;
  #key = '';
  #session: Session | null = null;
  /** The last session's account, terminal and clock (shown after it closed). */
  #known: Known | null = null;
  #opening: Promise<Session> | null = null;
  #closing: Promise<void> = Promise.resolve();
  #inUse = 0;
  #state: MetaTraderState = 'idle';
  #message: string | null = null;
  #nextAt = 0;
  #coolUntil = 0;
  #coolMs = 0;
  #halted: string | null = null;
  #written = 0;
  #days = 0;
  #hostWrites: () => Promise<number | null> = async () => null;
  /** Per instrument, what was asked (UTC); never asked again while the config stays. */
  #asked = new Map<string, IntervalSet>();
  /** Per instrument, the first day the terminal answers (its history was cut or starts there). */
  #floor = new Map<string, number>();
  /** Why the terminal's history files cannot be read (not tried again while the config stays). */
  #noFiles: string | null = null;
  #markets: Map<string, BrokerSymbol> | null = null;
  /** Instrument -> why it is not served (no such symbol, or one with another market's prices). */
  #unserved = new Map<string, string>();
  #checked = new Set<string>();
  #idleTimer: ReturnType<typeof setTimeout> | undefined;
  #readyTimer: ReturnType<typeof setTimeout> | undefined;
  #lastStatus = '';
  #closed = false;

  constructor(opts: MetaTraderOptions) {
    this.#store = opts.store;
    this.#clockFile = path.join(opts.cacheDir, 'metatrader', 'clock.json');
    this.#log = opts.log ?? (() => {});
    this.#onStatus = opts.onStatus;
    this.#onReady = opts.onReady;
    this.#now = opts.now ?? Date.now;
    this.#makeBridge = opts.bridge ?? ((spec) => new Bridge(spec, { log: this.#log }));
    this.#writesOption = opts.writes;
  }

  // -------------------------------------------------------------------------------------------
  // What the app sees

  /** A new config; another terminal or account starts over (the open session closes first). */
  async configure(config: MetaTraderConfig | null): Promise<MetaTraderStatus> {
    const key = config ? keyOf(config) : '';
    this.#config = config;
    if (key !== this.#key) {
      this.#key = key;
      await this.#closeSession();
      this.#asked.clear();
      this.#floor.clear();
      this.#noFiles = null;
      this.#markets = null;
      this.#known = null;
      this.#unserved.clear();
      this.#checked.clear();
      this.#coolUntil = 0;
      this.#coolMs = 0;
      this.#halted = null;
      this.#written = 0;
      this.#state = 'idle';
      this.#message = null;
      if (config) {
        const exe = config.terminal.toLowerCase().replace(/\//g, '\\');
        const base = exe.split('\\').pop() ?? 'terminal64.exe';
        // Wine names the process by its Windows path.
        this.#hostWrites =
          this.#writesOption ??
          procWrites((cmdline) => {
            const c = cmdline.toLowerCase().replace(/\//g, '\\');
            return c.includes(exe) || c.includes(base);
          });
      }
    }
    this.#emit();
    // Even when it cannot serve: a new mode, or why it waits, is the fetcher's to know.
    this.#onReady?.();
    return this.status();
  }

  status(): MetaTraderStatus {
    const s = this.#session ?? this.#known;
    const now = this.#now();
    const retryAt = this.#coolUntil > now ? this.#coolUntil : null;
    let state = this.#state;
    let message = this.#message;
    if (this.#halted) {
      state = 'error';
      message = this.#halted;
    } else if (this.mode() === 'off' && !this.#session && !this.#opening) {
      state = 'off';
    } else if (retryAt === null && (state === 'error' || state === 'paused')) {
      state = this.#session ? 'ready' : 'idle';
      message = null;
    }
    return {
      mode: this.mode(),
      state,
      message,
      account: s ? { ...s.account } : null,
      terminal: s?.terminal ? { ...s.terminal } : null,
      clock: s ? { model: s.clock.model, assumed: s.clock.assumed, fit: s.clock.fit, of: s.clock.of } : null,
      markets: this.#markets?.size ?? null,
      days: this.#days,
      retryAt,
      switched: s?.switched ?? false,
      tied: s?.tied ?? false,
    };
  }

  /** Opens a session now (the user's test of the connection), whatever the mode and a cool-down. */
  async connect(): Promise<MetaTraderStatus> {
    this.#coolUntil = 0;
    this.#inUse += 1;
    try {
      await this.#open();
    } finally {
      this.#inUse -= 1;
      this.#touch();
    }
    if (this.serves('')) this.#onReady?.();
    return this.status();
  }

  async close(): Promise<void> {
    this.#closed = true;
    clearTimeout(this.#idleTimer);
    clearTimeout(this.#readyTimer);
    await this.#closeSession();
  }

  // -------------------------------------------------------------------------------------------
  // AlternateSource

  mode(): AlternateMode {
    const mode = this.#config?.mode ?? 'off';
    // The only source waits for its setup rather than letting Dukascopy download meanwhile.
    return mode === 'only' || complete(this.#config) ? mode : 'off';
  }

  serves(instrument: string): boolean {
    if (this.mode() === 'off' || this.blocked() !== null) return false;
    return !instrument || this.lacks(instrument) === null;
  }

  lacks(instrument: string): string | null {
    const reason = this.#unserved.get(instrument);
    if (reason) return reason;
    if (!this.#markets || this.#markets.has(instrument)) return null;
    const server = (this.#session ?? this.#known)?.account.server ?? this.#config?.server ?? 'The account';
    return `${server} has no symbol for ${instrumentInfo(instrument).name}`;
  }

  blocked(): string | null {
    if (this.#closed) return 'MetaTrader is closed';
    if (this.#halted) return this.#halted;
    const c = this.#config;
    if (!complete(c)) {
      if (!c || c.terminal === '') return 'The MetaTrader terminal chosen is not on this computer';
      if (c.login <= 0 || c.server === '') return 'Choose a MetaTrader account';
      return "MetaTrader's bridge is not installed yet";
    }
    const now = this.#now();
    if (now >= this.#coolUntil) return null;
    // A pause says how long itself; a failure says what failed.
    if (this.#state === 'paused' && this.#message) return this.#message;
    const wait = Math.max(1, Math.ceil((this.#coolUntil - now) / 60_000));
    return `MetaTrader: ${this.#message ?? 'it failed'}; trying again in ${wait} min`;
  }

  readyAt(): number {
    return Math.max(this.#nextAt, this.#coolUntil);
  }

  async fetchChunk(instrument: string, start: number, signal: AbortSignal): Promise<AlternateChunk> {
    this.#inUse += 1;
    try {
      return await this.#chunk(instrument, start, signal);
    } finally {
      this.#inUse -= 1;
      this.#touch();
    }
  }

  // -------------------------------------------------------------------------------------------
  // Chunks

  async #chunk(instrument: string, start: number, signal: AbortSignal): Promise<AlternateChunk> {
    // Asked already: nothing new, and no reason to start the terminal for it.
    if (this.#askedOf(instrument).contains(start)) return { range: [start, start + DAY], days: new Map() };
    const session = await abortable(this.#open(), signal);
    const symbol = this.#markets?.get(instrument);
    if (!symbol) {
      const message = `${session.account.server} has no symbol for ${instrumentInfo(instrument).name}`;
      this.#unserved.set(instrument, message);
      throw new RpcError('NO_SYMBOL', message);
    }
    const nowSec = Math.floor(this.#now() / 1000);
    const plan = this.#rangeFor(instrument, start, nowSec, this.#reachOf(instrument, session, nowSec));
    if (!plan) return { range: [start, start + DAY], days: new Map() };
    const [from, to] = plan.range;
    const closed = dayOf(nowSec - 2 * CLOSE_GRACE_SECONDS);
    let answered: Answered;
    if (plan.files) {
      answered = await this.#fromFiles(instrument, symbol, session, from, to, nowSec, signal);
    } else {
      const got = await this.#fromTerminal(instrument, symbol, session, from, to, nowSec, signal);
      answered = got;
      // What it did not answer at the start of the range, from its files.
      const filesTo = Math.min(got.trust, closed);
      if (filesTo > from && this.serves(instrument)) {
        const older = await this.#fromFiles(instrument, symbol, session, from, filesTo, nowSec, signal);
        answered = { days: new Map([...older.days, ...got.days]), bars: [...older.bars, ...got.bars] };
      }
    }
    const { days, bars } = answered;

    // Asked: every day closed (for a couple of minutes) when it was asked.
    const askedTo = Math.min(to, closed);
    if (askedTo > from) this.#askedOf(instrument).add(from, askedTo);

    if (days.size > 0 && !this.#checked.has(instrument)) {
      await this.#checkPrices(instrument, symbol, session, days, bars);
      this.#checked.add(instrument);
    }
    this.#days += days.size;
    this.#emit();
    return { range: plan.range, days };
  }

  /** One M1 request to the terminal; `trust`: its first day answered (`to` when none). */
  async #fromTerminal(
    instrument: string,
    symbol: BrokerSymbol,
    session: Session,
    from: number,
    to: number,
    nowSec: number,
    signal: AbortSignal,
  ): Promise<Answered & { trust: number }> {
    const model = session.clock.model;
    const before = await this.#writesNow(session);
    const startedAt = this.#now();
    this.#set('downloading', `Downloading ${symbol.name}, ${bucketKey('m1', from)} to ${bucketKey('m1', to - DAY)}`);
    const answer = await this.#ask(instrument, session, 'rates', signal, {
      symbol: symbol.name,
      from: toServer(model, from - MARGIN),
      to: toServer(model, Math.min(to + MARGIN, nowSec + DAY)),
      timeframe: 'M1',
      pause: 5,
    });
    this.#guard(before, await this.#writesNow(session), this.#now() - startedAt);
    if (this.#state === 'downloading') this.#set('ready', null);
    const bars = this.#toUtc(instrument, model, answer);
    const { days, trust } = this.#classify(instrument, from, to, nowSec, bars, answer, model);
    // Bars, none before the range: Max bars in chart cut its history there (or it starts there).
    if (bars.length > 0 && trust > from) {
      this.#floor.set(instrument, Math.max(this.#floor.get(instrument) ?? -Infinity, trust));
    }
    return { days, bars, trust };
  }

  /** Days of [from, to) from the terminal's history files; none when they cannot be read. */
  async #fromFiles(
    instrument: string,
    symbol: BrokerSymbol,
    session: Session,
    from: number,
    to: number,
    nowSec: number,
    signal: AbortSignal,
  ): Promise<Answered> {
    if (this.#noFiles) return { days: new Map(), bars: [] };
    const model = session.clock.model;
    this.#set(
      'downloading',
      `Reading ${symbol.name} from MetaTrader's history files, ${bucketKey('m1', from)} to ${bucketKey('m1', to - DAY)}`,
    );
    let answer: RatesAnswer;
    try {
      answer = await this.#ask(instrument, session, 'history', signal, {
        symbol: symbol.name,
        from: toServer(model, from - MARGIN),
        to: toServer(model, Math.min(to + MARGIN, nowSec + DAY)),
      });
    } catch (error) {
      if (!(error instanceof BridgeError && NO_FILES.has(error.code)) || signal.aborted) throw error;
      this.#noFiles = error.message;
      this.#log(
        'warn',
        `MetaTrader: its history files cannot be read (${error.message}); only Max bars in chart is used`,
      );
      this.#set('ready', null);
      return { days: new Map(), bars: [] };
    }
    if (this.#state === 'downloading') this.#set('ready', null);
    const bars = this.#toUtc(instrument, model, answer);
    return { days: this.#classify(instrument, from, to, nowSec, bars, answer, model).days, bars };
  }

  /** A chunk's request to the bridge. A terminal that stopped answering: a new session after the
   *  cool-down; the files' own errors are the caller's. */
  async #ask(
    instrument: string,
    session: Session,
    method: 'rates' | 'history',
    signal: AbortSignal,
    params: Record<string, unknown>,
  ): Promise<RatesAnswer> {
    try {
      return await abortable(session.bridge.call<RatesAnswer>(method, params, RATES_TIMEOUT_MS), signal);
    } catch (error) {
      if (signal.aborted) throw error;
      if (error instanceof BridgeError && error.code === 'NO_SYMBOL') {
        this.#unserved.set(instrument, error.message);
        this.#set('ready', null);
      } else if (!(method === 'history' && error instanceof BridgeError && NO_FILES.has(error.code))) {
        await this.#closeSession();
        this.#cool(error);
      }
      throw error;
    } finally {
      this.#nextAt = this.#now() + PACE_MS;
    }
  }

  #askedOf(instrument: string): IntervalSet {
    let set = this.#asked.get(instrument);
    if (!set) {
      set = new IntervalSet();
      this.#asked.set(instrument, set);
    }
    return set;
  }

  /** The first day the terminal answers: as many days back as Max bars in chart has minutes (no
   *  market has more bars a day), less what a request asks before its range; or where its history
   *  was found cut. */
  #reachOf(instrument: string, session: Session, nowSec: number): number {
    const maxBars = session.terminal?.maxBars ?? 0;
    const byBars = maxBars > 0 ? dayOf(nowSec) - Math.floor(maxBars / 1440) * DAY + MARGIN + DAY : -Infinity;
    return Math.max(byBars, this.#floor.get(instrument) ?? -Infinity);
  }

  /** The days to ask around `start`, from the terminal (from `reach` on) or its files (before), or
   *  null when they are not worth asking. */
  #rangeFor(instrument: string, start: number, nowSec: number, reach: number): { range: Range; files: boolean } | null {
    const asked = this.#askedOf(instrument);
    const year = new Date(start * 1000).getUTCFullYear();
    const first = Date.UTC(year, 0, 1) / 1000;
    const limit = Math.min(Date.UTC(year + 1, 0, 1) / 1000, dayOf(nowSec) + DAY);
    if (start >= limit || asked.contains(start)) return null;
    const files = start < reach;
    if (files && this.#noFiles) return null;
    const gap = asked
      .gaps(files ? first : Math.max(first, reach), files ? Math.min(limit, reach) : limit)
      .find(([f, t]) => f <= start && start < t);
    if (!gap) return null;
    let [from, to] = gap;
    // One request still, with what the store has at either end left out.
    const { covered, unavailable } = this.#store.sets(instrument, 'm1');
    const stored = covered.union(unavailable).subtract(start, start + DAY);
    const head = stored.rangeAt(from);
    if (head) from = Math.min(head[1], start);
    const tail = stored.rangeAt(to - 1);
    if (tail) to = Math.max(tail[0], start + DAY);
    return { range: [from, to], files };
  }

  /** The answer's bars in UTC, ascending, one per minute (a clock change's repeated hour: the first). */
  #toUtc(instrument: string, model: ZoneModel, answer: RatesAnswer): Bar[] {
    // A currency pair's own weekly opens correct the weeks its history ran the other clock.
    const shifts = isCurrencyPair(instrument) ? weekShifts(model, answer.time) : new Map<number, number>();
    const bars: Bar[] = [];
    for (let i = 0; i < answer.time.length; i += 1) {
      const server = answer.time[i]!;
      const shift = shifts.get(weekOf(server));
      const t = shift === undefined ? toUtc(model, server) : server - shift;
      if (t % 60 !== 0) continue;
      bars.push({
        t,
        o: answer.open[i]!,
        h: answer.high[i]!,
        l: answer.low[i]!,
        c: answer.close[i]!,
        v: answer.volume[i]!,
      });
    }
    bars.sort((a, b) => a.t - b.t);
    return bars.filter((b, i) => i === 0 || b.t !== bars[i - 1]!.t);
  }

  #classify(
    instrument: string,
    from: number,
    to: number,
    nowSec: number,
    bars: Bar[],
    answer: RatesAnswer,
    model: ZoneModel,
  ): { days: Map<number, AlternateDay>; trust: number } {
    const days = new Map<number, AlternateDay>();
    const first = bars[0]?.t;
    const last = bars.at(-1)?.t;
    if (first === undefined || last === undefined) return { days, trust: to };
    // Nothing before the range: the history starts here, or Max bars in chart cut it here.
    const trust = first >= from ? dayOf(first) + DAY : from;
    const trustTo = answer.connected ? to : Math.min(to, dayOf(last));
    const lastQuote = answer.last ? toUtc(model, answer.last) : null;
    const fx = isCurrencyPair(instrument);
    const grouped = byDay(bars.filter((b) => b.t >= trust && b.t < trustTo));
    for (let d = trust; d < trustTo && d <= nowSec; d += DAY) {
      const end = d + DAY;
      const dayBars = grouped.get(d);
      if (!dayBars) {
        const after = last >= end || (lastQuote !== null && lastQuote >= end);
        const quiet =
          answer.connected && end <= nowSec && lastQuote !== null && lastQuote < d && d - lastQuote <= QUIET;
        if (!(after || quiet)) continue;
        // A currency pair trades every weekday but two: a weekday gap is the broker's.
        if (fx && !quietDay(d)) continue;
      }
      const final = nowSec - end >= CLOSE_GRACE_SECONDS;
      days.set(d, {
        buffer: dayBars ? encodeDay(dayBars, answer.digits) : null,
        final,
        // Built while open: due again right after the close; just closed: due a minute after it.
        builtAt: final || end > nowSec ? nowSec : end - 1,
      });
    }
    return { days, trust };
  }

  /** The broker's prices against Dukascopy's nearest stored day; throws when they are another market. */
  async #checkPrices(
    instrument: string,
    symbol: BrokerSymbol,
    session: Session,
    days: Map<number, AlternateDay>,
    bars: Bar[],
  ): Promise<void> {
    const { data, metatrader } = this.#store.sets(instrument, 'm1');
    const own = data.minus(metatrader);
    const withBars = [...days].filter(([, d]) => d.buffer !== null).map(([day]) => day);
    const mine = withBars.at(-1);
    if (own.isEmpty || mine === undefined) return;
    let nearest: number | null = null;
    for (const [f, t] of own) {
      const candidate = Math.min(Math.max(mine, dayOf(f)), dayOf(t - 1));
      if (nearest === null || Math.abs(candidate - mine) < Math.abs(nearest - mine)) nearest = candidate;
    }
    if (nearest === null) return;
    const theirs = await this.#store.readNative(instrument, 'm1', nearest, nearest + DAY);
    const reference = median(theirs.map((b) => b.c));
    const price = median(bars.filter((b) => b.t >= mine && b.t < mine + DAY).map((b) => b.c));
    if (!(reference > 0) || !(price > 0)) return;
    const ratio = price / reference;
    const [lo, hi] = Math.abs(nearest - mine) <= 30 * DAY ? [0.75, 1.33] : [0.5, 2];
    if (ratio >= lo && ratio <= hi) return;
    const message = `${symbol.name} on ${session.account.server} is not ${instrumentInfo(instrument).name}: its price is ${ratio.toFixed(2)} times Dukascopy's`;
    this.#unserved.set(instrument, message);
    this.#log('warn', message);
    throw new RpcError('OTHER_MARKET', message);
  }

  // -------------------------------------------------------------------------------------------
  // The write guard

  async #writesNow(session: Session): Promise<number | null> {
    try {
      const host = await this.#hostWrites();
      if (host !== null) return host;
      if (session.pid === null || !session.bridge.alive) return null;
      const io = await session.bridge.call<{ written: number | null }>('io', { pid: session.pid }, 10_000);
      return typeof io?.written === 'number' ? io.written : null;
    } catch {
      return null;
    }
  }

  #guard(before: number | null, after: number | null, ms: number): void {
    if (before === null || after === null || after < before) return;
    const bytes = after - before;
    this.#written += bytes;
    const mb = (n: number) => `${Math.round(n / 1024 / 1024)} MB`;
    if (this.#written > WRITE_TOTAL_LIMIT) {
      this.#halted = `MetaTrader wrote ${mb(this.#written)} to disk while Demido downloaded from it, so Demido stopped using it. Set it up again to resume.`;
      this.#log('warn', this.#halted);
      this.#emit();
    } else if (ms >= 5000 && bytes / (ms / 1000) > WRITE_RATE_LIMIT) {
      this.#coolUntil = this.#now() + WRITE_PAUSE_MS;
      this.#set('paused', `MetaTrader wrote ${mb(bytes)} to disk in ${Math.round(ms / 1000)} s; paused for 15 min`);
      this.#log('warn', this.#message!);
      this.#readyLater();
    }
  }

  // -------------------------------------------------------------------------------------------
  // Sessions

  #open(): Promise<Session> {
    if (this.#session?.bridge.alive && this.#session.key === this.#key) return Promise.resolve(this.#session);
    this.#opening ??= this.#start().finally(() => (this.#opening = null));
    return this.#opening;
  }

  async #start(): Promise<Session> {
    await this.#closing;
    await this.#closeSession();
    const config = this.#config;
    if (!complete(config)) throw new RpcError('NOT_SET_UP', 'MetaTrader is not set up');
    if (this.#halted) throw new RpcError('STOPPED', this.#halted);
    if (this.#closed) throw new RpcError('CLOSED', 'The market service is closing');
    const key = this.#key;
    let bridge: BridgeLike | null = null;
    try {
      this.#set('starting', 'Starting MetaTrader');
      bridge = this.#makeBridge(config.bridge);
      const hello = await bridge.call<{ package: string | null }>('hello', {}, 60_000);
      if (!hello?.package) throw new BridgeError('NO_PACKAGE', 'The MetaTrader5 Python package is missing');
      let { pid } = await bridge.call<{ pid: number | null }>('running', { path: config.terminal }, 30_000);
      let tied = false;
      let launched = false;
      if (pid === null) {
        const l = await bridge.call<{ pid: number; tied: boolean; launched: boolean }>(
          'launch',
          { path: config.terminal, args: config.terminalArgs ?? [], settle: 20 },
          90_000,
        );
        ({ pid, tied, launched } = l);
        if (launched) this.#log('info', `Started MetaTrader minimized (${config.terminal})`);
      }
      this.#set('connecting', `Connecting to ${config.login} on ${config.server}`);
      const c = await bridge.call<ConnectAnswer>(
        'connect',
        {
          path: config.terminal,
          login: config.login,
          server: config.server,
          timeout: 60_000,
          allowSwitch: config.allowSwitch ?? true,
          settle: launched ? 60 : 15,
        },
        240_000,
      );
      if (!c?.account || !c.terminal) throw new BridgeError('INIT_FAILED', 'MetaTrader answered without an account');
      if (c.switched) this.#log('info', `MetaTrader switched to ${config.login} on ${config.server}`);
      if (key !== this.#key) throw new RpcError('CHANGED', 'MetaTrader was set up again meanwhile');
      this.#markets ??= mapSymbols(await bridge.call<BrokerSymbol[]>('symbols', {}, 120_000));
      const clock = await this.#clockOf(bridge, config.server, this.#markets);
      const session: Session = {
        key,
        bridge,
        account: {
          login: Number(c.account.login),
          server: String(c.account.server),
          name: String(c.account.name ?? ''),
          company: String(c.account.company ?? ''),
          currency: String(c.account.currency ?? ''),
        },
        terminal: {
          build: typeof c.terminal.build === 'number' ? c.terminal.build : null,
          maxBars: typeof c.terminal.maxbars === 'number' ? c.terminal.maxbars : null,
          connected: Boolean(c.terminal.connected),
        },
        clock,
        pid,
        switched: Boolean(c.switched),
        tied,
      };
      this.#session = session;
      this.#known = session;
      this.#coolMs = 0;
      this.#set('ready', null);
      return session;
    } catch (error) {
      if (bridge) {
        // Closing its stdin switches the terminal back when it had switched.
        const b = bridge;
        this.#closing = b.close(30_000).catch(() => undefined);
      }
      const code = error instanceof RpcError ? error.code : '';
      if (code !== 'CHANGED' && code !== 'CLOSED') this.#cool(error);
      throw error;
    }
  }

  async #closeSession(): Promise<void> {
    const s = this.#session;
    this.#session = null;
    if (s) {
      this.#closing = s.bridge.close(30_000).catch(() => undefined);
      if (this.#state !== 'error' && this.#state !== 'paused') this.#set('idle', null);
    }
    await this.#closing;
  }

  #touch(): void {
    clearTimeout(this.#idleTimer);
    if (this.#closed || !this.#session) return;
    this.#idleTimer = setTimeout(() => {
      if (this.#inUse === 0) void this.#closeSession();
      else this.#touch();
    }, IDLE_MS);
    this.#idleTimer.unref?.();
  }

  #cool(error: unknown): void {
    this.#coolMs = this.#coolMs ? Math.min(this.#coolMs * 2, COOL_MAX_MS) : COOL_MIN_MS;
    this.#coolUntil = this.#now() + this.#coolMs;
    const message = messageOf(error);
    this.#log('warn', `MetaTrader: ${message}; trying again in ${Math.round(this.#coolMs / 60_000)} min`);
    this.#set('error', message);
    this.#readyLater();
  }

  /** Tells the app (and the fetcher) when a cool-down or a pause is over. */
  #readyLater(): void {
    clearTimeout(this.#readyTimer);
    if (this.#closed) return;
    this.#readyTimer = setTimeout(
      () => {
        this.#emit();
        if (this.serves('')) this.#onReady?.();
      },
      Math.max(0, this.#coolUntil - this.#now()) + 50,
    );
    this.#readyTimer.unref?.();
  }

  #set(state: MetaTraderState, message: string | null): void {
    this.#state = state;
    this.#message = message;
    this.#emit();
  }

  #emit(): void {
    const status = this.status();
    const text = JSON.stringify(status);
    if (text === this.#lastStatus) return;
    this.#lastStatus = text;
    this.#onStatus?.(status);
  }

  // -------------------------------------------------------------------------------------------
  // The server's clock

  async #clockOf(bridge: BridgeLike, server: string, markets: Map<string, BrokerSymbol>): Promise<ModelChoice> {
    const known = this.#readClocks()[server];
    if (known && this.#now() - known.at < CLOCK_MAX_AGE_MS) {
      return { model: known.model, fit: known.fit, of: known.of, assumed: false };
    }
    const reference =
      markets.get('eurusd') ??
      [...markets].sort((a, b) => a[0].localeCompare(b[0])).find(([i]) => isCurrencyPair(i))?.[1];
    const evidence: Evidence[] = [];
    if (reference) {
      const nowSec = Math.floor(this.#now() / 1000);
      try {
        const h1 = await bridge.call<RatesAnswer>(
          'rates',
          { symbol: reference.name, from: nowSec - 400 * DAY, to: nowSec + DAY, timeframe: 'H1', pause: 5 },
          RATES_TIMEOUT_MS,
        );
        evidence.push(...weeklyOpens(h1.time));
        const tick = await bridge.call<{ timeMsc: number } | null>('tick', { symbol: reference.name }, 30_000);
        const fresh = tick ? tickEvidence(tick.timeMsc, this.#now()) : null;
        if (fresh) evidence.push(fresh);
      } catch (error) {
        this.#log('warn', `MetaTrader: could not read ${server}'s clock: ${messageOf(error)}`);
      }
    }
    const choice = chooseModel(evidence);
    this.#log(
      'info',
      choice.assumed
        ? `MetaTrader: ${server}'s clock is assumed New York + 7 h`
        : `MetaTrader: ${server} runs ${choice.model} (fits ${choice.fit} of ${choice.of})`,
    );
    if (!choice.assumed) await this.#writeClock(server, choice);
    return choice;
  }

  #readClocks(): Record<string, { model: ZoneModel; fit: number; of: number; at: number }> {
    try {
      const json = JSON.parse(fs.readFileSync(this.#clockFile, 'utf8')) as { version?: number; servers?: unknown };
      if (json.version !== CLOCK_VERSION || !json.servers || typeof json.servers !== 'object') return {};
      const out: Record<string, { model: ZoneModel; fit: number; of: number; at: number }> = {};
      for (const [server, v] of Object.entries(json.servers as Record<string, Record<string, unknown>>)) {
        if (!MODELS.includes(v?.model as ZoneModel) || typeof v.at !== 'number') continue;
        out[server] = { model: v.model as ZoneModel, fit: Number(v.fit) || 0, of: Number(v.of) || 0, at: v.at };
      }
      return out;
    } catch {
      return {};
    }
  }

  async #writeClock(server: string, choice: ModelChoice): Promise<void> {
    const servers = this.#readClocks();
    servers[server] = { model: choice.model, fit: choice.fit, of: choice.of, at: this.#now() };
    try {
      await writeAtomic(this.#clockFile, `${JSON.stringify({ version: CLOCK_VERSION, servers }, null, 2)}\n`);
    } catch (error) {
      this.#log('warn', `MetaTrader: could not save the server's clock: ${messageOf(error)}`);
    }
  }
}
