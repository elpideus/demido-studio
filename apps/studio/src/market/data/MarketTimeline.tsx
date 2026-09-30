// A market's history on one timeline, in the Data tab and in the assistant's data-status card.
// History is 1-minute candles and every timeframe is built from them, so there is one row per market,
// never one per timeframe: stored time in its source's colour (Dukascopy, or TradingView where only
// TradingView has bars), time the source has nothing for hatched, time not downloaded blank.
// Hovering reads out the dates under the pointer.

import { useLayoutEffect, useMemo, useRef, useState, type CSSProperties } from 'react';
import { createPortal } from 'react-dom';
import { cx, formatBytes } from '@demido/ui';

import type { MarketCoverageItem, MarketSource } from '@/lib/types';
import { SOURCE_NAMES } from '../DownloadProgress';
import {
  axisTicks,
  coveredShare,
  formatDay,
  marketCoverage,
  marketSegments,
  minuteTier,
  pct,
  rangeText,
  segmentAt,
  shareText,
  simplify,
  tierSpan,
  type Range,
  type Segment,
} from './coverage';
import palette from './sources.module.css';
import styles from './Timeline.module.css';

// Segments shorter than this share of the axis fold into their neighbours; finer detail can't be
// seen on a timeline a few hundred pixels wide.
const RESOLUTION = 480;
// How near (px) the pointer has to be to the learned-start marker for the readout to explain it.
const MARKER_SLOP = 5;

const FILL: Record<Segment['kind'], string | undefined> = {
  data: palette.fillData,
  empty: palette.fillEmpty,
  gap: palette.fillGap,
};

type ReadoutKind = Segment['kind'] | 'learned' | 'outside';

interface Hover {
  /** Pointer position inside the track, px. */
  x: number;
  width: number;
  clientX: number;
  top: number;
  bottom: number;
}

interface ReadoutText {
  kind: ReadoutKind;
  source?: MarketSource;
  title: string;
  body: string;
}

const place = (from: number, to: number, within: Range): CSSProperties => ({
  left: `${pct(from, within)}%`,
  width: `${pct(to, within) - pct(from, within)}%`,
});

export interface MarketTimelineProps {
  item: MarketCoverageItem;
  /** The axis, shared with the row's TimelineAxis. */
  domain: Range;
}

