/* Markets, preview data. Prices are made up from a fixed seed. The indicator maths is real: every built-in
   below is computed from those candles the usual way.

   Nothing here talks to TradingView. The "account" is made up, and so are its scripts and layouts.
   Pine scripts are NOT interpreted in the preview: each sample comes with a hand-written drawing function
   that does what the script says. The screen marks all of this. */

export interface Candle { o: number; h: number; l: number; c: number; v: number }
export type Num = number | null

/** Extra candles before the first one shown, so slow indicators have values everywhere. */
export const WARM_UP = 220
/** Candles kept per chart. Zooming and panning move inside these. */
export const HISTORY = 420

// ───────── symbols ─────────
export type Kind = 'crypto' | 'forex' | 'metal' | 'stock' | 'index'
export const KIND_LABEL: Record<Kind, string> = { crypto: 'Crypto', forex: 'Currencies', metal: 'Metals', stock: 'Stocks', index: 'Indices' }
export interface Sym { id: string; name: string; kind: Kind; venue: string; base: number; vol: number; seed: number }
const S = (id: string, name: string, kind: Kind, venue: string, base: number, vol: number, seed: number): Sym => ({ id, name, kind, venue, base, vol, seed })
export const UNIVERSE: Sym[] = [
  S('BTC/EUR', 'Bitcoin', 'crypto', 'Kraken', 84200, 0.012, 7), S('ETH/EUR', 'Ether', 'crypto', 'Kraken', 2950, 0.016, 21),
  S('SOL/EUR', 'Solana', 'crypto', 'Kraken', 168, 0.022, 31), S('XRP/EUR', 'XRP', 'crypto', 'Kraken', 1.92, 0.02, 12),
  S('ADA/EUR', 'Cardano', 'crypto', 'Kraken', 0.61, 0.021, 15), S('DOT/EUR', 'Polkadot', 'crypto', 'Kraken', 5.4, 0.022, 19),
  S('BTC/USD', 'Bitcoin', 'crypto', 'Coinbase', 91300, 0.012, 8), S('ETH/USD', 'Ether', 'crypto', 'Coinbase', 3200, 0.016, 22),
  S('LTC/EUR', 'Litecoin', 'crypto', 'Kraken', 88, 0.018, 27), S('LINK/EUR', 'Chainlink', 'crypto', 'Kraken', 14.6, 0.021, 33),
  S('EUR/USD', 'Euro / US dollar', 'forex', 'ECB reference', 1.0842, 0.002, 3), S('GBP/USD', 'Pound / US dollar', 'forex', 'ECB reference', 1.2715, 0.0022, 5),
  S('USD/JPY', 'US dollar / Yen', 'forex', 'ECB reference', 152.4, 0.0024, 9), S('EUR/GBP', 'Euro / Pound', 'forex', 'ECB reference', 0.8527, 0.0016, 11),
  S('EUR/CHF', 'Euro / Swiss franc', 'forex', 'ECB reference', 0.9412, 0.0015, 13), S('AUD/USD', 'Australian dollar / US dollar', 'forex', 'ECB reference', 0.6581, 0.0024, 17),
  S('USD/CAD', 'US dollar / Canadian dollar', 'forex', 'ECB reference', 1.3922, 0.0018, 23), S('EUR/RON', 'Euro / Romanian leu', 'forex', 'ECB reference', 4.976, 0.0008, 29),
  S('XAU/EUR', 'Gold', 'metal', 'Spot', 3610, 0.006, 44), S('XAG/EUR', 'Silver', 'metal', 'Spot', 42.8, 0.011, 46),
  S('XPT/EUR', 'Platinum', 'metal', 'Spot', 1380, 0.009, 48), S('XAU/USD', 'Gold', 'metal', 'Spot', 3915, 0.006, 45),
  S('AAPL', 'Apple', 'stock', 'Nasdaq', 241, 0.009, 51), S('MSFT', 'Microsoft', 'stock', 'Nasdaq', 468, 0.008, 53),
  S('NVDA', 'Nvidia', 'stock', 'Nasdaq', 152, 0.017, 55), S('AMZN', 'Amazon', 'stock', 'Nasdaq', 214, 0.011, 57),
  S('GOOGL', 'Alphabet', 'stock', 'Nasdaq', 187, 0.01, 59), S('TSLA', 'Tesla', 'stock', 'Nasdaq', 318, 0.022, 61),
  S('ASML', 'ASML Holding', 'stock', 'Euronext', 702, 0.012, 63), S('SAP', 'SAP', 'stock', 'Xetra', 236, 0.009, 65),
  S('SIE', 'Siemens', 'stock', 'Xetra', 198, 0.009, 67), S('MC', 'LVMH', 'stock', 'Euronext', 612, 0.011, 69),
  S('ENI', 'Eni', 'stock', 'Borsa Italiana', 14.2, 0.008, 71), S('RACE', 'Ferrari', 'stock', 'Borsa Italiana', 421, 0.01, 73),
  S('SPX', 'S&P 500', 'index', 'Index', 6120, 0.005, 81), S('NDX', 'Nasdaq 100', 'index', 'Index', 21900, 0.007, 83),
  S('DAX', 'DAX 40', 'index', 'Index', 20480, 0.006, 85), S('SX5E', 'Euro Stoxx 50', 'index', 'Index', 5210, 0.006, 87),
  S('FTSEMIB', 'FTSE MIB', 'index', 'Index', 36900, 0.007, 89), S('N225', 'Nikkei 225', 'index', 'Index', 39800, 0.008, 91),
]
export const WATCHLIST_DEFAULT = ['BTC/EUR', 'ETH/EUR', 'EUR/USD', 'XAU/EUR', 'NVDA', 'DAX']

