import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { Circle, CircleDot, Download, LogIn, X } from 'lucide-react';
import { Button, TextField, cx } from '@demido/ui';

import { api, errorText } from '@/lib/api';
import type { MarketGap, MarketJob, MarketPlan, MarketRange, MarketSource } from '@/lib/types';
import { useMarket } from '@/stores/market';
import { type HistoryOption, formatDay, optionRange } from './chartData';
import { DownloadProgress, planDetail, planSummary } from './DownloadProgress';
import styles from './DownloadPopup.module.css';

/** Space between the card and the oldest candle, and between the card and the chart's edges. */
const GAP = 8;
/** Where the card sits when the oldest candle is off screen: top left, clear of the legend. */
const HOME_TOP = 36;
const DAY = 86_400;

const shortDate = (t: number) =>
  new Date(t * 1000).toLocaleDateString([], { year: 'numeric', month: 'short', day: 'numeric', timeZone: 'UTC' });

interface Props {
  /** Screen position of the oldest loaded bar (its time and its price), in the chart's own coordinate
   *  space; the card sits beside that point, level with it. Null when the bar is off screen. */
  anchor: { x: number; y: number } | null;
  symbol: string;
  /** Where older history comes from: Dukascopy offers ranges, TradingView only "everything". */
  source: MarketSource;
  /** The chart's oldest bar; "more" ranges end there. */
  oldest: number | null;
  /** The uncovered stretch right before the oldest bar, when the store knows it. */
  gap: MarketGap | null;
  /** A moving download that already fetches the gap; shown instead of the picker. */
  job: MarketJob | null;
  /** Another unfinished download for the chart's data (paused, failed, or over another range): one line
   *  above the picker, with its Resume or Pause. */
  other?: MarketJob | null;
  loggedIn: boolean;
  /** A download was started (or joined) from the card. */
  onStarted: (jobId: string) => void;
  onClose: () => void;
}

/** A small card at the edge of the stored history offering an explicit download, since paging the
 *  chart back only ever reads what is stored. Never starts a download on its own. */
