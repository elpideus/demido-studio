import {
  forwardRef,
  useLayoutEffect,
  useRef,
  type InputHTMLAttributes,
  type ReactNode,
  type SelectHTMLAttributes,
  type TextareaHTMLAttributes,
} from 'react';
import { Check, ChevronDown } from 'lucide-react';
import type { LucideIcon } from 'lucide-react';

import { cx } from '../utils';
import styles from './Controls.module.css';

/* ---------------------------------------------------------------- Switch */

export interface SwitchProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
  size?: 'sm' | 'md';
  label?: string;
  className?: string;
}

export function Switch({ checked, onChange, disabled, size = 'md', label, className }: SwitchProps) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      className={cx(styles.switch, styles[`switch-${size}`], checked && styles.on, className)}
      onClick={(e) => {
        e.stopPropagation();
        onChange(!checked);
      }}
    >
      <span className={styles.thumb} />
    </button>
  );
}

/* -------------------------------------------------------------- Checkbox */

export interface CheckboxProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label?: ReactNode;
  description?: ReactNode;
  disabled?: boolean;
  className?: string;
}

export function Checkbox({ checked, onChange, label, description, disabled, className }: CheckboxProps) {
  return (
    <label className={cx(styles.checkRow, disabled && styles.disabledRow, className)}>
      <input
        type="checkbox"
        className="visually-hidden"
        checked={checked}
        disabled={disabled}
        onChange={(e) => onChange(e.target.checked)}
      />
      <span className={cx(styles.checkbox, checked && styles.checked)} aria-hidden>
        {checked && <Check size={12} strokeWidth={3} />}
      </span>
      {(label || description) && (
        <span className={styles.checkText}>
          {label && <span className={styles.checkLabel}>{label}</span>}
          {description && <span className={styles.checkDescription}>{description}</span>}
        </span>
      )}
    </label>
  );
}

/* ------------------------------------------------------------- TextField */

export interface TextFieldProps extends Omit<InputHTMLAttributes<HTMLInputElement>, 'size'> {
  icon?: LucideIcon;
  trailing?: ReactNode;
  invalid?: boolean;
  size?: 'sm' | 'md';
}

export const TextField = forwardRef<HTMLInputElement, TextFieldProps>(function TextField(
  { icon: Icon, trailing, invalid, size = 'md', className, ...rest },
  ref,
) {
  return (
    <div className={cx(styles.field, styles[`field-${size}`], invalid && styles.invalid, className)}>
      {Icon && <Icon size={15} strokeWidth={1.9} className={styles.fieldIcon} aria-hidden />}
      <input ref={ref} className={styles.input} spellCheck={false} {...rest} />
      {trailing && <span className={styles.trailing}>{trailing}</span>}
    </div>
  );
});

/* -------------------------------------------------------------- TextArea */

export interface TextAreaProps extends TextareaHTMLAttributes<HTMLTextAreaElement> {
  /** Grow with the content between these row counts. */
  autoSize?: { min: number; max: number };
  mono?: boolean;
}

export const TextArea = forwardRef<HTMLTextAreaElement, TextAreaProps>(function TextArea(
  { autoSize, mono, className, value, ...rest },
  forwarded,
) {
  const inner = useRef<HTMLTextAreaElement | null>(null);
  useLayoutEffect(() => {
    const el = inner.current;
    if (!el || !autoSize) return;
    const line = parseFloat(getComputedStyle(el).lineHeight) || 20;
    const pad = el.offsetHeight - el.clientHeight + 16;
    el.style.height = 'auto';
    const h = Math.min(Math.max(el.scrollHeight, autoSize.min * line), autoSize.max * line + pad);
    el.style.height = `${h}px`;
  }, [value, autoSize]);
  return (
    <textarea
      ref={(el) => {
        inner.current = el;
        if (typeof forwarded === 'function') forwarded(el);
        else if (forwarded) forwarded.current = el;
      }}
      className={cx(styles.textarea, mono && styles.mono, className)}
      value={value}
      spellCheck={false}
      {...rest}
    />
  );
});

/* ---------------------------------------------------------------- Slider */

export interface SliderProps {
  value: number;
  onChange: (value: number) => void;
  min: number;
  max: number;
  step?: number;
  disabled?: boolean;
  format?: (value: number) => string;
  label?: string;
}

export function Slider({ value, onChange, min, max, step = 0.01, disabled, format, label }: SliderProps) {
  const pct = ((value - min) / (max - min)) * 100;
  return (
    <div className={styles.sliderRow}>
      <input
        type="range"
        aria-label={label}
        className={styles.slider}
        min={min}
        max={max}
        step={step}
        value={value}
        disabled={disabled}
        onChange={(e) => onChange(Number(e.target.value))}
        style={{ ['--fill' as string]: `${pct}%` }}
      />
      <span className={styles.sliderValue}>{format ? format(value) : value}</span>
    </div>
  );
}

/* ---------------------------------------------------------------- Select */

export interface SelectProps extends Omit<SelectHTMLAttributes<HTMLSelectElement>, 'size'> {
  options: Array<{ value: string; label: string }>;
  size?: 'sm' | 'md';
}

export function Select({ options, size = 'md', className, ...rest }: SelectProps) {
  return (
    <div className={cx(styles.selectWrap, styles[`field-${size}`], className)}>
      <select className={styles.select} {...rest}>
        {options.map((o) => (
          <option key={o.value} value={o.value}>
            {o.label}
          </option>
        ))}
      </select>
      <ChevronDown size={14} className={styles.selectChevron} aria-hidden />
    </div>
  );
}

/* ------------------------------------------------------ SegmentedControl */

export interface SegmentedOption<T extends string> {
  value: T;
  label: string;
  icon?: LucideIcon;
}

export interface SegmentedControlProps<T extends string> {
  value: T;
  onChange: (value: T) => void;
  options: Array<SegmentedOption<T>>;
  size?: 'sm' | 'md';
  className?: string;
}

export function SegmentedControl<T extends string>({
  value,
  onChange,
  options,
  size = 'md',
  className,
}: SegmentedControlProps<T>) {
  return (
    <div role="tablist" className={cx(styles.segmented, styles[`segmented-${size}`], className)}>
      {options.map(({ value: v, label, icon: Icon }) => (
        <button
          key={v}
          type="button"
          role="tab"
          aria-selected={v === value}
          className={cx(styles.segment, v === value && styles.segmentActive)}
          onClick={() => onChange(v)}
        >
          {Icon && <Icon size={14} strokeWidth={1.9} aria-hidden />}
          {label}
        </button>
      ))}
    </div>
  );
}