export function MarketTimeline({ item, domain }: MarketTimelineProps) {
  const [hover, setHover] = useState<Hover | null>(null);
  const coverage = useMemo(() => marketCoverage(item), [item]);
  const span = useMemo(() => (coverage ? tierSpan(coverage) : null), [coverage]);
  const segments = useMemo(
    () => (span ? simplify(marketSegments(item, span), (domain[1] - domain[0]) / RESOLUTION) : []),
    [item, span, domain],
  );
  const minute = minuteTier(item);
  // Where the history comes from is Dukascopy's 1-minute candles, else TradingView's bars.
  const label = minute ? '1-minute' : 'TradingView';
  const share = coverage && span ? coveredShare(coverage, span) : 0;
  // Learned starts exist only for Dukascopy: its metadata can claim older data than it serves.
  const learned = typeof minute?.learnedStart === 'number' ? minute.learnedStart : null;

  const readout = (h: Hover): ReadoutText => {
    const t = domain[0] + (h.x / h.width) * (domain[1] - domain[0]);
    if (learned !== null && Math.abs(h.x - (pct(learned, domain) / 100) * h.width) <= MARKER_SLOP) {
      return {
        kind: 'learned',
        title: 'Data seems to start here',
        body: `${formatDay(learned)}. Older 1-minute files came back empty, so they were skipped.`,
      };
    }
    if (!span) return { kind: 'outside', title: 'Nothing stored', body: item.name };
    if (t < span[0]) {
      return {
        kind: 'outside',
        title: 'Before the history',
        body: minute
          ? `Dukascopy has 1-minute data from ${formatDay(span[0])}`
          : `TradingView's stored history starts ${formatDay(span[0])}`,
      };
    }
    const seg = segmentAt(segments, t);
    if (!seg) return { kind: 'outside', title: 'Nothing later yet', body: `Up to ${formatDay(span[1] - 1)}` };
    if (seg.kind === 'data') {
      const source = seg.source ?? (minute ? 'dukascopy' : 'tradingview');
      return {
        kind: 'data',
        source,
        title: `Stored · ${SOURCE_NAMES[source]}`,
        body: rangeText(seg.from, seg.to),
      };
    }
    return {
      kind: seg.kind,
      title: seg.kind === 'empty' ? 'No data at source' : 'Not downloaded',
      body: rangeText(seg.from, seg.to),
    };
  };

  return (
    <div className={styles.row}>
      <span className={styles.label} title={minute ? '1-minute history: every timeframe is built from it' : label}>
        {label}
      </span>
      {span ? (
        <div
          className={styles.track}
          role="img"
          aria-label={`${item.name}: ${shareText(share)} downloaded between ${rangeText(span[0], span[1])}`}
          onPointerMove={(e) => {
            const r = e.currentTarget.getBoundingClientRect();
            if (r.width <= 0) return;
            const x = Math.max(0, Math.min(r.width, e.clientX - r.left));
            setHover({ x, width: r.width, clientX: e.clientX, top: r.top, bottom: r.bottom });
          }}
          onPointerLeave={() => setHover(null)}
          // Scrolling moves the track from under a pointer that stays put; the readout would lag behind.
          onWheel={() => setHover(null)}
        >
          <div className={styles.span} style={place(span[0], span[1], domain)}>
            {segments.map((seg) => (
              <div
                key={seg.from}
                data-source={seg.kind === 'data' ? (seg.source ?? (minute ? 'dukascopy' : 'tradingview')) : undefined}
                className={cx(styles.seg, FILL[seg.kind], seg.kind === 'gap' && styles.gap)}
                style={place(seg.from, seg.to, span)}
              />
            ))}
          </div>
          {learned !== null && <div className={styles.learned} style={{ left: `${pct(learned, domain)}%` }} />}
          {hover && (
            <>
              <div className={styles.cursor} style={{ left: hover.x }} />
              <Readout hover={hover} text={readout(hover)} />
            </>
          )}
        </div>
      ) : (
        <span className={styles.nothing}>Nothing stored</span>
      )}
      <span className={styles.share}>{span ? shareText(share) : ''}</span>
      <span className={styles.bytes}>{coverage && coverage.bytes > 0 ? formatBytes(coverage.bytes) : ''}</span>
    </div>
  );
}

/** Round dates under a market's timeline. */
export function TimelineAxis({ domain }: { domain: Range }) {
  // Labels right at an edge would hang out of the axis.
  const ticks = axisTicks(domain[0], domain[1]).filter((t) => {
    const at = pct(t.t, domain);
    return at > 4 && at < 96;
  });
  return (
    <div className={styles.axisRow} aria-hidden>
      <div className={styles.axis}>
        {ticks.map((tick) => (
          <span key={tick.t} className={styles.tick} style={{ left: `${pct(tick.t, domain)}%` }}>
            {tick.label}
          </span>
        ))}
      </div>
    </div>
  );
}

function keyClass(kind: ReadoutKind): string | undefined {
  if (kind === 'learned') return styles.keyLearned;
  if (kind === 'outside') return cx(palette.fillGap, styles.keyBlank);
  return cx(FILL[kind], kind === 'data' ? undefined : styles.keyBlank);
}

// Above the track, or below it near the top of the screen; kept inside the window horizontally.
function Readout({ hover, text }: { hover: Hover; text: ReadoutText }) {
  const ref = useRef<HTMLDivElement>(null);
  const [left, setLeft] = useState<number | null>(null);
  const below = hover.top < 72;

  useLayoutEffect(() => {
    const width = ref.current?.offsetWidth ?? 0;
    setLeft(Math.max(8, Math.min(hover.clientX - width / 2, window.innerWidth - width - 8)));
  }, [hover.clientX, text.title, text.body]);

  return createPortal(
    <div
      ref={ref}
      role="tooltip"
      data-source={text.source}
      className={cx(palette.palette, styles.readout, !below && styles.above)}
      style={{ top: below ? hover.bottom + 6 : hover.top - 6, left: left ?? -9999 }}
    >
      <div className={styles.readoutTitle}>
        <span className={cx(styles.key, keyClass(text.kind))} aria-hidden />
        {text.title}
      </div>
      <div className={styles.readoutBody}>{text.body}</div>
    </div>,
    document.body,
  );
}
