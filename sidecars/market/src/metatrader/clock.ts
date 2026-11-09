// A MetaTrader server's clock. MT5 stamps bars with the server's wall clock, not UTC, and brokers
// pick their own: most run New York's clock plus 7 hours (so the FX week opens at 00:00 Monday),
// MetaQuotes' servers run Athens' (EET, the EU's summer time), a few CET, London or a fixed offset.
// Most of the year NY+7 and EET agree; they differ in the weeks the US and the EU change to or from
// summer time on different dates, so those weeks decide between them.
//
// Evidence is an offset measured at a UTC instant:
//   - a fresh tick: its server time against this machine's clock (only while FX trades);
//   - a weekly open: the first FX bar after a weekend gap, against Sunday 17:00 New York, when FX
//     opens. Rounded to the hour; a late open (a holiday) is not evidence.
// `chooseModel` takes the model that fits the most evidence (NY+7 first on a tie, then EET); with
// none it assumes NY+7. Per week, `weekShifts` corrects an FX pair's own history where its weekly
// open says the server ran the other of NY+7 and EET that week (old histories did, per symbol).
//
// API: offsetAt(model, utc), toUtc(model, server), toServer(model, utc), fxOpenUtc(serverTime),
//      weeklyOpens(serverTimes), tickEvidence(tickMs, nowMs), chooseModel(evidence),
//      weekShifts(model, serverTimes) -> Map<week start (server), offset>, MODELS

export const MODELS = ['ny+7', 'eet', 'cet', 'gmt', 'utc+2', 'utc+3', 'utc', 'utc+1'] as const;
export type ZoneModel = (typeof MODELS)[number];

export interface Evidence {
  /** UTC seconds. */
  at: number;
  /** Seconds the server's clock is ahead of UTC then. */
  offset: number;
}

export interface ModelChoice {
  model: ZoneModel;
  /** Evidence it fits, out of `of`. */
  fit: number;
  of: number;
  /** No evidence: NY+7, the most common broker clock. */
  assumed: boolean;
}

const HOUR = 3600;
const DAY = 86400;
const ZONES: Partial<Record<ZoneModel, [string, number]>> = {
  'ny+7': ['America/New_York', 7 * HOUR],
  eet: ['Europe/Athens', 0],
  cet: ['Europe/Berlin', 0],
  gmt: ['Europe/London', 0],
};
const FIXED: Partial<Record<ZoneModel, number>> = { utc: 0, 'utc+1': HOUR, 'utc+2': 2 * HOUR, 'utc+3': 3 * HOUR };

const formatters = new Map<string, Intl.DateTimeFormat>();
/** Zone offsets per UTC hour (zones change on whole hours). */
const hourly = new Map<string, number>();

function zoneOffset(timeZone: string, utc: number): number {
  const hour = Math.floor(utc / HOUR);
  const key = `${timeZone}@${hour}`;
  const known = hourly.get(key);
  if (known !== undefined) return known;
  let f = formatters.get(timeZone);
  if (!f) {
    f = new Intl.DateTimeFormat('en-US', {
      timeZone,
      hourCycle: 'h23',
      year: 'numeric',
      month: 'numeric',
      day: 'numeric',
      hour: 'numeric',
      minute: 'numeric',
    });
    formatters.set(timeZone, f);
  }
  const at = hour * HOUR;
  const parts: Record<string, number> = {};
  for (const p of f.formatToParts(new Date(at * 1000))) if (p.type !== 'literal') parts[p.type] = Number(p.value);
  const local = Date.UTC(parts.year!, parts.month! - 1, parts.day!, parts.hour!, parts.minute!) / 1000;
  const offset = local - at;
  if (hourly.size > 200_000) hourly.clear();
  hourly.set(key, offset);
  return offset;
}

/** Seconds the model's clock is ahead of UTC at a UTC instant. */
export function offsetAt(model: ZoneModel, utc: number): number {
  const fixed = FIXED[model];
  if (fixed !== undefined) return fixed;
  const [zone, plus] = ZONES[model]!;
  return zoneOffset(zone, utc) + plus;
}

export function toServer(model: ZoneModel, utc: number): number {
  return utc + offsetAt(model, utc);
}

/** UTC for a server time. Around a change of clock the earlier reading wins (markets are closed then). */
export function toUtc(model: ZoneModel, server: number): number {
  let utc = server - offsetAt(model, server);
  for (let i = 0; i < 2; i += 1) {
    const next = server - offsetAt(model, utc);
    if (next === utc) break;
    utc = next;
  }
  return utc;
}

