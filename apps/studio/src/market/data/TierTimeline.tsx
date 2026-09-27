// One tier's timeline in a market's Data row: stored time in the source's colour, time the source
// has nothing for hatched, time not downloaded blank. Hovering reads out the dates under the pointer.

import { useLayoutEffect, useMemo, useRef, useState, type CSSProperties } from 'react';
import { createPortal } from 'react-dom';
import { cx, formatBytes } from '@demido/ui';

import type { MarketSource, MarketTierCoverage } from '@/lib/types';
import { SOURCE_NAMES, tierLabel } from '../DownloadProgress';
import {
  axisTicks,
  coveredShare,
  formatDay,
  pct,
  rangeText,
  segmentAt,
  segmentsOf,
  shareText,
  simplify,
  tierSpan,
  type Range,
  type SegmentKind,
} from './coverage';
import palette from './sources.module.css';
import styles from './Timeline.module.css';

// Segments shorter than this share of the axis fold into their neighbours; finer detail can't be
// seen on a timeline a few hundred pixels wide.
const RESOLUTION = 480;
// How near (px) the pointer has to be to the learned-start marker for the readout to explain it.
const MARKER_SLOP = 5;

const KIND_TEXT: Record<SegmentKind, string> = { data: 'Stored', empty: 'No data at source', gap: 'Not downloaded' };
const FILL: Record<SegmentKind, string | undefined> = {
  data: palette.fillData,
  empty: palette.fillEmpty,
  gap: palette.fillGap,
};

type ReadoutKind = SegmentKind | 'learned' | 'outside';

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
  title: string;
  body: string;
}

const place = (from: number, to: number, within: Range): CSSProperties => ({
  left: `${pct(from, within)}%`,
  width: `${pct(to, within) - pct(from, within)}%`,
});

export interface TierTimelineProps {
  source: MarketSource;
  tier: MarketTierCoverage;
  /** The market's shared axis. */
  domain: Range;
}

export function TierTimeline({ source, tier, domain }: TierTimelineProps) {
  const [hover, setHover] = useState<Hover | null>(null);
  const span = useMemo(() => tierSpan(tier), [tier]);
  const segments = useMemo(
    () => (span ? simplify(segmentsOf(tier, span), (domain[1] - domain[0]) / RESOLUTION) : []),
    [tier, span, domain],
  );
  const label = tierLabel(tier.tier);
  const share = span ? coveredShare(tier, span) : 0;
  // Learned starts exist only for Dukascopy: its metadata can claim older data than it serves.
  const learned = source === 'dukascopy' && typeof tier.learnedStart === 'number' ? tier.learnedStart : null;

  const readout = (h: Hover): ReadoutText => {
    const t = domain[0] + (h.x / h.width) * (domain[1] - domain[0]);
    if (learned !== null && Math.abs(h.x - (pct(learned, domain) / 100) * h.width) <= MARKER_SLOP) {
      return {
        kind: 'learned',
        title: 'Data seems to start here',
        body: `${formatDay(learned)}. Older ${label} files came back empty, so they were skipped.`,
      };
    }
    if (!span) return { kind: 'outside', title: 'Nothing stored', body: `${SOURCE_NAMES[source]} ${label}` };
    if (t < span[0]) {
      return {
        kind: 'outside',
        title: `Before the ${label} data`,
        body:
          source === 'dukascopy'
            ? `Dukascopy has ${label} data from ${formatDay(span[0])}`
            : `The TradingView ${label} timeline starts ${formatDay(span[0])}`,
      };
    }
    const seg = segmentAt(segments, t);
    if (!seg) return { kind: 'outside', title: 'Nothing later yet', body: `Up to ${formatDay(span[1] - 1)}` };
    return { kind: seg.kind, title: KIND_TEXT[seg.kind], body: rangeText(seg.from, seg.to) };
  };

  return (
    <div className={styles.row} data-source={source}>
      <span className={styles.label} title={`${SOURCE_NAMES[source]} ${label}`}>
        {label}
      </span>
      {span ? (
        <div
          className={styles.track}
          role="img"
          aria-label={`${label}: ${shareText(share)} downloaded between ${rangeText(span[0], span[1])}`}
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
                className={cx(styles.seg, FILL[seg.kind], seg.kind === 'gap' && styles.gap)}
                style={place(seg.from, seg.to, span)}
              />
            ))}
          </div>
          {learned !== null && <div className={styles.learned} style={{ left: `${pct(learned, domain)}%` }} />}
          {hover && (
            <>
              <div className={styles.cursor} style={{ left: hover.x }} />
              <Readout hover={hover} source={source} text={readout(hover)} />
            </>
          )}
        </div>
      ) : (
        <span className={styles.nothing}>Nothing stored</span>
      )}
      <span className={styles.share}>{span ? shareText(share) : ''}</span>
      <span className={styles.bytes}>{tier.bytes > 0 ? formatBytes(tier.bytes) : ''}</span>
    </div>
  );
}

/** Round dates under the tracks of one market. */
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
function Readout({ hover, source, text }: { hover: Hover; source: MarketSource; text: ReadoutText }) {
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
      data-source={source}
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
