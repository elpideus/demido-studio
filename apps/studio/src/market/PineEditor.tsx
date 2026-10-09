// The Pine Editor under the chart: writes the scripts of Demido's Pine library (the assistant's
// too), compiles them on TradingView as they are typed, puts them on the chart and, when asked,
// saves them to the user's TradingView account.

import { useEffect, useMemo, useRef, useState, type KeyboardEvent, type PointerEvent } from 'react';
import {
  AlertCircle,
  AlertTriangle,
  Check,
  CircleCheck,
  CloudUpload,
  Code2,
  Plus,
  Save,
  SquareFunction,
  Trash2,
  X,
} from 'lucide-react';
import { Badge, Button, Dialog, IconButton, Select, Spinner, cx } from '@demido/ui';

import { api, errorText } from '@/lib/api';
import type { PineCheck, PineMessage, PineScript, PineSummary } from '@/lib/types';
import { usePine } from '@/stores/pine';
import { toast } from '@/stores/toasts';
import { marksByLine, offsetAt, segmentsOf } from './pineSyntax';
import styles from './MarketWindow.module.css';

const TEMPLATE = `//@version=6
indicator("My script", overlay = true)

length = input.int(20, "Length", minval = 1)
plot(ta.sma(close, length), "SMA", color = #2962ff, linewidth = 2)
`;

/** Matches `.pineHighlight` and `.pineInput` in MarketWindow.module.css. */
const LINE_HEIGHT = 18;
const INDENT = '    ';
export const MIN_HEIGHT = 160;

/** Edits not saved yet, by script (`new` for one never saved): kept while switching scripts. */
const unsaved = new Map<string, string>();

const summaryOf = ({ source: _source, ...summary }: PineScript): PineSummary => summary;

/** Types into the textarea the way the keyboard would, so Undo still works. */
function insert(el: HTMLTextAreaElement, text: string, onEdit: (value: string) => void) {
  if (!document.execCommand('insertText', false, text)) {
    el.setRangeText(text, el.selectionStart, el.selectionEnd, 'end');
    onEdit(el.value);
  }
}

function plural(n: number, one: string, many = `${one}s`) {
  return `${n} ${n === 1 ? one : many}`;
}

export interface PineEditorProps {
  /** The library script open, or null for a new one. */
  scriptId: string | null;
  height: number;
  loggedIn: boolean;
  /** The library scripts on the chart. */
  onChart: ReadonlySet<string>;
  onOpen: (id: string | null) => void;
  onResize: (height: number) => void;
  onClose: () => void;
  onAddToChart: (script: PineSummary) => void;
  /** A script deleted from the library: off the chart too. */
  onDeleted: (id: string) => void;
}

