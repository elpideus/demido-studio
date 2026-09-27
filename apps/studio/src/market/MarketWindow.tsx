import { useCallback, useEffect, useRef, useState } from 'react';
import {
  CandlestickChart,
  Database,
  Download,
  History,
  LogIn,
  LogOut,
  Radio,
  RefreshCw,
  Search,
  UserRound,
} from 'lucide-react';
import {
  Button,
  EmptyState,
  IconButton,
  Menu,
  Popover,
  SegmentedControl,
  Spinner,
  TextField,
  cx,
  useDebounced,
} from '@demido/ui';

import { api, errorText } from '@/lib/api';
import { type MarketEvent, on } from '@/lib/events';
import { formatPercent, formatPrice } from '@/lib/format';
import type {
  Bar,
  ChartInfo,
  MarketBarsPage,
  MarketGap,
  MarketJob,
  MarketKeys,
  MarketLatestBars,
  MarketMore,
  MarketSource,
  MarketSpan,
  MarketStoreUpdate,
  Quote,
  SymbolMatch,
} from '@/lib/types';
import { useMarket } from '@/stores/market';
import { toast } from '@/stores/toasts';
import { type WindowState, useWindows } from '@/stores/windows';
import { TabbedLayout, type TabSpec } from '@/wm/TabbedLayout';
import {
  chartJobs,
  historySource,
  isBehind,
  isChartJob,
  isMoving,
  keysMatch,
  latestView,
  mergeListed,
  spanOf,
  upsertJob,
} from './chartData';
import { DataTab } from './DataTab';
import { DownloadPopup } from './DownloadPopup';
import { DownloadProgress, SOURCE_NAMES } from './DownloadProgress';
import { PriceChart, type PriceChartHandle } from './PriceChart';
import styles from './MarketWindow.module.css';

const TIMEFRAMES = ['1m', '5m', '15m', '1h', '4h', '1d', '1w'] as const;
type Tf = (typeof TIMEFRAMES)[number];
const TF_SECONDS: Record<Tf, number> = {
  '1m': 60,
  '5m': 300,
  '15m': 900,
  '1h': 3600,
  '4h': 14400,
  '1d': 86400,
  '1w': 604800,
};
const POPULAR = [
  'FX:EURUSD',
  'OANDA:XAUUSD',
  'BINANCE:BTCUSDT',
  'SP:SPX',
  'NASDAQ:NDX',
  'NASDAQ:AAPL',
  'NASDAQ:NVDA',
  'FX:GBPUSD',
];

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
      <Popover
        open={open}
        onClose={() => setOpen(false)}
        anchorRef={anchor}
        placement="bottom-start"
        width={380}
        maxHeight={380}
      >
        <div className={styles.results}>
          {!query.trim() && <div className={styles.resultsTitle}>Popular</div>}
          {(query.trim()
            ? results
            : POPULAR.map((p) => ({
                symbol: p,
                ticker: p.split(':')[1] ?? p,
                description: '',
                exchange: p.split(':')[0] ?? '',
                type: '',
              }))
          )
            .slice(0, 14)
            .map((r) => (
              <button key={r.symbol} type="button" className={styles.result} onClick={() => pick(r.symbol)}>
                <span className={styles.resultTicker}>{r.ticker}</span>
                <span className={styles.resultDesc}>{r.description || r.exchange}</span>
                <span className={styles.resultMeta}>{r.type || r.exchange}</span>
              </button>
            ))}
          {query.trim() && !busy && results.length === 0 && (
            <div className={styles.resultsEmpty}>No symbols match.</div>
          )}
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

type Tab = 'chart' | 'data';

const TABS: Array<TabSpec<Tab>> = [
  { id: 'chart', label: 'Chart', icon: CandlestickChart },
  { id: 'data', label: 'Data', icon: Database },
];

/** Charts with live TradingView data or stored history, and what is stored for each market. */
export function MarketWindow({ win }: { win: WindowState }) {
  const setProps = useWindows((s) => s.setProps);
  const tab = (TABS.some((t) => t.id === win.props.tab) ? win.props.tab : 'chart') as Tab;
  return (
    <TabbedLayout tabs={TABS} active={tab} onChange={(id) => setProps(win.id, { tab: id })}>
      {/* Kept mounted behind the Data tab, so coming back does not reopen the stream. */}
      <ChartView win={win} hidden={tab !== 'chart'} />
      {tab === 'data' && <DataTab onOpenChart={(s) => setProps(win.id, { symbol: s, tab: 'chart' })} />}
    </TabbedLayout>
  );
}

