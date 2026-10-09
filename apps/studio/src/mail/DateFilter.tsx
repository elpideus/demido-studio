import { type KeyboardEvent, type RefObject, useEffect, useRef, useState } from 'react';
import { ChevronLeft, ChevronRight, ChevronsLeft, ChevronsRight } from 'lucide-react';
import { Button, IconButton, Popover, cx } from '@demido/ui';

import type { MailDateRange } from '@/lib/types';
import {
  dayKey,
  firstWeekday,
  fromRange,
  monthGrid,
  presets,
  sameDay,
  startOfDay,
  toRange,
  weekdayNames,
} from './dateRange';
import styles from './DateFilter.module.css';

const WEEK_START = firstWeekday();
const WEEKDAYS = weekdayNames(WEEK_START);

/** How far each arrow key moves the focused day. */
const STEPS: Record<string, number> = { ArrowLeft: -1, ArrowRight: 1, ArrowUp: -7, ArrowDown: 7 };

/**
 * A calendar for searching mail by date: pick a day, or two for a range, the times they start and
 * end at, or one of the usual ranges. `onApply` gets the range, or null when it is cleared.
 */
export function DateFilter({
  open,
  onClose,
  anchorRef,
  value,
  onApply,
}: {
  open: boolean;
  onClose: () => void;
  anchorRef: RefObject<HTMLElement | null>;
  value: MailDateRange | null;
  onApply: (range: MailDateRange | null) => void;
}) {
  return (
    <Popover open={open} onClose={onClose} anchorRef={anchorRef} placement="bottom-end" aria-label="Search by date">
      <Calendar value={value} onApply={onApply} />
    </Popover>
  );
}