// ───────── timeframes ─────────
export interface Timeframe { id: string; label: string; minutes: number; swing: number }
export const TIMEFRAMES: Timeframe[] = [
  { id: '1m', label: '1 min', minutes: 1, swing: 0.07 }, { id: '3m', label: '3 min', minutes: 3, swing: 0.1 },
  { id: '5m', label: '5 min', minutes: 5, swing: 0.13 }, { id: '15m', label: '15 min', minutes: 15, swing: 0.2 },
  { id: '30m', label: '30 min', minutes: 30, swing: 0.28 }, { id: '1h', label: '1 hour', minutes: 60, swing: 0.4 },
  { id: '2h', label: '2 hours', minutes: 120, swing: 0.52 }, { id: '4h', label: '4 hours', minutes: 240, swing: 0.7 },
  { id: '12h', label: '12 hours', minutes: 720, swing: 0.85 }, { id: '1D', label: '1 day', minutes: 1440, swing: 1 },
  { id: '3D', label: '3 days', minutes: 4320, swing: 1.5 }, { id: '1W', label: '1 week', minutes: 10080, swing: 2 },
  { id: '1M', label: '1 month', minutes: 43200, swing: 3.4 },
]
/** The ones on the bar; the rest sit in a menu. */
export const QUICK_TF = ['5m', '15m', '1h', '4h', '1D', '1W']

/** Made-up candles from a fixed seed, so a chart is the same on every visit. */
export function candles(base: number, vol: number, seed: number, n: number): Candle[] {
  let x = seed * 9301 + 49297
  const rnd = () => ((x = (x * 9301 + 49297) % 233280) / 233280)
  const out: Candle[] = []
  let p = base * 0.82
  let drift = 0
  for (let i = 0; i < n; i++) {
    if (i % 37 === 0) drift = (rnd() - 0.42) * vol * 0.5 // trends come and go
    const o = p
    const c = o * (1 + (rnd() - 0.5) * vol * 2 + drift)
    const h = Math.max(o, c) * (1 + rnd() * vol * 0.6)
    const l = Math.min(o, c) * (1 - rnd() * vol * 0.6)
    const v = (0.4 + rnd() * 0.6) * (1 + (Math.abs(c - o) / o / vol) * 1.5)
    out.push({ o, h, l, c, v })
    p = c
  }
  return out
}
const cache = new Map<string, Candle[]>()
export function series(sym: Sym, tf: Timeframe): Candle[] {
  const key = `${sym.id}@${tf.id}`
  let s = cache.get(key)
  if (!s) cache.set(key, (s = candles(sym.base, sym.vol * tf.swing, sym.seed + tf.minutes, HISTORY + WARM_UP)))
  return s
}
/** The newest candle closes at this moment; older ones step back by the timeframe. Fixed, like the prices. */
const NOW = Date.UTC(2026, 10, 20, 12, 0)
export const timeOf = (i: number, total: number, tf: Timeframe) => new Date(NOW - (total - 1 - i) * tf.minutes * 60_000)
export function timeLabel(d: Date, tf: Timeframe, long = false): string {
  const day = d.toLocaleDateString('en-GB', { day: 'numeric', month: 'short', timeZone: 'UTC' })
  const clock = d.toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit', timeZone: 'UTC' })
  if (tf.minutes >= 43200) return d.toLocaleDateString('en-GB', { month: 'short', year: 'numeric', timeZone: 'UTC' })
  if (tf.minutes >= 1440) return long ? `${day} ${d.getUTCFullYear()}` : day
  return long ? `${day}, ${clock}` : clock === '00:00' ? day : clock
}