/** One chart: the live TradingView stream when signed in, stored history otherwise. Paging back
 *  only reads what is stored; where the store ends, the download popup offers more. */
function ChartView({ win, hidden }: { win: WindowState; hidden: boolean }) {
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
  // Signed out, showing TradingView bars stored earlier: no live source until sign-in.
  const [stale, setStale] = useState(false);
  const [newestSource, setNewestSource] = useState<MarketSource | null>(null);
  const [keys, setKeys] = useState<MarketKeys>({});
  const [oldest, setOldest] = useState<number | null>(null);
  const [gap, setGap] = useState<MarketGap | null>(null);
  const [jobs, setJobs] = useState<MarketJob[]>([]);
  const [popup, setPopup] = useState(false);
  const [edgePos, setEdgePos] = useState<{ x: number; y: number } | null>(null);
  const [updating, setUpdating] = useState(false);
  const [reload, setReload] = useState(0);

  const stream = useRef<string | null>(null);
  const earliest = useRef<number | null>(null);
  const lastBar = useRef<Bar | null>(null);
  const keysRef = useRef<MarketKeys>({});
  const jobsRef = useRef<MarketJob[]>([]);
  jobsRef.current = jobs;
  // What the store said lies right before the oldest bar; only `cached` pages further back.
  const more = useRef<MarketMore>('none');
  const loadingMore = useRef(false);
  // Bumped by every store.updated that reaches before the oldest bar, so a page asked for while
  // new data landed is not taken as the final word.
  const storeVersion = useRef(0);
  // Bumped whenever the chart is reloaded, so answers to a symbol or timeframe that is no longer
  // shown are dropped instead of being glued onto the new chart.
  const generation = useRef(0);
  // The oldest bar when the popup was last closed; it stays closed at that edge (at every edge while
  // a download for the chart moves, since the toolbar already shows it). Undefined: never closed.
  const dismissedAt = useRef<number | null | undefined>(undefined);
  // The download the popup started, and the one it shows instead of its picker: when that one
  // finishes, the popup has nothing more to say at this edge.
  const popupStarted = useRef<string | null>(null);
  const popupJob = useRef<string | null>(null);
  const refreshing = useRef(false);
  const refreshAgain = useRef(false);
  // The oldest bar's screen position, reported on every pan frame; only an open popup follows it, so
  // panning does not re-render the whole window.
  const edge = useRef<{ x: number; y: number } | null>(null);
  const popupOpen = useRef(false);
  popupOpen.current = popup;

  // The toolbar shows the lead download; only a moving one takes the Download button's place, and
  // only a moving one that already fetches the gap takes the popup's picker.
  const { lead, moving, covering, other } = chartJobs(jobs, gap, timeframe, TF_SECONDS[timeframe]);
  popupJob.current = popup ? (covering?.id ?? popupStarted.current) : null;

  const showKeys = (next: MarketKeys) => {
    keysRef.current = next;
    setKeys(next);
  };

  const showLast = (bar: Bar | null) => {
    lastBar.current = bar;
    setLast(bar);
  };

  const showBars = (bars: Bar[], spans: MarketSpan[]) => {
    chart.current?.setBars(bars, spans);
    earliest.current = bars[0]?.t ?? null;
    setOldest(earliest.current);
    setNewestSource(spans[spans.length - 1]?.source ?? null);
    showLast(bars[bars.length - 1] ?? null);
  };

  const showEdge = (page: Pick<MarketBarsPage, 'more' | 'gap'>) => {
    more.current = page.more;
    setGap(page.gap ?? null);
  };

  useEffect(() => {
    let cancelled = false;
    let opened: string | null = null;
    setMode('loading');
    setError(null);
    setQuote(null);
    setStale(false);
    setGap(null);
    setPopup(false);
    showKeys({});
    generation.current += 1;
    loadingMore.current = false;
    refreshing.current = false;
    more.current = 'none';
    earliest.current = null;
    setOldest(null);
    dismissedAt.current = undefined;
    popupStarted.current = null;

    const run = async () => {
      if (loggedIn) {
        const res = await api.marketOpenStream(symbol, timeframe, 500);
        if (cancelled) {
          void api.marketCloseStream(res.id);
          return;
        }
        opened = res.id;
        stream.current = res.id;
        // stream.open names the store keys it records into; without them the first older page does.
        showKeys(res.keys ?? {});
        setInfo(res.info);
        // Whether older bars are stored is only known once asked.
        more.current = 'cached';
        showBars(res.bars, spanOf(res.bars, 'tradingview'));
        setMode('live');
        api.marketQuote([symbol]).then(
          ([q]) => !cancelled && q && !q.error && setQuote(q),
          () => undefined,
        );
        // Fills the stored history up to now so paging back from the stream meets no hole; the chart
        // picks the new bars up from store.updated.
        api.marketBarsFreshen(symbol, timeframe).catch(() => undefined);
        return;
      }
      let res: MarketLatestBars;
      try {
        res = await api.marketBarsLatest(symbol, timeframe, 500);
      } catch (e) {
        if (cancelled) return;
        // A market only TradingView has, with nothing stored for it.
        if (/sign(ed)? in/i.test(errorText(e))) {
          setMode('signin');
          return;
        }
        throw e;
      }
      if (cancelled) return;
      showKeys(res.keys ?? {});
      const view = latestView(res);
      if (view.mode === 'signin') {
        setMode('signin');
        return;
      }
      if (view.mode === 'error') throw new Error(view.message);
      setInfo({ ...res.info, symbol, pricescale: res.info?.pricescale ?? guessScale(res.bars) });
      setStale(!!res.stale);
      showEdge(res);
      showBars(res.bars, res.spans ?? []);
      setMode('history');
      // Nothing stored near now: the chart is empty, so say where downloading starts right away.
      if (view.offer) {
        setEdgePos(null);
        setPopup(true);
      }
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

  // Another market's downloads must not linger until the new listing arrives.
  useEffect(() => {
    jobsRef.current = [];
    setJobs([]);
  }, [symbol]);

  // Downloads already running for this market (from the chat, the Data tab or before a restart) show
  // in the toolbar. Every job is listed and matched here, since a chat job may name the symbol
  // differently from the chart. Listed again on coming back from the Data tab, where a download can
  // be cancelled without any event saying so.
  const keysId = JSON.stringify(keys);
  useEffect(() => {
    if (hidden) return undefined;
    let cancelled = false;
    const chartKeys = JSON.parse(keysId) as MarketKeys;
    api.marketListDownloads().then(
      (list) => {
        if (cancelled) return;
        const mine = list.filter((j) => isChartJob(j, chartKeys, symbol));
        setJobs((prev) => mergeListed(prev, mine));
      },
      () => undefined,
    );
    return () => {
      cancelled = true;
    };
  }, [symbol, keysId, hidden]);

  const trackEdge = useCallback((pos: { x: number; y: number } | null) => {
    edge.current = pos;
    if (popupOpen.current) setEdgePos(pos);
  }, []);

  const openPopup = useCallback(() => {
    setEdgePos(edge.current);
    setPopup(true);
  }, []);

  const offerDownload = useCallback(() => {
    if (earliest.current === null) return;
    const closed = dismissedAt.current;
    // A paused or failed download fills nothing by itself, so the next edge offers the picker again.
    const busy = jobsRef.current.some((j) => isMoving(j.status));
    if (closed !== undefined && (closed === earliest.current || busy)) return;
    openPopup();
  }, [openPopup]);

  const loadMore = useCallback(async () => {
    if (loadingMore.current || more.current !== 'cached' || earliest.current === null) return;
    const gen = generation.current;
    const edge = earliest.current;
    const version = storeVersion.current;
    loadingMore.current = true;
    try {
      const page = await api.marketBarsOlder(symbol, timeframe, edge, 500);
      if (gen !== generation.current) return;
      if (!keysRef.current.dukascopy && !keysRef.current.tradingview && page.keys) showKeys(page.keys);
      const added = page.bars.length ? (chart.current?.prependBars(page.bars, page.spans ?? []) ?? 0) : 0;
      if (added > 0) {
        earliest.current = page.bars.reduce((min, b) => Math.min(min, b.t), edge);
        setOldest(earliest.current);
      }
      // "cached" with nothing new would be asked for again forever.
      showEdge({ more: added === 0 && page.more === 'cached' ? 'none' : page.more, gap: page.gap });
    } catch (e) {
      if (gen !== generation.current) return;
      toast.error('Could not load older data', errorText(e));
      more.current = 'none';
    } finally {
      if (gen === generation.current) loadingMore.current = false;
    }
    if (gen !== generation.current) return;
    // Data landed before the edge while this page was on its way: the page may already be out of date.
    if (storeVersion.current !== version) more.current = 'cached';
    // Widened again: TypeScript keeps the check at the top in force across the awaits.
    const next = more.current as MarketMore;
    if (chart.current?.nearOldest()) {
      if (next === 'cached') void loadMore();
      else if (next === 'gap') offerDownload();
    }
  }, [symbol, timeframe, offerDownload]);

  const needMore = useCallback(() => {
    if (more.current === 'cached') void loadMore();
    else if (more.current === 'gap') offerDownload();
  }, [loadMore, offerDownload]);

  // Signed out there is no stream: newer stored bars (a download bringing the history up to today)
  // are read from the store and added on the right.
  const refreshNewest = useCallback(async () => {
    if (refreshing.current) {
      refreshAgain.current = true;
      return;
    }
    refreshing.current = true;
    const gen = generation.current;
    try {
      do {
        refreshAgain.current = false;
        const newest = lastBar.current;
        const future = Math.floor(Date.now() / 1000) + 2 * TF_SECONDS[timeframe];
        const page = await api.marketBarsOlder(symbol, timeframe, future, 500);
        if (gen !== generation.current) return;
        if (!newest) {
          // The chart was empty (nothing stored near now): the first stored bars fill it.
          if (page.bars.length) {
            setInfo((prev) => prev && { ...prev, pricescale: guessScale(page.bars) });
            showEdge(page);
            showBars(page.bars, page.spans ?? []);
          }
          continue;
        }
        const newer = page.bars.filter((b) => b.t >= newest.t);
        if (!newer.length) continue;
        // More new bars than one page: a hole would open between them and the chart, so redraw.
        if (page.bars[0]!.t > newest.t) {
          setReload((n) => n + 1);
          return;
        }
        for (const b of newer) chart.current?.updateBar(b);
        showLast(newer[newer.length - 1]!);
      } while (refreshAgain.current);
    } catch {
      // The next store.updated tries again.
    } finally {
      if (gen === generation.current) refreshing.current = false;
    }
  }, [symbol, timeframe]);

  const storeUpdated = (u: MarketStoreUpdate) => {
    if (!keysMatch(keysRef.current, u.source, u.key)) return;
    const edge = earliest.current;
    if (edge !== null && u.from < edge) {
      // A download (or the freshness fill) stored bars older than the chart shows: page into them.
      storeVersion.current += 1;
      if (more.current !== 'cached') {
        more.current = 'cached';
        if (chart.current?.nearOldest()) void loadMore();
      }
    }
    if (mode === 'history' && (!lastBar.current || u.to > lastBar.current.t)) void refreshNewest();
  };

  const jobChanged = (next: MarketJob, done: boolean) => {
    if (!isChartJob(next, keysRef.current, symbol)) return;
    const shown = popupJob.current === next.id;
    const remaining = upsertJob(jobsRef.current, next);
    jobsRef.current = remaining;
    setJobs(remaining);
    if (!done) return;
    // The last coalesced store.updated may still be on its way; asking once more is one cache read.
    storeVersion.current += 1;
    if (earliest.current !== null && more.current !== 'cached') {
      more.current = 'cached';
      if (chart.current?.nearOldest()) void loadMore();
    }
    // The popup was showing this download instead of its picker: it has nothing more to say at this edge.
    if (shown) {
      setPopup(false);
      dismissedAt.current = earliest.current;
      popupStarted.current = null;
    }
  };

  // Live updates for the open stream (reopened if TradingView drops it), store changes and downloads.
  const handle = useRef<(e: MarketEvent) => void>(() => undefined);
  handle.current = (e) => {
    if (e.event === 'stream.update' && e.params.id === stream.current) {
      chart.current?.updateBar(e.params.bar);
      showLast(e.params.bar);
    } else if (e.event === 'stream.closed' && e.params.id === stream.current) {
      stream.current = null;
      window.setTimeout(() => setReload((n) => n + 1), 1500);
    } else if (e.event === 'store.updated') {
      storeUpdated(e.params);
    } else if (e.event === 'download.progress' || e.event === 'download.error' || e.event === 'download.done') {
      if (e.params.job) jobChanged(e.params.job, e.event === 'download.done');
    } else if (e.event === 'download.removed') {
      const remaining = jobsRef.current.filter((j) => j.id !== e.params.jobId);
      if (remaining.length !== jobsRef.current.length) {
        jobsRef.current = remaining;
        setJobs(remaining);
      }
    }
  };
  useEffect(() => {
    const unlisten = on('market://event', (e) => handle.current(e));
    return () => void unlisten.then((f) => f());
  }, []);

  // A download started from this chart shows right away, before its first progress event.
  const trackStarted = useCallback((jobId: string) => {
    dismissedAt.current = undefined;
    api.marketGetDownload(jobId).then(
      (started) => {
        if (started) setJobs((prev) => upsertJob(prev, started));
      },
      () => undefined,
    );
  }, []);

  const startedInPopup = useCallback(
    (jobId: string) => {
      popupStarted.current = jobId;
      trackStarted(jobId);
    },
    [trackStarted],
  );

  const closePopup = () => {
    setPopup(false);
    dismissedAt.current = earliest.current;
    popupStarted.current = null;
  };

  const updateToToday = async () => {
    const newest = lastBar.current;
    if (!newest) return;
    setUpdating(true);
    try {
      const res = await api.marketStartDownload(symbol, { from: newest.t }, 'chart');
      trackStarted(res.jobId);
    } catch (e) {
      toast.error('Could not update the history', errorText(e));
    } finally {
      setUpdating(false);
    }
  };

  const price = last?.c ?? quote?.price;
  const change = quote?.changePercent;
  const pricescale = info?.pricescale ?? 100;
  const showing = mode === 'live' || mode === 'history';
  const behind =
    mode === 'history' &&
    !stale &&
    !!keys.dukascopy &&
    !!last &&
    isBehind(last.t, TF_SECONDS[timeframe], Math.floor(Date.now() / 1000));

  return (
    <div className={cx(styles.window, hidden && styles.hidden)}>
      <div className={styles.toolbar}>
        <SymbolSearch onPick={(s) => setProps(win.id, { symbol: s })} />
        <SegmentedControl
          size="sm"
          value={timeframe}
          onChange={(tf) => setProps(win.id, { timeframe: tf })}
          options={TIMEFRAMES.map((tf) => ({ value: tf, label: tf === '1d' ? '1D' : tf === '1w' ? '1W' : tf }))}
        />
        <span className={styles.flex} />
        {showing && lead && (
          <button
            type="button"
            className={styles.jobButton}
            aria-label="Show the history download"
            onClick={() => (popup ? closePopup() : openPopup())}
          >
            <DownloadProgress key={lead.id} jobId={lead.id} initialJob={lead} compact />
          </button>
        )}
        {showing && !moving && behind && (
          <Button
            size="sm"
            variant="secondary"
            icon={RefreshCw}
            loading={updating}
            onClick={() => void updateToToday()}
          >
            Update to today
          </Button>
        )}
        {showing && !moving && (
          <IconButton
            icon={Download}
            label="Download history"
            size="sm"
            tooltipPlacement="bottom"
            onClick={() => {
              dismissedAt.current = undefined;
              openPopup();
            }}
          />
        )}
        {mode === 'live' && (
          <span className={cx(styles.status, styles.statusLive)}>
            <Radio size={13} aria-hidden /> Live
          </span>
        )}
        {mode === 'history' && (
          <span
            className={styles.status}
            title={
              stale
                ? 'TradingView data stored on this computer. Sign in to TradingView for live prices.'
                : 'Stored history. Sign in to TradingView for live prices.'
            }
          >
            <History size={13} aria-hidden /> {stale ? 'Saved' : 'History'}
            {newestSource ? ` · ${SOURCE_NAMES[newestSource]}` : ''}
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
        {price !== undefined && showing && (
          <div className={styles.priceBlock}>
            <div className={styles.price}>{formatPrice(price, pricescale)}</div>
            {change !== undefined && (
              <div className={cx(styles.change, change >= 0 ? styles.up : styles.down)}>
                {formatPercent(change)} today
              </div>
            )}
          </div>
        )}
      </div>
      <div className={styles.body}>
        <PriceChart ref={chart} pricescale={pricescale} onNeedMore={needMore} onEdgeAnchor={trackEdge} />
        {popup && showing && (
          <DownloadPopup
            anchor={edgePos}
            symbol={symbol}
            source={historySource(keys, gap)}
            oldest={oldest}
            gap={gap}
            job={covering}
            other={other}
            loggedIn={loggedIn}
            onStarted={startedInPopup}
            onClose={closePopup}
          />
        )}
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
              description="Live data comes from your TradingView account. Forex, metals, indices, commodities and crypto also have free history from Dukascopy."
              action={
                <Button
                  variant="primary"
                  icon={LogIn}
                  loading={status?.loginPending}
                  onClick={() => void useMarket.getState().login()}
                >
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
          {stale
            ? 'Showing TradingView data saved earlier. Sign in for live data.'
            : 'Showing stored history. Sign in to TradingView for live prices.'}
          <Button
            size="sm"
            variant="primary"
            icon={LogIn}
            loading={status.loginPending}
            onClick={() => void useMarket.getState().login()}
          >
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
