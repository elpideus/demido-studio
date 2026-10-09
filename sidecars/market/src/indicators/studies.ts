// Indicators running on a live chart, computed by TradingView.
//
// Each one is a "study" on the chart session of a live stream (tradingview.ts), created the way
// TradingView's own chart creates it, so its values are TradingView's. Results go out as events:
//
//   indicator.data      {id, full, rows}     rows are [time, ...values] for the meta's columns;
//                                            `full` replaces everything, otherwise rows merge
//   indicator.graphics  {id, graphics}       the script's labels, lines, boxes and tables
//   indicator.error     {id, code, message}  TradingView stopped computing it
//
// A study lives as long as its stream: when the chart closes, its studies go with it.
//
// Scripts from Demido's Pine library (`DEMIDO;…`) are compiled by TradingView first (pine.ts) and
// then run the same way. `trial` runs a script once on a chart session of its own, for testing.

import PineIndicator from '@mathieuc/tradingview/src/classes/PineIndicator.js';
import type { ChartSession, Study } from '@mathieuc/tradingview';

import { type Bar, RpcError, emit, log } from '../protocol.ts';
import type { Timeframe } from '../timeframes.ts';
import { type ChartInfo, onStreamClosed, streamChart, withChart } from '../tradingview.ts';
import { type Script, script } from './catalog.ts';
import {
  type Graphics,
  type IndicatorMeta,
  type LayoutState,
  type Row,
  describe,
  graphicsOf,
  inputValues,
  rowsOf,
} from './describe.ts';
import { type PineMessage, PineLibrary, compile, fillMessage, runnable, sourceOf } from './pine.ts';

interface Active {
  id: string;
  stream: string;
  chart: ChartSession;
  study: Study;
  meta: IndicatorMeta;
  /** Last row sent per bar time, to send only what changed. */
  sent: Map<number, string>;
  graphicsSent: string;
  changes: Set<string>;
  timer: ReturnType<typeof setTimeout> | undefined;
  closed: boolean;
}

const active = new Map<string, Active>();
let next = 1;

/** How often a study's changes go out at most: a few frames a second is plenty. */
const FLUSH_MS = 250;
/** Rows kept per study (a chart session holds up to 5000 bars; scripts add warm-up bars). */
const MAX_ROWS = 6000;
/** Above this many changed rows, one full replacement is cheaper than a merge. */
const MERGE_LIMIT = 64;

function errorText(args: unknown[]): string {
  return args
    .map((a) => (typeof a === 'string' ? a : JSON.stringify(a)))
    .join(' ')
    .replace(/\s+/g, ' ')
    .trim();
}

/** Where and why a script stopped while it ran (a Pine runtime error). */
export interface Fault {
  message: string;
  code?: string;
  /** Source line, from 1. */
  line?: number;
  /** The bar it stopped on: its index from the first bar loaded. */
  bar?: number;
}

/**
 * The error TradingView sent, filled in: `{error: "Error on bar {bar_index}: …", ctx, stack_trace}`.
 * The words after it name the server that computed the study, which helps nobody.
 */
export function faultOf(args: unknown[]): Fault | null {
  for (const a of args) {
    if (!a || typeof a !== 'object') continue;
    const o = a as Record<string, unknown>;
    if (typeof o.error !== 'string' || !o.error) continue;
    const ctx = o.ctx && typeof o.ctx === 'object' ? (o.ctx as Record<string, unknown>) : {};
    const fault: Fault = { message: fillMessage(o.error, ctx) };
    if (typeof ctx.code === 'string') fault.code = ctx.code;
    if (typeof ctx.bar_index === 'number') fault.bar = ctx.bar_index;
    const top = Array.isArray(o.stack_trace) ? (o.stack_trace[0] as Record<string, unknown> | undefined) : undefined;
    if (top && typeof top.p === 'number' && top.p > 0) fault.line = top.p;
    return fault;
  }
  return null;
}

