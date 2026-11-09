// MetaTrader's 1-minute bars as the store keeps a day: a Dukascopy candle response (store/decode.ts),
// so the store, series and reads treat both sources alike. The first candle is the base, its deltas
// zero; prices are integer units of 10^-digits; volume is MT5's tick volume.
//
// API: byDay(bars) -> Map<UTC day start, Bar[]>;  encodeDay(bars, digits) -> Buffer (JSON)

import { type Bar } from '../protocol.ts';
import { type CandleResponse } from '../store/decode.ts';

const DAY = 86400;

export function byDay(bars: readonly Bar[]): Map<number, Bar[]> {
  const out = new Map<number, Bar[]>();
  for (const bar of bars) {
    const day = Math.floor(bar.t / DAY) * DAY;
    const list = out.get(day);
    if (list) list.push(bar);
    else out.set(day, [bar]);
  }
  return out;
}

/** One day's bars (ascending, minute-aligned, one UTC day) as a candle response. */
export function encodeDay(bars: readonly Bar[], digits: number): Buffer {
  const d = Math.max(0, Math.min(10, Math.floor(digits)));
  const multiplier = Number(`1e-${d}`);
  const units = (price: number) => Math.round(price * 10 ** d);
  const first = bars[0];
  const response: CandleResponse = {
    timestamp: first ? first.t * 1000 : 0,
    multiplier,
    shift: 60_000,
    open: first ? first.o : null,
    high: first ? first.h : null,
    low: first ? first.l : null,
    close: first ? first.c : null,
    times: [],
    opens: [],
    highs: [],
    lows: [],
    closes: [],
    volumes: [],
  };
  let prev: Bar | null = null;
  for (const bar of bars) {
    response.times.push(prev ? (bar.t - prev.t) / 60 : 0);
    response.opens.push(prev ? units(bar.o) - units(prev.o) : 0);
    response.highs.push(prev ? units(bar.h) - units(prev.h) : 0);
    response.lows.push(prev ? units(bar.l) - units(prev.l) : 0);
    response.closes.push(prev ? units(bar.c) - units(prev.c) : 0);
    response.volumes.push(bar.v);
    prev = bar;
  }
  return Buffer.from(JSON.stringify(response));
}
