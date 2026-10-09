// What the Indicators menu offers: TradingView's built-in indicators, the user's own scripts and
// Favorites, community scripts, and the indicators saved in the user's chart layouts. Everything
// is read with the user's session, exactly as TradingView's own Indicators menu reads it.

import { RpcError } from '../protocol.ts';
import { WEBVIEW_USER_AGENT, sessionCookie } from '../tradingview.ts';
import { type Layout, parseFavorites, parseLayout } from './describe.ts';

export const FACADE = 'https://pine-facade.tradingview.com/pine-facade';
export const SITE = 'https://www.tradingview.com';

export interface CatalogEntry {
  id: string;
  /** The script version to run; null runs the latest. */
  version: string | null;
  name: string;
  short?: string;
  /** Drawn over the candles; unknown until the script is loaded for some lists. */
  overlay?: boolean;
  kind: 'study' | 'strategy';
  author?: string;
  /** Community scripts: open source, protected (closed source) or invite-only. */
  access?: 'open' | 'protected' | 'invite';
}

export interface Catalog {
  favorites: CatalogEntry[];
  mine: CatalogEntry[];
  builtins: CatalogEntry[];
}

export interface LayoutEntry {
  id: string;
  name: string;
  symbol: string;
  interval: string;
  /** Last saved, seconds since the epoch. */
  modified: number | null;
}

export interface Script {
  metaInfo: Record<string, unknown>;
  ilTemplate: string;
}

type Obj = Record<string, unknown>;
const obj = (v: unknown): Obj => (v && typeof v === 'object' && !Array.isArray(v) ? (v as Obj) : {});
const str = (v: unknown): string => (typeof v === 'string' ? v : typeof v === 'number' ? String(v) : '');

async function request(url: string, init: { cookie?: string; json?: boolean } = {}): Promise<Response> {
  let res: Response;
  try {
    res = await fetch(url, {
      headers: {
        'user-agent': WEBVIEW_USER_AGENT,
        accept: init.json ? 'application/json' : 'text/html,application/xhtml+xml',
        ...(init.json ? { 'x-requested-with': 'XMLHttpRequest' } : {}),
        referer: `${SITE}/chart/`,
        ...(init.cookie ? { cookie: init.cookie } : {}),
      },
      signal: AbortSignal.timeout(20_000),
    });
  } catch (error) {
    throw new RpcError('NETWORK', `Could not reach TradingView: ${(error as Error).message}`);
  }
  if (res.status === 403 || res.status >= 500) {
    throw new RpcError('NETWORK', `TradingView refused the request (HTTP ${res.status}). Try again in a moment.`);
  }
  return res;
}

export async function json(url: string, cookie?: string): Promise<unknown> {
  const res = await request(url, { cookie, json: true });
  const text = await res.text();
  try {
    return JSON.parse(text);
  } catch {
    throw new RpcError('NETWORK', `TradingView sent something unexpected (HTTP ${res.status}).`);
  }
}

/** Short-lived memo: the menu is opened often, the lists change rarely. */
function cached<T>(ms: number, load: () => Promise<T>): { get: () => Promise<T>; clear: () => void } {
  let value: { at: number; promise: Promise<T> } | null = null;
  return {
    get: () => {
      if (!value || Date.now() - value.at > ms) {
        const promise = load();
        value = { at: Date.now(), promise };
        promise.catch(() => {
          if (value?.promise === promise) value = null;
        });
      }
      return value.promise;
    },
    clear: () => {
      value = null;
    },
  };
}

function entryOf(raw: Obj, fallbackKind: 'study' | 'strategy' = 'study'): CatalogEntry | null {
  const id = str(raw.scriptIdPart);
  if (!id) return null;
  const extra = obj(raw.extra);
  const entry: CatalogEntry = {
    id,
    version: str(raw.version) || null,
    name: str(raw.scriptName) || str(raw.scriptTitle) || id,
    kind: extra.kind === 'strategy' ? 'strategy' : fallbackKind,
  };
  if (str(extra.shortDescription)) entry.short = str(extra.shortDescription);
  if (typeof extra.is_price_study === 'boolean') entry.overlay = extra.is_price_study;
  return entry;
}

const builtinList = cached(6 * 3600_000, async () => {
  const list = await json(`${FACADE}/list?filter=standard`);
  if (!Array.isArray(list)) return [];
  return list
    .map((x) => entryOf(obj(x)))
    .filter((e): e is CatalogEntry => e !== null)
    .sort((a, b) => a.name.localeCompare(b.name));
});

const mineList = cached(60_000, async () => {
  const list = await json(`${FACADE}/list?filter=saved`, await sessionCookie());
  if (!Array.isArray(list)) return [];
  return list
    .map((x) => entryOf(obj(x)))
    .filter((e): e is CatalogEntry => e !== null)
    // Saved versions of a script ("5.0") are its drafts; the menu runs the latest one.
    .map((e) => ({ ...e, version: null }))
    .sort((a, b) => a.name.localeCompare(b.name));
});