/** Heikin Ashi: averaged candles that smooth a trend. */
export function heikin(c: Candle[]): Candle[] {
  const out: Candle[] = []
  c.forEach((x, i) => {
    const close = (x.o + x.h + x.l + x.c) / 4
    const open = i === 0 ? (x.o + x.c) / 2 : (out[i - 1].o + out[i - 1].c) / 2
    out.push({ o: open, c: close, h: Math.max(x.h, open, close), l: Math.min(x.l, open, close), v: x.v })
  })
  return out
}

// ───────── indicator maths ─────────
const mean = (v: number[]) => v.reduce((a, b) => a + b, 0) / v.length
const closes = (c: Candle[]) => c.map((x) => x.c)

export function sma(v: Num[], n: number): Num[] {
  return v.map((_, i) => {
    const w = v.slice(i - n + 1, i + 1)
    return i < n - 1 || w.some((x) => x === null) ? null : mean(w as number[])
  })
}
export function ema(v: Num[], n: number): Num[] {
  const k = 2 / (n + 1)
  let e: number | null = null
  let seen = 0
  return v.map((x) => {
    if (x === null) return null
    seen++
    e = e === null ? x : x * k + e * (1 - k)
    return seen < n ? null : e
  })
}
/** Wilder's moving average, used by RSI and ATR. */
function rma(v: Num[], n: number): Num[] {
  let e: number | null = null
  let seen = 0
  return v.map((x) => {
    if (x === null) return null
    seen++
    e = e === null ? x : (e * (n - 1) + x) / n
    return seen < n ? null : e
  })
}
export function stdev(v: Num[], n: number): Num[] {
  return v.map((_, i) => {
    const w = v.slice(i - n + 1, i + 1)
    if (i < n - 1 || w.some((x) => x === null)) return null
    const m = mean(w as number[])
    return Math.sqrt(mean((w as number[]).map((x) => (x - m) ** 2)))
  })
}
export function rsi(v: number[], n = 14): Num[] {
  const up = rma(v.map((x, i) => (i === 0 ? null : Math.max(x - v[i - 1], 0))), n)
  const down = rma(v.map((x, i) => (i === 0 ? null : Math.max(v[i - 1] - x, 0))), n)
  return up.map((u, i) => (u === null || down[i] === null ? null : down[i] === 0 ? 100 : 100 - 100 / (1 + u / (down[i] as number))))
}
function atr(c: Candle[], n = 14): Num[] {
  return rma(c.map((x, i) => (i === 0 ? null : Math.max(x.h - x.l, Math.abs(x.h - c[i - 1].c), Math.abs(x.l - c[i - 1].c)))), n)
}
const sub = (a: Num[], b: Num[]): Num[] => a.map((x, i) => (x === null || b[i] === null ? null : x - (b[i] as number)))
const add = (a: Num[], b: Num[]): Num[] => a.map((x, i) => (x === null || b[i] === null ? null : x + (b[i] as number)))

// ───────── what an indicator draws ─────────
export interface Line { values: Num[]; color: string; dash?: boolean; colorAt?: (i: number) => string }
export interface Plot {
  lines?: Line[]
  /** A filled area between two lines. */
  band?: { upper: Num[]; lower: Num[]; color: string; colorAt?: (i: number) => string }
  /** Columns from zero. */
  bars?: { values: Num[]; colorAt: (i: number) => string }
  guides?: number[]
  range?: [number, number]
  /** The value shown in the legend for candle i. */
  read: (i: number) => string
}
export interface Drawer { where: 'price' | 'pane'; plot: (c: Candle[]) => Plot }