export function DownloadPopup({
  anchor,
  symbol,
  source,
  oldest,
  gap,
  job,
  other,
  loggedIn,
  onStarted,
  onClose,
}: Props) {
  const [option, setOption] = useState<HistoryOption>(gap ? 'gap' : 'month');
  const [fromDate, setFromDate] = useState('');
  const [plan, setPlan] = useState<MarketPlan | null>(null);
  const [planning, setPlanning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);
  // The job this card started, drawn until the chart's own job list picks it up.
  const [started, setStarted] = useState<{ jobId: string; plan: MarketPlan } | null>(null);
  const loginPending = useMarket((s) => !!s.status?.loginPending);
  // Fixed while the card is open: a range ending "now" must not change on every render.
  const [now] = useState(() => Math.floor(Date.now() / 1000));
  const card = useRef<HTMLDivElement>(null);
  const [place, setPlace] = useState<{ left: number; top: number } | null>(null);
  const [, remeasure] = useState(0);

  // Left of the oldest candle when there is room for the card there, else to its right, and never
  // past the chart's edges (a window pinned narrow has less room than the card needs). Measured after
  // every render: the card grows with a date field, a progress bar or an error.
  useLayoutEffect(() => {
    const el = card.current;
    const box = el?.offsetParent;
    if (!el || !(box instanceof HTMLElement)) return;
    const w = el.offsetWidth;
    const h = el.offsetHeight;
    const clamp = (v: number, max: number) => Math.max(GAP, Math.min(v, max));
    const left = anchor ? (anchor.x - GAP - w >= GAP ? anchor.x - GAP - w : anchor.x + GAP) : GAP;
    const top = anchor ? anchor.y - h / 2 : HOME_TOP;
    const next = {
      left: Math.round(clamp(left, box.clientWidth - GAP - w)),
      top: Math.round(clamp(top, box.clientHeight - GAP - h)),
    };
    setPlace((p) => (p && p.left === next.left && p.top === next.top ? p : next));
  });

  // The chart can change size without the candle moving on screen (the window is resized).
  useEffect(() => {
    const box = card.current?.offsetParent;
    if (!(box instanceof HTMLElement)) return undefined;
    const observer = new ResizeObserver(() => remeasure((n) => n + 1));
    observer.observe(box);
    return () => observer.disconnect();
  }, []);

  // The chart paged past the gap (a download filled it): that choice is gone.
  useEffect(() => {
    if (!gap && option === 'gap') setOption('month');
  }, [gap, option]);

  // The chart guesses the source from its keys, which a stream may not have named yet; a plan knows.
  const [planSource, setPlanSource] = useState<MarketSource | null>(null);
  const tradingView = (planSource ?? source) === 'tradingview';
  const range: MarketRange | null = tradingView ? {} : optionRange(option, { oldest, gap, fromDate, now });
  // A stable string, so the plan is asked for again only when the range really changes.
  const rangeKey = range ? JSON.stringify(range) : null;
  const following = job?.id ?? started?.jobId ?? null;
  const line = other && other.id !== following ? other : null;

  useEffect(() => {
    if (following || rangeKey === null) {
      setPlan(null);
      return undefined;
    }
    let cancelled = false;
    // The last option's estimate must not sit next to this option's Download button.
    setPlan(null);
    setPlanning(true);
    setError(null);
    api
      .marketPlanDownload(symbol, JSON.parse(rangeKey) as MarketRange)
      .then(
        (next) => {
          if (cancelled) return;
          setPlan(next);
          setPlanSource(next.source);
        },
        (e: unknown) => {
          if (cancelled) return;
          setPlan(null);
          setError(errorText(e));
        },
      )
      .finally(() => !cancelled && setPlanning(false));
    return () => {
      cancelled = true;
    };
  }, [symbol, rangeKey, following]);

  const start = async () => {
    if (!range) return;
    setStarting(true);
    setError(null);
    try {
      const res = await api.marketStartDownload(symbol, range, 'chart');
      setStarted(res);
      onStarted(res.jobId);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setStarting(false);
    }
  };

  const end = oldest ?? now;
  const options: Array<{ id: HistoryOption; label: string }> = [
    { id: 'month', label: '1 more month' },
    { id: 'year', label: '1 more year' },
    { id: 'all', label: 'Everything' },
    { id: 'from', label: 'From date…' },
  ];
  if (gap) options.push({ id: 'gap', label: `Fill gap back to ${shortDate(gap.from)}` });

  const needsSignIn = tradingView && !loggedIn;
  // Nothing to request: all stored, or (with `plan.job`) the rest is queued by a download that starting
  // this would only join, so that case keeps its button.
  const nothingToDo = !!plan && (plan.complete || (plan.requests === 0 && !plan.job));
  // "~8,400 files · ~75 MB · about 40 min · every timeframe at 1-minute detail".
  let estimate: string | null = null;
  if (range === null) estimate = option === 'from' ? `Pick a date before ${shortDate(end)}.` : null;
  else if (plan && !nothingToDo && plan.requests === 0) estimate = 'Already being downloaded';
  else if (plan) {
    const parts = [planSummary(plan)];
    if (!nothingToDo) parts.push(planDetail(plan));
    if (plan.approximate && !nothingToDo) parts.push('approximate');
    estimate = parts.join(' · ');
  } else if (planning) estimate = 'Estimating…';

  return (
    <div
      ref={card}
      className={styles.popup}
      style={{ left: place?.left ?? 0, top: place?.top ?? 0, visibility: place ? undefined : 'hidden' }}
    >
      <div className={styles.header}>
        <span>{following ? 'History download' : oldest === null ? 'Download history' : 'Load more history'}</span>
        <button type="button" className={styles.close} onClick={onClose} aria-label="Close">
          <X size={14} />
        </button>
      </div>
      {following ? (
        <DownloadProgress
          key={following}
          jobId={following}
          initialJob={job?.id === following ? job : null}
          plan={started?.jobId === following ? started.plan : null}
        />
      ) : (
        <>
          {line && (
            <div className={styles.other}>
              <DownloadProgress key={line.id} jobId={line.id} initialJob={line} inline />
            </div>
          )}
          {oldest === null && <div className={styles.note}>Nothing is stored for this market near today yet.</div>}
          {tradingView ? (
            <div className={styles.only}>Everything TradingView allows (all timeframes)</div>
          ) : (
            <div className={styles.options} role="radiogroup" aria-label="How much history">
              {options.map((o) => {
                const Icon = o.id === option ? CircleDot : Circle;
                return (
                  <button
                    key={o.id}
                    type="button"
                    role="radio"
                    aria-checked={o.id === option}
                    className={cx(styles.option, o.id === option && styles.optionActive)}
                    onClick={() => setOption(o.id)}
                  >
                    <Icon size={13} strokeWidth={2} aria-hidden />
                    {o.label}
                  </button>
                );
              })}
            </div>
          )}
          {!tradingView && option === 'from' && (
            <TextField
              type="date"
              size="sm"
              aria-label="Download from"
              value={fromDate}
              onChange={(e) => setFromDate(e.target.value)}
              max={formatDay(end - DAY)}
            />
          )}
          <div className={styles.estimate}>{estimate}</div>
          {plan?.job && !nothingToDo && (
            <div className={styles.note}>
              {plan.job.status === 'paused' || plan.job.status === 'error'
                ? 'A stopped download covers this; Download resumes it.'
                : 'A download already covers this; Download shows its progress.'}
            </div>
          )}
          {error && <div className={styles.error}>{error}</div>}
          {needsSignIn ? (
            <>
              <div className={styles.note}>Downloading from TradingView needs a TradingView sign-in.</div>
              <Button
                size="sm"
                variant="primary"
                icon={LogIn}
                loading={loginPending}
                onClick={() => void useMarket.getState().login()}
              >
                Sign in
              </Button>
            </>
          ) : (
            <Button
              size="sm"
              variant="primary"
              icon={Download}
              loading={starting}
              disabled={range === null || !plan || nothingToDo}
              onClick={() => void start()}
            >
              Download
            </Button>
          )}
        </>
      )}
    </div>
  );
}