/** Fetches a tradingview.com page with the user's session, following real redirects only. */
async function page(path: string): Promise<string> {
  const cookie = await sessionCookie();
  let url = `${SITE}${path}`;
  for (let hop = 0; hop < 5; hop += 1) {
    let res: Response;
    try {
      res = await fetch(url, {
        redirect: 'manual',
        headers: { cookie, accept: 'text/html,application/xhtml+xml', 'user-agent': WEBVIEW_USER_AGENT },
        signal: AbortSignal.timeout(20_000),
      });
    } catch (error) {
      throw new RpcError('NETWORK', `Could not reach TradingView: ${(error as Error).message}`);
    }
    const location = res.headers.get('location');
    if (res.status >= 300 && res.status < 400 && location) {
      url = new URL(location, url).href;
      continue;
    }
    if (res.status === 404) throw new RpcError('NOT_FOUND', 'TradingView has no such chart layout.');
    if (res.status === 403 || res.status >= 500) {
      throw new RpcError('NETWORK', `TradingView refused the request (HTTP ${res.status}). Try again in a moment.`);
    }
    return res.text();
  }
  throw new RpcError('NETWORK', 'TradingView kept redirecting.');
}

const favoriteIds = cached(60_000, async () => parseFavorites(await page('/chart/')));

const scripts = new Map<string, Promise<Script>>();

/** A script's metadata and compiled body, as TradingView's chart loads it before running it. */
export function script(id: string, version: string | null): Promise<Script> {
  const key = `${id}@${version ?? 'last'}`;
  let pending = scripts.get(key);
  if (!pending) {
    pending = loadScript(id, version ?? 'last').catch(async (error: unknown) => {
      // A layout can pin a version the author has since replaced; run the current one then.
      if (version && version !== 'last') return loadScript(id, 'last');
      throw error;
    });
    scripts.set(key, pending);
    pending.catch(() => scripts.delete(key));
  }
  return pending;
}

async function loadScript(id: string, version: string): Promise<Script> {
  const cookie = await sessionCookie();
  const data = obj(await json(`${FACADE}/translate/${encodeURIComponent(id)}/${encodeURIComponent(version)}`, cookie));
  const result = obj(data.result);
  if (data.success !== true || !result.metaInfo || typeof result.ilTemplate !== 'string') {
    const reason = str(data.reason) || str(obj(data.result).reason);
    if (/access|permission|invite/i.test(reason)) {
      throw new RpcError('NO_ACCESS', 'This script is invite-only, and your TradingView account has no access to it.');
    }
    throw new RpcError('SCRIPT_ERROR', `TradingView could not load this indicator${reason ? `: ${reason}` : '.'}`);
  }
  return { metaInfo: obj(result.metaInfo), ilTemplate: result.ilTemplate };
}

/** Favorites, own scripts and built-ins, each sorted by name. */
export async function catalog(): Promise<Catalog> {
  const [builtins, mine, favorites] = await Promise.all([builtinList.get(), mineList.get(), favoriteIds.get()]);
  const known = new Map<string, CatalogEntry>();
  for (const e of [...builtins, ...mine]) known.set(e.id, e);
  const favoriteEntries = await Promise.all(
    favorites.map(async (id): Promise<CatalogEntry | null> => {
      const hit = known.get(id);
      if (hit) return hit;
      // Community favorites: their name is in the script itself.
      try {
        const s = await script(id, null);
        const m = s.metaInfo;
        return {
          id,
          version: null,
          name: str(m.description) || id,
          short: str(m.shortDescription) || undefined,
          overlay: m.is_price_study === true,
          kind: m.isTVScriptStrategy === true ? 'strategy' : 'study',
        };
      } catch {
        return null;
      }
    }),
  );
  return {
    favorites: favoriteEntries.filter((e): e is CatalogEntry => e !== null),
    mine,
    builtins,
  };
}

/** Forgets the cached lists, so the next menu shows changes made on tradingview.com. */
export function refresh(): void {
  mineList.clear();
  favoriteIds.clear();
}

const ACCESS: Record<number, CatalogEntry['access']> = { 1: 'open', 2: 'protected', 3: 'invite' };

/** Community scripts matching `query`, most popular first (TradingView's own suggestions). */
export async function search(query: string): Promise<CatalogEntry[]> {
  const data = obj(await json(`${SITE}/pubscripts-suggest-json?search=${encodeURIComponent(query)}`));
  const results = Array.isArray(data.results) ? data.results : [];
  const out: CatalogEntry[] = [];
  for (const r of results.map(obj)) {
    const entry = entryOf(r);
    if (!entry) continue;
    entry.name = str(r.scriptName) || str(r.title) || entry.id;
    entry.version = null;
    const author = str(obj(r.author).username);
    if (author) entry.author = author;
    const access = ACCESS[Number(r.access)];
    if (access) entry.access = access;
    out.push(entry);
  }
  return out.slice(0, 40);
}

/** The user's saved chart layouts, most recently saved first. */
export async function layouts(): Promise<LayoutEntry[]> {
  const list = await json(`${SITE}/my-charts/`, await sessionCookie());
  if (!Array.isArray(list)) return [];
  return list
    .map(obj)
    .map((l) => ({
      id: str(l.image_url) || str(l.url),
      name: str(l.name) || 'Untitled layout',
      symbol: str(l.symbol) || str(l.short_symbol),
      interval: str(l.interval) || str(l.resolution),
      modified: typeof l.modified_iso === 'number' ? Math.round(l.modified_iso) : null,
    }))
    .filter((l) => /^[A-Za-z0-9]+$/.test(l.id))
    .sort((a, b) => (b.modified ?? 0) - (a.modified ?? 0));
}

/** The indicators saved in one chart layout. */
export async function layout(id: string): Promise<Layout> {
  if (!/^[A-Za-z0-9]+$/.test(id)) throw new RpcError('BAD_REQUEST', 'That is not a chart layout id.');
  const parsed = parseLayout(await page(`/chart/${id}/`));
  if (!parsed) throw new RpcError('NOT_FOUND', 'TradingView sent this layout without its charts.');
  return parsed;
}