/** TradingView's study errors in plain words. */
export function studyError(args: unknown[]): RpcError {
  const text = errorText(args);
  if (/study_limit_exceeded|maximum number of studies/i.test(text)) {
    return new RpcError(
      'STUDY_LIMIT',
      'Your TradingView plan has no room for another indicator on this chart. Remove one, or upgrade the plan on tradingview.com.',
    );
  }
  if (/access|permission|invite/i.test(text)) {
    return new RpcError('NO_ACCESS', 'Your TradingView account has no access to this script.');
  }
  const fault = faultOf(args);
  if (fault) {
    const at = fault.line ? ` on line ${fault.line}` : '';
    return new RpcError('SCRIPT_RUNTIME', `The script stopped${at}: ${fault.message.slice(0, 400)}`);
  }
  return new RpcError('STUDY_ERROR', `TradingView could not compute this indicator: ${text.slice(0, 300) || 'no reason given'}.`);
}

/** A script by its chart id: TradingView's (catalog) or the Pine library's, compiled. */
async function load(id: string, version: string | null): Promise<Script> {
  const own = await sourceOf(id);
  if (!own) return script(id, version);
  return (await runnable(own.source)).script;
}

/** `meta` must be the script's own (`describe`): its id and version are what TradingView runs. */
function indicatorOf(meta: IndicatorMeta, body: string): PineIndicator {
  const inputs: Record<string, unknown> = {};
  for (const input of meta.inputs) {
    inputs[input.id] = {
      name: input.name,
      inline: input.id,
      internalID: input.id,
      type: input.type,
      value: input.value,
      isHidden: input.hidden,
      isFake: input.fake,
    };
  }
  // `plots: {}` keeps the library's raw `plot_N` keys, which the meta's columns use.
  return new PineIndicator({
    pineId: meta.id,
    pineVersion: meta.version,
    description: meta.name,
    shortDescription: meta.short,
    inputs,
    plots: {},
    script: body,
  });
}

function flush(entry: Active): void {
  entry.timer = undefined;
  if (entry.closed) return;
  const changes = entry.changes;
  entry.changes = new Set();
  if (changes.has('plots') && entry.meta.columns.length) sendRows(entry);
  if (changes.has('graphic')) sendGraphics(entry);
}

function sendRows(entry: Active): void {
  let rows = rowsOf(entry.study.periods, entry.meta.columns);
  if (rows.length > MAX_ROWS) rows = rows.slice(-MAX_ROWS);
  const changed: Row[] = [];
  const keys = new Map<number, string>();
  for (const row of rows) {
    const key = JSON.stringify(row);
    keys.set(row[0], key);
    if (entry.sent.get(row[0]) !== key) changed.push(row);
  }
  if (!changed.length && rows.length === entry.sent.size) return;
  const full = entry.sent.size === 0 || changed.length > MERGE_LIMIT || rows.length < entry.sent.size;
  entry.sent = keys;
  emit('indicator.data', { id: entry.id, full, rows: full ? rows : changed });
}

/** The script's drawings on `chart`, or null before it has bars. */
function drawingsOf(chart: ChartSession, study: Study, meta: IndicatorMeta): Graphics | null {
  const bars = chart.periods; // newest first, like the parser's "bars back"
  if (!bars.length) return null;
  const g = study.graphic;
  return graphicsOf(
    g as unknown as Record<string, unknown>,
    g.raw() as Record<string, unknown>,
    meta.palette,
    (back) => bars[back]?.time,
    bars[bars.length - 1]!.time,
    bars[0]!.time,
  );
}

function sendGraphics(entry: Active): void {
  const graphics = drawingsOf(entry.chart, entry.study, entry.meta);
  if (!graphics) return;
  const key = JSON.stringify(graphics);
  if (key === entry.graphicsSent) return;
  entry.graphicsSent = key;
  emit('indicator.graphics', { id: entry.id, graphics });
}

/** How long `add` waits for TradingView's first answer before returning anyway. */
const FIRST_ANSWER_MS = 20_000;