const C = { blue: 'var(--mk-blue)', amber: 'var(--mk-amber)', purple: 'var(--mk-purple)', gray: 'var(--mk-gray)', teal: 'var(--mk-teal)', orange: 'var(--mk-orange)', up: 'var(--mk-up)', down: 'var(--mk-down)' }
const num = (v: Num | undefined, digits = 2) => (v == null ? '–' : Math.abs(v) >= 1000 ? v.toLocaleString(undefined, { maximumFractionDigits: 0 }) : v.toFixed(Math.abs(v) < 10 ? Math.max(digits, 4) : digits))
const one = (values: Num[], color: string, digits = 2): Plot => ({ lines: [{ values, color }], read: (i) => num(values[i], digits) })

const ribbon = (c: Candle[]): Plot => {
  const fast = ema(closes(c), 9), slow = ema(closes(c), 21)
  const tone = (i: number) => ((fast[i] ?? 0) > (slow[i] ?? 0) ? C.teal : C.orange)
  return { lines: [{ values: fast, color: C.teal, colorAt: tone }, { values: slow, color: C.gray }], band: { upper: fast, lower: slow, color: C.teal, colorAt: tone }, read: (i) => `${num(fast[i])} · ${num(slow[i])}` }
}
const wave = (c: Candle[]): Plot => {
  const v = closes(c)
  const w = ema(v.map((x, i) => (i < 10 ? null : ((x - v[i - 10]) / v[i - 10]) * 100)), 5)
  return { bars: { values: w, colorAt: (i) => ((w[i] ?? 0) >= 0 ? C.teal : C.orange) }, guides: [0], read: (i) => num(w[i]) }
}
const fade = (c: Candle[]): Plot => {
  const r = rsi(closes(c), 14)
  const basis = sma(r, 20), dev = stdev(r, 20)
  const upper = add(basis, dev), lower = sub(basis, dev)
  return { lines: [{ values: r, color: C.purple }, { values: upper, color: C.gray }, { values: lower, color: C.gray }], band: { upper, lower, color: C.gray }, guides: [50], range: [0, 100], read: (i) => num(r[i], 1) }
}

