import { useCallback, useEffect, useRef, useState } from 'react';
import { History, LogIn, LogOut, Radio, Search, UserRound } from 'lucide-react';
import { Button, EmptyState, Menu, Popover, SegmentedControl, Spinner, TextField, cx, useDebounced } from '@demido/ui';

import { api, errorText } from '@/lib/api';
import { on } from '@/lib/events';
import { formatPercent, formatPrice } from '@/lib/format';
import type { Bar, ChartInfo, Quote, SymbolMatch } from '@/lib/types';
import { useMarket } from '@/stores/market';
import { toast } from '@/stores/toasts';
import { type WindowState, useWindows } from '@/stores/windows';
import { PriceChart, type PriceChartHandle } from './PriceChart';
import styles from './MarketWindow.module.css';

const TIMEFRAMES = ['1m', '5m', '15m', '1h', '4h', '1d', '1w'] as const;
type Tf = (typeof TIMEFRAMES)[number];
const TF_SECONDS: Record<Tf, number> = { '1m': 60, '5m': 300, '15m': 900, '1h': 3600, '4h': 14400, '1d': 86400, '1w': 604800 };
const POPULAR = ['FX:EURUSD', 'OANDA:XAUUSD', 'BINANCE:BTCUSDT', 'SP:SPX', 'NASDAQ:NDX', 'NASDAQ:AAPL', 'NASDAQ:NVDA', 'FX:GBPUSD'];

type Mode = 'loading' | 'live' | 'history' | 'signin' | 'error';

function SymbolSearch({ onPick }: { onPick: (symbol: string) => void }) {
  const [query, setQuery] = useState('');
  const [open, setOpen] = useState(false);
  const [results, setResults] = useState<SymbolMatch[]>([]);
  const [busy, setBusy] = useState(false);
  const debounced = useDebounced(query, 250);
  const anchor = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const q = debounced.trim();
    if (!q) {
      setResults([]);
      return;
    }
    setBusy(true);
    api
      .marketSearch(q)
      .then(setResults, () => setResults([]))
      .finally(() => setBusy(false));
  }, [debounced]);

  const pick = (symbol: string) => {
    onPick(symbol);
    setQuery('');
    setOpen(false);
  };

  return (
    <div ref={anchor} className={styles.search}>
      <TextField
        icon={Search}
        size="sm"
        placeholder="Search a symbol"
        value={query}
        onFocus={() => setOpen(true)}
        onChange={(e) => {
          setQuery(e.target.value);
          setOpen(true);
        }}
        onKeyDown={(e) => {
          if (e.key === 'Enter' && results[0]) pick(results[0].symbol);
          if (e.key === 'Escape') setOpen(false);
        }}
        trailing={busy ? <Spinner size={12} /> : undefined}
      />
      <Popover open={open} onClose={() => setOpen(false)} anchorRef={anchor} placement="bottom-start" width={380} maxHeight={380}>
        <div className={styles.results}>
          {!query.trim() && <div className={styles.resultsTitle}>Popular</div>}
          {(query.trim() ? results : POPULAR.map((p) => ({ symbol: p, ticker: p.split(':')[1] ?? p, description: '', exchange: p.split(':')[0] ?? '', type: '' })))
            .slice(0, 14)
            .map((r) => (
              <button key={r.symbol} type="button" className={styles.result} onClick={() => pick(r.symbol)}>
                <span className={styles.resultTicker}>{r.ticker}</span>
                <span className={styles.resultDesc}>{r.description || r.exchange}</span>
                <span className={styles.resultMeta}>{r.type || r.exchange}</span>
              </button>
            ))}
          {query.trim() && !busy && results.length === 0 && <div className={styles.resultsEmpty}>No symbols match.</div>}
        </div>
      </Popover>
    </div>
  );
}

function Account() {
  const status = useMarket((s) => s.status);
  const login = useMarket((s) => s.login);
  const logout = useMarket((s) => s.logout);
  const [menu, setMenu] = useState(false);
  const anchor = useRef<HTMLButtonElement>(null);
  if (!status?.available) return null;
  if (!status.loggedIn) {
    return (
      <Button size="sm" variant="primary" icon={LogIn} loading={status.loginPending} onClick={() => void login()}>
        {status.loginPending ? 'Waiting for sign-in' : 'Sign in'}
      </Button>
    );
  }
  return (
    <>
      <Button ref={anchor} size="sm" variant="ghost" icon={UserRound} onClick={() => setMenu(true)}>
        {status.username ?? 'TradingView'}
      </Button>
      <Menu
        open={menu}
        onClose={() => setMenu(false)}
        anchorRef={anchor}
        items={[{ id: 'logout', label: 'Sign out of TradingView', icon: LogOut, onSelect: () => void logout() }]}
        width={230}
      />
    </>
  );
}

