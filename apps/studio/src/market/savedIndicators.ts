// The indicators a chart window keeps (in its window props, so they survive restarts): which
// script, how it is set up and looks, and whether its eye is closed. Pure, so it is unit-tested.

import type {
  ChartLayoutStudy,
  IndicatorEntry,
  IndicatorInput,
  IndicatorLook,
  IndicatorSetup,
  PineSummary,
} from '@/lib/types';
import { lookOf } from './look';

/** Scripts of Demido's Pine library, on the chart. */
export const PINE_PREFIX = 'DEMIDO;';

export interface SavedIndicator {
  /** Unique within the window. */
  key: string;
  /** TradingView's script id (`STD;RSI`, `PUB;…`, `USER;…`), or a library script's (`DEMIDO;…`). */
  script: string;
  /** The version to run; null runs the latest. */
  version: string | null;
  name: string;
  setup: IndicatorSetup;
  /** Its Style and Visibility settings, applied without computing it again. */
  look?: IndicatorLook;
  hidden?: boolean;
  /** A library script's revision (from the library, not kept): a new one computes it again. */
  revision?: number;
}

const isObj = (v: unknown): v is Record<string, unknown> => !!v && typeof v === 'object' && !Array.isArray(v);

/** The saved indicators in a window's props; anything malformed is left out. */
export function savedIndicators(value: unknown): SavedIndicator[] {
  if (!Array.isArray(value)) return [];
  const seen = new Set<string>();
  const out: SavedIndicator[] = [];
  for (const v of value) {
    if (!isObj(v) || typeof v.key !== 'string' || typeof v.script !== 'string' || !v.script) continue;
    if (seen.has(v.key)) continue;
    seen.add(v.key);
    out.push({
      key: v.key,
      script: v.script,
      version: typeof v.version === 'string' ? v.version : null,
      name: typeof v.name === 'string' && v.name ? v.name : v.script,
      setup: isObj(v.setup) ? (v.setup as IndicatorSetup) : {},
      ...(lookOf(v.look) ? { look: lookOf(v.look) } : {}),
      ...(v.hidden === true ? { hidden: true } : {}),
    });
  }
  return out;
}

function newKey(taken: ReadonlySet<string>): string {
  for (;;) {
    const key = Math.random().toString(36).slice(2, 10);
    if (key && !taken.has(key)) return key;
  }
}

/** An indicator picked in the Indicators menu, with TradingView's default settings. */
export function fromEntry(entry: IndicatorEntry, taken: ReadonlySet<string>): SavedIndicator {
  return { key: newKey(taken), script: entry.id, version: entry.version, name: entry.short || entry.name, setup: {} };
}

/** A script named by its id (as the assistant does), latest version, these inputs or the defaults. */
export function fromScript(
  script: string,
  name: string,
  taken: ReadonlySet<string>,
  inputs?: Record<string, unknown>,
): SavedIndicator {
  return {
    key: newKey(taken),
    script,
    version: null,
    name,
    setup: inputs && Object.keys(inputs).length ? { inputs } : {},
  };
}

/** A script of Demido's Pine library, latest source. */
export function fromPine(
  script: Pick<PineSummary, 'id' | 'name'>,
  taken: ReadonlySet<string>,
  inputs?: Record<string, unknown>,
): SavedIndicator {
  return fromScript(`${PINE_PREFIX}${script.id}`, script.name, taken, inputs);
}

/** An indicator of a saved TradingView chart layout, set up and shown as it is there. */
export function fromLayout(study: ChartLayoutStudy, taken: ReadonlySet<string>): SavedIndicator {
  return {
    key: newKey(taken),
    script: study.id,
    version: study.version || null,
    name: study.name,
    setup: study.state,
    ...(study.hidden ? { hidden: true } : {}),
  };
}

/** What an indicator runs with: when it changes, the indicator is computed again (not when only
 *  its look or its eye does). */
export function runKey(s: SavedIndicator): string {
  return JSON.stringify([s.script, s.version, s.setup, s.revision ?? null]);
}

/** The library script id of a chart script, or null for TradingView's. */
export function pineId(script: string): string | null {
  return script.startsWith(PINE_PREFIX) ? script.slice(PINE_PREFIX.length) : null;
}

/** Input values worth keeping: those that differ from the script's defaults. */
export function changedInputs(
  inputs: readonly IndicatorInput[],
  values: Record<string, unknown>,
): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const i of inputs) {
    if (!(i.id in values)) continue;
    const v = values[i.id];
    if (JSON.stringify(v) !== JSON.stringify(i.defval)) out[i.id] = v;
  }
  return out;
}

/** TradingView's refusal when the plan's indicators-per-chart limit is reached (the market
 *  sidecar's STUDY_LIMIT wording, sidecars/market/src/indicators/studies.ts). */
export function isPlanLimit(message: string | undefined): boolean {
  return !!message && /no room for another indicator/i.test(message);
}
