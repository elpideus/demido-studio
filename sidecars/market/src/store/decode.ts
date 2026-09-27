// Decodes one raw Dukascopy candle response (a bucket) into bars. The API answers in columns of
// deltas: `times[i]` counts `shift` steps from the previous candle (from `timestamp` for the first),
// and each price column is an integer delta in units of `multiplier` added to the previous candle's
// value (starting from the base `open/high/low/close`). Derived from dukascopy-node's `normalise`,
// but: seconds instead of ms, no synthetic flat rows for skipped steps, no volume filter (real
// zero-volume candles stay), and no dependency on which tier the URL named.
//
// API:
//   decode(raw)       Buffer | string | parsed object -> Bar[] ascending; throws DecodeError if malformed
//   isEmpty(raw)      the response is valid and holds no candles (weekends, holes); throws if malformed
//   parseResponse(raw)  the validated response object

import { type Bar } from '../protocol.ts';

export class DecodeError extends Error {}

export interface CandleResponse {
  timestamp: number;
  multiplier: number;
  shift: number;
  open: number | null;
  high: number | null;
  low: number | null;
  close: number | null;
  times: number[];
  opens: number[];
  highs: number[];
  lows: number[];
  closes: number[];
  volumes: number[];
}

const COLUMNS = ['times', 'opens', 'highs', 'lows', 'closes', 'volumes'] as const;

function toObject(raw: Buffer | Uint8Array | string | object): Record<string, unknown> {
  let value: unknown = raw;
  if (typeof raw === 'string' || raw instanceof Uint8Array) {
    const text =
      typeof raw === 'string' ? raw : Buffer.from(raw.buffer, raw.byteOffset, raw.byteLength).toString('utf8');
    try {
      value = JSON.parse(text);
    } catch {
      throw new DecodeError('Invalid candle response: not JSON');
    }
  }
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    throw new DecodeError('Invalid candle response: not an object');
  }
  return value as Record<string, unknown>;
}

const finite = (v: unknown): v is number => typeof v === 'number' && Number.isFinite(v);

export function parseResponse(raw: Buffer | Uint8Array | string | object): CandleResponse {
  const data = toObject(raw);
  if (!finite(data.timestamp)) throw new DecodeError('Invalid candle response: missing timestamp');
  const length = Array.isArray(data.times) ? data.times.length : -1;
  for (const name of COLUMNS) {
    const column = data[name];
    if (!Array.isArray(column) || column.length !== length) {
      throw new DecodeError('Invalid candle response: column lengths do not match');
    }
    if (!column.every(finite)) throw new DecodeError(`Invalid candle response: ${name} must be finite numbers`);
  }
  if (!(data.times as number[]).every((t) => Number.isInteger(t) && t >= 0)) {
    throw new DecodeError('Invalid candle response: time deltas must be non-negative integers');
  }
  if (length > 0) {
    if (!finite(data.multiplier) || data.multiplier <= 0) {
      throw new DecodeError('Invalid candle response: multiplier must be a positive number');
    }
    if (!finite(data.shift) || data.shift <= 0) throw new DecodeError('Invalid candle response: missing shift');
    if (![data.open, data.high, data.low, data.close].every(finite)) {
      throw new DecodeError('Invalid candle response: missing base candle');
    }
  }
  return data as unknown as CandleResponse;
}

export function isEmpty(raw: Buffer | Uint8Array | string | object): boolean {
  return parseResponse(raw).times.length === 0;
}

/** Decimal places of the multiplier (1e-5 -> 5, 0.001 -> 3), the precision prices are quoted in. */
function priceScale(multiplier: number): number {
  const [coefficient = '', exponent = '0'] = multiplier.toString().toLowerCase().split('e');
  const decimals = coefficient.split('.')[1]?.length ?? 0;
  return Math.max(0, decimals - Number(exponent));
}

export function decode(raw: Buffer | Uint8Array | string | object): Bar[] {
  const data = parseResponse(raw);
  const n = data.times.length;
  if (n === 0) return [];
  const { multiplier, shift } = data;
  const scale = priceScale(multiplier);
  const price = (units: number) => Number((units * multiplier).toFixed(scale));
  // Integer units, so accumulating deltas never drifts.
  let open = Math.round(data.open! / multiplier);
  let high = Math.round(data.high! / multiplier);
  let low = Math.round(data.low! / multiplier);
  let close = Math.round(data.close! / multiplier);
  let ms = data.timestamp;
  const bars: Bar[] = new Array(n);
  for (let i = 0; i < n; i += 1) {
    ms += data.times[i]! * shift;
    open += data.opens[i]!;
    high += data.highs[i]!;
    low += data.lows[i]!;
    close += data.closes[i]!;
    bars[i] = {
      t: Math.floor(ms / 1000),
      o: price(open),
      h: price(high),
      l: price(low),
      c: price(close),
      v: data.volumes[i]!,
    };
  }
  return bars;
}