/** Charts with live TradingView data, and Dukascopy history when signed out. */
export function MarketWindow({ win }: { win: WindowState }) {
  const setProps = useWindows((s) => s.setProps);
  const status = useMarket((s) => s.status);
  const loggedIn = !!status?.loggedIn;
  const symbol = typeof win.props.symbol === 'string' ? win.props.symbol : 'FX:EURUSD';
  const timeframe = (TIMEFRAMES.includes(win.props.timeframe as Tf) ? win.props.timeframe : '1h') as Tf;
  const chart = useRef<PriceChartHandle>(null);
  const [mode, setMode] = useState<Mode>('loading');
  const [info, setInfo] = useState<ChartInfo | null>(null);
  const [quote, setQuote] = useState<Quote | null>(null);
  const [last, setLast] = useState<Bar | null>(null);
  const [error, setError] = useState<string | null>(null);
  const stream = useRef<string | null>(null);
  const earliest = useRef<number | null>(null);
  const loadingMore = useRef(false);
  const exhausted = useRef(false);
  const dukascopy = useRef<string | null>(null);
  const [reload, setReload] = useState(0);

  useEffect(() => {
    let cancelled = false;
    let opened: string | null = null;
    setMode('loading');
    setError(null);
    setQuote(null);
    exhausted.current = false;
    earliest.current = null;
    dukascopy.current = null;

    const run = async () => {
      if (loggedIn) {
        const res = await api.marketOpenStream(symbol, timeframe, 500);
        if (cancelled) {
          void api.marketCloseStream(res.id);
          return;
        }
        opened = res.id;
        stream.current = res.id;
        setInfo(res.info);
        chart.current?.setBars(res.bars);
        earliest.current = res.bars[0]?.t ?? null;
        setLast(res.bars[res.bars.length - 1] ?? null);
        setMode('live');
        api.marketQuote([symbol]).then(([q]) => !cancelled && q && !q.error && setQuote(q), () => undefined);
        return;
      }
      const resolved = await api.marketResolveDukascopy(symbol).catch(() => null);
      if (cancelled) return;
      if (!resolved) {
        setMode('signin');
        return;
      }
      dukascopy.current = resolved.instrument;
      const span = TF_SECONDS[timeframe] * 600;
      const from = new Date(Date.now() - span * 1000).toISOString().slice(0, 10);
      const res = await api.marketHistory(resolved.instrument, timeframe === '1w' ? '1w' : timeframe, from);
      if (cancelled) return;
      setInfo({ ...res.info, symbol, pricescale: guessScale(res.bars) });
      chart.current?.setBars(res.bars);
      earliest.current = res.bars[0]?.t ?? null;
      setLast(res.bars[res.bars.length - 1] ?? null);
      setMode('history');
    };
    run().catch((e) => {
      if (cancelled) return;
      setError(errorText(e));
      setMode('error');
    });
    return () => {
      cancelled = true;
      if (opened) void api.marketCloseStream(opened);
      stream.current = null;
    };
  }, [symbol, timeframe, loggedIn, reload]);

  // Live updates for the open stream; reopen if TradingView drops the connection.
  useEffect(() => {
    const unlisten = on('market://event', (e) => {
      if (e.event === 'stream.update' && e.params.id === stream.current) {
        chart.current?.updateBar(e.params.bar);
        setLast(e.params.bar);
      } else if (e.event === 'stream.closed' && e.params.id === stream.current) {
        stream.current = null;
        window.setTimeout(() => setReload((n) => n + 1), 1500);
      }
    });
    return () => void unlisten.then((f) => f());
  }, []);

  const loadMore = useCallback(async () => {
    if (loadingMore.current || exhausted.current || earliest.current === null) return;
    loadingMore.current = true;
    try {
      let older: Bar[] = [];
      if (stream.current) {
        const res = await api.marketStreamMore(stream.current, 500, earliest.current);
        older = res.bars;
        exhausted.current = res.exhausted;
      } else if (dukascopy.current) {
        const to = new Date(earliest.current * 1000);
        const from = new Date(to.getTime() - TF_SECONDS[timeframe] * 500 * 1000);
        const res = await api.marketHistory(dukascopy.current, timeframe, from.toISOString().slice(0, 10), to.toISOString().slice(0, 10));
        older = res.bars.filter((b) => b.t < earliest.current!);
        exhausted.current = older.length === 0;
      }
      if (older.length) {
        chart.current?.prependBars(older);
        earliest.current = Math.min(earliest.current, ...older.map((b) => b.t));
      }
    } catch (e) {
      toast.error('Could not load older data', errorText(e));
      exhausted.current = true;
    } finally {
      loadingMore.current = false;
    }
  }, [timeframe]);

  const price = last?.c ?? quote?.price;
  const change = quote?.changePercent;
  const pricescale = info?.pricescale ?? 100;

  return (
    <div className={styles.window}>
      <div className={styles.toolbar}>
        <SymbolSearch onPick={(s) => setProps(win.id, { symbol: s })} />
        <SegmentedControl
          size="sm"
          value={timeframe}
          onChange={(tf) => setProps(win.id, { timeframe: tf })}
          options={TIMEFRAMES.map((tf) => ({ value: tf, label: tf === '1d' ? '1D' : tf === '1w' ? '1W' : tf }))}
        />
        <span className={styles.flex} />
        {mode === 'live' && (
          <span className={cx(styles.status, styles.statusLive)}>
            <Radio size={13} aria-hidden /> Live
          </span>
        )}
        {mode === 'history' && (
          <span className={styles.status} title="Historical data from Dukascopy. Sign in to TradingView for live prices.">
            <History size={13} aria-hidden /> History · Dukascopy
          </span>
        )}
        <Account />
      </div>
      <div className={styles.header}>
        <div>
          <div className={styles.symbol}>{info?.symbol ?? symbol}</div>
          <div className={styles.description}>
            {[info?.description, info?.exchange, info?.currency].filter(Boolean).join(' · ') || ' '}
          </div>
        </div>
        {price !== undefined && (mode === 'live' || mode === 'history') && (
          <div className={styles.priceBlock}>
            <div className={styles.price}>{formatPrice(price, pricescale)}</div>
            {change !== undefined && (
              <div className={cx(styles.change, change >= 0 ? styles.up : styles.down)}>{formatPercent(change)} today</div>
            )}
          </div>
        )}
      </div>
      <div className={styles.body}>
        <PriceChart ref={chart} pricescale={pricescale} onNeedMore={() => void loadMore()} />
        {mode === 'loading' && (
          <div className={styles.overlay}>
            <Spinner size={22} />
          </div>
        )}
        {mode === 'signin' && (
          <div className={styles.overlay}>
            <EmptyState
              icon={LogIn}
              title="Sign in to TradingView to see this market"
              description="Live data comes from your TradingView account. Forex, metals, indices and large US stocks also have free history from Dukascopy."
              action={
                <Button variant="primary" icon={LogIn} loading={status?.loginPending} onClick={() => void useMarket.getState().login()}>
                  Sign in
                </Button>
              }
            />
          </div>
        )}
        {mode === 'error' && (
          <div className={styles.overlay}>
            <EmptyState
              title="This market could not be loaded"
              description={error ?? undefined}
              action={
                <Button variant="secondary" onClick={() => setReload((n) => n + 1)}>
                  Try again
                </Button>
              }
            />
          </div>
        )}
      </div>
      {mode === 'history' && status?.available && (
        <div className={styles.banner}>
          Showing history from Dukascopy. Sign in to TradingView for live prices.
          <Button size="sm" variant="primary" icon={LogIn} loading={status.loginPending} onClick={() => void useMarket.getState().login()}>
            Sign in
          </Button>
        </div>
      )}
      {status && !status.available && <div className={styles.banner}>{status.reason}</div>}
    </div>
  );
}

/** Price precision for Dukascopy data, which carries no instrument metadata. */
function guessScale(bars: Bar[]): number {
  const sample = bars.slice(-50).map((b) => b.c);
  let decimals = 0;
  for (const v of sample) {
    const text = String(v);
    const d = text.includes('.') ? text.split('.')[1]!.length : 0;
    decimals = Math.max(decimals, Math.min(d, 6));
  }
  return 10 ** decimals;
}