/** UTC of 17:00 New York on a calendar date. */
function newYork17(year: number, month: number, day: number): number {
  const wall = Date.UTC(year, month, day, 17) / 1000;
  let utc = wall + 5 * HOUR;
  for (let i = 0; i < 2; i += 1) utc = wall - zoneOffset('America/New_York', utc);
  return utc;
}

/** The FX week open (Sunday 17:00 New York, UTC) that a first bar after a weekend at `server` would
 *  follow, or null when the bar is not on the Sunday evening or Monday morning of one. */
export function fxOpenUtc(server: number): number | null {
  // Servers run 0 to 3 hours ahead of UTC (a few run up to 4 ahead or 1 behind): New York's date of
  // (server - 3 h) is the Sunday of the open, unless the bar came on Monday after a holiday.
  const guess = server - 3 * HOUR;
  const ny = new Date((guess + zoneOffset('America/New_York', guess)) * 1000);
  if (ny.getUTCDay() !== 0) return null;
  return newYork17(ny.getUTCFullYear(), ny.getUTCMonth(), ny.getUTCDate());
}

/** Offsets measured from the first bar after each weekend gap of an FX pair (server times, ascending). */
export function weeklyOpens(serverTimes: readonly number[]): Evidence[] {
  const out: Evidence[] = [];
  for (let i = 1; i < serverTimes.length; i += 1) {
    const first = serverTimes[i]!;
    if (first - serverTimes[i - 1]! < 36 * HOUR) continue;
    const open = fxOpenUtc(first);
    if (open === null) continue;
    const ahead = first - open;
    const offset = Math.round(ahead / HOUR) * HOUR;
    // A broker opens within minutes of the market; later than 20 min is a late open, not a clock.
    if (Math.abs(ahead - offset) > 20 * 60 || offset < -HOUR || offset > 4 * HOUR) continue;
    out.push({ at: open, offset });
  }
  return out;
}

/** FX trades (Sunday 22:00 to Friday 21:00 UTC, with an hour's margin at each end). */
function fxTrading(utc: number): boolean {
  const d = new Date(utc * 1000);
  const day = d.getUTCDay();
  const hour = d.getUTCHours();
  if (day === 6) return false;
  if (day === 0) return hour >= 23;
  if (day === 5) return hour < 20;
  return true;
}

/** A fresh tick's offset: its server time against this machine's clock, in quarter hours. */
export function tickEvidence(tickMs: number, nowMs: number): Evidence | null {
  const now = Math.floor(nowMs / 1000);
  if (!fxTrading(now)) return null;
  const ahead = (tickMs - nowMs) / 1000;
  const offset = Math.round(ahead / 900) * 900;
  // A tick older than 10 s (or a clock off by more) says nothing exact.
  if (Math.abs(ahead - offset) > 10 || offset < -12 * HOUR || offset > 14 * HOUR) return null;
  return { at: now, offset };
}

export function chooseModel(evidence: readonly Evidence[]): ModelChoice {
  if (evidence.length === 0) return { model: 'ny+7', fit: 0, of: 0, assumed: true };
  let best: ModelChoice | null = null;
  for (const model of MODELS) {
    const fit = evidence.filter((e) => offsetAt(model, e.at) === e.offset).length;
    if (!best || fit > best.fit) best = { model, fit, of: evidence.length, assumed: false };
  }
  return best!;
}

/** The server-time start of the trading week a server time falls in (its Saturday 00:00). */
export function weekOf(server: number): number {
  const day = Math.floor(server / DAY);
  // 1970-01-01 was a Thursday; Saturday is 2 days later.
  return (day - ((((day - 2) % 7) + 7) % 7)) * DAY;
}

/** Weeks of an FX pair's history whose open says the server ran NY+7 where the model says EET, or
 *  the other way round: week start (server) -> the offset that week. */
export function weekShifts(model: ZoneModel, serverTimes: readonly number[]): Map<number, number> {
  const out = new Map<number, number>();
  if (model !== 'ny+7' && model !== 'eet') return out;
  const other: ZoneModel = model === 'ny+7' ? 'eet' : 'ny+7';
  for (const e of weeklyOpens(serverTimes)) {
    if (e.offset === offsetAt(model, e.at) || e.offset !== offsetAt(other, e.at)) continue;
    out.set(weekOf(e.at + e.offset), e.offset);
  }
  return out;
}