export interface AddRequest {
  stream: string;
  script: string;
  version: string | null;
  state: LayoutState;
}

/**
 * Runs an indicator on a live stream. Resolves once TradingView answered (or kept quiet for a
 * while: slow scripts send their values later as events); rejects when TradingView refused it,
 * for instance because the plan's indicators-per-chart limit is reached.
 */
export async function add(req: AddRequest): Promise<{ id: string; meta: IndicatorMeta }> {
  const chart = streamChart(req.stream);
  if (!chart) throw new RpcError('NO_STREAM', 'That chart is no longer live.');
  const s = await load(req.script, req.version);
  const base = describe(s.metaInfo);
  if (base.kind === 'strategy') {
    throw new RpcError('UNSUPPORTED', 'Strategies are not supported yet; add indicators only.');
  }
  const state: LayoutState = { ...req.state, inputs: inputValues(base.inputs, req.state.inputs ?? {}) };
  const meta = describe(s.metaInfo, state);
  if (streamChart(req.stream) !== chart) throw new RpcError('NO_STREAM', 'That chart is no longer live.');

  const id = `i${next++}`;
  const study = new chart.Study(indicatorOf(meta, s.ilTemplate));
  // A library script compiles as TradingView's unsaved "last" script; the chart knows it by its own id.
  const shown = PineLibrary.idOf(req.script) === null ? meta : { ...meta, id: req.script, version: '' };
  const entry: Active = {
    id,
    stream: req.stream,
    chart,
    study,
    meta,
    sent: new Map(),
    graphicsSent: '',
    changes: new Set(),
    timer: undefined,
    closed: false,
  };
  active.set(id, entry);

  let answered: (error?: RpcError) => void = () => undefined;
  const first = new Promise<void>((resolve, reject) => {
    const timer = setTimeout(() => resolve(), FIRST_ANSWER_MS);
    answered = (error) => {
      clearTimeout(timer);
      answered = () => undefined;
      if (error) reject(error);
      else resolve();
    };
  });

  study.onUpdate((changes) => {
    if (entry.closed) return;
    answered();
    for (const c of changes) entry.changes.add(c);
    entry.timer ??= setTimeout(() => flush(entry), FLUSH_MS);
  });
  study.onReady(() => answered());
  study.onError((...err: unknown[]) => {
    if (entry.closed) return;
    const error = studyError(err);
    log('warn', `indicator ${meta.id} on ${req.stream}: ${errorText(err)}`);
    stop(entry, false);
    emit('indicator.error', { id, code: error.code, message: error.message });
    answered(error);
  });

  await first;
  return { id, meta: shown };
}

/** How long a trial waits for TradingView to finish computing. */
const TRIAL_MS = 45_000;
/** A trial is done once the study has been quiet this long. */
const TRIAL_QUIET_MS = 1200;

export interface TrialRequest {
  /** The script: source to compile, or a chart script id (`DEMIDO;…`, `USER;…`, `STD;…`). */
  source?: string;
  script?: string;
  symbol: string;
  tf: Timeframe;
  bars: number;
  state: LayoutState;
}

export interface TrialResult {
  ok: boolean;
  /** What stopped it: the compiler, or the script while it ran. */
  failed?: 'compile' | 'runtime';
  errors: PineMessage[];
  warnings: PineMessage[];
  fault?: Fault;
  meta?: IndicatorMeta;
  info?: ChartInfo;
  /** The chart's bars, oldest first, and the indicator's rows over them (time, then the meta's columns). */
  bars?: Bar[];
  rows?: Row[];
  graphics?: Graphics;
}

/**
 * Runs a script once on a chart of its own, over the newest `bars` bars, and returns everything it
 * computed. Compile and runtime errors are a result (`ok: false`), so a model can fix the script.
 */
