// Pine scripts written in Demido Studio.
//
// The library keeps them on this computer (DEMIDO_PINE_DIR/library.json); TradingView compiles
// them without saving anything (pine-facade translate_source), and they run on the chart like any
// other indicator, under the id `DEMIDO;<id>`. Saving one to the user's TradingView account
// (`publish`) is the only step that changes the account, and Demido asks the user before it.
//
//   pine.changed  {id, script}   a script was saved, linked or deleted (`script` null)

import { createHash, randomBytes } from 'node:crypto';
import fs from 'node:fs/promises';
import path from 'node:path';

import { RpcError, emit } from '../protocol.ts';
import { writeAtomic } from '../store/dukascopy-store.ts';
import { WEBVIEW_USER_AGENT, sessionCookie } from '../tradingview.ts';
import { FACADE, SITE, type Script, refresh } from './catalog.ts';

type Obj = Record<string, unknown>;
const obj = (v: unknown): Obj => (v && typeof v === 'object' && !Array.isArray(v) ? (v as Obj) : {});
const arr = (v: unknown): unknown[] => (Array.isArray(v) ? v : []);
const str = (v: unknown): string => (typeof v === 'string' ? v : typeof v === 'number' ? String(v) : '');
const num = (v: unknown): number | undefined => (typeof v === 'number' && Number.isFinite(v) ? v : undefined);

/** Ids of library scripts on the chart: `DEMIDO;` and the library id. */
export const PREFIX = 'DEMIDO;';

/** Largest source Demido sends; TradingView's own limit is lower and answers in words. */
const MAX_SOURCE = 1_000_000;
/** TradingView refuses longer script names. */
const MAX_NAME = 128;

// ---------------------------------------------------------------------------------------------
// Compiling

/** A compiler message at a place in the source: lines and columns from 1, the end inclusive. */
export interface PineMessage {
  line: number;
  column: number;
  endLine: number;
  endColumn: number;
  message: string;
  code?: string;
}

export type PineKind = 'indicator' | 'strategy' | 'library';

export interface Compiled {
  ok: boolean;
  errors: PineMessage[];
  warnings: PineMessage[];
  /** What the script declares (`indicator(…)`, `strategy(…)`, `library(…)`). */
  kind: PineKind | null;
  /** Its title, from the declaration. */
  title: string;
  /** Drawn over the candles; known once it compiled. */
  overlay?: boolean;
  /** Present when it compiled: what a chart runs. */
  script?: Script;
}

/** TradingView's messages are templates (`Undeclared identifier "{identifier}"`); fills them in. */
export function fillMessage(template: string, ctx: Obj): string {
  return template.replace(/\{(\w+)\}/g, (whole, key: string) => {
    const v = ctx[key];
    return typeof v === 'string' || typeof v === 'number' || typeof v === 'boolean' ? String(v) : whole;
  });
}

/** One compiler message as TradingView sends it, in plain fields. */
export function messageOf(raw: unknown): PineMessage | null {
  const m = obj(raw);
  const text = str(m.message) || str(m.text);
  if (!text) return null;
  const start = obj(m.start);
  const end = obj(m.end);
  const line = Math.max(1, num(start.line) ?? 1);
  const column = Math.max(1, num(start.column) ?? 1);
  const out: PineMessage = {
    line,
    column,
    endLine: Math.max(line, num(end.line) ?? line),
    endColumn: num(end.column) ?? column,
    message: fillMessage(text, obj(m.ctx)),
  };
  if (str(m.code)) out.code = str(m.code);
  return out;
}

