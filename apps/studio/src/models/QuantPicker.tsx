import { type KeyboardEvent, type ReactNode, useLayoutEffect, useRef, useState } from 'react';
import { Check, ChevronDown, Layers, Star } from 'lucide-react';
import { Button, Popover, Tooltip, cx, formatBytes } from '@demido/ui';

import type { HfModelFile, MemoryBudget } from '@/lib/types';
import { bitsPerWeight, fitFor, isUsual, quantLabels } from './fit';
import styles from './ModelsWindow.module.css';

/** Wide enough for every column, whatever the trigger's width. */
const MIN_WIDTH = 640;

interface QuantPickerProps {
  files: HfModelFile[];
  parameters: number | null;
  chosen: HfModelFile;
  best: HfModelFile | undefined;
  /** Bytes of the projector that comes along, counted into each file's fit. */
  projector: number;
  budget: MemoryBudget;
  onChoose: (file: HfModelFile) => void;
  /** What the file's download or installed copy is doing, shown at the end of its row. */
  status: (file: HfModelFile) => ReactNode;
}

/** The quantization to download, as a dropdown: each with its bits, how it fits, its size and status. */
export function QuantPicker({
  files,
  parameters,
  chosen,
  best,
  projector,
  budget,
  onChoose,
  status,
}: QuantPickerProps) {
  const labels = quantLabels(files);
  const label = (file: HfModelFile) => labels.get(file.name) ?? file.name;
  const [open, setOpen] = useState(false);
  const [all, setAll] = useState(false);
  const [active, setActive] = useState(chosen.name);
  const [width, setWidth] = useState<number>();
  const trigger = useRef<HTMLButtonElement>(null);
  const list = useRef<HTMLDivElement>(null);

  const shown = all
    ? files
    : files.filter((f) => isUsual(f, parameters) || f.name === chosen.name || f.name === best?.name);
  const hidden = files.length - shown.length;
  const fit = fitFor(chosen.size + projector, budget);
  const bits = bitsPerWeight(chosen, parameters);

  useLayoutEffect(() => {
    if (!open) return;
    setWidth(Math.max(trigger.current?.offsetWidth ?? 0, MIN_WIDTH));
    list.current?.querySelector('[data-active="true"]')?.scrollIntoView({ block: 'nearest' });
  }, [open]);

  const show = () => {
    setActive(chosen.name);
    setOpen(true);
  };
  const choose = (file: HfModelFile) => {
    onChoose(file);
    setOpen(false);
    trigger.current?.focus();
  };
  const onKeyDown = (e: KeyboardEvent) => {
    if (!open) {
      if (e.key === 'ArrowDown' || e.key === 'ArrowUp' || e.key === 'Enter' || e.key === ' ') {
        e.preventDefault();
        show();
      }
      return;
    }
    const i = shown.findIndex((f) => f.name === active);
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      const next = shown[Math.max(0, Math.min(shown.length - 1, i + (e.key === 'ArrowDown' ? 1 : -1)))];
      if (next) {
        setActive(next.name);
        list.current?.querySelector(`[data-file="${CSS.escape(next.name)}"]`)?.scrollIntoView({ block: 'nearest' });
      }
    } else if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      const file = shown[i];
      if (file) choose(file);
    } else if (e.key === 'Tab') {
      setOpen(false);
    }
  };

  return (
    <>
      <button
        ref={trigger}
        type="button"
        className={styles.picker}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label={`Quantization: ${label(chosen)}`}
        onClick={() => (open ? setOpen(false) : show())}
        onKeyDown={onKeyDown}
      >
        <span className={styles.quant}>
          {label(chosen)}
          {chosen.name === best?.name && <Star size={12} className={styles.star} aria-label="Best fit" />}
        </span>
        {bits !== null && <span className={styles.bits}>{bits.toFixed(2)} bpw</span>}
        <span className={cx(styles.pill, styles[`tone-${fit.tone}`])}>{fit.label}</span>
        <span className={styles.grow} />
        <span className={styles.size}>{formatBytes(chosen.size + projector)}</span>
        <ChevronDown size={15} className={cx(styles.chevron, open && styles.chevronOpen)} aria-hidden />
      </button>
      <Popover
        open={open}
        onClose={() => setOpen(false)}
        anchorRef={trigger}
        placement="bottom-start"
        width={width}
        maxHeight={420}
        aria-label="Quantizations"
      >
        <div ref={list} className={styles.options} role="listbox" aria-label="Quantizations" onKeyDown={onKeyDown}>
          <div className={styles.optionsHead} aria-hidden>
            <span>Quant</span>
            <span>Bits</span>
            <span>Fit</span>
            <span className={styles.end}>Size</span>
            <span className={styles.end}>Status</span>
          </div>
          {shown.map((file) => {
            const f = fitFor(file.size + projector, budget);
            const b = bitsPerWeight(file, parameters);
            return (
              <div
                key={file.name}
                role="option"
                aria-selected={file.name === chosen.name}
                data-active={file.name === active}
                data-file={file.name}
                className={styles.option}
                onPointerMove={() => setActive(file.name)}
                onClick={() => choose(file)}
              >
                <span className={styles.quant}>
                  <span className={styles.checkSlot}>
                    {file.name === chosen.name && <Check size={13} className={styles.checkMark} aria-hidden />}
                  </span>
                  {label(file)}
                  {file.name === best?.name && (
                    <Tooltip content="The best quality that fits, with room to spare" delay={200}>
                      <Star size={12} className={styles.star} aria-label="Best fit" />
                    </Tooltip>
                  )}
                  {file.paths.length > 1 && (
                    <Tooltip content={`${file.paths.length} files, downloaded one after another`} delay={200}>
                      <span className={styles.pill}>
                        <Layers size={9} aria-hidden />
                        {file.paths.length}
                      </span>
                    </Tooltip>
                  )}
                </span>
                <span className={styles.bits}>{b === null ? '' : b.toFixed(2)}</span>
                <span>
                  <Tooltip content={f.text} delay={200}>
                    <span className={cx(styles.pill, styles[`tone-${f.tone}`])}>{f.label}</span>
                  </Tooltip>
                </span>
                <span className={cx(styles.size, styles.end)}>{formatBytes(file.size)}</span>
                <span className={styles.optionStatus}>{status(file)}</span>
              </div>
            );
          })}
          {(hidden > 0 || all) && (
            <Button
              size="sm"
              variant="ghost"
              className={styles.more}
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => setAll((a) => !a)}
            >
              {all ? 'Show the usual quants' : `Show ${hidden} more (extreme and full precision)`}
            </Button>
          )}
        </div>
      </Popover>
    </>
  );
}