/** The picker's contents. Mounted each time it opens, so it starts from the range in use. */
function Calendar({ value, onApply }: { value: MailDateRange | null; onApply: (range: MailDateRange | null) => void }) {
  const [initial] = useState(() => fromRange(value));
  const [start, setStart] = useState(initial.start);
  const [end, setEnd] = useState(initial.end);
  const [hover, setHover] = useState<Date | null>(null);
  const [fromTime, setFromTime] = useState(initial.fromTime);
  const [toTime, setToTime] = useState(initial.toTime);
  const [cursor, setCursor] = useState(() => initial.start ?? startOfDay(new Date()));
  const [view, setView] = useState(() => ({ year: cursor.getFullYear(), month: cursor.getMonth() }));
  const grid = useRef<HTMLDivElement>(null);
  const keyed = useRef(false);

  // Arrow keys move the focus with the cursor, from month to month.
  useEffect(() => {
    if (!keyed.current) return;
    keyed.current = false;
    grid.current?.querySelector<HTMLButtonElement>('[tabindex="0"]')?.focus();
  }, [cursor]);

  const today = startOfDay(new Date());
  const days = monthGrid(view.year, view.month, WEEK_START);
  const inView = (d: Date) => d.getMonth() === view.month && d.getFullYear() === view.year;
  const focusable = inView(cursor) ? cursor : new Date(view.year, view.month, 1);

  // While the second day is being chosen, the range follows the pointer.
  const other = end ?? (start && hover) ?? start;
  const [lo, hi] = start && other ? (dayKey(start) <= dayKey(other) ? [start, other] : [other, start]) : [null, null];

  const shift = (months: number) => {
    const d = new Date(view.year, view.month + months, 1);
    setView({ year: d.getFullYear(), month: d.getMonth() });
  };

  const pick = (day: Date) => {
    setCursor(day);
    if (!inView(day)) setView({ year: day.getFullYear(), month: day.getMonth() });
    if (!start || end) {
      setStart(day);
      setEnd(null);
    } else if (dayKey(day) < dayKey(start)) {
      setEnd(start);
      setStart(day);
    } else {
      setEnd(day);
    }
  };

  const onGridKey = (e: KeyboardEvent) => {
    const step = STEPS[e.key];
    if (step == null) return;
    e.preventDefault();
    const next = new Date(focusable.getFullYear(), focusable.getMonth(), focusable.getDate() + step);
    keyed.current = true;
    setCursor(next);
    setView({ year: next.getFullYear(), month: next.getMonth() });
  };

  const apply = () => start && onApply(toRange(start, end ?? start, fromTime, toTime));
  const onTimeKey = (e: KeyboardEvent) => {
    if (e.key === 'Enter') apply();
  };

  const title = new Date(view.year, view.month, 1).toLocaleDateString(undefined, { month: 'long', year: 'numeric' });
  const longDay = (d: Date | null) =>
    d ? d.toLocaleDateString(undefined, { weekday: 'short', month: 'short', day: 'numeric', year: 'numeric' }) : '—';

  return (
    <div className={styles.picker}>
      <div className={styles.presets}>
        {presets(today).map((p) => (
          <button key={p.label} type="button" className={styles.preset} onClick={() => onApply(p.range)}>
            {p.label}
          </button>
        ))}
      </div>

      <div className={styles.month}>
        <IconButton icon={ChevronsLeft} label="Previous year" size="xs" tooltip={false} onClick={() => shift(-12)} />
        <IconButton icon={ChevronLeft} label="Previous month" size="xs" tooltip={false} onClick={() => shift(-1)} />
        <span className={styles.title} aria-live="polite">
          {title}
        </span>
        <IconButton icon={ChevronRight} label="Next month" size="xs" tooltip={false} onClick={() => shift(1)} />
        <IconButton icon={ChevronsRight} label="Next year" size="xs" tooltip={false} onClick={() => shift(12)} />
      </div>

      <div className={styles.weekdays} aria-hidden>
        {WEEKDAYS.map((name, i) => (
          <span key={i}>{name}</span>
        ))}
      </div>
      <div ref={grid} className={styles.grid} role="grid" onKeyDown={onGridKey} onMouseLeave={() => setHover(null)}>
        {days.map((d) => {
          const key = dayKey(d);
          const edge = sameDay(d, lo) || sameDay(d, hi);
          const between = !!lo && !!hi && key > dayKey(lo) && key < dayKey(hi);
          const span = !!lo && !!hi && !sameDay(lo, hi);
          return (
            <div
              key={key}
              className={cx(
                styles.cell,
                between && styles.between,
                span && sameDay(d, lo) && styles.first,
                span && sameDay(d, hi) && styles.last,
              )}
            >
              <button
                type="button"
                tabIndex={sameDay(d, focusable) ? 0 : -1}
                className={cx(
                  styles.day,
                  !inView(d) && styles.outside,
                  sameDay(d, today) && styles.today,
                  edge && styles.edge,
                )}
                aria-label={longDay(d)}
                aria-pressed={edge || between}
                onClick={() => pick(d)}
                onMouseEnter={() => setHover(d)}
              >
                {d.getDate()}
              </button>
            </div>
          );
        })}
      </div>

      <div className={styles.times}>
        <label className={styles.when}>
          <span className={styles.whenLabel}>From</span>
          <span className={styles.whenDay}>{longDay(lo)}</span>
          <input
            type="time"
            className={styles.time}
            value={fromTime}
            onChange={(e) => setFromTime(e.target.value)}
            onKeyDown={onTimeKey}
          />
        </label>
        <label className={styles.when}>
          <span className={styles.whenLabel}>To</span>
          <span className={styles.whenDay}>{longDay(hi)}</span>
          <input
            type="time"
            className={styles.time}
            value={toTime}
            onChange={(e) => setToTime(e.target.value)}
            onKeyDown={onTimeKey}
          />
        </label>
      </div>

      <div className={styles.actions}>
        <Button size="sm" variant="ghost" onClick={() => onApply(null)}>
          Clear
        </Button>
        <Button size="sm" variant="primary" disabled={!start} onClick={apply}>
          Search
        </Button>
      </div>
    </div>
  );
}