/** The declaration's kind and title, read from the source (`indicator("RSI cross", …)`). */
export function declaration(source: string): { kind: PineKind | null; title: string } {
  const decl = /^[ \t]*(indicator|study|strategy|library)[ \t]*\(/m.exec(source);
  if (!decl) return { kind: null, title: '' };
  const kind: PineKind = decl[1] === 'strategy' ? 'strategy' : decl[1] === 'library' ? 'library' : 'indicator';
  const rest = source.slice(decl.index + decl[0].length);
  const title = /^\s*(?:title\s*=\s*)?(["'])((?:\\.|(?!\1)[^\\\n])*)\1/.exec(rest);
  return { kind, title: title ? title[2]!.replace(/\\(.)/g, '$1').trim() : '' };
}

function headers(cookie: string): Record<string, string> {
  // pine-facade answers a web page instead of JSON without the site's origin.
  return {
    cookie,
    'user-agent': WEBVIEW_USER_AGENT,
    accept: 'application/json',
    origin: SITE,
    referer: `${SITE}/`,
  };
}

/** A request to pine-facade, as TradingView's Pine Editor sends it (a POST carries a form). */
async function facade(urlPath: string, params: Record<string, string>, form?: Record<string, string>): Promise<Obj> {
  const url = new URL(`${FACADE}/${urlPath}`);
  for (const [k, v] of Object.entries(params)) url.searchParams.set(k, v);
  let body: FormData | undefined;
  if (form) {
    body = new FormData();
    for (const [k, v] of Object.entries(form)) body.append(k, v);
  }
  let res: Response;
  try {
    res = await fetch(url, {
      method: form ? 'POST' : 'GET',
      headers: headers(await sessionCookie()),
      body,
      signal: AbortSignal.timeout(30_000),
    });
  } catch (error) {
    if (error instanceof RpcError) throw error;
    throw new RpcError('NETWORK', `Could not reach TradingView: ${(error as Error).message}`);
  }
  if (res.status === 413) throw new RpcError('TOO_LARGE', 'TradingView refused the script: it is too large.');
  if (res.status === 420) throw new RpcError('BAD_NAME', `TradingView refused the name: at most ${MAX_NAME} characters.`);
  if (res.status === 401) throw new RpcError('SESSION_EXPIRED', 'TradingView did not accept the session. Sign in again.');
  const text = await res.text();
  try {
    return obj(JSON.parse(text));
  } catch {
    throw new RpcError('NETWORK', `TradingView sent something unexpected (HTTP ${res.status}).`);
  }
}

/** Compiled results by source hash: the editor checks the same source again and again. */
const compiled = new Map<string, Promise<Compiled>>();
const COMPILED_KEPT = 40;

export function hashOf(source: string): string {
  return createHash('sha1').update(source).digest('hex');
}

/**
 * Compiles a script with TradingView's compiler, without saving it anywhere. Compile errors are
 * a result, not a failure: `ok` is false and `errors` says where.
 */
export function compile(source: string): Promise<Compiled> {
  const key = hashOf(source);
  let pending = compiled.get(key);
  if (pending) {
    // Most recently used last, so the oldest goes first.
    compiled.delete(key);
    compiled.set(key, pending);
    return pending;
  }
  pending = compileNow(source);
  compiled.set(key, pending);
  pending.catch(() => compiled.delete(key));
  while (compiled.size > COMPILED_KEPT) compiled.delete(compiled.keys().next().value!);
  return pending;
}

async function compileNow(source: string): Promise<Compiled> {
  const { kind, title } = declaration(source);
  const at = (message: string): PineMessage => ({ line: 1, column: 1, endLine: 1, endColumn: 1, message });
  if (!source.trim()) return { ok: false, errors: [at('The script is empty.')], warnings: [], kind, title };
  if (source.length > MAX_SOURCE) {
    return { ok: false, errors: [at('The script is too large (over a million characters).')], warnings: [], kind, title };
  }
  const data = await facade('translate_source/last', {}, { source, inputs: '{}' });
  const reason2 = obj(data.reason2);
  const errors = arr(reason2.errors).map(messageOf).filter((m): m is PineMessage => m !== null);
  const warnings = arr(reason2.warnings).map(messageOf).filter((m): m is PineMessage => m !== null);
  const result = obj(data.result);
  const metaInfo = obj(result.metaInfo);
  if (data.success === true && typeof result.ilTemplate === 'string' && Object.keys(metaInfo).length) {
    return {
      ok: true,
      errors,
      warnings,
      kind,
      title: str(metaInfo.description) || title,
      overlay: metaInfo.is_price_study === true,
      script: { metaInfo, ilTemplate: result.ilTemplate },
    };
  }
  if (!errors.length) {
    // An error without a place: `reason` is a message, or one more JSON-encoded error.
    const reason = str(data.reason) || str(data.error);
    let parsed: PineMessage | null = null;
    try {
      parsed = messageOf(JSON.parse(reason));
    } catch {
      // plain words
    }
    errors.push(parsed ?? at(reason || 'TradingView could not compile the script.'));
  }
  return { ok: false, errors, warnings, kind, title };
}

/** "line 3: Undeclared identifier "lenx"", for messages that have no editor to point at. */
export function where(m: PineMessage): string {
  return `line ${m.line}: ${m.message}`;
}

/** A script ready to run on a chart, or why it cannot be. */
export async function runnable(source: string): Promise<{ script: Script; warnings: PineMessage[] }> {
  const c = await compile(source);
  if (!c.ok || !c.script) {
    const first = c.errors[0];
    const more = c.errors.length > 1 ? ` (and ${c.errors.length - 1} more)` : '';
    throw new RpcError('COMPILE_ERROR', `The script does not compile: ${first ? where(first) : 'no reason given'}${more}.`);
  }
  if (c.kind === 'library') {
    throw new RpcError('UNSUPPORTED', 'A library cannot run on a chart; scripts import it.');
  }
  if (c.kind === 'strategy') {
    throw new RpcError('UNSUPPORTED', 'Strategies are not supported yet; write an indicator (`indicator(…)`).');
  }
  return { script: c.script, warnings: c.warnings };
}

// ---------------------------------------------------------------------------------------------
// The library

export interface TradingViewLink {
  /** The script's id on TradingView (`USER;…`). */
  id: string;
  /** Its version there after the last save or import. */
  version: string;
  /** When Demido last saved or imported it, ms since the epoch. */
  synced: number;
  /** Hash of the source at that moment: a different hash means local edits. */
  hash: string;
}

export interface PineScript {
  id: string;
  name: string;
  source: string;
  /** ms since the epoch. */
  created: number;
  modified: number;
  /** Grows with every change to the source: charts re-run the script when it does. */
  revision: number;
  tradingview?: TradingViewLink;
}

/** A script without its source, as lists carry it. */
export interface PineSummary {
  id: string;
  name: string;
  kind: PineKind | null;
  created: number;
  modified: number;
  revision: number;
  lines: number;
  tradingview?: TradingViewLink & { /** Edited since it was saved there. */ changed: boolean };
}

export function summary(s: PineScript): PineSummary {
  const out: PineSummary = {
    id: s.id,
    name: s.name,
    kind: declaration(s.source).kind,
    created: s.created,
    modified: s.modified,
    revision: s.revision,
    lines: s.source.split('\n').length,
  };
  if (s.tradingview) out.tradingview = { ...s.tradingview, changed: s.tradingview.hash !== hashOf(s.source) };
  return out;
}

const ID = /^[a-z0-9]{1,32}$/;

function cleanName(name: string): string {
  return name.replace(/\s+/g, ' ').trim().slice(0, MAX_NAME);
}

export class PineLibrary {
  readonly #file: string;
  #scripts: Map<string, PineScript> | null = null;
  #loading: Promise<Map<string, PineScript>> | null = null;
  /** Writes go one after another, each with everything known by then. */
  #writing: Promise<void> = Promise.resolve();
  readonly #emit: (event: string, params: unknown) => void;

  constructor(dir: string, opts: { emit?: (event: string, params: unknown) => void } = {}) {
    this.#file = path.join(dir, 'library.json');
    this.#emit = opts.emit ?? emit;
  }

  async #all(): Promise<Map<string, PineScript>> {
    if (this.#scripts) return this.#scripts;
    this.#loading ??= this.#read();
    this.#scripts = await this.#loading;
    return this.#scripts;
  }

  async #read(): Promise<Map<string, PineScript>> {
    const out = new Map<string, PineScript>();
    let text: string;
    try {
      text = await fs.readFile(this.#file, 'utf8');
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code === 'ENOENT') return out;
      throw error;
    }
    let data: Obj;
    try {
      data = obj(JSON.parse(text));
    } catch {
      // Keep the damaged file for the person to recover from, and start over.
      await fs.copyFile(this.#file, `${this.#file}.damaged-${Date.now()}`).catch(() => undefined);
      return out;
    }
    for (const raw of arr(data.scripts).map(obj)) {
      const id = str(raw.id);
      if (!ID.test(id) || typeof raw.source !== 'string') continue;
      const s: PineScript = {
        id,
        name: str(raw.name) || 'Untitled script',
        source: raw.source,
        created: num(raw.created) ?? 0,
        modified: num(raw.modified) ?? 0,
        revision: num(raw.revision) ?? 1,
      };
      const link = obj(raw.tradingview);
      if (str(link.id)) {
        s.tradingview = {
          id: str(link.id),
          version: str(link.version),
          synced: num(link.synced) ?? 0,
          hash: str(link.hash),
        };
      }
      out.set(id, s);
    }
    return out;
  }

  #persist(): Promise<void> {
    const write = this.#writing.then(async () => {
      const scripts = [...(this.#scripts ?? new Map()).values()];
      await writeAtomic(this.#file, `${JSON.stringify({ version: 1, scripts }, null, 1)}\n`);
    });
    this.#writing = write.catch(() => undefined);
    return write;
  }

  #changed(id: string, s: PineScript | null): void {
    this.#emit('pine.changed', { id, script: s ? summary(s) : null });
  }

  /** Every script, most recently changed first. */
  async list(): Promise<PineSummary[]> {
    return [...(await this.#all()).values()].sort((a, b) => b.modified - a.modified).map(summary);
  }

  async get(id: string): Promise<PineScript> {
    const s = (await this.#all()).get(id);
    if (!s) throw new RpcError('NOT_FOUND', `There is no Pine script "${id}" in the library.`);
    return s;
  }

  /** Library id of a chart script id (`DEMIDO;abc` → `abc`), or null for TradingView's own. */
  static idOf(scriptId: string): string | null {
    return scriptId.startsWith(PREFIX) ? scriptId.slice(PREFIX.length) : null;
  }

  /**
   * Creates a script (no `id`) or replaces one's source. Without a `name`, it is named after the
   * title its declaration gives, as TradingView names a script on its first save.
   */
  async save(req: { id?: string; source: string; name?: string }): Promise<PineScript> {
    if (req.source.length > MAX_SOURCE) throw new RpcError('TOO_LARGE', 'The script is too large (over a million characters).');
    const all = await this.#all();
    const now = Date.now();
    const named = cleanName(req.name ?? '') || cleanName(declaration(req.source).title);
    let s: PineScript;
    if (req.id) {
      const old = all.get(req.id);
      if (!old) throw new RpcError('NOT_FOUND', `There is no Pine script "${req.id}" in the library.`);
      const sourceChanged = old.source !== req.source;
      s = {
        ...old,
        source: req.source,
        name: named || old.name,
        modified: sourceChanged || named !== old.name ? now : old.modified,
        revision: old.revision + (sourceChanged ? 1 : 0),
      };
    } else {
      let id = '';
      do id = randomBytes(6).toString('hex');
      while (all.has(id));
      s = { id, name: named || 'Untitled script', source: req.source, created: now, modified: now, revision: 1 };
    }
    all.set(s.id, s);
    await this.#persist();
    this.#changed(s.id, s);
    return s;
  }

  async remove(id: string): Promise<void> {
    const all = await this.#all();
    if (!all.delete(id)) return;
    await this.#persist();
    this.#changed(id, null);
  }

  /** Records that the script's current source is saved on TradingView as `tvId`. */
  async link(id: string, tvId: string, version: string, source?: string): Promise<PineScript> {
    const all = await this.#all();
    const old = await this.get(id);
    const s: PineScript = { ...old, tradingview: { id: tvId, version, synced: Date.now(), hash: hashOf(source ?? old.source) } };
    all.set(id, s);
    await this.#persist();
    this.#changed(id, s);
    return s;
  }

  /** The library script saved on TradingView as `tvId`, if any. */
  async linkedTo(tvId: string): Promise<PineScript | null> {
    for (const s of (await this.#all()).values()) if (s.tradingview?.id === tvId) return s;
    return null;
  }
}

let shared: PineLibrary | null = null;

/** The library the sidecar's requests use (set up by main.ts). */
export function useLibrary(lib: PineLibrary): void {
  shared = lib;
}

export function library(): PineLibrary {
  if (!shared) throw new RpcError('ERROR', 'The Pine library is not set up.');
  return shared;
}

/** The source of a chart script id from the library (`DEMIDO;…`), or null for other ids. */
export async function sourceOf(scriptId: string): Promise<{ source: string; revision: number } | null> {
  const id = PineLibrary.idOf(scriptId);
  if (id === null) return null;
  const s = await library().get(id);
  return { source: s.source, revision: s.revision };
}

// ---------------------------------------------------------------------------------------------
// TradingView: saving and importing

export interface Published {
  script: PineSummary;
  tradingview: { id: string; version: string; created: boolean };
  warnings: PineMessage[];
}

/** TradingView's answer to a save in plain words, or the metaInfo of what it saved. */
function savedMeta(data: Obj): Obj {
  const result = obj(data.result);
  const meta = obj(result.metaInfo);
  if (str(meta.scriptIdPart)) return meta;
  const reason = str(data.reason) || str(data.error) || str(obj(data.error).message);
  if (/exist/i.test(reason)) {
    throw new RpcError('NAME_TAKEN', `Your TradingView account already has a script with this name. Give this one another title. (${reason})`);
  }
  throw new RpcError('SAVE_FAILED', `TradingView did not save the script${reason ? `: ${reason}` : '.'}`);
}

/**
 * Saves a library script to the user's TradingView account: the first time as a new script
 * (named like its title, or `name`), then as the next version of that same script. It must
 * compile first; TradingView would otherwise keep a broken version.
 */
export async function publish(lib: PineLibrary, id: string, opts: { name?: string } = {}): Promise<Published> {
  const s = await lib.get(id);
  const c = await compile(s.source);
  if (!c.ok) {
    const first = c.errors[0];
    throw new RpcError('COMPILE_ERROR', `Fix the script before saving it to TradingView: ${first ? where(first) : 'it does not compile'}.`);
  }
  const name = cleanName(opts.name ?? '') || s.name;
  const source = s.source;
  let meta: Obj;
  let created = false;
  if (s.tradingview?.id) {
    const params: Record<string, string> = { allow_create_new: 'false' };
    if (opts.name) params.name = name;
    meta = savedMeta(await facade(`save/next/${encodeURIComponent(s.tradingview.id)}`, params, { source }));
  } else {
    meta = savedMeta(await facade('save/new', { name }, { source }));
    created = true;
  }
  const tvId = str(meta.scriptIdPart);
  const version = str(obj(meta.pine).version) || str(meta.version);
  const linked = await lib.link(id, tvId, version, source);
  // The Indicators menu lists the user's scripts from TradingView: show the new one there.
  refresh();
  return { script: summary(linked), tradingview: { id: tvId, version, created }, warnings: c.warnings };
}

export interface Imported {
  script: PineSummary;
  /** False when it was already in the library with local edits, which are kept. */
  updated: boolean;
}

/**
 * Brings a TradingView script's source into the library: one of the user's own (linked, so a
 * later save updates it there) or an open-source community script (a copy). One already in the
 * library is updated from TradingView unless it has local edits.
 */
export async function importScript(lib: PineLibrary, tvId: string): Promise<Imported> {
  const data = await facade(`get/${encodeURIComponent(tvId)}/last`, { no_4xx: 'true' });
  const source = typeof data.source === 'string' ? data.source : '';
  if (!source) {
    const reason = str(data.reason) || str(data.error);
    // "User is not allowed to see source code of pine": closed source or invite-only.
    if (/allowed|access|permission|protected|auth/i.test(reason) || !reason) {
      throw new RpcError('NO_ACCESS', 'TradingView does not share the source of this script (it is closed source or invite-only).');
    }
    throw new RpcError('NOT_FOUND', `TradingView could not find this script: ${reason}`);
  }
  const own = tvId.startsWith('USER;');
  const version = str(data.version);
  const existing = own ? await lib.linkedTo(tvId) : null;
  if (existing) {
    if (existing.tradingview && existing.tradingview.hash !== hashOf(existing.source)) {
      return { script: summary(existing), updated: false };
    }
    await lib.save({ id: existing.id, source });
    return { script: summary(await lib.link(existing.id, tvId, version, source)), updated: true };
  }
  const name = str(data.scriptName) || str(data.scriptTitle);
  const saved = await lib.save({ source, ...(name ? { name } : {}) });
  if (!own) return { script: summary(saved), updated: true };
  return { script: summary(await lib.link(saved.id, tvId, version, source)), updated: true };
}