/** Every drawing function the preview has, by id. Catalogue entries point at these. */
export const DRAW: Record<string, Drawer> = {
  ema20: { where: 'price', plot: (c) => one(ema(closes(c), 20), C.blue) },
  ema50: { where: 'price', plot: (c) => one(ema(closes(c), 50), C.amber) },
  sma200: { where: 'price', plot: (c) => one(sma(closes(c), 200), C.purple) },
  bb: {
    where: 'price',
    plot: (c) => {
      const mid = sma(closes(c), 20)
      const dev = stdev(closes(c), 20).map((d) => (d === null ? null : d * 2))
      const upper = add(mid, dev), lower = sub(mid, dev)
      return { lines: [{ values: mid, color: C.gray, dash: true }, { values: upper, color: C.gray }, { values: lower, color: C.gray }], band: { upper, lower, color: C.gray }, read: (i) => `${num(upper[i])} · ${num(lower[i])}` }
    },
  },
  vwap: {
    where: 'price',
    plot: (c) => {
      let pv = 0, vol = 0
      // anchored where the kept history starts; the real one restarts every session
      const v = c.map((x, i) => { if (i < WARM_UP) return null; pv += ((x.h + x.l + x.c) / 3) * x.v; vol += x.v; return pv / vol })
      return one(v, C.orange)
    },
  },
  supertrend: {
    where: 'price',
    plot: (c) => {
      const a = atr(c, 10)
      let up = NaN, dn = NaN, dir = 1
      const dirs: number[] = []
      const line = c.map((x, i) => {
        const r = a[i]
        if (r === null) { dirs.push(1); return null }
        const mid = (x.h + x.l) / 2
        let bu = mid + 3 * r, bl = mid - 3 * r
        if (!Number.isNaN(up)) {
          bu = bu < up || c[i - 1].c > up ? bu : up
          bl = bl > dn || c[i - 1].c < dn ? bl : dn
          dir = x.c > up ? 1 : x.c < dn ? -1 : dir
        }
        up = bu; dn = bl
        dirs.push(dir)
        return dir === 1 ? dn : up
      })
      return { lines: [{ values: line, color: C.up, colorAt: (i) => (dirs[i] === 1 ? C.up : C.down) }], read: (i) => num(line[i]) }
    },
  },
  vol: { where: 'pane', plot: (c) => ({ bars: { values: c.map((x) => x.v), colorAt: (i) => (c[i].c >= c[i].o ? C.up : C.down) }, read: (i) => `${(c[i].v * 1000).toFixed(0)}` }) },
  rsi: { where: 'pane', plot: (c) => ({ ...one(rsi(closes(c), 14), C.purple, 1), guides: [30, 70], range: [0, 100] }) },
  macd: {
    where: 'pane',
    plot: (c) => {
      const line = sub(ema(closes(c), 12), ema(closes(c), 26))
      const signal = ema(line, 9)
      const hist = sub(line, signal)
      return { bars: { values: hist, colorAt: (i) => ((hist[i] ?? 0) >= 0 ? C.up : C.down) }, lines: [{ values: line, color: C.blue }, { values: signal, color: C.amber }], guides: [0], read: (i) => `${num(line[i])} · ${num(signal[i])}` }
    },
  },
  stoch: {
    where: 'pane',
    plot: (c) => {
      const raw: Num[] = c.map((x, i) => {
        if (i < 13) return null
        const w = c.slice(i - 13, i + 1)
        const hh = Math.max(...w.map((d) => d.h)), ll = Math.min(...w.map((d) => d.l))
        return hh === ll ? 50 : (100 * (x.c - ll)) / (hh - ll)
      })
      const k = sma(raw, 3), d = sma(k, 3)
      return { lines: [{ values: k, color: C.blue }, { values: d, color: C.amber }], guides: [20, 80], range: [0, 100], read: (i) => `${num(k[i], 1)} · ${num(d[i], 1)}` }
    },
  },
  atr: { where: 'pane', plot: (c) => one(atr(c, 14), C.teal) },
  obv: {
    where: 'pane',
    plot: (c) => { let t = 0; return one(c.map((x, i) => (i === 0 ? 0 : (t += x.c > c[i - 1].c ? x.v : x.c < c[i - 1].c ? -x.v : 0))), C.blue, 1) },
  },
  ribbon: { where: 'price', plot: ribbon },
  wave: { where: 'pane', plot: wave },
  fade: { where: 'pane', plot: fade },
}