export function PineEditor({
  scriptId,
  height,
  loggedIn,
  onChart,
  onOpen,
  onResize,
  onClose,
  onAddToChart,
  onDeleted,
}: PineEditorProps) {
  const scripts = usePine((s) => s.scripts);
  const pineLoaded = usePine((s) => s.loaded && !s.error);
  const listed = scriptId ? scripts.find((s) => s.id === scriptId) : undefined;
  const [saved, setSaved] = useState<PineScript | null>(null);
  const [source, setSource] = useState(() => unsaved.get('new') ?? TEMPLATE);
  const [loading, setLoading] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [check, setCheck] = useState<PineCheck | null>(null);
  const [checkError, setCheckError] = useState<string | null>(null);
  const [checking, setChecking] = useState(false);
  const [busy, setBusy] = useState<'save' | 'chart' | 'publish' | 'delete' | null>(null);
  const [confirm, setConfirm] = useState<'publish' | 'delete' | null>(null);
  const [dragHeight, setDragHeight] = useState<number | null>(null);
  const area = useRef<HTMLTextAreaElement>(null);
  const highlight = useRef<HTMLPreElement>(null);
  const gutter = useRef<HTMLDivElement>(null);
  const sourceRef = useRef(source);
  sourceRef.current = source;
  const savedRef = useRef(saved);
  savedRef.current = saved;

  const isNew = scriptId === null;
  const draftKey = scriptId ?? 'new';
  // Deleted meanwhile (by the assistant, say): saving creates it again.
  const deleted = !isNew && !!saved && pineLoaded && !listed;
  const dirty = isNew || deleted || (!!saved && source !== saved.source);
  // The assistant saved a newer version while it was open.
  const newer = !!listed && !!saved && listed.revision > saved.revision;

  useEffect(() => {
    let cancelled = false;
    setLoadError(null);
    setCheck(null);
    setCheckError(null);
    if (!scriptId) {
      setSaved(null);
      setSource(unsaved.get('new') ?? TEMPLATE);
      return undefined;
    }
    // Just saved as this script: what is shown is it.
    if (savedRef.current?.id === scriptId) return undefined;
    setLoading(true);
    api
      .marketPineGet(scriptId)
      .then(
        (s) => {
          if (cancelled) return;
          setSaved(s);
          setSource(unsaved.get(s.id) ?? s.source);
        },
        (e) => !cancelled && setLoadError(errorText(e)),
      )
      .finally(() => !cancelled && setLoading(false));
    return () => {
      cancelled = true;
    };
  }, [scriptId]);

  const reload = async () => {
    if (!scriptId) return;
    try {
      const s = await api.marketPineGet(scriptId);
      unsaved.delete(scriptId);
      setSaved(s);
      setSource(s.source);
    } catch (e) {
      toast.error('Could not load the script', errorText(e));
    }
  };

  // A newer version over no edits of the user's is simply shown.
  useEffect(() => {
    if (newer && saved && sourceRef.current === saved.source) void reload();
  }, [newer]);

  // Compiled a moment after typing stops; only the latest answer counts.
  const checkSeq = useRef(0);
  useEffect(() => {
    if (loading || loadError) return undefined;
    const seq = ++checkSeq.current;
    setChecking(true);
    const timer = window.setTimeout(() => {
      api
        .marketPineCheck(source)
        .then(
          (c) => {
            if (seq !== checkSeq.current) return;
            setCheck(c);
            setCheckError(null);
          },
          (e) => seq === checkSeq.current && setCheckError(errorText(e)),
        )
        .finally(() => seq === checkSeq.current && setChecking(false));
    }, 900);
    return () => window.clearTimeout(timer);
  }, [source, loading, loadError, loggedIn]);

  const edit = (text: string) => {
    setSource(text);
    if (scriptId && savedRef.current && text === savedRef.current.source) unsaved.delete(scriptId);
    else unsaved.set(draftKey, text);
  };

  const lines = useMemo(() => source.split('\n'), [source]);
  const marks = useMemo(
    () => marksByLine(lines, check?.errors ?? [], check?.warnings ?? []),
    [lines, check],
  );

  /** Saves the editor's text in the library; the script it is saved as. */
  const persist = async (): Promise<PineScript> => {
    const text = sourceRef.current;
    const s = await api.marketPineSave(isNew || deleted ? null : scriptId, text);
    unsaved.delete(draftKey);
    setSaved(s);
    savedRef.current = s;
    usePine.getState().changed(s.id, summaryOf(s));
    if (s.id !== scriptId) onOpen(s.id);
    return s;
  };

  const save = async () => {
    if (busy) return;
    setBusy('save');
    try {
      await persist();
    } catch (e) {
      toast.error('Could not save the script', errorText(e));
    } finally {
      setBusy(null);
    }
  };

  const kind = check?.kind ?? listed?.kind ?? null;
  const chartable = kind === null || kind === 'indicator';
  const onChartNow = !!scriptId && onChart.has(scriptId);

  const addToChart = async () => {
    if (busy) return;
    setBusy('chart');
    try {
      const s = dirty || !saved ? await persist() : saved;
      // On the chart already, saving is what updates it.
      if (!onChart.has(s.id)) onAddToChart(summaryOf(s));
    } catch (e) {
      toast.error('Could not save the script', errorText(e));
    } finally {
      setBusy(null);
    }
  };

  const publish = async () => {
    setConfirm(null);
    if (busy) return;
    setBusy('publish');
    try {
      const s = dirty || !saved ? await persist() : saved;
      const res = await api.marketPinePublish(s.id);
      usePine.getState().changed(res.script.id, res.script);
      toast.success(
        res.tradingview.created ? 'Saved to TradingView' : 'Updated on TradingView',
        `${res.script.name} is in My scripts on tradingview.com${res.tradingview.created ? '' : `, version ${res.tradingview.version}`}.`,
      );
    } catch (e) {
      toast.error('Could not save to TradingView', errorText(e));
    } finally {
      setBusy(null);
    }
  };

  const remove = async () => {
    setConfirm(null);
    if (!scriptId || busy) return;
    setBusy('delete');
    try {
      await api.marketPineDelete(scriptId);
      unsaved.delete(scriptId);
      usePine.getState().changed(scriptId, null);
      onDeleted(scriptId);
      onOpen(scripts.find((s) => s.id !== scriptId)?.id ?? null);
    } catch (e) {
      toast.error('Could not delete the script', errorText(e));
    } finally {
      setBusy(null);
    }
  };

  const jump = (m: PineMessage) => {
    const el = area.current;
    if (!el) return;
    const start = offsetAt(source, m.line, m.column);
    const end = m.endLine === m.line ? offsetAt(source, m.line, m.endColumn + 1) : start;
    el.focus();
    el.setSelectionRange(start, Math.max(start, end));
    el.scrollTop = Math.max(0, (m.line - 4) * LINE_HEIGHT);
  };

  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    const el = e.currentTarget;
    const mod = e.ctrlKey || e.metaKey;
    if (mod && !e.altKey && e.key.toLowerCase() === 's') {
      e.preventDefault();
      void save();
      return;
    }
    if (mod && e.key === 'Enter') {
      e.preventDefault();
      void addToChart();
      return;
    }
    if (e.key === 'Tab' && !mod && !e.altKey) {
      e.preventDefault();
      const { selectionStart: s, selectionEnd: end, value } = el;
      if (!e.shiftKey && s === end) {
        insert(el, INDENT, edit);
        return;
      }
      const from = value.lastIndexOf('\n', s - 1) + 1;
      const block = value.slice(from, end);
      const next = block
        .split('\n')
        .map((l) => (e.shiftKey ? l.replace(/^ {1,4}|^\t/, '') : INDENT + l))
        .join('\n');
      el.setSelectionRange(from, end);
      insert(el, next, edit);
      el.setSelectionRange(from, from + next.length);
      return;
    }
    if (e.key === 'Enter' && !mod && !e.shiftKey && !e.altKey) {
      // The new line keeps the indent, one more after a block's head.
      const { selectionStart: s, value } = el;
      const line = value.slice(value.lastIndexOf('\n', s - 1) + 1, s);
      const lead = /^[ \t]*/.exec(line)![0];
      const opens = /=>\s*$/.test(line) || /^\s*(if|else|for|while|switch|type|enum)\b/.test(line);
      e.preventDefault();
      insert(el, `\n${lead}${opens ? INDENT : ''}`, edit);
    }
  };

  const onScroll = () => {
    const el = area.current;
    if (!el) return;
    if (highlight.current) {
      highlight.current.scrollTop = el.scrollTop;
      highlight.current.scrollLeft = el.scrollLeft;
    }
    if (gutter.current) gutter.current.scrollTop = el.scrollTop;
  };

  const startResize = (e: PointerEvent<HTMLDivElement>) => {
    e.preventDefault();
    const startY = e.clientY;
    const start = height;
    const max = Math.max(MIN_HEIGHT, (e.currentTarget.closest('[data-chart-window]')?.clientHeight ?? 800) - 180);
    let latest = start;
    const move = (ev: globalThis.PointerEvent) => {
      latest = Math.round(Math.min(max, Math.max(MIN_HEIGHT, start + startY - ev.clientY)));
      setDragHeight(latest);
    };
    const up = () => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', up);
      setDragHeight(null);
      if (latest !== start) onResize(latest);
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', up);
  };

  const errors = check?.errors ?? [];
  const warnings = check?.warnings ?? [];
  const name = check?.title || listed?.name || saved?.name || 'This script';
  const linked = listed?.tradingview;
  const failing = !!check && !check.ok;

  let status;
  if (loading) status = <span className={styles.pineStatus}>Loading…</span>;
  else if (checking && !check) status = <span className={styles.pineStatus}><Spinner size={12} /> Compiling…</span>;
  else if (checkError)
    status = (
      <span className={cx(styles.pineStatus, styles.pineStatusWarn)} title={checkError}>
        <AlertTriangle size={13} aria-hidden /> Not compiled
      </span>
    );
  else if (check && !check.ok)
    status = (
      <span className={cx(styles.pineStatus, styles.pineStatusError)}>
        <AlertCircle size={13} aria-hidden /> {plural(errors.length, 'error')}
      </span>
    );
  else if (check)
    status = (
      <span className={cx(styles.pineStatus, styles.pineStatusOk)}>
        <CircleCheck size={13} aria-hidden /> Compiles{checking ? '…' : ''}
      </span>
    );

  const options = [
    ...(isNew ? [{ value: '', label: 'New script' }] : []),
    ...scripts.map((s) => ({ value: s.id, label: unsaved.has(s.id) && s.id !== scriptId ? `${s.name} •` : s.name })),
    ...(deleted && saved ? [{ value: saved.id, label: `${saved.name} (deleted)` }] : []),
  ];

  return (
    <div className={styles.pineEditor} style={{ height: dragHeight ?? height }}>
      <div
        className={styles.pineResize}
        role="separator"
        aria-orientation="horizontal"
        aria-label="Resize the Pine Editor"
        onPointerDown={startResize}
      />
      <div className={styles.pineToolbar}>
        <Code2 size={15} className={styles.pineIcon} aria-hidden />
        <Select
          size="sm"
          aria-label="Script"
          className={styles.pineSelect}
          value={scriptId ?? ''}
          options={options}
          onChange={(e) => onOpen(e.target.value || null)}
        />
        {dirty && !isNew && <span className={styles.pineStatus}>Unsaved changes</span>}
        {status}
        {linked && (
          <Badge
            tone={linked.changed ? 'warning' : 'accent'}
            title={linked.changed ? 'Changed since it was last saved to TradingView' : 'Saved to your TradingView account'}
          >
            {linked.changed ? 'TradingView: older' : 'On TradingView'}
          </Badge>
        )}
        <span className={styles.flex} />
        <Button size="sm" variant="ghost" icon={Plus} onClick={() => onOpen(null)} disabled={isNew}>
          New
        </Button>
        <Button
          size="sm"
          variant="secondary"
          icon={Save}
          loading={busy === 'save'}
          disabled={!dirty || !!busy || loading}
          title="Save (Ctrl+S)"
          onClick={() => void save()}
        >
          Save
        </Button>
        <Button
          size="sm"
          variant="primary"
          icon={onChartNow && !dirty ? Check : SquareFunction}
          loading={busy === 'chart'}
          disabled={!chartable || failing || !!busy || loading || (onChartNow && !dirty)}
          title={
            !chartable
              ? `A ${kind} does not go on a chart by itself.`
              : onChartNow
                ? 'Saving updates it on the chart (Ctrl+Enter)'
                : 'Save and add to the chart (Ctrl+Enter)'
          }
          onClick={() => void addToChart()}
        >
          {onChartNow ? (dirty ? 'Update chart' : 'On the chart') : 'Add to chart'}
        </Button>
        <Button
          size="sm"
          variant="ghost"
          icon={CloudUpload}
          loading={busy === 'publish'}
          disabled={!loggedIn || failing || !!busy || loading}
          title={loggedIn ? 'Save it to your TradingView account, to use it on tradingview.com' : 'Sign in to TradingView first'}
          onClick={() => setConfirm('publish')}
        >
          {linked ? 'Update on TradingView' : 'Save to TradingView'}
        </Button>
        {!isNew && !deleted && (
          <IconButton icon={Trash2} label="Delete the script" size="sm" disabled={!!busy} onClick={() => setConfirm('delete')} />
        )}
        <IconButton icon={X} label="Close the Pine Editor" size="sm" onClick={onClose} />
      </div>
      {newer && saved && source !== saved.source && (
        <div className={styles.pineNotice}>
          A newer version was saved meanwhile (by the assistant?), over your unsaved changes.
          <Button size="sm" variant="secondary" onClick={() => void reload()}>
            Load it
          </Button>
        </div>
      )}
      {deleted && <div className={styles.pineNotice}>This script was deleted from the library. Saving it creates it again.</div>}
      <div className={styles.pineMain}>
        {loadError ? (
          <div className={styles.resultsEmpty}>{loadError}</div>
        ) : (
          <div className={styles.pineCode}>
            <div ref={gutter} className={styles.pineGutter} aria-hidden>
              {lines.map((_, i) => {
                const m = marks.get(i);
                const kindHere = m?.some((x) => x.kind === 'error') ? 'error' : m ? 'warning' : null;
                return (
                  <div
                    key={i}
                    className={cx(kindHere === 'error' && styles.pineGutterError, kindHere === 'warning' && styles.pineGutterWarn)}
                  >
                    {i + 1}
                  </div>
                );
              })}
            </div>
            <div className={styles.pineText}>
              <pre ref={highlight} className={styles.pineHighlight} aria-hidden>
                {lines.map((line, i) => (
                  <div key={i}>
                    {line || marks.has(i)
                      ? segmentsOf(line, marks.get(i)).map((s, k) => (
                          <span
                            key={k}
                            className={cx(
                              s.cls && styles[`pine_${s.cls}`],
                              s.mark === 'error' && styles.pineMarkError,
                              s.mark === 'warning' && styles.pineMarkWarn,
                            )}
                          >
                            {s.text}
                          </span>
                        ))
                      : ' '}
                  </div>
                ))}
              </pre>
              <textarea
                ref={area}
                className={styles.pineInput}
                aria-label="Pine Script source"
                value={source}
                spellCheck={false}
                autoCapitalize="off"
                autoCorrect="off"
                autoComplete="off"
                wrap="off"
                readOnly={loading}
                onChange={(e) => edit(e.target.value)}
                onKeyDown={onKeyDown}
                onScroll={onScroll}
              />
            </div>
          </div>
        )}
        <div className={styles.pineMessages}>
          {checkError && (
            <div className={styles.pineMessage}>
              <AlertTriangle size={13} className={styles.pineStatusWarn} aria-hidden />
              <span>{checkError}</span>
            </div>
          )}
          {!checkError && check?.ok && (
            <div className={styles.pineSummary}>
              <div>
                <CircleCheck size={13} className={styles.pineStatusOk} aria-hidden /> {check.title || 'Untitled'}
              </div>
              <div className={styles.resultMeta}>
                {[
                  check.kind ? check.kind[0]!.toUpperCase() + check.kind.slice(1) : null,
                  check.overlay === undefined ? null : check.overlay ? 'over the candles' : 'own pane',
                  check.meta ? plural(check.meta.plots.length, 'plot') : null,
                  check.meta ? plural(check.meta.inputs.length, 'input') : null,
                ]
                  .filter(Boolean)
                  .join(' · ')}
              </div>
            </div>
          )}
          {[...errors.map((m) => ['error', m] as const), ...warnings.map((m) => ['warning', m] as const)].map(
            ([level, m], i) => (
              <button key={i} type="button" className={styles.pineMessage} onClick={() => jump(m)}>
                {level === 'error' ? (
                  <AlertCircle size={13} className={styles.pineStatusError} aria-label="Error" />
                ) : (
                  <AlertTriangle size={13} className={styles.pineStatusWarn} aria-label="Warning" />
                )}
                <span>
                  <span className={styles.resultMeta}>
                    Line {m.line}:{m.column}
                  </span>{' '}
                  {m.message}
                </span>
              </button>
            ),
          )}
          {!check && !checkError && (
            <div className={styles.resultsEmpty}>{checking ? <Spinner size={14} /> : 'Compiles as you type.'}</div>
          )}
        </div>
      </div>
      <Dialog
        open={confirm === 'publish'}
        onClose={() => setConfirm(null)}
        title={linked ? 'Update it on TradingView?' : 'Save it to your TradingView account?'}
        icon={<CloudUpload size={18} />}
        footer={
          <>
            <span className={styles.flex} />
            <Button variant="secondary" onClick={() => setConfirm(null)}>
              Cancel
            </Button>
            <Button variant="primary" icon={CloudUpload} onClick={() => void publish()}>
              {linked ? 'Update on TradingView' : 'Save to TradingView'}
            </Button>
          </>
        }
      >
        <p className={styles.pineConfirm}>
          <strong>{name}</strong> is saved as a private script under My scripts on tradingview.com
          {linked ? ', as a new version of the one there' : ''}, so you can add it to your charts in the browser. Nothing is
          published to the community.
          {dirty ? ' Your unsaved changes are saved here first.' : ''}
        </p>
      </Dialog>
      <Dialog
        open={confirm === 'delete'}
        onClose={() => setConfirm(null)}
        title={`Delete ${listed?.name ?? 'this script'}?`}
        icon={<Trash2 size={18} />}
        footer={
          <>
            <span className={styles.flex} />
            <Button variant="secondary" onClick={() => setConfirm(null)}>
              Cancel
            </Button>
            <Button variant="danger" icon={Trash2} onClick={() => void remove()}>
              Delete
            </Button>
          </>
        }
      >
        <p className={styles.pineConfirm}>
          It is removed from Demido's library and from this chart.
          {linked ? ' The copy saved on your TradingView account stays there.' : ''}
        </p>
      </Dialog>
    </div>
  );
}