export async function trial(req: TrialRequest): Promise<TrialResult> {
  let s: Script;
  let warnings: PineMessage[] = [];
  if (req.source !== undefined) {
    const c = await compile(req.source);
    if (!c.ok || !c.script) return { ok: false, failed: 'compile', errors: c.errors, warnings: c.warnings };
    if (c.kind === 'library') throw new RpcError('UNSUPPORTED', 'A library cannot run on a chart; scripts import it.');
    s = c.script;
    warnings = c.warnings;
  } else if (req.script) {
    const own = await sourceOf(req.script);
    if (own) {
      const c = await compile(own.source);
      if (!c.ok || !c.script) return { ok: false, failed: 'compile', errors: c.errors, warnings: c.warnings };
      s = c.script;
      warnings = c.warnings;
    } else {
      s = await script(req.script, null);
    }
  } else {
    throw new RpcError('BAD_REQUEST', 'Give the script: its source or its id.');
  }
  const base = describe(s.metaInfo);
  if (base.kind === 'strategy') {
    throw new RpcError('UNSUPPORTED', 'Strategies are not supported yet; write an indicator (`indicator(…)`).');
  }
  const state: LayoutState = { ...req.state, inputs: inputValues(base.inputs, req.state.inputs ?? {}) };
  const meta = describe(s.metaInfo, state);

  return withChart(req.symbol, req.tf, req.bars, async (chart, info, bars) => {
    const study = new chart.Study(indicatorOf(meta, s.ilTemplate));
    const outcome = await new Promise<{ fault?: Fault; error?: RpcError }>((resolve) => {
      let quiet: ReturnType<typeof setTimeout> | undefined;
      let done = false;
      const finish = (v: { fault?: Fault; error?: RpcError }) => {
        if (done) return;
        done = true;
        clearTimeout(quiet);
        clearTimeout(timer);
        resolve(v);
      };
      const timer = setTimeout(() => finish({}), TRIAL_MS);
      study.onUpdate(() => {
        clearTimeout(quiet);
        quiet = setTimeout(() => finish({}), TRIAL_QUIET_MS);
      });
      study.onError((...err: unknown[]) => {
        const error = studyError(err);
        if (error.code === 'SCRIPT_RUNTIME') finish({ fault: faultOf(err) ?? { message: error.message } });
        else finish({ error });
      });
    });
    // TradingView computes some bars before the chart's first one (warm-up); keep the chart's.
    const first = bars[0]?.t ?? 0;
    const rows = rowsOf(study.periods, meta.columns).filter((r) => r[0] >= first);
    const graphics = drawingsOf(chart, study, meta) ?? undefined;
    try {
      study.remove();
    } catch {
      // the chart goes next anyway
    }
    if (outcome.error) throw outcome.error;
    const shown = req.script && PineLibrary.idOf(req.script) !== null ? { ...meta, id: req.script, version: '' } : meta;
    if (outcome.fault) {
      // Its bar is Pine's bar_index, counted from the script's first bar, not the chart's.
      return { ok: false, failed: 'runtime', errors: [], warnings, fault: outcome.fault, meta: shown, info, bars };
    }
    return { ok: true, errors: [], warnings, meta: shown, info, bars, rows, ...(graphics ? { graphics } : {}) };
  });
}

function stop(entry: Active, removeStudy: boolean): void {
  if (entry.closed) return;
  entry.closed = true;
  clearTimeout(entry.timer);
  active.delete(entry.id);
  if (!removeStudy) return;
  try {
    entry.study.remove();
  } catch {
    // the socket is already gone
  }
}

/** Stops an indicator and frees its place on the chart. */
export function remove(id: string): void {
  const entry = active.get(id);
  if (entry) stop(entry, true);
}

/** The indicators running on a stream, by id. */
export function onStream(stream: string): string[] {
  return [...active.values()].filter((e) => e.stream === stream).map((e) => e.id);
}

// A closed stream's chart session takes its studies with it.
onStreamClosed((stream) => {
  for (const entry of [...active.values()]) {
    if (entry.stream === stream) stop(entry, false);
  }
});