// ───────── the Indicators menu ─────────
/** Who works an indicator out. TradingView's own, on TradingView's servers; or Demido, on this computer. */
export type Engine = 'tradingview' | 'local'
export type Shelf = 'favourites' | 'mine' | 'tv' | 'community' | 'local' | 'pine'
export interface Entry {
  id: string
  name: string
  shelf: Shelf
  /** Which drawing function stands in for it in the preview. */
  draw: string
  author?: string
  /** Community scripts: open source, closed source, or by invitation only. */
  access?: 'open' | 'protected' | 'invite'
  /** Demido can work it out itself: it has the maths (built-ins), or the script's source can be read (your own
      scripts, open-source community scripts). Closed-source scripts can only run at TradingView. */
  localToo?: boolean
}
const tv = (id: string, name: string, draw: string, localToo = true): Entry => ({ id: `STD;${id}`, name, shelf: 'tv', draw, localToo })
/** TradingView's built-in indicators. The real list has more than a hundred; these are the ones the preview can draw. */
export const TV_BUILT_IN: Entry[] = [
  tv('ATR', 'Average True Range', 'atr'), tv('BB', 'Bollinger Bands', 'bb'), tv('MACD', 'MACD', 'macd'),
  tv('EMA', 'Moving Average Exponential', 'ema20'), tv('SMA', 'Moving Average Simple', 'sma200'), tv('OBV', 'On Balance Volume', 'obv'),
  tv('RSI', 'Relative Strength Index', 'rsi'), tv('Stochastic', 'Stochastic', 'stoch'), tv('Supertrend', 'Supertrend', 'supertrend'),
  tv('Volume', 'Volume', 'vol'), tv('VWAP', 'VWAP', 'vwap'),
]
export const TV_BUILT_IN_TOTAL = 112
export const TV_MINE: Entry[] = [
  { id: 'USER;ribbon', name: 'Trend ribbon', shelf: 'mine', draw: 'ribbon', author: 'you', localToo: true },
  { id: 'USER;wave', name: 'Momentum wave', shelf: 'mine', draw: 'wave', author: 'you', localToo: true },
]
export const TV_FAVOURITES = ['STD;RSI', 'STD;MACD', 'USER;ribbon', 'PUB;squeeze']
export const TV_COMMUNITY: Entry[] = [
  { id: 'PUB;squeeze', name: 'Squeeze and release', shelf: 'community', draw: 'wave', author: 'northwind_charts', access: 'open', localToo: true },
  { id: 'PUB;bands', name: 'Adaptive RSI bands', shelf: 'community', draw: 'fade', author: 'quietquant', access: 'open', localToo: true },
  { id: 'PUB;cloud', name: 'Trend cloud', shelf: 'community', draw: 'ribbon', author: 'lowtide', access: 'protected' },
  { id: 'PUB;flow', name: 'Order flow pressure', shelf: 'community', draw: 'obv', author: 'tapereader', access: 'protected' },
  { id: 'PUB;edge', name: 'Smart edge signals', shelf: 'community', draw: 'supertrend', author: 'edge_labs', access: 'invite' },
]
/** Demido's own maths: needs no account and counts toward no limit. */
export const LOCAL: Entry[] = [
  { id: 'L;ema20', name: 'EMA 20', shelf: 'local', draw: 'ema20' }, { id: 'L;ema50', name: 'EMA 50', shelf: 'local', draw: 'ema50' },
  { id: 'L;sma200', name: 'SMA 200', shelf: 'local', draw: 'sma200' }, { id: 'L;bb', name: 'Bollinger Bands 20, 2', shelf: 'local', draw: 'bb' },
  { id: 'L;vwap', name: 'VWAP', shelf: 'local', draw: 'vwap' }, { id: 'L;supertrend', name: 'Supertrend 10, 3', shelf: 'local', draw: 'supertrend' },
  { id: 'L;vol', name: 'Volume', shelf: 'local', draw: 'vol' }, { id: 'L;rsi', name: 'RSI 14', shelf: 'local', draw: 'rsi' },
  { id: 'L;macd', name: 'MACD 12, 26, 9', shelf: 'local', draw: 'macd' }, { id: 'L;stoch', name: 'Stochastic 14, 3, 3', shelf: 'local', draw: 'stoch' },
  { id: 'L;atr', name: 'ATR 14', shelf: 'local', draw: 'atr' }, { id: 'L;obv', name: 'On Balance Volume', shelf: 'local', draw: 'obv' },
]

/** One indicator on a chart. */
export interface OnChart { key: string; entry: string; name: string; draw: string; engine: Engine; hidden: boolean }

/** The made-up TradingView account. */
export const ACCOUNT = {
  user: 'demo_trader',
  plan: 'Basic (free)',
  /** Indicators per chart that this plan lets TradingView work out. Demido works out any beyond it. */
  limit: 2,
  layouts: [
    { id: 'swing', name: 'Swing setup', symbol: 'BTC/EUR', tf: '1D', saved: '18 Nov', indicators: ['STD;BB', 'STD;RSI', 'STD;MACD', 'USER;ribbon', 'PUB;edge'], up: '#2bb3a3', down: '#f08a3c' },
    { id: 'scalp', name: 'Scalping', symbol: 'EUR/USD', tf: '5m', saved: '9 Nov', indicators: ['STD;EMA', 'STD;VWAP', 'STD;Stochastic', 'STD;Volume'], up: '#26a69a', down: '#ef5350' },
    { id: 'gold', name: 'Gold, weekly', symbol: 'XAU/EUR', tf: '1W', saved: '2 Oct', indicators: ['STD;SMA', 'STD;Supertrend', 'STD;ATR'], up: '#e6e6e6', down: '#7a7a7a' },
  ],
}

