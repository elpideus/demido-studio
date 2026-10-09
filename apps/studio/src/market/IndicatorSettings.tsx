import { useEffect, useState } from 'react';
import { Settings2 } from 'lucide-react';
import { Button, Checkbox, Dialog, Field, SegmentedControl, Select, Slider, Switch, TextField, cx } from '@demido/ui';

import type { BandLook, IndicatorInput, IndicatorLook, IndicatorMeta, PlotLook } from '@/lib/types';
import { SOURCES } from './indicators';
import { NUMERIC_INPUTS as NUMERIC, editableInputs } from './indicatorView';
import { PLOT_KINDS, applyLook, joinColor, shownOn, splitColor, tidyLook } from './look';
import styles from './MarketWindow.module.css';

/** What a field shows for an input value (numbers are edited as text, so they can be cleared). */
function draftOf(input: IndicatorInput, value: unknown): unknown {
  if (NUMERIC.has(input.type)) return value === null || value === undefined ? '' : String(value);
  if (input.type === 'time') return typeof value === 'number' ? localDateTime(value) : '';
  return value;
}

function localDateTime(ms: number): string {
  const d = new Date(ms);
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/** A draft back as the value the script takes; undefined when it is not one. */
function valueOf(input: IndicatorInput, draft: unknown): unknown {
  if (NUMERIC.has(input.type)) {
    const text = String(draft).trim();
    const n = text === '' ? NaN : Number(text);
    if (!Number.isFinite(n)) return undefined;
    return input.type === 'integer' ? Math.round(n) : n;
  }
  if (input.type === 'time') {
    const ms = new Date(String(draft)).getTime();
    return Number.isFinite(ms) ? ms : undefined;
  }
  return draft;
}

type Tab = 'inputs' | 'style' | 'visibility';

const TABS: Array<{ value: Tab; label: string }> = [
  { value: 'inputs', label: 'Inputs' },
  { value: 'style', label: 'Style' },
  { value: 'visibility', label: 'Visibility' },
];

const WIDTHS = [1, 2, 3, 4].map((w) => ({ value: String(w), label: `${w}px` }));
const DASHES = [
  { value: '0', label: 'Solid' },
  { value: '1', label: 'Dotted' },
  { value: '2', label: 'Dashed' },
];
const PRECISIONS = [
  { value: '', label: 'Default' },
  ...Array.from({ length: 9 }, (_, i) => ({ value: String(i), label: String(i) })),
];
/** Kinds drawn as a line, which can be dotted or dashed. */
const LINES = new Set(['line', 'step', 'area']);

/** TradingView's palette: greys, then its hues bright, light and dark. */
const PALETTE = [
  '#ffffff',
  '#d1d4dc',
  '#b2b5be',
  '#9598a1',
  '#787b86',
  '#5d606b',
  '#434651',
  '#2a2e39',
  '#131722',
  '#000000',
  '#f23645',
  '#ff9800',
  '#ffeb3b',
  '#4caf50',
  '#089981',
  '#00bcd4',
  '#2962ff',
  '#673ab7',
  '#9c27b0',
  '#e91e63',
  '#faa1a4',
  '#ffcc80',
  '#fff59d',
  '#a5d6a7',
  '#70ccbd',
  '#80deea',
  '#90bff9',
  '#b39ddb',
  '#ce93d8',
  '#f48fb1',
  '#b22833',
  '#f57c00',
  '#fbc02d',
  '#388e3c',
  '#056656',
  '#0097a7',
  '#1848cc',
  '#512da8',
  '#7b1fa2',
  '#c2185b',
];

/** A palette, a hex field and an opacity slider, opened under the swatch that was clicked. */
function ColorPicker({ value, onChange }: { value: string; onChange: (color: string) => void }) {
  const parts = splitColor(value);
  const alpha = parts?.alpha ?? 1;
  const [text, setText] = useState(parts?.hex ?? value);
  useEffect(() => setText(splitColor(value)?.hex ?? value), [value]);

  const typed = (t: string) => {
    setText(t);
    const s = splitColor(t);
    // A typed opacity (`#rrggbbaa`, `rgba(…)`) wins over the slider's.
    if (s) onChange(joinColor(s.hex, /^#[0-9a-f]{8}$|^rgba/i.test(t.trim()) ? s.alpha : alpha));
  };

  return (
    <div className={styles.colorPicker}>
      <div className={styles.palette}>
        {PALETTE.map((c) => (
          <button
            key={c}
            type="button"
            aria-label={c}
            title={c}
            className={cx(styles.paletteColor, parts?.hex === c && styles.paletteActive)}
            style={{ background: c }}
            onClick={() => onChange(joinColor(c, alpha))}
          />
        ))}
      </div>
      <div className={styles.colorControls}>
        <TextField
          size="sm"
          value={text}
          aria-label="Color"
          invalid={!splitColor(text)}
          onChange={(e) => typed(e.target.value)}
          className={styles.colorHex}
        />
        <Slider
          label="Opacity"
          value={Math.round(alpha * 100)}
          min={0}
          max={100}
          step={1}
          disabled={!parts}
          format={(v) => `${v}%`}
          onChange={(v) => parts && onChange(joinColor(parts.hex, v / 100))}
        />
      </div>
    </div>
  );
}

function Swatch({
  color,
  active,
  label,
  onClick,
}: {
  color: string;
  active: boolean;
  label: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      aria-expanded={active}
      title={label}
      className={cx(styles.swatchButton, active && styles.swatchActive)}
      onClick={onClick}
    >
      <span style={{ background: color }} />
    </button>
  );
}

/** A band's level, edited as text so it can be cleared on the way to another number. */
function LevelField({ value, onChange }: { value: number; onChange: (v: number) => void }) {
  const [text, setText] = useState(String(value));
  useEffect(() => setText((t) => (Number(t) === value ? t : String(value))), [value]);
  return (
    <TextField
      size="sm"
      type="number"
      inputMode="decimal"
      step="any"
      aria-label="Level"
      value={text}
      invalid={!Number.isFinite(Number(text)) || text.trim() === ''}
      onChange={(e) => {
        setText(e.target.value);
        const n = Number(e.target.value);
        if (e.target.value.trim() !== '' && Number.isFinite(n)) onChange(n);
      }}
      className={styles.settingsNumber}
    />
  );
}

export interface IndicatorSettingsProps {
  open: boolean;
  /** Its description, without its look. */
  meta: IndicatorMeta | undefined;
  /** Its look on this chart. */
  look: IndicatorLook | undefined;
  /** The chart's timeframes, for the Visibility tab. */
  timeframes: ReadonlyArray<{ value: string; label: string }>;
  onClose: () => void;
  /** The edited inputs' values (by input id) and the look worth keeping. */
  onApply: (values: Record<string, unknown>, look: IndicatorLook | undefined) => void;
}

/** An indicator's settings as TradingView has them: its inputs (grouped as the script groups
 *  them), its style and the timeframes it shows on. */
export function IndicatorSettings({ open, meta, look, timeframes, onClose, onApply }: IndicatorSettingsProps) {
  const inputs = meta ? editableInputs(meta) : [];
  const [tab, setTab] = useState<Tab>('inputs');
  const [drafts, setDrafts] = useState<Record<string, unknown>>({});
  const [lookDraft, setLookDraft] = useState<IndicatorLook>({});
  // The swatch whose picker is open: `plot:<id>`, `plot:<id>:<palette key>` or `band:<id>`.
  const [picking, setPicking] = useState<string | null>(null);

  useEffect(() => {
    if (!open || !meta) return;
    const editable = editableInputs(meta);
    setDrafts(Object.fromEntries(editable.map((i) => [i.id, draftOf(i, i.value)])));
    setLookDraft(structuredClone(look ?? {}));
    setTab(editable.length ? 'inputs' : 'style');
    setPicking(null);
    // Taken once per opening (not on `look`): a look saved meanwhile does not overwrite the edits.
  }, [open, meta]);

  const invalid = inputs.filter((i) => valueOf(i, drafts[i.id]) === undefined);
  const set = (id: string, v: unknown) => setDrafts((prev) => ({ ...prev, [id]: v }));
  const shown = meta ? applyLook(meta, lookDraft) : undefined;

  const setPlot = (id: string, patch: PlotLook) =>
    setLookDraft((l) => ({ ...l, plots: { ...l.plots, [id]: { ...l.plots?.[id], ...patch } } }));
  const setBand = (id: string, patch: BandLook) =>
    setLookDraft((l) => ({ ...l, bands: { ...l.bands, [id]: { ...l.bands?.[id], ...patch } } }));
  const setOption = (patch: IndicatorLook) => setLookDraft((l) => ({ ...l, ...patch }));
  const toggleTimeframe = (tf: string, on: boolean) =>
    setLookDraft((l) => {
      const now = l.timeframes ?? timeframes.map((t) => t.value);
      return { ...l, timeframes: on ? [...now.filter((t) => t !== tf), tf] : now.filter((t) => t !== tf) };
    });

  const groups: Array<[string, IndicatorInput[]]> = [];
  for (const input of inputs) {
    const name = input.group ?? '';
    const group = groups.find(([g]) => g === name);
    if (group) group[1].push(input);
    else groups.push([name, [input]]);
  }

  const apply = () => {
    if (!meta) return;
    onApply(
      Object.fromEntries(inputs.map((i) => [i.id, valueOf(i, drafts[i.id])])),
      tidyLook(
        meta,
        lookDraft,
        timeframes.map((t) => t.value),
      ),
    );
    onClose();
  };

  const defaults = () => {
    setDrafts(Object.fromEntries(inputs.map((i) => [i.id, draftOf(i, i.defval)])));
    setLookDraft({});
    setPicking(null);
  };

  const control = (input: IndicatorInput) => {
    const draft = drafts[input.id];
    const options = input.options?.length ? input.options : input.type === 'source' ? [...SOURCES] : null;
    if (input.type === 'bool')
      return <Switch size="sm" checked={draft === true} onChange={(on) => set(input.id, on)} />;
    if (options) {
      const value = String(draft ?? '');
      return (
        <Select
          size="sm"
          value={value}
          onChange={(e) => set(input.id, e.target.value)}
          options={(options.includes(value) ? options : [value, ...options]).map((o) => ({ value: o, label: o }))}
        />
      );
    }
    if (NUMERIC.has(input.type)) {
      return (
        <TextField
          size="sm"
          type="number"
          inputMode="decimal"
          value={String(draft ?? '')}
          min={input.min}
          max={input.max}
          step={input.step ?? (input.type === 'integer' ? 1 : 'any')}
          invalid={valueOf(input, draft) === undefined}
          onChange={(e) => set(input.id, e.target.value)}
          className={styles.settingsNumber}
        />
      );
    }
    if (input.type === 'time') {
      return (
        <TextField
          size="sm"
          type="datetime-local"
          value={String(draft ?? '')}
          invalid={valueOf(input, draft) === undefined}
          onChange={(e) => set(input.id, e.target.value)}
        />
      );
    }
    return (
      <TextField
        size="sm"
        value={String(draft ?? '')}
        placeholder={input.type === 'resolution' ? "The chart's" : undefined}
        trailing={
          input.type === 'color' && typeof draft === 'string' && draft ? (
            <span className={styles.settingsSwatch} style={{ background: draft }} aria-hidden />
          ) : undefined
        }
        onChange={(e) => set(input.id, e.target.value)}
      />
    );
  };

  const swatch = (key: string, color: string, label: string) => (
    <Swatch
      key={key}
      color={color}
      label={label}
      active={picking === key}
      onClick={() => setPicking((p) => (p === key ? null : key))}
    />
  );

  const styleTab = shown && (
    <>
      {shown.plots.length > 0 && (
        <section className={styles.settingsGroup}>
          <div className={styles.resultsTitle}>Plots</div>
          {shown.plots.map((p) => {
            const palette = p.colors ? Object.entries(p.colors).slice(0, 8) : null;
            const open = picking?.startsWith(`plot:${p.id}`) ? picking : null;
            const paletteKey = open && open !== `plot:${p.id}` ? open.slice(`plot:${p.id}:`.length) : null;
            return (
              <div key={p.id}>
                <div className={styles.styleRow}>
                  <Checkbox
                    checked={!p.hidden}
                    onChange={(on) => setPlot(p.id, { hidden: !on })}
                    label={p.title || p.id}
                  />
                  <span className={styles.flex} />
                  {palette
                    ? palette.map(([k, c]) =>
                        swatch(`plot:${p.id}:${k}`, c, `${p.title || p.id}: color ${Number(k) + 1 || k}`),
                      )
                    : swatch(`plot:${p.id}`, p.color, `${p.title || p.id}: color`)}
                  {p.kind !== 'shapes' && (
                    <Select
                      size="sm"
                      aria-label="Line width"
                      className={styles.styleSelect}
                      value={String(p.width)}
                      options={WIDTHS}
                      onChange={(e) => setPlot(p.id, { width: Number(e.target.value) })}
                    />
                  )}
                  {LINES.has(p.kind) && (
                    <Select
                      size="sm"
                      aria-label="Line style"
                      className={styles.styleSelect}
                      value={String(p.dash)}
                      options={DASHES}
                      onChange={(e) => setPlot(p.id, { dash: Number(e.target.value) })}
                    />
                  )}
                  {p.kind !== 'shapes' && (
                    <Select
                      size="sm"
                      aria-label="Plot type"
                      className={styles.styleSelect}
                      value={p.kind}
                      options={PLOT_KINDS.map((k) => ({ value: k.value, label: k.label }))}
                      onChange={(e) => setPlot(p.id, { kind: e.target.value as PlotLook['kind'] })}
                    />
                  )}
                </div>
                {open && (
                  <ColorPicker
                    value={paletteKey !== null ? (p.colors?.[paletteKey] ?? p.color) : p.color}
                    onChange={(c) =>
                      paletteKey !== null
                        ? setPlot(p.id, { colors: { ...lookDraft.plots?.[p.id]?.colors, [paletteKey]: c } })
                        : setPlot(p.id, { color: c })
                    }
                  />
                )}
              </div>
            );
          })}
        </section>
      )}
      {shown.bands.length > 0 && (
        <section className={styles.settingsGroup}>
          <div className={styles.resultsTitle}>Levels</div>
          {shown.bands.map((b) => (
            <div key={b.id}>
              <div className={styles.styleRow}>
                <Checkbox
                  checked={!b.hidden}
                  onChange={(on) => setBand(b.id, { hidden: !on })}
                  label={b.title || b.id}
                />
                <span className={styles.flex} />
                {swatch(`band:${b.id}`, b.color, `${b.title || b.id}: color`)}
                <LevelField value={b.value} onChange={(value) => setBand(b.id, { value })} />
                <Select
                  size="sm"
                  aria-label="Line width"
                  className={styles.styleSelect}
                  value={String(b.width)}
                  options={WIDTHS}
                  onChange={(e) => setBand(b.id, { width: Number(e.target.value) })}
                />
                <Select
                  size="sm"
                  aria-label="Line style"
                  className={styles.styleSelect}
                  value={String(b.dash)}
                  options={DASHES}
                  onChange={(e) => setBand(b.id, { dash: Number(e.target.value) })}
                />
              </div>
              {picking === `band:${b.id}` && (
                <ColorPicker value={b.color} onChange={(c) => setBand(b.id, { color: c })} />
              )}
            </div>
          ))}
        </section>
      )}
      <section className={styles.settingsGroup}>
        <div className={styles.resultsTitle}>Values</div>
        <Field
          layout="inline"
          label="Precision"
          description="Decimals of its values. Default: the script's, or the chart's."
        >
          <Select
            size="sm"
            className={styles.styleSelect}
            value={lookDraft.precision === undefined ? '' : String(lookDraft.precision)}
            options={PRECISIONS}
            onChange={(e) => setOption({ precision: e.target.value === '' ? undefined : Number(e.target.value) })}
          />
        </Field>
        <Checkbox
          checked={lookDraft.scaleLabels !== false}
          onChange={(on) => setOption({ scaleLabels: on })}
          label="Labels on price scale"
        />
        <Checkbox
          checked={lookDraft.legendInputs !== false}
          onChange={(on) => setOption({ legendInputs: on })}
          label="Inputs in status line"
        />
        <Checkbox
          checked={lookDraft.legendValues !== false}
          onChange={(on) => setOption({ legendValues: on })}
          label="Values in status line"
        />
      </section>
    </>
  );

  const visibilityTab = (
    <section className={styles.settingsGroup}>
      <div className={styles.resultsTitle}>Show on timeframes</div>
      <div className={styles.timeframeGrid}>
        {timeframes.map((t) => (
          <Checkbox
            key={t.value}
            checked={shownOn(lookDraft, t.value)}
            onChange={(on) => toggleTimeframe(t.value, on)}
            label={t.label}
          />
        ))}
      </div>
    </section>
  );

  return (
    <Dialog
      open={open && !!meta}
      onClose={onClose}
      title={meta?.name ?? 'Indicator'}
      icon={<Settings2 size={18} />}
      width={540}
      footer={
        <>
          <Button variant="ghost" onClick={defaults}>
            Defaults
          </Button>
          <span className={styles.flex} />
          <Button variant="secondary" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" disabled={invalid.length > 0} onClick={apply}>
            OK
          </Button>
        </>
      }
    >
      <SegmentedControl size="sm" value={tab} onChange={setTab} options={TABS} className={styles.settingsTabs} />
      <div className={styles.settings}>
        {tab === 'inputs' && (
          <>
            {inputs.length === 0 && <div className={styles.resultsEmpty}>This indicator has no inputs.</div>}
            {groups.map(([group, list]) => (
              <section key={group} className={styles.settingsGroup}>
                {group && <div className={styles.resultsTitle}>{group}</div>}
                {list.map((input) => (
                  <Field key={input.id} layout="inline" label={input.name} description={input.tooltip}>
                    {control(input)}
                  </Field>
                ))}
              </section>
            ))}
          </>
        )}
        {tab === 'style' && styleTab}
        {tab === 'visibility' && visibilityTab}
      </div>
    </Dialog>
  );
}