// ───────── Pine scripts kept on this computer ─────────
export interface PineScript { id: string; name: string; by: 'you' | 'model'; draw: string; source: string }
export const PINE: PineScript[] = [
  {
    id: 'ribbon', name: 'Trend ribbon', by: 'you', draw: 'ribbon',
    source: `//@version=6
indicator("Trend ribbon", overlay = true)

fast = ta.ema(close, 9)
slow = ta.ema(close, 21)
up   = fast > slow

p1 = plot(fast, "Fast", color = up ? color.teal : color.orange)
p2 = plot(slow, "Slow", color = color.gray)
fill(p1, p2, color = color.new(up ? color.teal : color.orange, 80))
`,
  },
  {
    id: 'wave', name: 'Momentum wave', by: 'you', draw: 'wave',
    source: `//@version=6
indicator("Momentum wave")

length = input.int(10, "Length")
roc    = ta.roc(close, length)
wave   = ta.ema(roc, 5)

plot(wave, "Wave", style = plot.style_columns,
     color = wave >= 0 ? color.teal : color.orange)
hline(0)
`,
  },
]

/* The script a model writes in the preview. The first draft carries a typo on purpose: the point is the
   test run that catches it and the fix that follows. */
const FADE_SOURCE = `//@version=6
indicator("RSI fade bands")

length = input.int(14, "RSI length")
r      = ta.rsi(close, length)
basis  = ta.sma(r, 20)
dev    = ta.stdev(r, 20)

plot(r, "RSI", color = color.purple)
plot(basis + dev, "Upper", color = color.gray)
plot(basis - dev, "Lower", color = color.gray)
hline(50)
`
export const MODEL_SCRIPT: PineScript = { id: 'fade', name: 'RSI fade bands', by: 'model', draw: 'fade', source: FADE_SOURCE }
export const MODEL_DRAFT = FADE_SOURCE.replace('ta.rsi(close, length)', 'ta.rsi(close, lenght)')

export interface RunStep { state: 'running' | 'done' | 'failed'; text: string; detail?: string }
/** What the model does, step by step. `at` is milliseconds from the start. */
export const MODEL_RUN: { at: number; step: RunStep; source?: string; finish?: boolean }[] = [
  { at: 0, step: { state: 'running', text: 'Writing the script' } },
  { at: 1500, step: { state: 'done', text: 'Wrote "RSI fade bands"', detail: '12 lines of Pine Script, version 6' }, source: MODEL_DRAFT },
  { at: 1700, step: { state: 'running', text: 'Test run on this chart' } },
  { at: 3000, step: { state: 'failed', text: 'Test run failed', detail: 'Line 5: undeclared identifier "lenght"' } },
  { at: 3200, step: { state: 'running', text: 'Fixing line 5' } },
  { at: 4300, step: { state: 'done', text: 'Fixed line 5', detail: '"lenght" is now "length"' }, source: FADE_SOURCE },
  { at: 4500, step: { state: 'running', text: 'Test run on this chart' } },
  { at: 5900, step: { state: 'done', text: 'Test run passed', detail: 'A value on every candle. No look-ahead found.' }, finish: true },
]

/** What signing in does by itself, step by step. */
export const SIGN_IN_RUN: { at: number; step: RunStep; finish?: boolean }[] = [
  { at: 0, step: { state: 'running', text: 'Checking the session with TradingView' } },
  { at: 900, step: { state: 'done', text: `Signed in as ${ACCOUNT.user}`, detail: `${ACCOUNT.plan} plan: ${ACCOUNT.limit} indicators per chart` } },
  { at: 1100, step: { state: 'running', text: 'Reading your favourites and your scripts' } },
  { at: 2000, step: { state: 'done', text: 'Read 4 favourites and 2 scripts of yours' } },
  { at: 2200, step: { state: 'running', text: 'Reading your chart layouts' } },
  { at: 3100, step: { state: 'done', text: `Found ${ACCOUNT.layouts.length} chart layouts`, detail: 'Newest: "Swing setup", saved 18 Nov' } },
  { at: 3300, step: { state: 'running', text: 'Loading "Swing setup": indicators and colours' } },
  { at: 4500, step: { state: 'failed', text: '1 indicator skipped', detail: '"Smart edge signals" is invite-only and this account has no access to it.' } },
  { at: 4700, step: { state: 'done', text: '4 indicators and the candle colours are on the chart', detail: `TradingView works out ${ACCOUNT.limit}, as your plan allows. Demido works out the others itself.` }, finish: true },
]
